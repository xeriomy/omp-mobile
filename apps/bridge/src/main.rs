use clap::{Args, Parser, Subcommand};
use omp_bridge::RunningBridge;
use omp_bridge::api_types::{BRIDGE_VERSION, EngineStatus};
use omp_bridge::config::{
    BridgeConfig, DEFAULT_REQUEST_TIMEOUT_SECS, DEFAULT_STARTUP_TIMEOUT_SECS,
};
use omp_bridge::config_store::{
    ConfigOverrides, ConfigPaths, StoredConfig, load_config, save_config,
};
use omp_bridge::discovery::{
    VERSION_TIMEOUT, discover, format_discovery, platform_info, verify_installation,
};
use omp_bridge::process_manager::{
    ProcessError, RuntimeLock, format_status, print_logs, run_background_child, running_pid,
    start_background, status, stop, uninstall,
};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

#[derive(Debug, Parser)]
#[command(
    name = "omp-bridge",
    version,
    about = "Authenticated loopback HTTP/WebSocket bridge for OMP RPC"
)]
struct Cli {
    #[command(flatten)]
    config: ConfigArgs,

    #[command(subcommand)]
    command: Option<Command>,

    /// Validate configuration and OMP without starting a persistent bridge.
    #[arg(long)]
    doctor: bool,
}

#[derive(Debug, Clone, Args)]
struct ConfigArgs {
    /// Path to the real OMP executable.
    #[arg(long, global = true, env = "OMP_BRIDGE_OMP_BIN")]
    omp_bin: Option<PathBuf>,

    /// Working directory for the OMP process.
    #[arg(long, global = true, env = "OMP_BRIDGE_WORKSPACE")]
    workspace: Option<PathBuf>,

    /// Loopback listen address. Non-loopback addresses are rejected.
    #[arg(long, global = true, env = "OMP_BRIDGE_LISTEN")]
    listen: Option<String>,

    /// Exact browser origins allowed to call the bridge.
    #[arg(
        long,
        global = true,
        env = "OMP_BRIDGE_ALLOWED_ORIGINS",
        value_delimiter = ','
    )]
    allowed_origins: Option<Vec<String>>,

    /// Optional OMP model argument, for example provider/model.
    #[arg(long, global = true, env = "OMP_BRIDGE_MODEL")]
    model: Option<String>,

    #[arg(long, global = true, env = "OMP_BRIDGE_STARTUP_TIMEOUT_SECS")]
    startup_timeout_secs: Option<u64>,

    #[arg(long, global = true, env = "OMP_BRIDGE_REQUEST_TIMEOUT_SECS")]
    request_timeout_secs: Option<u64>,

    #[arg(long, global = true, env = "OMP_BRIDGE_PAIRING_TTL_SECS")]
    pairing_ttl_secs: Option<u64>,

    #[arg(long, global = true, env = "OMP_BRIDGE_TOKEN_TTL_SECS")]
    token_ttl_secs: Option<u64>,

    #[arg(long, global = true, env = "OMP_BRIDGE_MAX_FRAME_BYTES")]
    max_frame_bytes: Option<usize>,

    #[arg(long, global = true, env = "OMP_BRIDGE_MAX_REASSEMBLED_BYTES")]
    max_reassembled_bytes: Option<usize>,

    #[arg(long, global = true, env = "OMP_BRIDGE_LOG_MAX_BYTES")]
    log_max_bytes: Option<u64>,
}

impl ConfigArgs {
    fn overrides(&self) -> ConfigOverrides {
        ConfigOverrides {
            omp_bin: self.omp_bin.clone(),
            workspace: self.workspace.clone(),
            model: self.model.clone(),
            listen: self.listen.clone(),
            allowed_origins: self.allowed_origins.clone(),
            startup_timeout_secs: self.startup_timeout_secs,
            request_timeout_secs: self.request_timeout_secs,
            pairing_ttl_secs: self.pairing_ttl_secs,
            token_ttl_secs: self.token_ttl_secs,
            max_frame_bytes: self.max_frame_bytes,
            max_reassembled_bytes: self.max_reassembled_bytes,
            log_max_bytes: self.log_max_bytes,
        }
    }
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Detect OMP, verify an RPC startup, and save the bridge configuration.
    Setup,
    /// Start the bridge in the background. Use --foreground to stay attached.
    Start(StartArgs),
    /// Stop the background bridge and its OMP child.
    Stop,
    /// Stop and start the bridge.
    Restart(StartArgs),
    /// Show bridge, engine, and OMP status.
    Status {
        /// Print machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Show bridge log lines.
    Logs {
        /// Number of trailing lines.
        #[arg(long, default_value_t = 200)]
        lines: usize,
        /// Keep printing new lines until interrupted.
        #[arg(long)]
        follow: bool,
    },
    /// Check configuration, discovery, platform, and a real RPC startup.
    Doctor,
    /// Show the saved configuration and file locations.
    Config {
        /// Print machine-readable JSON.
        #[arg(long)]
        json: bool,
        /// Print only the configuration file path.
        #[arg(long)]
        path: bool,
    },
    /// Show how to upgrade the bridge safely.
    Update,
    /// Remove runtime files, logs, and the bridge binary. Configuration is kept
    /// unless --purge is passed.
    Uninstall {
        /// Remove configuration as well.
        #[arg(long)]
        purge: bool,
        /// Keep the bridge executable.
        #[arg(long)]
        keep_binary: bool,
        /// Print actions without changing anything.
        #[arg(long)]
        dry_run: bool,
        /// Confirm the destructive action.
        #[arg(long)]
        yes: bool,
    },
    /// Internal background child entry point.
    #[command(hide = true)]
    Run(RunArgs),
}

#[derive(Debug, Clone, Args)]
struct StartArgs {
    /// Stay attached to the terminal instead of detaching.
    #[arg(long)]
    foreground: bool,
}

#[derive(Debug, Clone, Args)]
struct RunArgs {
    #[arg(long)]
    startup_id: String,
    #[arg(long)]
    handshake_file: PathBuf,
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
    let paths = ConfigPaths::discover().map_err(|error| error.to_string())?;
    let overrides = cli.config.overrides();

    match cli.command {
        Some(Command::Setup) => setup(&paths, &overrides).await,
        Some(Command::Start(args)) => start(&paths, &overrides, args).await,
        Some(Command::Stop) => stop_command(&paths).await,
        Some(Command::Restart(args)) => {
            stop_command(&paths).await?;
            start(&paths, &overrides, args).await
        }
        Some(Command::Status { json }) => status_command(&paths, json),
        Some(Command::Logs { lines, follow }) => print_logs(&paths, lines, follow)
            .await
            .map_err(|error| format!("cannot read logs: {error}")),
        Some(Command::Doctor) => doctor(&paths, &overrides).await,
        Some(Command::Config { json, path }) => config_command(&paths, &overrides, json, path),
        Some(Command::Update) => update(),
        Some(Command::Uninstall {
            purge,
            keep_binary,
            dry_run,
            yes,
        }) => uninstall_command(&paths, purge, keep_binary, dry_run, yes),
        Some(Command::Run(args)) => run_child(&paths, &overrides, args).await,
        None if cli.doctor => doctor(&paths, &overrides).await,
        None => legacy_foreground(&paths, &overrides).await,
    }
}

async fn setup(paths: &ConfigPaths, overrides: &ConfigOverrides) -> Result<(), String> {
    let existing = load_config(paths).map_err(|error| error.to_string())?;

    println!("OMP Bridge {BRIDGE_VERSION} setup");
    println!("  Platform:  {}", format_platform());

    let stored = initialize_configuration(paths, overrides, existing.as_ref()).await?;
    save_config(paths, &stored).map_err(|error| error.to_string())?;

    println!("  Saved:     {}", paths.config_file.display());
    println!();
    println!("Next: omp-bridge start");
    Ok(())
}

/// Shared discovery/validation/configuration path used by `setup` and the
/// first-run initialization inside `start`. Both produce the same stored
/// configuration shape and go through the same validation.
async fn initialize_configuration(
    paths: &ConfigPaths,
    overrides: &ConfigOverrides,
    existing: Option<&StoredConfig>,
) -> Result<StoredConfig, String> {
    let workspace = workspace_for(existing, overrides)?;
    let saved_bin = existing.map(|config| config.omp_bin.as_path());

    println!("  Workspace: {}", workspace.display());
    println!("  Config:    {}", paths.config_file.display());

    let result = discover(overrides.omp_bin.as_deref(), saved_bin, VERSION_TIMEOUT).await;
    print!("{}", format_discovery(&result));
    let selected = result.selected().cloned().ok_or_else(|| {
        let detail = result
            .errors
            .first()
            .map(|error| format!("{}: {}", error.code, error.message))
            .unwrap_or_else(|| "no usable OMP installation found".to_owned());
        format!("{detail}\nInstall OMP or pass --omp-bin, then run `omp-bridge start` again.")
    })?;

    let startup_timeout = overrides
        .startup_timeout_secs
        .or_else(|| existing.map(|config| config.startup_timeout_secs))
        .unwrap_or(DEFAULT_STARTUP_TIMEOUT_SECS);
    let request_timeout = overrides
        .request_timeout_secs
        .or_else(|| existing.map(|config| config.request_timeout_secs))
        .unwrap_or(DEFAULT_REQUEST_TIMEOUT_SECS);
    let model = overrides
        .model
        .clone()
        .or_else(|| existing.and_then(|config| config.model.clone()));

    println!(
        "  Checking a real OMP RPC startup (up to {}s)...",
        startup_timeout
    );
    let verified = verify_installation(
        &selected,
        &workspace,
        model.as_deref(),
        Duration::from_secs(startup_timeout),
        Duration::from_secs(request_timeout),
    )
    .await
    .map_err(|error| {
        let base = format!("{}: {}", error.code, error.message);
        if error.code == "OMP_MISSING_MODEL" {
            format!(
                "{base}\nConfigure the model in your existing OMP installation (or pass \
                 --model), then run `omp-bridge start` again."
            )
        } else {
            base
        }
    })?;

    let mut stored = existing
        .cloned()
        .unwrap_or_else(|| StoredConfig::new(verified.path.clone(), workspace.clone()));
    stored.omp_bin = verified.path.clone();
    stored.omp_version = Some(verified.version.clone());
    stored.workspace = workspace;
    stored.apply_overrides(overrides);
    stored.validate().map_err(|error| error.to_string())?;

    println!(
        "  OMP:       {} ({})",
        verified.path.display(),
        verified.version
    );
    if let Some(protocol) = verified.protocol_version {
        println!("  Protocol:  v{protocol}");
    }
    if let Some(model) = &verified.model {
        println!("  Model:     {model}");
    }
    Ok(stored)
}

/// Load the saved configuration for a normal start, or run first-run
/// initialization and persist it atomically when none exists. An existing but
/// damaged configuration is never treated as fresh and never overwritten.
async fn ensure_stored_config(
    paths: &ConfigPaths,
    overrides: &ConfigOverrides,
) -> Result<StoredConfig, String> {
    match load_config(paths) {
        Ok(Some(mut stored)) => {
            stored.apply_overrides(overrides);
            stored.validate().map_err(|error| error.to_string())?;
            Ok(stored)
        }
        Ok(None) => {
            println!("OMP Bridge {BRIDGE_VERSION}");
            println!("First run detected. Initializing bridge...");
            let stored = initialize_configuration(paths, overrides, None).await?;
            save_config(paths, &stored).map_err(|error| error.to_string())?;
            println!("  Saved:     {}", paths.config_file.display());
            println!();
            Ok(stored)
        }
        Err(error) => Err(format!(
            "saved configuration {} is invalid: {error}\nFix or back it up and remove it, then \
             run `omp-bridge setup`.",
            paths.config_file.display()
        )),
    }
}

async fn start(
    paths: &ConfigPaths,
    overrides: &ConfigOverrides,
    args: StartArgs,
) -> Result<(), String> {
    let stored = ensure_stored_config(paths, overrides).await?;
    let config = stored
        .to_bridge_config()
        .map_err(|error| error.to_string())?;

    if args.foreground {
        return start_foreground(paths, config, &stored).await;
    }

    let bridge_exe = std::env::current_exe()
        .map_err(|error| format!("cannot locate the bridge executable: {error}"))?;
    let result = start_background(
        paths,
        &bridge_exe,
        overrides,
        Duration::from_secs(stored.startup_timeout_secs),
        stored.log_max_bytes,
    )
    .await
    .map_err(|error| error.to_string())?;

    println!("OMP Bridge {BRIDGE_VERSION} started");
    println!("  PID:       {}", result.pid);
    println!("  API:       http://{}/api/v1", result.listen);
    println!("  WebSocket: ws://{}/api/v1/events", result.listen);
    println!("  Logs:      {}", result.log_path.display());
    println!();
    println!(
        "  Pairing:   {} (single use, expires in {} minutes)",
        result.pairing_code,
        stored.pairing_ttl_secs / 60
    );
    println!("Pair the app now. The code is not saved anywhere.");
    Ok(())
}

async fn start_foreground(
    paths: &ConfigPaths,
    config: BridgeConfig,
    stored: &StoredConfig,
) -> Result<(), String> {
    paths.ensure().map_err(|error| error.to_string())?;
    if let Some(pid) = running_pid(paths) {
        return Err(format!("omp-bridge is already running with PID {pid}"));
    }
    let _lock = RuntimeLock::acquire(&paths.lock_file)
        .map_err(|error| format!("cannot acquire the runtime lock: {error}"))?;

    let pairing_minutes = stored.pairing_ttl_secs / 60;
    let bridge = RunningBridge::start(config)
        .await
        .map_err(|error| format!("cannot bind loopback listener: {error}"))?;
    let engine_status = bridge.engine.status();

    println!("OMP Bridge {BRIDGE_VERSION}");
    println!("  API:       http://{}/api/v1", bridge.addr);
    println!("  WebSocket: ws://{}/api/v1/events", bridge.addr);
    println!("  OMP:       {}", bridge.launch_summary);
    print_engine_status(&engine_status);
    if !engine_status.ready {
        eprintln!(
            "omp-bridge: engine is not ready: {}",
            engine_status
                .error
                .as_ref()
                .map(|error| format!("{}: {}", error.code, error.message))
                .unwrap_or_else(|| "unknown startup failure".to_owned())
        );
        bridge.shutdown().await;
        return Err("OMP did not become ready; run `omp-bridge doctor`".to_owned());
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

async fn stop_command(paths: &ConfigPaths) -> Result<(), String> {
    match stop(paths).await {
        Ok(result) => {
            if result.stale {
                println!(
                    "Removed stale runtime state for PID {}; the process was already gone.",
                    result.pid
                );
            } else if result.forced {
                println!("Stopped bridge PID {} after a forced shutdown.", result.pid);
            } else {
                println!("Stopped bridge PID {}.", result.pid);
            }
            Ok(())
        }
        Err(ProcessError::NotRunning) => {
            println!("omp-bridge is not running.");
            Ok(())
        }
        Err(error) => Err(error.to_string()),
    }
}

fn status_command(paths: &ConfigPaths, json: bool) -> Result<(), String> {
    let configured = load_config(paths)
        .map_err(|error| error.to_string())?
        .is_some();
    let report = status(paths, configured);
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report)
                .map_err(|error| format!("cannot encode status: {error}"))?
        );
    } else {
        print!("{}", format_status(&report));
    }
    Ok(())
}

async fn doctor(paths: &ConfigPaths, overrides: &ConfigOverrides) -> Result<(), String> {
    println!("omp-bridge doctor");
    println!("  Bridge:    {BRIDGE_VERSION}");
    println!("  Platform:  {}", format_platform());
    println!("  Config:    {}", paths.config_file.display());
    println!("  Runtime:   {}", paths.runtime_dir.display());
    println!("  Logs:      {}", paths.log_file.display());

    let existing = load_config(paths).map_err(|error| error.to_string())?;
    let mut stored = existing.clone().unwrap_or_else(|| {
        StoredConfig::new(
            overrides
                .omp_bin
                .clone()
                .unwrap_or_else(|| PathBuf::from("omp")),
            overrides
                .workspace
                .clone()
                .unwrap_or_else(|| PathBuf::from(".")),
        )
    });
    stored.apply_overrides(overrides);
    stored.validate().map_err(|error| error.to_string())?;

    let result = discover(
        overrides.omp_bin.as_deref(),
        existing.as_ref().map(|config| config.omp_bin.as_path()),
        VERSION_TIMEOUT,
    )
    .await;
    print!("{}", format_discovery(&result));
    let Some(selected) = result.selected().cloned() else {
        return Err("no usable OMP installation found".to_owned());
    };

    let workspace = workspace_for(Some(&stored), overrides)?;
    println!("  Workspace: {}", workspace.display());
    println!(
        "  Checking a real OMP RPC startup (up to {}s)...",
        stored.startup_timeout_secs
    );
    let verified = verify_installation(
        &selected,
        &workspace,
        stored.model.as_deref(),
        Duration::from_secs(stored.startup_timeout_secs),
        Duration::from_secs(stored.request_timeout_secs),
    )
    .await
    .map_err(|error| format!("{}: {}", error.code, error.message))?;
    println!(
        "  RPC:       ready, protocol {}, model {}",
        verified
            .protocol_version
            .map(|version| format!("v{version}"))
            .unwrap_or_else(|| "unknown".to_owned()),
        verified.model.as_deref().unwrap_or("not reported")
    );

    let report = status(paths, existing.is_some());
    println!("  Service:   {}", report.phase);
    if let Some(pid) = report.pid {
        println!("  PID:       {pid}");
    }
    println!("doctor: configuration and a real RPC startup look usable.");
    Ok(())
}

fn config_command(
    paths: &ConfigPaths,
    overrides: &ConfigOverrides,
    json: bool,
    path_only: bool,
) -> Result<(), String> {
    if path_only {
        println!("{}", paths.config_file.display());
        return Ok(());
    }
    let mut stored = load_config(paths).map_err(|error| error.to_string())?;
    match &mut stored {
        Some(config) => config.apply_overrides(overrides),
        None => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "configured": false,
                        "configFile": paths.config_file,
                        "runtimeDir": paths.runtime_dir,
                        "logFile": paths.log_file,
                    })
                );
            } else {
                println!("Configuration: not set up");
                println!("  Config:  {}", paths.config_file.display());
                println!("  Runtime: {}", paths.runtime_dir.display());
                println!("  Logs:    {}", paths.log_file.display());
                println!(
                    "Run `omp-bridge start` to initialize automatically, or \
`omp-bridge setup` for manual configuration."
                );
            }
            return Ok(());
        }
    }

    let stored = stored.expect("configured");
    stored.validate().map_err(|error| error.to_string())?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "configured": true,
                "configFile": paths.config_file,
                "runtimeDir": paths.runtime_dir,
                "logFile": paths.log_file,
                "config": stored,
            }))
            .map_err(|error| format!("cannot encode configuration: {error}"))?
        );
    } else {
        println!("Configuration: {}", paths.config_file.display());
        println!("  OMP:        {}", stored.omp_bin.display());
        if let Some(version) = &stored.omp_version {
            println!("  OMP version:{version}");
        }
        println!("  Workspace:  {}", stored.workspace.display());
        println!("  Listen:     {}", stored.listen);
        println!("  Origins:    {}", stored.allowed_origins.join(", "));
        if let Some(model) = &stored.model {
            println!("  Model:      {model}");
        }
        println!(
            "  Timeouts:   startup {}s, request {}s, pairing {}s, token {}s",
            stored.startup_timeout_secs,
            stored.request_timeout_secs,
            stored.pairing_ttl_secs,
            stored.token_ttl_secs
        );
        println!(
            "  Limits:     {} byte frames, {} byte reassembled, {} byte logs",
            stored.max_frame_bytes, stored.max_reassembled_bytes, stored.log_max_bytes
        );
        println!("  Runtime:    {}", paths.runtime_dir.display());
        println!("  Logs:       {}", paths.log_file.display());
    }
    Ok(())
}

fn update() -> Result<(), String> {
    println!("OMP Bridge {BRIDGE_VERSION}");
    println!("This command is informational only; it does not update the binary.");
    println!("Use the official installer, which verifies the published SHA-256:");
    println!(
        "  curl -fsSL https://raw.githubusercontent.com/xeriomy/omp-mobile/main/scripts/install-bridge.sh | sh"
    );
    println!("If the bridge is running, stop it first: omp-bridge stop");
    println!("Then install and run `omp-bridge doctor`.");
    Ok(())
}

fn uninstall_command(
    paths: &ConfigPaths,
    purge: bool,
    keep_binary: bool,
    dry_run: bool,
    yes: bool,
) -> Result<(), String> {
    if !yes && !dry_run {
        return Err(
            "refusing to uninstall without --yes; run with --dry-run to inspect the changes"
                .to_owned(),
        );
    }
    uninstall(paths, purge, keep_binary, dry_run).map_err(|error| error.to_string())?;
    if !dry_run {
        println!("Uninstall complete.");
    }
    Ok(())
}

async fn run_child(
    paths: &ConfigPaths,
    overrides: &ConfigOverrides,
    args: RunArgs,
) -> Result<(), String> {
    let mut stored = load_config(paths)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "configuration disappeared before startup".to_owned())?;
    stored.apply_overrides(overrides);
    stored.validate().map_err(|error| error.to_string())?;
    let config = stored
        .to_bridge_config()
        .map_err(|error| error.to_string())?;
    run_background_child(paths, config, &args.startup_id, &args.handshake_file)
        .await
        .map_err(|error| error.to_string())
}

async fn legacy_foreground(paths: &ConfigPaths, overrides: &ConfigOverrides) -> Result<(), String> {
    let stored = match load_config(paths).map_err(|error| error.to_string())? {
        Some(mut stored) => {
            stored.apply_overrides(overrides);
            stored
        }
        None => {
            let mut stored = StoredConfig::new(
                overrides
                    .omp_bin
                    .clone()
                    .unwrap_or_else(|| PathBuf::from("omp")),
                overrides
                    .workspace
                    .clone()
                    .unwrap_or_else(|| PathBuf::from(".")),
            );
            stored.apply_overrides(overrides);
            stored
        }
    };
    stored.validate().map_err(|error| error.to_string())?;
    let config = stored
        .to_bridge_config()
        .map_err(|error| error.to_string())?;
    start_foreground(paths, config, &stored).await
}

fn workspace_for(
    existing: Option<&StoredConfig>,
    overrides: &ConfigOverrides,
) -> Result<PathBuf, String> {
    let workspace = overrides
        .workspace
        .clone()
        .or_else(|| existing.map(|config| config.workspace.clone()))
        .unwrap_or_else(|| PathBuf::from("."));
    let canonical = workspace
        .canonicalize()
        .map_err(|error| format!("cannot use workspace {}: {error}", workspace.display()))?;
    if !canonical.is_dir() {
        return Err(format!(
            "workspace {} is not a directory",
            canonical.display()
        ));
    }
    Ok(canonical)
}

fn format_platform() -> String {
    let platform = platform_info();
    format!(
        "{} {} / {} / {}",
        platform.os, platform.arch, platform.libc, platform.environment
    )
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
