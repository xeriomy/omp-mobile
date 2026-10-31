//! Persistent user configuration and runtime files.
//!
//! Configuration never contains provider credentials, bearer tokens, or pairing
//! codes. Runtime files live in a separate directory with restrictive
//! permissions where the platform supports them.

use crate::config::{
    BridgeConfig, DEFAULT_ALLOWED_ORIGINS, DEFAULT_LISTEN, DEFAULT_MAX_FRAME_BYTES,
    DEFAULT_MAX_REASSEMBLED_BYTES, DEFAULT_PAIRING_TTL_SECS, DEFAULT_REQUEST_TIMEOUT_SECS,
    DEFAULT_STARTUP_TIMEOUT_SECS, DEFAULT_TOKEN_TTL_SECS,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

pub const CONFIG_SCHEMA_VERSION: u32 = 1;
pub const RUNTIME_SCHEMA_VERSION: u32 = 1;
pub const DEFAULT_LOG_MAX_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone)]
pub struct ConfigPaths {
    pub home: PathBuf,
    pub config_file: PathBuf,
    pub runtime_dir: PathBuf,
    pub log_dir: PathBuf,
    pub log_file: PathBuf,
    pub pid_file: PathBuf,
    pub state_file: PathBuf,
    pub lock_file: PathBuf,
}

impl ConfigPaths {
    pub fn discover() -> Result<Self, ConfigStoreError> {
        let home = match std::env::var_os("OMP_BRIDGE_HOME") {
            Some(value) => expand_home(PathBuf::from(value)),
            None => {
                let base = std::env::var_os("XDG_CONFIG_HOME")
                    .map(PathBuf::from)
                    .filter(|path| path.is_absolute())
                    .or_else(|| {
                        std::env::var_os("HOME")
                            .map(PathBuf::from)
                            .map(|home| home.join(".config"))
                    })
                    .ok_or(ConfigStoreError::HomeUnavailable)?;
                base.join("omp-bridge")
            }
        };
        let runtime = match std::env::var_os("OMP_BRIDGE_RUNTIME_DIR") {
            Some(value) => expand_home(PathBuf::from(value)),
            None => match std::env::var_os("XDG_RUNTIME_DIR") {
                Some(value) if !value.is_empty() => PathBuf::from(value).join("omp-bridge"),
                _ => home.join("run"),
            },
        };
        let log_dir = std::env::var_os("OMP_BRIDGE_LOG_DIR")
            .map(|value| expand_home(PathBuf::from(value)))
            .unwrap_or_else(|| home.join("logs"));
        Ok(Self::from_dirs(home, runtime, log_dir))
    }

    /// Deterministic paths for tests and embedders.
    pub fn from_home(home: impl Into<PathBuf>) -> Self {
        let home = home.into();
        Self::from_dirs(home.clone(), home.join("run"), home.join("logs"))
    }

    fn from_dirs(home: PathBuf, runtime_dir: PathBuf, log_dir: PathBuf) -> Self {
        Self {
            config_file: home.join("config.json"),
            pid_file: runtime_dir.join("bridge.pid"),
            state_file: runtime_dir.join("state.json"),
            lock_file: runtime_dir.join("bridge.lock"),
            log_file: log_dir.join("bridge.log"),
            home,
            runtime_dir,
            log_dir,
        }
    }

    pub fn ensure(&self) -> Result<(), ConfigStoreError> {
        create_private_dir(&self.home)?;
        create_private_dir(&self.runtime_dir)?;
        create_private_dir(&self.log_dir)?;
        Ok(())
    }

    pub fn startup_file(&self, startup_id: &str) -> PathBuf {
        self.runtime_dir.join(format!("startup-{startup_id}.json"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StoredConfig {
    pub schema_version: u32,
    pub omp_bin: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub omp_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub workspace: PathBuf,
    pub listen: String,
    pub allowed_origins: Vec<String>,
    pub startup_timeout_secs: u64,
    pub request_timeout_secs: u64,
    pub pairing_ttl_secs: u64,
    pub token_ttl_secs: u64,
    pub max_frame_bytes: usize,
    pub max_reassembled_bytes: usize,
    pub log_max_bytes: u64,
}

impl StoredConfig {
    pub fn new(omp_bin: PathBuf, workspace: PathBuf) -> Self {
        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            omp_bin,
            omp_version: None,
            model: None,
            workspace,
            listen: DEFAULT_LISTEN.to_owned(),
            allowed_origins: DEFAULT_ALLOWED_ORIGINS
                .split(',')
                .map(str::to_owned)
                .collect(),
            startup_timeout_secs: DEFAULT_STARTUP_TIMEOUT_SECS,
            request_timeout_secs: DEFAULT_REQUEST_TIMEOUT_SECS,
            pairing_ttl_secs: DEFAULT_PAIRING_TTL_SECS,
            token_ttl_secs: DEFAULT_TOKEN_TTL_SECS,
            max_frame_bytes: DEFAULT_MAX_FRAME_BYTES,
            max_reassembled_bytes: DEFAULT_MAX_REASSEMBLED_BYTES,
            log_max_bytes: DEFAULT_LOG_MAX_BYTES,
        }
    }

    pub fn to_bridge_config(&self) -> Result<BridgeConfig, crate::config::ConfigError> {
        let listen = self
            .listen
            .parse()
            .map_err(|_| crate::config::ConfigError::InvalidListen(self.listen.clone()))?;
        let mut omp_args = vec!["--mode".to_owned(), "rpc-ui".to_owned()];
        if let Some(model) = &self.model {
            omp_args.push("--model".to_owned());
            omp_args.push(model.clone());
        }
        BridgeConfig {
            omp_bin: self.omp_bin.clone(),
            omp_args,
            workspace: self.workspace.clone(),
            listen,
            allowed_origins: self.allowed_origins.clone(),
            startup_timeout: Duration::from_secs(self.startup_timeout_secs),
            request_timeout: Duration::from_secs(self.request_timeout_secs),
            pairing_ttl: Duration::from_secs(self.pairing_ttl_secs),
            token_ttl: Duration::from_secs(self.token_ttl_secs),
            max_frame_bytes: self.max_frame_bytes,
            max_reassembled_bytes: self.max_reassembled_bytes,
        }
        .validate()
    }

    pub fn validate(&self) -> Result<(), ConfigStoreError> {
        if self.schema_version != CONFIG_SCHEMA_VERSION {
            return Err(ConfigStoreError::Invalid(format!(
                "unsupported configuration schema version {}",
                self.schema_version
            )));
        }
        if self.omp_bin.as_os_str().is_empty() {
            return Err(ConfigStoreError::Invalid(
                "ompBin must not be empty".to_owned(),
            ));
        }
        if self.workspace.as_os_str().is_empty() {
            return Err(ConfigStoreError::Invalid(
                "workspace must not be empty".to_owned(),
            ));
        }
        let listen = self
            .listen
            .parse::<std::net::SocketAddr>()
            .map_err(|error| {
                ConfigStoreError::Invalid(format!("invalid listen address: {error}"))
            })?;
        if !listen.ip().is_loopback() {
            return Err(ConfigStoreError::Invalid(
                "listen address must be loopback; remote access is not implemented".to_owned(),
            ));
        }
        if self.allowed_origins.is_empty()
            || self
                .allowed_origins
                .iter()
                .any(|origin| origin.trim() == "*" || origin.trim().is_empty())
        {
            return Err(ConfigStoreError::Invalid(
                "allowedOrigins must be a non-empty exact allowlist".to_owned(),
            ));
        }
        for (name, value) in [
            ("startupTimeoutSecs", self.startup_timeout_secs),
            ("requestTimeoutSecs", self.request_timeout_secs),
            ("pairingTtlSecs", self.pairing_ttl_secs),
            ("tokenTtlSecs", self.token_ttl_secs),
            ("logMaxBytes", self.log_max_bytes),
        ] {
            if value == 0 {
                return Err(ConfigStoreError::Invalid(format!("{name} must be > 0")));
            }
        }
        if self.max_frame_bytes == 0 || self.max_frame_bytes > DEFAULT_MAX_FRAME_BYTES {
            return Err(ConfigStoreError::Invalid(format!(
                "maxFrameBytes must be between 1 and {DEFAULT_MAX_FRAME_BYTES}"
            )));
        }
        if self.max_reassembled_bytes == 0
            || self.max_reassembled_bytes > DEFAULT_MAX_REASSEMBLED_BYTES
        {
            return Err(ConfigStoreError::Invalid(format!(
                "maxReassembledBytes must be between 1 and {DEFAULT_MAX_REASSEMBLED_BYTES}"
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigOverrides {
    pub omp_bin: Option<PathBuf>,
    pub workspace: Option<PathBuf>,
    pub model: Option<String>,
    pub listen: Option<String>,
    pub allowed_origins: Option<Vec<String>>,
    pub startup_timeout_secs: Option<u64>,
    pub request_timeout_secs: Option<u64>,
    pub pairing_ttl_secs: Option<u64>,
    pub token_ttl_secs: Option<u64>,
    pub max_frame_bytes: Option<usize>,
    pub max_reassembled_bytes: Option<usize>,
    pub log_max_bytes: Option<u64>,
}

impl ConfigOverrides {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

impl StoredConfig {
    pub fn apply_overrides(&mut self, overrides: &ConfigOverrides) {
        if let Some(value) = &overrides.omp_bin {
            self.omp_bin = value.clone();
        }
        if let Some(value) = &overrides.workspace {
            self.workspace = value.clone();
        }
        if let Some(value) = &overrides.model {
            self.model = Some(value.clone());
        }
        if let Some(value) = &overrides.listen {
            self.listen = value.clone();
        }
        if let Some(value) = &overrides.allowed_origins {
            self.allowed_origins = value.clone();
        }
        if let Some(value) = overrides.startup_timeout_secs {
            self.startup_timeout_secs = value;
        }
        if let Some(value) = overrides.request_timeout_secs {
            self.request_timeout_secs = value;
        }
        if let Some(value) = overrides.pairing_ttl_secs {
            self.pairing_ttl_secs = value;
        }
        if let Some(value) = overrides.token_ttl_secs {
            self.token_ttl_secs = value;
        }
        if let Some(value) = overrides.max_frame_bytes {
            self.max_frame_bytes = value;
        }
        if let Some(value) = overrides.max_reassembled_bytes {
            self.max_reassembled_bytes = value;
        }
        if let Some(value) = overrides.log_max_bytes {
            self.log_max_bytes = value;
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimePhase {
    Starting,
    Running,
    Stopping,
    Stopped,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeState {
    pub schema_version: u32,
    pub instance_id: String,
    pub phase: RuntimePhase,
    pub pid: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_start_time: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub listen: Option<String>,
    pub bridge_version: String,
    pub omp_bin: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub omp_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub workspace: String,
    pub engine_state: String,
    pub engine_ready: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine_error: Option<RuntimeEngineError>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub omp_pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub omp_start_time: Option<u64>,
    pub started_at: u64,
    pub updated_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeEngineError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PidFile {
    pub schema_version: u32,
    pub pid: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_start_time: Option<u64>,
    pub exe: String,
    pub instance_id: String,
    pub started_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HandshakePhase {
    Ready,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StartupHandshake {
    pub schema_version: u32,
    pub startup_id: String,
    pub phase: HandshakePhase,
    pub bridge_pid: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub listen: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pairing_code: Option<String>,
    pub engine_ready: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
}

#[derive(Debug)]
pub enum ConfigStoreError {
    HomeUnavailable,
    Io(io::Error),
    Corrupted(String),
    FutureSchema(u32),
    Invalid(String),
}

impl std::fmt::Display for ConfigStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HomeUnavailable => {
                write!(f, "HOME is not set; set HOME or OMP_BRIDGE_HOME")
            }
            Self::Io(error) => write!(f, "{error}"),
            Self::Corrupted(message) => write!(f, "configuration is corrupted: {message}"),
            Self::FutureSchema(version) => write!(
                f,
                "configuration schema version {version} is newer than this bridge supports"
            ),
            Self::Invalid(message) => write!(f, "configuration is invalid: {message}"),
        }
    }
}

impl std::error::Error for ConfigStoreError {}

impl From<io::Error> for ConfigStoreError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

pub fn load_config(paths: &ConfigPaths) -> Result<Option<StoredConfig>, ConfigStoreError> {
    let text = match fs::read_to_string(&paths.config_file) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(ConfigStoreError::Io(error)),
    };
    let mut value: Value = serde_json::from_str(&text)
        .map_err(|error| ConfigStoreError::Corrupted(error.to_string()))?;
    let version = value
        .get("schemaVersion")
        .and_then(Value::as_u64)
        .unwrap_or(0) as u32;
    if version > CONFIG_SCHEMA_VERSION {
        return Err(ConfigStoreError::FutureSchema(version));
    }
    if version < CONFIG_SCHEMA_VERSION {
        migrate_config(&mut value, version)?;
    }
    let config: StoredConfig = serde_json::from_value(value)
        .map_err(|error| ConfigStoreError::Corrupted(error.to_string()))?;
    config.validate()?;
    Ok(Some(config))
}

fn migrate_config(value: &mut Value, from: u32) -> Result<(), ConfigStoreError> {
    match from {
        0 => {
            let defaults =
                serde_json::to_value(StoredConfig::new(PathBuf::from("omp"), PathBuf::from(".")))
                    .map_err(|error| ConfigStoreError::Corrupted(error.to_string()))?;
            if let (Some(current), Some(defaults)) = (value.as_object_mut(), defaults.as_object()) {
                for (key, default) in defaults {
                    current
                        .entry(key.clone())
                        .or_insert_with(|| default.clone());
                }
                current.insert(
                    "schemaVersion".to_owned(),
                    Value::from(CONFIG_SCHEMA_VERSION),
                );
            }
            Ok(())
        }
        other => Err(ConfigStoreError::Corrupted(format!(
            "no migration path from schema version {other}"
        ))),
    }
}

pub fn save_config(paths: &ConfigPaths, config: &StoredConfig) -> Result<(), ConfigStoreError> {
    config.validate()?;
    paths.ensure()?;
    let bytes = serde_json::to_vec_pretty(config)
        .map_err(|error| ConfigStoreError::Corrupted(error.to_string()))?;
    write_atomic(&paths.config_file, &bytes, 0o600)?;
    Ok(())
}

pub fn save_runtime_state(
    paths: &ConfigPaths,
    state: &RuntimeState,
) -> Result<(), ConfigStoreError> {
    paths.ensure()?;
    let bytes = serde_json::to_vec_pretty(state)
        .map_err(|error| ConfigStoreError::Corrupted(error.to_string()))?;
    write_atomic(&paths.state_file, &bytes, 0o600)?;
    Ok(())
}

pub fn load_runtime_state(paths: &ConfigPaths) -> Option<RuntimeState> {
    let text = fs::read_to_string(&paths.state_file).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    if value.get("schemaVersion").and_then(Value::as_u64) != Some(RUNTIME_SCHEMA_VERSION as u64) {
        return None;
    }
    serde_json::from_value(value).ok()
}

pub fn save_pid_file(paths: &ConfigPaths, pid: &PidFile) -> Result<(), ConfigStoreError> {
    paths.ensure()?;
    let bytes = serde_json::to_vec_pretty(pid)
        .map_err(|error| ConfigStoreError::Corrupted(error.to_string()))?;
    write_atomic(&paths.pid_file, &bytes, 0o600)?;
    Ok(())
}

pub fn load_pid_file(paths: &ConfigPaths) -> Option<PidFile> {
    let text = fs::read_to_string(&paths.pid_file).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    if value.get("schemaVersion").and_then(Value::as_u64) != Some(RUNTIME_SCHEMA_VERSION as u64) {
        return None;
    }
    serde_json::from_value(value).ok()
}

pub fn save_handshake(path: &Path, handshake: &StartupHandshake) -> Result<(), ConfigStoreError> {
    let bytes = serde_json::to_vec(handshake)
        .map_err(|error| ConfigStoreError::Corrupted(error.to_string()))?;
    write_atomic(path, &bytes, 0o600)?;
    Ok(())
}

pub fn load_handshake(path: &Path) -> Result<StartupHandshake, ConfigStoreError> {
    let text = fs::read_to_string(path)?;
    let handshake: StartupHandshake = serde_json::from_str(&text)
        .map_err(|error| ConfigStoreError::Corrupted(error.to_string()))?;
    if handshake.schema_version != RUNTIME_SCHEMA_VERSION {
        return Err(ConfigStoreError::Corrupted(
            "unsupported startup handshake schema".to_owned(),
        ));
    }
    Ok(handshake)
}

pub fn remove_if_exists(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

pub fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

pub fn random_id() -> String {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).expect("OS random source");
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn create_private_dir(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn expand_home(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join(rest);
    }
    path
}

fn write_atomic(path: &Path, bytes: &[u8], mode: u32) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no parent"))?;
    create_private_dir(parent)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("config.json");
    let mut last_error = None;
    for attempt in 0..10 {
        let temp = parent.join(format!(
            ".{file_name}.tmp-{}-{}-{attempt}",
            std::process::id(),
            unix_millis()
        ));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(mode);
        }
        match options.open(&temp) {
            Ok(mut file) => {
                let result = (|| {
                    file.write_all(bytes)?;
                    file.sync_all()?;
                    drop(file);
                    fs::rename(&temp, path)?;
                    sync_parent(parent);
                    Ok(())
                })();
                if result.is_err() {
                    let _ = fs::remove_file(&temp);
                }
                return result;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                last_error = Some(error);
            }
            Err(error) => return Err(error),
        }
    }
    Err(last_error.unwrap_or_else(|| io::Error::other("cannot create temporary config file")))
}

fn sync_parent(parent: &Path) {
    if let Ok(directory) = File::open(parent) {
        let _ = directory.sync_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths() -> (tempfile::TempDir, ConfigPaths) {
        let dir = tempfile::tempdir().unwrap();
        let paths = ConfigPaths::from_home(dir.path().join("home"));
        (dir, paths)
    }

    #[test]
    fn save_and_load_round_trip() {
        let (_dir, paths) = paths();
        let config = StoredConfig::new(PathBuf::from("/usr/bin/omp"), PathBuf::from("/tmp"));
        save_config(&paths, &config).unwrap();
        assert_eq!(load_config(&paths).unwrap(), Some(config));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&paths.config_file)
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn configuration_never_contains_secrets() {
        let (_dir, paths) = paths();
        let mut config = StoredConfig::new(PathBuf::from("/usr/bin/omp"), PathBuf::from("/tmp"));
        config.model = Some("provider/model".to_owned());
        save_config(&paths, &config).unwrap();
        let text = fs::read_to_string(&paths.config_file).unwrap();
        for needle in [
            "\"token\"",
            "\"pairingCode\"",
            "\"apiKey\"",
            "\"secret\"",
            "\"password\"",
        ] {
            assert!(!text.contains(needle), "config contained {needle}");
        }
    }

    #[test]
    fn corrupted_config_is_reported_and_preserved() {
        let (_dir, paths) = paths();
        paths.ensure().unwrap();
        fs::write(&paths.config_file, "{not json").unwrap();
        assert!(matches!(
            load_config(&paths),
            Err(ConfigStoreError::Corrupted(_))
        ));
        assert!(paths.config_file.exists());
    }

    #[test]
    fn schema_zero_is_migrated() {
        let (_dir, paths) = paths();
        paths.ensure().unwrap();
        fs::write(
            &paths.config_file,
            r#"{"schemaVersion":0,"ompBin":"omp","workspace":"/tmp","listen":"127.0.0.1:8765","allowedOrigins":["http://tauri.localhost"]}"#,
        )
        .unwrap();
        let config = load_config(&paths).unwrap().unwrap();
        assert_eq!(config.schema_version, CONFIG_SCHEMA_VERSION);
        assert_eq!(config.startup_timeout_secs, DEFAULT_STARTUP_TIMEOUT_SECS);
    }

    #[test]
    fn future_schema_is_rejected() {
        let (_dir, paths) = paths();
        paths.ensure().unwrap();
        fs::write(
            &paths.config_file,
            r#"{"schemaVersion":99,"ompBin":"omp","workspace":"/tmp","listen":"127.0.0.1:8765","allowedOrigins":["http://tauri.localhost"]}"#,
        )
        .unwrap();
        assert!(matches!(
            load_config(&paths),
            Err(ConfigStoreError::FutureSchema(99))
        ));
    }

    #[test]
    fn non_loopback_listen_is_rejected() {
        let mut config = StoredConfig::new(PathBuf::from("/usr/bin/omp"), PathBuf::from("/tmp"));
        config.listen = "0.0.0.0:8765".to_owned();
        assert!(config.validate().is_err());
    }

    #[test]
    fn missing_config_is_not_an_error() {
        let (_dir, paths) = paths();
        assert!(load_config(&paths).unwrap().is_none());
    }
}
