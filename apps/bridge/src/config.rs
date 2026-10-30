//! Validated bridge configuration. The listener is loopback-only by design.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;
use url::Url;

pub const DEFAULT_LISTEN: &str = "127.0.0.1:8765";
pub const DEFAULT_MAX_FRAME_BYTES: usize = 1024 * 1024;
pub const DEFAULT_MAX_REASSEMBLED_BYTES: usize = 64 * 1024 * 1024;
pub const DEFAULT_STARTUP_TIMEOUT_SECS: u64 = 90;
pub const DEFAULT_REQUEST_TIMEOUT_SECS: u64 = 60;
pub const DEFAULT_PAIRING_TTL_SECS: u64 = 600;
pub const DEFAULT_TOKEN_TTL_SECS: u64 = 24 * 60 * 60;
pub const DEFAULT_ALLOWED_ORIGINS: &str =
    "http://tauri.localhost,https://tauri.localhost,tauri://localhost,http://localhost:1420";

#[derive(Debug, Clone)]
pub struct BridgeConfig {
    pub omp_bin: PathBuf,
    pub omp_args: Vec<String>,
    pub workspace: PathBuf,
    pub listen: SocketAddr,
    pub allowed_origins: Vec<String>,
    pub startup_timeout: Duration,
    pub request_timeout: Duration,
    pub pairing_ttl: Duration,
    pub token_ttl: Duration,
    pub max_frame_bytes: usize,
    pub max_reassembled_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    NonLoopback(SocketAddr),
    InvalidOrigin(String),
    WorkspaceNotFound(PathBuf),
    WorkspaceNotDirectory(PathBuf),
    OmpBinaryNotFound(PathBuf),
    EmptyOmpBinary,
    InvalidLimit(&'static str),
    ZeroTimeout(&'static str),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonLoopback(address) => write!(
                f,
                "refusing non-loopback listen address {address}; remote mode is not implemented"
            ),
            Self::InvalidOrigin(origin) => write!(
                f,
                "invalid allowed origin {origin:?}; use exact http(s) or tauri origins, never '*'"
            ),
            Self::WorkspaceNotFound(path) => {
                write!(f, "workspace does not exist: {}", path.display())
            }
            Self::WorkspaceNotDirectory(path) => {
                write!(f, "workspace is not a directory: {}", path.display())
            }
            Self::OmpBinaryNotFound(path) => {
                write!(f, "OMP executable was not found: {}", path.display())
            }
            Self::EmptyOmpBinary => write!(f, "OMP executable path is empty"),
            Self::InvalidLimit(name) => write!(f, "{name} must be greater than zero"),
            Self::ZeroTimeout(name) => write!(f, "{name} must be greater than zero"),
        }
    }
}

impl std::error::Error for ConfigError {}

impl BridgeConfig {
    pub fn validate(mut self) -> Result<Self, ConfigError> {
        if !self.listen.ip().is_loopback() {
            return Err(ConfigError::NonLoopback(self.listen));
        }
        if self.max_frame_bytes == 0 {
            return Err(ConfigError::InvalidLimit("max_frame_bytes"));
        }
        if self.max_reassembled_bytes == 0 {
            return Err(ConfigError::InvalidLimit("max_reassembled_bytes"));
        }
        if self.max_frame_bytes > DEFAULT_MAX_FRAME_BYTES {
            return Err(ConfigError::InvalidLimit(
                "max_frame_bytes (maximum is 1 MiB)",
            ));
        }
        if self.max_reassembled_bytes > DEFAULT_MAX_REASSEMBLED_BYTES {
            return Err(ConfigError::InvalidLimit(
                "max_reassembled_bytes (maximum is 64 MiB)",
            ));
        }
        if self.startup_timeout.is_zero() {
            return Err(ConfigError::ZeroTimeout("startup_timeout"));
        }
        if self.request_timeout.is_zero() {
            return Err(ConfigError::ZeroTimeout("request_timeout"));
        }
        if self.pairing_ttl.is_zero() {
            return Err(ConfigError::ZeroTimeout("pairing_ttl"));
        }
        if self.token_ttl.is_zero() {
            return Err(ConfigError::ZeroTimeout("token_ttl"));
        }

        let workspace = self
            .workspace
            .canonicalize()
            .map_err(|_| ConfigError::WorkspaceNotFound(self.workspace.clone()))?;
        if !workspace.is_dir() {
            return Err(ConfigError::WorkspaceNotDirectory(workspace));
        }
        self.workspace = workspace;

        if self.omp_bin.as_os_str().is_empty() {
            return Err(ConfigError::EmptyOmpBinary);
        }
        if self.omp_bin.is_absolute() || self.omp_bin.components().count() > 1 {
            let binary = self
                .omp_bin
                .canonicalize()
                .map_err(|_| ConfigError::OmpBinaryNotFound(self.omp_bin.clone()))?;
            if !binary.is_file() {
                return Err(ConfigError::OmpBinaryNotFound(binary));
            }
            self.omp_bin = binary;
        }

        if self.allowed_origins.is_empty() {
            return Err(ConfigError::InvalidOrigin(String::new()));
        }
        let mut origins = Vec::with_capacity(self.allowed_origins.len());
        for origin in self.allowed_origins {
            let normalized = normalize_origin(&origin)?;
            if !origins.contains(&normalized) {
                origins.push(normalized);
            }
        }
        self.allowed_origins = origins;
        Ok(self)
    }

    pub fn origin_allowed(&self, origin: &str) -> bool {
        let Ok(normalized) = normalize_origin(origin) else {
            return false;
        };
        self.allowed_origins
            .iter()
            .any(|value| value == &normalized)
    }
}

fn normalize_origin(origin: &str) -> Result<String, ConfigError> {
    let origin = origin.trim().trim_end_matches('/');
    if origin.is_empty() || origin == "*" {
        return Err(ConfigError::InvalidOrigin(origin.to_owned()));
    }
    let url = Url::parse(origin).map_err(|_| ConfigError::InvalidOrigin(origin.to_owned()))?;
    if !matches!(url.scheme(), "http" | "https" | "tauri") {
        return Err(ConfigError::InvalidOrigin(origin.to_owned()));
    }
    if url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/")
    {
        return Err(ConfigError::InvalidOrigin(origin.to_owned()));
    }
    Ok(origin.to_owned())
}

pub fn duration_secs(seconds: u64) -> Duration {
    Duration::from_secs(seconds)
}

pub fn canonical_workspace(path: &Path) -> Result<PathBuf, ConfigError> {
    path.canonicalize()
        .map_err(|_| ConfigError::WorkspaceNotFound(path.to_path_buf()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    fn config() -> BridgeConfig {
        BridgeConfig {
            omp_bin: PathBuf::from("omp"),
            omp_args: vec!["--mode".to_owned(), "rpc-ui".to_owned()],
            workspace: std::env::current_dir().unwrap(),
            listen: DEFAULT_LISTEN.parse().unwrap(),
            allowed_origins: DEFAULT_ALLOWED_ORIGINS
                .split(',')
                .map(str::to_owned)
                .collect(),
            startup_timeout: Duration::from_secs(DEFAULT_STARTUP_TIMEOUT_SECS),
            request_timeout: Duration::from_secs(DEFAULT_REQUEST_TIMEOUT_SECS),
            pairing_ttl: Duration::from_secs(DEFAULT_PAIRING_TTL_SECS),
            token_ttl: Duration::from_secs(DEFAULT_TOKEN_TTL_SECS),
            max_frame_bytes: DEFAULT_MAX_FRAME_BYTES,
            max_reassembled_bytes: DEFAULT_MAX_REASSEMBLED_BYTES,
        }
    }

    #[test]
    fn default_listener_is_loopback() {
        let config = config().validate().unwrap();
        assert!(config.listen.ip().is_loopback());
        assert_eq!(config.listen.port(), 8765);
    }

    #[test]
    fn public_bind_is_rejected() {
        let mut config = config();
        config.listen = "0.0.0.0:8765".parse().unwrap();
        assert!(matches!(
            config.validate(),
            Err(ConfigError::NonLoopback(_))
        ));
    }

    #[test]
    fn wildcard_origin_is_rejected() {
        let mut config = config();
        config.allowed_origins = vec!["*".to_owned()];
        assert!(matches!(
            config.validate(),
            Err(ConfigError::InvalidOrigin(_))
        ));
    }

    #[test]
    fn packaged_tauri_origin_is_allowed_exactly() {
        let config = config().validate().unwrap();
        assert!(config.origin_allowed("http://tauri.localhost"));
        assert!(config.origin_allowed("https://tauri.localhost/"));
        assert!(!config.origin_allowed("http://evil.example"));
        assert!(!config.origin_allowed("http://tauri.localhost.evil.example"));
    }

    #[test]
    fn missing_workspace_is_reported() {
        let mut config = config();
        config.workspace = PathBuf::from("/definitely/not/a/workspace");
        assert!(matches!(
            config.validate(),
            Err(ConfigError::WorkspaceNotFound(_))
        ));
    }

    #[test]
    fn oversized_limits_are_rejected() {
        let mut config = config();
        config.max_frame_bytes = DEFAULT_MAX_FRAME_BYTES + 1;
        assert!(matches!(
            config.validate(),
            Err(ConfigError::InvalidLimit(_))
        ));
    }

    #[test]
    fn bare_executable_name_is_left_for_path_lookup() {
        let config = config().validate().unwrap();
        assert_eq!(config.omp_bin, PathBuf::from("omp"));
    }

    #[test]
    fn non_loopback_ipv6_is_rejected() {
        let mut config = config();
        config.listen = SocketAddr::new(IpAddr::V6(std::net::Ipv6Addr::UNSPECIFIED), 8765);
        assert!(matches!(
            config.validate(),
            Err(ConfigError::NonLoopback(_))
        ));
    }

    #[test]
    fn localhost_origin_with_path_is_rejected() {
        assert!(normalize_origin("http://tauri.localhost/path").is_err());
    }

    #[test]
    fn ipv4_loopback_listener_is_accepted() {
        let mut config = config();
        config.listen = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);
        assert!(config.validate().is_ok());
    }
}
