use clap::Parser;
use omp_bridge::RunningBridge;
use omp_bridge::api_types::EngineStatus;
use omp_bridge::config::{
    BridgeConfig, DEFAULT_ALLOWED_ORIGINS, DEFAULT_LISTEN, DEFAULT_MAX_FRAME_BYTES,
    DEFAULT_MAX_REASSEMBLED_BYTES, DEFAULT_PAIRING_TTL_SECS, DEFAULT_REQUEST_TIMEOUT_SECS,
    DEFAULT_STARTUP_TIMEOUT_SECS, DEFAULT_TOKEN_TTL_SECS,
};
use omp_bridge::engine::redact_error;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;
use tokio::process::Command;

#[derive(Debug, Parser)]
#[command(
    name = "omp-bridge",
    version,
    about = "Authenticated loopback HTTP/WebSocket bridge for OMP RPC"
)]
struct Cli {
    /// Path to the real OMP executable.
    #[arg(long, env = "OMP_BRIDGE_OMP_BIN", default_value = "omp")]
    omp_bin: PathBuf,

    /// Working directory for the OMP process.
    #[arg(long, env = "OMP_BRIDGE_WORKSPACE", default_value = ".")]
    workspace: PathBuf,

    /// Loopback listen address. Non-loopback addresses are rejected.
    #[arg(long, env = "OMP_BRIDGE_LISTEN", default_value = DEFAULT_LISTEN)]
    listen: String,

    /// Exact browser origins allowed to call the bridge.
    #[arg(
        long,
        env = "OMP_BRIDGE_ALLOWED_ORIGINS",
        value_delimiter = ',',
        default_value = DEFAULT_ALLOWED_ORIGINS
    )]
    allowed_origins: Vec<String>,

    /// Optional OMP model argument, for example provider/model.
    #[arg(long, env = "OMP_BRIDGE_MODEL")]
    model: Option<String>,

    #[arg(
        long,
        env = "OMP_BRIDGE_STARTUP_TIMEOUT_SECS",
        default_value_t = DEFAULT_STARTUP_TIMEOUT_SECS
    )]
    startup_timeout_secs: u64,

    #[arg(
        long,
        env = "OMP_BRIDGE_REQUEST_TIMEOUT_SECS",
        default_value_t = DEFAULT_REQUEST_TIMEOUT_SECS
    )]
    request_timeout_secs: u64,

    #[arg(
        long,
        env = "OMP_BRIDGE_PAIRING_TTL_SECS",
        default_value_t = DEFAULT_PAIRING_TTL_SECS
    )]
    pairing_ttl_secs: u64,

    #[arg(
        long,
        env = "OMP_BRIDGE_TOKEN_TTL_SECS",
        default_value_t = DEFAULT_TOKEN_TTL_SECS
    )]
    token_ttl_secs: u64,

    #[arg(
        long,
        env = "OMP_BRIDGE_MAX_FRAME_BYTES",
        default_value_t = DEFAULT_MAX_FRAME_BYTES
    )]
    max_frame_bytes: usize,

    #[arg(
        long,
        env = "OMP_BRIDGE_MAX_REASSEMBLED_BYTES",
        default_value_t = DEFAULT_MAX_REASSEMBLED_BYTES
    )]
    max_reassembled_bytes: usize,

    /// Validate configuration and the OMP executable without starting the bridge.
    #[arg(long)]
    doctor: bool,
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("omp-bridge: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), String> {
    let cli = Cli::parse();
    let listen = cli
        .listen
        .parse()
        .map_err(|error| format!("invalid listen address {:?}: {error}", cli.listen))?;

    let mut omp_args = vec!["--mode".to_owned(), "rpc-ui".to_owned()];
    if let Some(model) = &cli.model {
        omp_args.push("--model".to_owned());
        omp_args.push(model.clone());
    }

    let config = BridgeConfig {
        omp_bin: cli.omp_bin,
        omp_args,
        workspace: cli.workspace,
        listen,
        allowed_origins: cli.allowed_origins,
        startup_timeout: Duration::from_secs(cli.startup_timeout_secs),
        request_timeout: Duration::from_secs(cli.request_timeout_secs),
        pairing_ttl: Duration::from_secs(cli.pairing_ttl_secs),
        token_ttl: Duration::from_secs(cli.token_ttl_secs),
        max_frame_bytes: cli.max_frame_bytes,
        max_reassembled_bytes: cli.max_reassembled_bytes,
    }
    .validate()
    .map_err(|error| error.to_string())?;

    if cli.doctor {
        return doctor(&config).await;
    }

    let pairing_minutes = cli.pairing_ttl_secs / 60;
    let bridge = RunningBridge::start(config)
        .await
        .map_err(|error| format!("cannot bind loopback listener: {error}"))?;
    let status = bridge.engine.status();

    println!("OMP Bridge {}", omp_bridge::api_types::BRIDGE_VERSION);
    println!("  API:       http://{}/api/v1", bridge.addr);
    println!("  WebSocket: ws://{}/api/v1/events", bridge.addr);
    println!("  OMP:       {}", bridge.launch_summary);
    print_engine_status(&status);
    if !status.ready {
        eprintln!(
            "omp-bridge: engine is not ready: {}",
            status
                .error
                .as_ref()
                .map(|error| format!("{}: {}", error.code, error.message))
                .unwrap_or_else(|| "unknown startup failure".to_owned())
        );
        bridge.shutdown().await;
        return Err("OMP did not become ready; fix the configuration and retry".to_owned());
    }

    println!(
        "  Pairing:   {} (single use, expires in {} minutes)",
        bridge.pairing_code, pairing_minutes
    );
    println!("Press Ctrl+C to stop the bridge and its OMP child process.");

    tokio::signal::ctrl_c()
        .await
        .map_err(|error| format!("cannot wait for shutdown signal: {error}"))?;
    println!("Stopping bridge...");
    bridge.shutdown().await;
    Ok(())
}

fn print_engine_status(status: &EngineStatus) {
    match &status.error {
        Some(error) => println!("  Engine:    {} ({})", error.code, error.message),
        None => {
            let protocol = status
                .negotiated_protocol_version
                .map(|version| format!("v{version}"))
                .unwrap_or_else(|| "unknown".to_owned());
            let model = status
                .summary
                .as_ref()
                .and_then(|summary| summary.model.as_ref())
                .map(|model| format!("{}/{}", model.provider, model.id))
                .unwrap_or_else(|| "not reported".to_owned());
            println!("  Engine:    ready, protocol {protocol}, model {model}");
        }
    }
}

async fn doctor(config: &BridgeConfig) -> Result<(), String> {
    println!("omp-bridge doctor");
    println!("  OMP binary: {}", config.omp_bin.display());
    println!("  Workspace:  {}", config.workspace.display());
    println!("  Listen:     {}", config.listen);
    println!("  Origins:    {}", config.allowed_origins.join(", "));
    println!(
        "  Limits:     {} byte frames, {} byte reassembled",
        config.max_frame_bytes, config.max_reassembled_bytes
    );

    let output = Command::new(&config.omp_bin)
        .arg("--version")
        .output()
        .await
        .map_err(|error| format!("cannot execute OMP --version: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "OMP --version failed with status {}: {}",
            output.status,
            redact_error(&String::from_utf8_lossy(&output.stderr))
        ));
    }
    let version = String::from_utf8_lossy(&output.stdout);
    println!("  Version:    {}", version.trim());
    println!("doctor: configuration looks usable; no OMP process was started.");
    Ok(())
}
