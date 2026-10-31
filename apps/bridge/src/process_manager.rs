//! Background bridge lifecycle: detach, startup handshake, PID identity,
//! status, logs, stop, restart, and uninstall helpers.

use crate::api_types::{BRIDGE_VERSION, EnginePhase, EngineStatus};
use crate::config::BridgeConfig;
use crate::config_store::{
    ConfigOverrides, ConfigPaths, HandshakePhase, PidFile, RUNTIME_SCHEMA_VERSION,
    RuntimeEngineError, RuntimePhase, RuntimeState, StartupHandshake, load_handshake,
    load_pid_file, load_runtime_state, random_id, remove_if_exists, save_handshake, save_pid_file,
    save_runtime_state, unix_millis,
};
use crate::discovery::{PlatformInfo, platform_info};
use crate::server::RunningBridge;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::process::{Child, Command};
use tokio::time::{sleep, timeout};

const STOP_TIMEOUT: Duration = Duration::from_secs(15);
const HANDSHAKE_GRACE: Duration = Duration::from_secs(15);

#[derive(Debug)]
pub enum ProcessError {
    AlreadyRunning(u32),
    NotRunning,
    NotConfigured,
    IdentityMismatch(String),
    Timeout(String),
    StartupFailed(String),
    Io(io::Error),
    Config(String),
}

impl std::fmt::Display for ProcessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyRunning(pid) => {
                write!(f, "omp-bridge is already running with PID {pid}")
            }
            Self::NotRunning => write!(f, "omp-bridge is not running"),
            Self::NotConfigured => write!(
                f,
                "omp-bridge is not configured; run `omp-bridge setup` first"
            ),
            Self::IdentityMismatch(message) => write!(f, "{message}"),
            Self::Timeout(message) => write!(f, "{message}"),
            Self::StartupFailed(message) => write!(f, "startup failed: {message}"),
            Self::Io(error) => write!(f, "{error}"),
            Self::Config(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for ProcessError {}

impl From<io::Error> for ProcessError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<crate::config_store::ConfigStoreError> for ProcessError {
    fn from(value: crate::config_store::ConfigStoreError) -> Self {
        Self::Config(value.to_string())
    }
}

#[derive(Debug, Clone)]
pub struct StartResult {
    pub pid: u32,
    pub listen: String,
    pub pairing_code: String,
    pub log_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct StopResult {
    pub pid: u32,
    pub forced: bool,
    pub stale: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusPhase {
    NotConfigured,
    Stopped,
    Starting,
    Running,
    EngineReady,
    EngineFailed,
    OmpExited,
    BridgeUnhealthy,
}

impl std::fmt::Display for StatusPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::NotConfigured => "not configured",
            Self::Stopped => "stopped",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::EngineReady => "engine ready",
            Self::EngineFailed => "engine failed",
            Self::OmpExited => "OMP process exited",
            Self::BridgeUnhealthy => "bridge unhealthy",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusReport {
    pub phase: StatusPhase,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listen: Option<String>,
    pub bridge_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub omp_bin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub omp_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    pub engine_state: String,
    pub engine_ready: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine_error: Option<RuntimeEngineError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    pub log_path: String,
    pub platform: PlatformInfo,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessIdentity {
    pub pid: u32,
    pub start_time: Option<u64>,
    pub exe: Option<PathBuf>,
}

pub fn identity_for(pid: u32) -> Option<ProcessIdentity> {
    if pid == 0 || !process_alive(pid) {
        return None;
    }
    let start_time = read_proc_start_time(pid);
    let exe = fs::read_link(format!("/proc/{pid}/exe")).ok();
    Some(ProcessIdentity {
        pid,
        start_time,
        exe,
    })
}

pub fn process_alive(pid: u32) -> bool {
    if pid == 0 || pid > i32::MAX as u32 {
        return false;
    }
    #[cfg(unix)]
    {
        // SAFETY: signal 0 performs an existence check.
        let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
        if result == 0 {
            return true;
        }
        io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
    #[cfg(not(unix))]
    {
        false
    }
}

fn read_proc_start_time(pid: u32) -> Option<u64> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let close = stat.rfind(')')?;
    let fields: Vec<&str> = stat[close + 1..].split_whitespace().collect();
    // After comm/state, field 22 (starttime) is index 19 in this slice.
    fields.get(19)?.parse().ok()
}

fn identity_matches(
    identity: &ProcessIdentity,
    expected_exe: &str,
    expected_start_time: Option<u64>,
) -> bool {
    if let Some(expected) = expected_start_time
        && identity.start_time != Some(expected)
    {
        return false;
    }
    let expected = Path::new(expected_exe);
    match &identity.exe {
        Some(exe) => {
            if exe == expected {
                return true;
            }
            match (exe.canonicalize(), expected.canonicalize()) {
                (Ok(exe), Ok(expected)) => exe == expected,
                _ => false,
            }
        }
        None => expected_start_time.is_some(),
    }
}

/// Exclusive runtime lock held for the lifetime of the background bridge.
pub struct RuntimeLock {
    _file: File,
}

impl RuntimeLock {
    pub fn acquire(path: &Path) -> io::Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
            set_private_dir(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(path)?;
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            // SAFETY: flock on a valid, open file descriptor.
            let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
            if result != 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(Self { _file: file })
    }
}

pub async fn start_background(
    paths: &ConfigPaths,
    bridge_exe: &Path,
    overrides: &ConfigOverrides,
    startup_timeout: Duration,
    log_max_bytes: u64,
) -> Result<StartResult, ProcessError> {
    paths.ensure()?;
    if let Some(pid) = running_pid(paths) {
        return Err(ProcessError::AlreadyRunning(pid));
    }

    let log = prepare_log(&paths.log_file, log_max_bytes)?;
    let stdout = log.try_clone()?;
    let stderr = log.try_clone()?;
    let startup_id = random_id();
    let handshake_path = paths.startup_file(&startup_id);
    let _ = remove_if_exists(&handshake_path);

    let mut command = Command::new(bridge_exe);
    command
        .arg("run")
        .arg("--startup-id")
        .arg(&startup_id)
        .arg("--handshake-file")
        .arg(&handshake_path)
        .env("OMP_BRIDGE_HOME", &paths.home)
        .env("OMP_BRIDGE_RUNTIME_DIR", &paths.runtime_dir)
        .env("OMP_BRIDGE_LOG_DIR", &paths.log_dir);
    if let Some(value) = &overrides.omp_bin {
        command.arg("--omp-bin").arg(value);
    }
    if let Some(value) = &overrides.workspace {
        command.arg("--workspace").arg(value);
    }
    if let Some(value) = &overrides.model {
        command.arg("--model").arg(value);
    }
    if let Some(value) = &overrides.listen {
        command.arg("--listen").arg(value);
    }
    if let Some(value) = &overrides.allowed_origins {
        command.arg("--allowed-origins").arg(value.join(","));
    }
    for (flag, value) in [
        ("--startup-timeout-secs", overrides.startup_timeout_secs),
        ("--request-timeout-secs", overrides.request_timeout_secs),
        ("--pairing-ttl-secs", overrides.pairing_ttl_secs),
        ("--token-ttl-secs", overrides.token_ttl_secs),
    ] {
        if let Some(value) = value {
            command.arg(flag).arg(value.to_string());
        }
    }
    if let Some(value) = overrides.max_frame_bytes {
        command.arg("--max-frame-bytes").arg(value.to_string());
    }
    if let Some(value) = overrides.max_reassembled_bytes {
        command
            .arg("--max-reassembled-bytes")
            .arg(value.to_string());
    }
    if let Some(value) = overrides.log_max_bytes {
        command.arg("--log-max-bytes").arg(value.to_string());
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    #[cfg(unix)]
    {
        // SAFETY: setsid is async-signal-safe and only detaches the child.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    command.kill_on_drop(false);

    let mut child = command
        .spawn()
        .map_err(|error| ProcessError::StartupFailed(format!("cannot spawn bridge: {error}")))?;
    let child_pid = child
        .id()
        .ok_or_else(|| ProcessError::StartupFailed("bridge child has no PID".to_owned()))?;

    let deadline = Instant::now() + startup_timeout + HANDSHAKE_GRACE;
    loop {
        if handshake_path.exists() {
            break;
        }
        if let Some(status) = child.try_wait()? {
            if handshake_path.exists() {
                break;
            }
            let _ = remove_if_exists(&handshake_path);
            return Err(ProcessError::StartupFailed(format!(
                "bridge exited before readiness (status {status})"
            )));
        }
        if Instant::now() >= deadline {
            terminate_child(&mut child).await;
            let _ = remove_if_exists(&handshake_path);
            return Err(ProcessError::Timeout(
                "bridge did not complete its startup handshake in time".to_owned(),
            ));
        }
        sleep(Duration::from_millis(100)).await;
    }

    let handshake = match read_handshake_checked(&handshake_path) {
        Ok(handshake) => handshake,
        Err(error) => {
            terminate_child(&mut child).await;
            let _ = remove_if_exists(&handshake_path);
            return Err(error);
        }
    };
    let _ = remove_if_exists(&handshake_path);

    if handshake.startup_id != startup_id || handshake.bridge_pid != child_pid {
        terminate_child(&mut child).await;
        return Err(ProcessError::StartupFailed(
            "startup handshake identity mismatch".to_owned(),
        ));
    }
    if handshake.phase == HandshakePhase::Failed || !handshake.engine_ready {
        terminate_child(&mut child).await;
        let code = handshake.error_code.as_deref().unwrap_or("STARTUP_FAILED");
        let message = handshake
            .error_message
            .as_deref()
            .unwrap_or("bridge did not become ready");
        return Err(ProcessError::StartupFailed(format!("{code}: {message}")));
    }
    let Some(pairing_code) = handshake.pairing_code else {
        terminate_child(&mut child).await;
        return Err(ProcessError::StartupFailed(
            "bridge did not deliver a pairing code".to_owned(),
        ));
    };
    let Some(listen) = handshake.listen else {
        terminate_child(&mut child).await;
        return Err(ProcessError::StartupFailed(
            "bridge did not report its listen address".to_owned(),
        ));
    };
    let address: SocketAddr = listen.parse().map_err(|_| {
        ProcessError::StartupFailed("bridge reported an invalid address".to_owned())
    })?;
    if !address.ip().is_loopback() {
        terminate_child(&mut child).await;
        return Err(ProcessError::StartupFailed(
            "bridge reported a non-loopback address".to_owned(),
        ));
    }

    Ok(StartResult {
        pid: child_pid,
        listen,
        pairing_code,
        log_path: paths.log_file.clone(),
    })
}

fn read_handshake_checked(path: &Path) -> Result<StartupHandshake, ProcessError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(path)?.permissions().mode();
        if mode & 0o077 != 0 {
            return Err(ProcessError::StartupFailed(
                "startup handshake file permissions are not private".to_owned(),
            ));
        }
    }
    load_handshake(path).map_err(|error| ProcessError::StartupFailed(error.to_string()))
}

pub async fn run_background_child(
    paths: &ConfigPaths,
    config: BridgeConfig,
    startup_id: &str,
    handshake_path: &Path,
) -> Result<(), ProcessError> {
    paths.ensure()?;
    let pid = std::process::id();
    let identity = identity_for(pid);
    let started_at = unix_millis();

    let _lock = match RuntimeLock::acquire(&paths.lock_file) {
        Ok(lock) => lock,
        Err(error) => {
            write_failed_handshake(
                handshake_path,
                startup_id,
                pid,
                "ALREADY_RUNNING",
                &format!("another bridge holds the runtime lock: {error}"),
            );
            return Err(ProcessError::AlreadyRunning(pid));
        }
    };

    save_pid_file(
        paths,
        &PidFile {
            schema_version: RUNTIME_SCHEMA_VERSION,
            pid,
            process_start_time: identity.as_ref().and_then(|identity| identity.start_time),
            exe: std::env::current_exe()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|_| "omp-bridge".to_owned()),
            instance_id: startup_id.to_owned(),
            started_at,
        },
    )?;

    let mut state = base_state(
        startup_id,
        pid,
        identity.as_ref(),
        &config,
        started_at,
        RuntimePhase::Starting,
        None,
    );
    save_runtime_state(paths, &state)?;
    log_line(
        "INFO",
        &format!("starting bridge pid {pid} on {}", config.listen),
    );
    log_line(
        "INFO",
        &format!(
            "OMP {} (cwd {})",
            config.omp_bin.display(),
            config.workspace.display()
        ),
    );

    let bridge = match RunningBridge::start(config.clone()).await {
        Ok(bridge) => bridge,
        Err(error) => {
            let message = format!("cannot bind loopback listener: {error}");
            log_line("ERROR", &message);
            write_failed_handshake(
                handshake_path,
                startup_id,
                pid,
                "BRIDGE_BIND_FAILED",
                &message,
            );
            state.phase = RuntimePhase::Failed;
            state.last_error = Some(message.clone());
            state.updated_at = unix_millis();
            let _ = save_runtime_state(paths, &state);
            let _ = remove_if_exists(&paths.pid_file);
            return Err(ProcessError::StartupFailed(message));
        }
    };
    let engine = bridge.engine.clone();
    let status = engine.status();
    if !status.ready {
        let (code, message) = status
            .error
            .as_ref()
            .map(|error| (error.code.clone(), error.message.clone()))
            .unwrap_or_else(|| {
                (
                    "ENGINE_NOT_READY".to_owned(),
                    "OMP did not become ready".to_owned(),
                )
            });
        log_line("ERROR", &format!("{code}: {message}"));
        write_failed_handshake(handshake_path, startup_id, pid, &code, &message);
        state.phase = RuntimePhase::Failed;
        state.engine_state = engine_state(&status);
        state.engine_ready = false;
        state.engine_error = Some(RuntimeEngineError {
            code: code.clone(),
            message: message.clone(),
        });
        state.last_error = Some(format!("{code}: {message}"));
        state.updated_at = unix_millis();
        let _ = save_runtime_state(paths, &state);
        bridge.shutdown().await;
        let _ = remove_if_exists(&paths.pid_file);
        return Err(ProcessError::StartupFailed(format!("{code}: {message}")));
    }

    state = state_from_engine(
        startup_id,
        pid,
        identity.as_ref(),
        &config,
        started_at,
        &status,
        engine.pid(),
        Some(bridge.addr.to_string()),
        RuntimePhase::Running,
        None,
    );
    state.omp_version = omp_version_for(paths);
    save_runtime_state(paths, &state)?;
    let handshake = StartupHandshake {
        schema_version: RUNTIME_SCHEMA_VERSION,
        startup_id: startup_id.to_owned(),
        phase: HandshakePhase::Ready,
        bridge_pid: pid,
        listen: Some(bridge.addr.to_string()),
        pairing_code: Some(bridge.pairing_code.clone()),
        engine_ready: true,
        error_code: None,
        error_message: None,
    };
    save_handshake(handshake_path, &handshake)?;
    log_line(
        "INFO",
        "pairing code delivered through the private startup handshake",
    );

    let state_task = spawn_state_writer(
        paths.clone(),
        state.clone(),
        engine.clone(),
        omp_version_for(paths),
    );

    let shutdown = wait_for_shutdown_signal().await;
    if let Err(error) = shutdown {
        log_line("ERROR", &format!("shutdown signal handler failed: {error}"));
    }
    log_line("INFO", "stopping bridge");
    state.phase = RuntimePhase::Stopping;
    state.updated_at = unix_millis();
    let _ = save_runtime_state(paths, &state);
    state_task.abort();
    bridge.shutdown().await;

    let final_status = engine.status();
    let mut stopped = state_from_engine(
        startup_id,
        pid,
        identity.as_ref(),
        &config,
        started_at,
        &final_status,
        engine.pid(),
        None,
        RuntimePhase::Stopped,
        None,
    );
    stopped.updated_at = unix_millis();
    stopped.engine_ready = false;
    stopped.engine_state = "stopped".to_owned();
    stopped.engine_error = None;
    stopped.last_error = None;
    stopped.omp_pid = None;
    stopped.omp_start_time = None;
    stopped.omp_version = state.omp_version.clone();
    let _ = save_runtime_state(paths, &stopped);
    let _ = remove_if_exists(&paths.pid_file);
    let _ = remove_if_exists(handshake_path);
    log_line("INFO", "bridge stopped");
    Ok(())
}

fn spawn_state_writer(
    paths: ConfigPaths,
    mut state: RuntimeState,
    engine: crate::engine::Engine,
    omp_version: Option<String>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            sleep(Duration::from_secs(2)).await;
            let status = engine.status();
            state.phase = RuntimePhase::Running;
            state.engine_ready = status.ready;
            state.engine_state = engine_state(&status);
            state.engine_error = status.error.as_ref().map(|error| RuntimeEngineError {
                code: error.code.clone(),
                message: error.message.clone(),
            });
            state.omp_pid = engine.pid();
            state.omp_start_time = state.omp_pid.and_then(read_proc_start_time);
            state.model = status
                .summary
                .as_ref()
                .and_then(|summary| summary.model.as_ref())
                .map(|model| format!("{}/{}", model.provider, model.id));
            state.updated_at = unix_millis();
            state.omp_version = omp_version.clone();
            let _ = save_runtime_state(&paths, &state);
            if matches!(status.state, EnginePhase::Exited) {
                break;
            }
        }
    })
}

fn omp_version_for(paths: &ConfigPaths) -> Option<String> {
    crate::config_store::load_config(paths)
        .ok()
        .flatten()
        .and_then(|config| config.omp_version)
}

fn base_state(
    startup_id: &str,
    pid: u32,
    identity: Option<&ProcessIdentity>,
    config: &BridgeConfig,
    started_at: u64,
    phase: RuntimePhase,
    last_error: Option<String>,
) -> RuntimeState {
    RuntimeState {
        schema_version: RUNTIME_SCHEMA_VERSION,
        instance_id: startup_id.to_owned(),
        phase,
        pid,
        process_start_time: identity.and_then(|identity| identity.start_time),
        listen: Some(config.listen.to_string()),
        bridge_version: BRIDGE_VERSION.to_owned(),
        omp_bin: config.omp_bin.display().to_string(),
        omp_version: None,
        model: None,
        workspace: config.workspace.display().to_string(),
        engine_state: "starting".to_owned(),
        engine_ready: false,
        engine_error: None,
        omp_pid: None,
        omp_start_time: None,
        started_at,
        updated_at: unix_millis(),
        last_error,
    }
}

#[allow(clippy::too_many_arguments)]
fn state_from_engine(
    startup_id: &str,
    pid: u32,
    identity: Option<&ProcessIdentity>,
    config: &BridgeConfig,
    started_at: u64,
    status: &EngineStatus,
    omp_pid: Option<u32>,
    listen: Option<String>,
    phase: RuntimePhase,
    last_error: Option<String>,
) -> RuntimeState {
    let omp_start_time = omp_pid.and_then(read_proc_start_time);
    let model = status
        .summary
        .as_ref()
        .and_then(|summary| summary.model.as_ref())
        .map(|model| format!("{}/{}", model.provider, model.id));
    RuntimeState {
        schema_version: RUNTIME_SCHEMA_VERSION,
        instance_id: startup_id.to_owned(),
        phase,
        pid,
        process_start_time: identity.and_then(|identity| identity.start_time),
        listen,
        bridge_version: BRIDGE_VERSION.to_owned(),
        omp_bin: config.omp_bin.display().to_string(),
        omp_version: None,
        model,
        workspace: config.workspace.display().to_string(),
        engine_state: engine_state(status),
        engine_ready: status.ready,
        engine_error: status.error.as_ref().map(|error| RuntimeEngineError {
            code: error.code.clone(),
            message: error.message.clone(),
        }),
        omp_pid,
        omp_start_time,
        started_at,
        updated_at: unix_millis(),
        last_error: last_error.or_else(|| status.error.as_ref().map(|error| error.message.clone())),
    }
}

fn engine_state(status: &EngineStatus) -> String {
    match status.state {
        EnginePhase::Starting => "starting",
        EnginePhase::Ready => "ready",
        EnginePhase::Failed => "failed",
        EnginePhase::Exited => "exited",
    }
    .to_owned()
}

fn write_failed_handshake(path: &Path, startup_id: &str, pid: u32, code: &str, message: &str) {
    let handshake = StartupHandshake {
        schema_version: RUNTIME_SCHEMA_VERSION,
        startup_id: startup_id.to_owned(),
        phase: HandshakePhase::Failed,
        bridge_pid: pid,
        listen: None,
        pairing_code: None,
        engine_ready: false,
        error_code: Some(code.to_owned()),
        error_message: Some(message.to_owned()),
    };
    let _ = save_handshake(path, &handshake);
}

async fn wait_for_shutdown_signal() -> io::Result<()> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut terminate = signal(SignalKind::terminate())?;
        let mut interrupt = signal(SignalKind::interrupt())?;
        tokio::select! {
            _ = terminate.recv() => {}
            _ = interrupt.recv() => {}
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await
    }
}

pub fn status(paths: &ConfigPaths, configured: bool) -> StatusReport {
    let platform = platform_info();
    let state = load_runtime_state(paths);
    let pid_file = load_pid_file(paths);
    let log_path = paths.log_file.display().to_string();

    if !configured {
        return StatusReport {
            phase: StatusPhase::NotConfigured,
            pid: None,
            listen: None,
            bridge_version: BRIDGE_VERSION.to_owned(),
            omp_bin: None,
            omp_version: None,
            model: None,
            workspace: None,
            engine_state: "unknown".to_owned(),
            engine_ready: false,
            engine_error: None,
            started_at: None,
            last_error: None,
            log_path,
            platform,
        };
    }

    let Some(pid_file) = pid_file else {
        return StatusReport {
            phase: StatusPhase::Stopped,
            pid: None,
            listen: state.as_ref().and_then(|state| state.listen.clone()),
            bridge_version: state
                .as_ref()
                .map(|state| state.bridge_version.clone())
                .unwrap_or_else(|| BRIDGE_VERSION.to_owned()),
            omp_bin: state.as_ref().map(|state| state.omp_bin.clone()),
            omp_version: state.as_ref().and_then(|state| state.omp_version.clone()),
            model: state.as_ref().and_then(|state| state.model.clone()),
            workspace: state.as_ref().map(|state| state.workspace.clone()),
            engine_state: state
                .as_ref()
                .map(|state| state.engine_state.clone())
                .unwrap_or_else(|| "stopped".to_owned()),
            engine_ready: false,
            engine_error: state.as_ref().and_then(|state| state.engine_error.clone()),
            started_at: state.as_ref().map(|state| state.started_at),
            last_error: state.as_ref().and_then(|state| state.last_error.clone()),
            log_path,
            platform,
        };
    };

    let identity = identity_for(pid_file.pid);
    let matches = identity.as_ref().is_some_and(|identity| {
        identity_matches(identity, &pid_file.exe, pid_file.process_start_time)
    });
    if !matches {
        return StatusReport {
            phase: StatusPhase::Stopped,
            pid: None,
            listen: state.as_ref().and_then(|state| state.listen.clone()),
            bridge_version: BRIDGE_VERSION.to_owned(),
            omp_bin: state.as_ref().map(|state| state.omp_bin.clone()),
            omp_version: state.as_ref().and_then(|state| state.omp_version.clone()),
            model: state.as_ref().and_then(|state| state.model.clone()),
            workspace: state.as_ref().map(|state| state.workspace.clone()),
            engine_state: "stopped".to_owned(),
            engine_ready: false,
            engine_error: None,
            started_at: state.as_ref().map(|state| state.started_at),
            last_error: Some(
                "stale PID file ignored; the recorded process is gone or was reused".to_owned(),
            ),
            log_path,
            platform,
        };
    }

    let listen = state
        .as_ref()
        .and_then(|state| state.listen.clone())
        .or_else(|| Some(format!("127.0.0.1:{}", 8765)));
    let healthy = listen.as_deref().map(probe_health).unwrap_or(false);
    let engine_state = state
        .as_ref()
        .map(|state| state.engine_state.clone())
        .unwrap_or_else(|| "unknown".to_owned());
    let phase = if state
        .as_ref()
        .is_some_and(|state| state.phase == RuntimePhase::Starting)
    {
        StatusPhase::Starting
    } else if !healthy {
        StatusPhase::BridgeUnhealthy
    } else if engine_state == "ready" {
        StatusPhase::EngineReady
    } else if engine_state == "failed" {
        StatusPhase::EngineFailed
    } else if engine_state == "exited" {
        StatusPhase::OmpExited
    } else {
        StatusPhase::Running
    };

    StatusReport {
        phase,
        pid: Some(pid_file.pid),
        listen,
        bridge_version: state
            .as_ref()
            .map(|state| state.bridge_version.clone())
            .unwrap_or_else(|| BRIDGE_VERSION.to_owned()),
        omp_bin: state.as_ref().map(|state| state.omp_bin.clone()),
        omp_version: state.as_ref().and_then(|state| state.omp_version.clone()),
        model: state.as_ref().and_then(|state| state.model.clone()),
        workspace: state.as_ref().map(|state| state.workspace.clone()),
        engine_state,
        engine_ready: state.as_ref().is_some_and(|state| state.engine_ready),
        engine_error: state.as_ref().and_then(|state| state.engine_error.clone()),
        started_at: state.as_ref().map(|state| state.started_at),
        last_error: state.as_ref().and_then(|state| state.last_error.clone()),
        log_path,
        platform,
    }
}

pub async fn stop(paths: &ConfigPaths) -> Result<StopResult, ProcessError> {
    let Some(pid_file) = load_pid_file(paths) else {
        let _ = remove_if_exists(&paths.state_file);
        return Err(ProcessError::NotRunning);
    };
    let identity = identity_for(pid_file.pid);
    let Some(identity) = identity else {
        cleanup_orphaned_omp(paths).await;
        let _ = remove_if_exists(&paths.pid_file);
        let _ = remove_if_exists(&paths.state_file);
        return Ok(StopResult {
            pid: pid_file.pid,
            forced: false,
            stale: true,
        });
    };
    if !identity_matches(&identity, &pid_file.exe, pid_file.process_start_time) {
        cleanup_orphaned_omp(paths).await;
        let _ = remove_if_exists(&paths.pid_file);
        let _ = remove_if_exists(&paths.state_file);
        return Ok(StopResult {
            pid: pid_file.pid,
            forced: false,
            stale: true,
        });
    }

    send_signal(pid_file.pid, libc::SIGTERM)?;
    let mut forced = false;
    if !wait_for_process_exit(pid_file.pid, STOP_TIMEOUT).await {
        forced = true;
        send_signal(pid_file.pid, libc::SIGKILL)?;
        let _ = wait_for_process_exit(pid_file.pid, Duration::from_secs(3)).await;
    }
    cleanup_orphaned_omp(paths).await;
    let _ = remove_if_exists(&paths.pid_file);
    if let Some(mut state) = load_runtime_state(paths) {
        state.phase = RuntimePhase::Stopped;
        state.engine_ready = false;
        state.engine_state = "stopped".to_owned();
        state.engine_error = None;
        state.last_error = None;
        state.omp_pid = None;
        state.omp_start_time = None;
        state.updated_at = unix_millis();
        let _ = save_runtime_state(paths, &state);
    }
    Ok(StopResult {
        pid: pid_file.pid,
        forced,
        stale: false,
    })
}

async fn cleanup_orphaned_omp(paths: &ConfigPaths) {
    let Some(state) = load_runtime_state(paths) else {
        return;
    };
    let Some(omp_pid) = state.omp_pid else {
        return;
    };
    let Some(identity) = identity_for(omp_pid) else {
        return;
    };
    let Some(start_time) = state.omp_start_time else {
        return;
    };
    if identity.start_time != Some(start_time) {
        return;
    }
    let Some(exe) = identity.exe.as_ref() else {
        return;
    };
    if exe.canonicalize().ok() != Path::new(&state.omp_bin).canonicalize().ok() {
        return;
    }
    let _ = send_signal(omp_pid, libc::SIGTERM);
    if !wait_for_process_exit(omp_pid, Duration::from_secs(3)).await {
        let _ = send_signal(omp_pid, libc::SIGKILL);
    }
}

fn send_signal(pid: u32, signal: i32) -> Result<(), ProcessError> {
    if pid == 0 || pid > i32::MAX as u32 {
        return Err(ProcessError::IdentityMismatch(
            "refusing to signal an invalid PID".to_owned(),
        ));
    }
    #[cfg(unix)]
    {
        // SAFETY: the caller verified process identity before signalling.
        let result = unsafe { libc::kill(pid as libc::pid_t, signal) };
        if result == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            return Ok(());
        }
        Err(ProcessError::Io(error))
    }
    #[cfg(not(unix))]
    {
        let _ = signal;
        Err(ProcessError::IdentityMismatch(
            "process signalling is not supported on this platform".to_owned(),
        ))
    }
}

async fn wait_for_process_exit(pid: u32, limit: Duration) -> bool {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        if !process_alive(pid) {
            return true;
        }
        sleep(Duration::from_millis(50)).await;
    }
    !process_alive(pid)
}

async fn terminate_child(child: &mut Child) {
    #[cfg(unix)]
    if let Some(pid) = child.id() {
        // SAFETY: this is the direct child we spawned.
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGTERM);
        }
    }
    if timeout(Duration::from_secs(3), child.wait()).await.is_err() {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
}

pub fn running_pid(paths: &ConfigPaths) -> Option<u32> {
    let pid_file = load_pid_file(paths)?;
    let identity = identity_for(pid_file.pid)?;
    if identity_matches(&identity, &pid_file.exe, pid_file.process_start_time) {
        Some(pid_file.pid)
    } else {
        None
    }
}

pub fn format_status(report: &StatusReport) -> String {
    let mut output = String::new();
    output.push_str(&format!("OMP Bridge status: {}\n", report.phase));
    if let Some(pid) = report.pid {
        output.push_str(&format!("  PID:        {pid}\n"));
    }
    if let Some(listen) = &report.listen {
        output.push_str(&format!("  Listen:     {listen}\n"));
    }
    output.push_str(&format!("  Bridge:     {}\n", report.bridge_version));
    if let Some(omp) = &report.omp_bin {
        output.push_str(&format!("  OMP:        {omp}\n"));
    }
    if let Some(version) = &report.omp_version {
        output.push_str(&format!("  OMP version:{version}\n"));
    }
    output.push_str(&format!("  Engine:     {}", report.engine_state));
    if report.engine_ready {
        output.push_str(" (ready)");
    }
    output.push('\n');
    if let Some(model) = &report.model {
        output.push_str(&format!("  Model:      {model}\n"));
    }
    if let Some(workspace) = &report.workspace {
        output.push_str(&format!("  Workspace:  {workspace}\n"));
    }
    if let Some(started_at) = report.started_at {
        output.push_str(&format!("  Started:    {}\n", format_timestamp(started_at)));
    }
    if let Some(error) = &report.engine_error {
        output.push_str(&format!(
            "  Error:      {}: {}\n",
            error.code, error.message
        ));
    } else if let Some(error) = &report.last_error {
        output.push_str(&format!("  Last error: {error}\n"));
    }
    output.push_str(&format!("  Logs:       {}\n", report.log_path));
    output.push_str(&format!(
        "  Platform:   {} {} / {} / {}\n",
        report.platform.os, report.platform.arch, report.platform.libc, report.platform.environment
    ));
    output
}

pub async fn print_logs(paths: &ConfigPaths, lines: usize, follow: bool) -> io::Result<()> {
    if !paths.log_file.exists() {
        println!("No bridge log exists yet at {}", paths.log_file.display());
        return Ok(());
    }
    let mut file = File::open(&paths.log_file)?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;
    let tail: Vec<&str> = content.lines().rev().take(lines).collect();
    for line in tail.into_iter().rev() {
        println!("{line}");
    }
    if !follow {
        return Ok(());
    }
    let mut offset = file.metadata()?.len();
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            _ = sleep(Duration::from_millis(250)) => {
                let metadata = match fs::metadata(&paths.log_file) {
                    Ok(metadata) => metadata,
                    Err(_) => continue,
                };
                if metadata.len() < offset {
                    offset = 0;
                }
                if metadata.len() == offset {
                    continue;
                }
                let mut file = File::open(&paths.log_file)?;
                file.seek(SeekFrom::Start(offset))?;
                let mut buffer = String::new();
                file.read_to_string(&mut buffer)?;
                print!("{buffer}");
                let _ = io::stdout().flush();
                offset = metadata.len();
            }
        }
    }
    Ok(())
}

pub fn uninstall(
    paths: &ConfigPaths,
    purge: bool,
    keep_binary: bool,
    dry_run: bool,
) -> Result<(), ProcessError> {
    if let Some(pid) = running_pid(paths) {
        return Err(ProcessError::AlreadyRunning(pid));
    }
    let binary = std::env::current_exe().ok();
    let mut targets = vec![
        paths.pid_file.clone(),
        paths.state_file.clone(),
        paths.lock_file.clone(),
    ];
    if let Ok(entries) = fs::read_dir(&paths.runtime_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            if name.to_string_lossy().starts_with("startup-") {
                targets.push(entry.path());
            }
        }
    }
    for target in &targets {
        if dry_run {
            println!("would remove {}", target.display());
        } else {
            let _ = remove_if_exists(target);
        }
    }
    if paths.log_dir.exists() {
        if dry_run {
            println!("would remove {}", paths.log_dir.display());
        } else {
            let _ = fs::remove_dir_all(&paths.log_dir);
        }
    }
    if purge {
        if dry_run {
            println!("would remove {}", paths.home.display());
        } else {
            let _ = fs::remove_dir_all(&paths.home);
        }
    } else {
        println!(
            "Configuration kept at {}. Use --purge to remove it.",
            paths.home.display()
        );
    }
    if !keep_binary && let Some(binary) = binary {
        if dry_run {
            println!("would remove {}", binary.display());
        } else {
            let _ = remove_if_exists(&binary);
        }
    }
    Ok(())
}

fn probe_health(listen: &str) -> bool {
    let Ok(address) = listen.parse::<SocketAddr>() else {
        return false;
    };
    let Ok(mut stream) = TcpStream::connect_timeout(&address, Duration::from_millis(500)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
    let host = match address {
        SocketAddr::V4(_) => format!("127.0.0.1:{}", address.port()),
        SocketAddr::V6(_) => format!("[::1]:{}", address.port()),
    };
    let request = format!("GET /healthz HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }
    let mut response = String::new();
    let _ = stream.read_to_string(&mut response);
    response.starts_with("HTTP/1.1 200") || response.starts_with("HTTP/1.0 200")
}

fn prepare_log(path: &Path, max_bytes: u64) -> io::Result<File> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
        set_private_dir(parent)?;
    }
    if let Ok(metadata) = fs::metadata(path)
        && metadata.len() >= max_bytes
    {
        let rotated = path.with_extension("log.1");
        let _ = remove_if_exists(&rotated);
        let _ = fs::rename(path, rotated);
    }
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

fn set_private_dir(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

pub fn log_line(level: &str, message: &str) {
    println!("{} {level:<5} {message}", format_timestamp(unix_millis()));
}

fn format_timestamp(millis: u64) -> String {
    let seconds = millis / 1000;
    let days = (seconds / 86_400) as i64;
    let seconds_of_day = seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3600;
    let minute = (seconds_of_day % 3600) / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn civil_from_days(days_since_epoch: i64) -> (i64, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    let year = year + i64::from(month <= 2);
    (year, month as u32, day as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_formats_epoch() {
        assert_eq!(format_timestamp(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_timestamp(1_700_000_000_000), "2023-11-14T22:13:20Z");
    }

    #[test]
    fn pid_identity_rejects_reused_pid_shape() {
        let identity = ProcessIdentity {
            pid: 123,
            start_time: Some(10),
            exe: Some(PathBuf::from("/tmp/omp-bridge")),
        };
        assert!(!identity_matches(&identity, "/tmp/omp-bridge", Some(11)));
        assert!(!identity_matches(&identity, "/tmp/other", Some(10)));
    }

    #[test]
    fn current_process_identity_is_available() {
        let identity = identity_for(std::process::id()).expect("current process");
        assert_eq!(identity.pid, std::process::id());
        assert!(identity.start_time.is_some());
    }

    #[test]
    fn handshake_schema_rejects_future_versions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("handshake.json");
        fs::write(
            &path,
            r#"{"schemaVersion":99,"startupId":"x","phase":"ready","bridgePid":1,"engineReady":true}"#,
        )
        .unwrap();
        assert!(load_handshake(&path).is_err());
    }
}
