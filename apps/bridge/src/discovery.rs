//! OMP executable discovery, validation, and compatibility probing.
//!
//! Discovery is deliberately bounded: explicit override, environment, saved
//! configuration, `PATH`, then a short list of documented user-writable install
//! locations. It never walks a filesystem tree.

use crate::api_types::{EnginePhase, EngineStatus};
use crate::config::BridgeConfig;
use crate::config_store::StoredConfig;
use crate::engine::{Engine, EngineConfig, redact_error};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

pub const MIN_OMP_VERSION: &str = "18.13.3";
pub const SUPPORTED_OMP_MAJOR: u64 = 18;
pub const VERSION_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateSource {
    Cli,
    Environment,
    Saved,
    Path,
    KnownLocation,
    Manual,
}

impl std::fmt::Display for CandidateSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::Cli => "--omp-bin",
            Self::Environment => "OMP_BRIDGE_OMP_BIN",
            Self::Saved => "saved configuration",
            Self::Path => "PATH",
            Self::KnownLocation => "known location",
            Self::Manual => "manual selection",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum CandidateStatus {
    Valid { version: String },
    Missing,
    NotAFile,
    NotExecutable,
    VersionFailed { message: String },
    UnsupportedVersion { version: String, reason: String },
    MissingRuntimeDependency { message: String },
    Timeout,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OmpCandidate {
    pub path: PathBuf,
    pub source: CandidateSource,
    pub status: CandidateStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OmpInstallation {
    pub path: PathBuf,
    pub source: CandidateSource,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol_version: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OmpDiscoveryError {
    pub code: String,
    pub message: String,
}

impl OmpDiscoveryError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryStatus {
    Found,
    Multiple,
    NotFound,
    InvalidExecutable,
    UnsupportedVersion,
    MissingRuntimeDependency,
    RpcStartupFailure,
    RequestedUnavailable,
    SavedUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OmpDiscoveryResult {
    pub status: DiscoveryStatus,
    pub candidates: Vec<OmpCandidate>,
    pub installations: Vec<OmpInstallation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected: Option<OmpInstallation>,
    pub searched: Vec<PathBuf>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<OmpDiscoveryError>,
}

impl OmpDiscoveryResult {
    pub fn not_found(searched: Vec<PathBuf>) -> Self {
        Self {
            status: DiscoveryStatus::NotFound,
            candidates: Vec::new(),
            installations: Vec::new(),
            selected: None,
            searched,
            errors: vec![OmpDiscoveryError::new(
                "OMP_NOT_FOUND",
                "No OMP executable was found. Install OMP or pass --omp-bin.",
            )],
        }
    }

    pub fn selected(&self) -> Option<&OmpInstallation> {
        self.selected.as_ref()
    }
}

/// Collect candidates without running them.
pub fn collect_candidates(
    explicit: Option<&Path>,
    saved: Option<&Path>,
) -> (Vec<OmpCandidate>, Vec<PathBuf>) {
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();
    let mut searched = Vec::new();

    if let Some(path) = explicit {
        push_candidate(&mut candidates, &mut seen, path, CandidateSource::Cli);
    }
    if let Some(path) = std::env::var_os("OMP_BRIDGE_OMP_BIN").map(PathBuf::from) {
        push_candidate(
            &mut candidates,
            &mut seen,
            &path,
            CandidateSource::Environment,
        );
    }
    if let Some(path) = saved {
        push_candidate(&mut candidates, &mut seen, path, CandidateSource::Saved);
    }

    if let Some(paths) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&paths) {
            let candidate = directory.join("omp");
            searched.push(candidate.clone());
            if candidate.is_file() {
                push_candidate(
                    &mut candidates,
                    &mut seen,
                    &candidate,
                    CandidateSource::Path,
                );
            }
        }
    }

    for candidate in known_locations() {
        searched.push(candidate.clone());
        if candidate.is_file() {
            push_candidate(
                &mut candidates,
                &mut seen,
                &candidate,
                CandidateSource::KnownLocation,
            );
        }
    }

    (candidates, searched)
}

fn push_candidate(
    candidates: &mut Vec<OmpCandidate>,
    seen: &mut HashSet<PathBuf>,
    path: &Path,
    source: CandidateSource,
) {
    let key = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if seen.insert(key) {
        candidates.push(OmpCandidate {
            path: path.to_path_buf(),
            source,
            status: CandidateStatus::Missing,
        });
    }
}

pub fn known_locations() -> Vec<PathBuf> {
    let mut locations = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        locations.push(home.join(".local/bin/omp"));
        locations.push(home.join(".omp/bin/omp"));
        locations.push(home.join("bin/omp"));
        locations.push(home.join(".local/share/omp/bin/omp"));
    }
    if let Some(prefix) = std::env::var_os("PREFIX") {
        locations.push(PathBuf::from(prefix).join("bin/omp"));
    }
    locations.push(PathBuf::from("/usr/local/bin/omp"));
    locations.push(PathBuf::from("/usr/bin/omp"));
    locations
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectionMode {
    Explicit,
    Environment,
    Saved,
    Auto,
}

fn selection_mode(explicit: Option<&Path>, saved: Option<&Path>) -> SelectionMode {
    if explicit.is_some() {
        SelectionMode::Explicit
    } else if std::env::var_os("OMP_BRIDGE_OMP_BIN").is_some() {
        SelectionMode::Environment
    } else if saved.is_some() {
        SelectionMode::Saved
    } else {
        SelectionMode::Auto
    }
}

fn select_installation(
    installations: &[OmpInstallation],
    mode: SelectionMode,
) -> Option<OmpInstallation> {
    let source = match mode {
        SelectionMode::Explicit => CandidateSource::Cli,
        SelectionMode::Environment => CandidateSource::Environment,
        SelectionMode::Saved => CandidateSource::Saved,
        SelectionMode::Auto => return installations.first().cloned(),
    };
    installations
        .iter()
        .find(|installation| installation.source == source)
        .cloned()
}

/// Validate candidates and select one. This checks executable shape and
/// `--version`; callers that need full compatibility must also call
/// [`verify_installation`], which starts the real RPC process.
pub async fn discover(
    explicit: Option<&Path>,
    saved: Option<&Path>,
    version_timeout: Duration,
) -> OmpDiscoveryResult {
    let (candidates, searched) = collect_candidates(explicit, saved);
    if candidates.is_empty() {
        return OmpDiscoveryResult::not_found(searched);
    }

    let mode = selection_mode(explicit, saved);
    let mut validated = Vec::with_capacity(candidates.len());
    for mut candidate in candidates {
        candidate.status = validate_candidate(&candidate.path, version_timeout).await;
        validated.push(candidate);
    }

    let installations: Vec<OmpInstallation> = validated
        .iter()
        .filter_map(|candidate| match &candidate.status {
            CandidateStatus::Valid { version } => Some(OmpInstallation {
                path: candidate.path.clone(),
                source: candidate.source,
                version: version.clone(),
                protocol_version: None,
                model: None,
            }),
            _ => None,
        })
        .collect();

    let selected = select_installation(&installations, mode);
    let status = match (&selected, installations.is_empty()) {
        (Some(_), false) if installations.len() > 1 => DiscoveryStatus::Multiple,
        (Some(_), false) => DiscoveryStatus::Found,
        (Some(_), true) => unreachable!("selection requires a valid installation"),
        (None, true) if mode == SelectionMode::Saved => DiscoveryStatus::SavedUnavailable,
        (None, true) if mode != SelectionMode::Auto => DiscoveryStatus::RequestedUnavailable,
        (None, true)
            if validated.iter().any(|candidate| {
                matches!(candidate.status, CandidateStatus::UnsupportedVersion { .. })
            }) =>
        {
            DiscoveryStatus::UnsupportedVersion
        }
        (None, true)
            if validated.iter().any(|candidate| {
                matches!(
                    candidate.status,
                    CandidateStatus::MissingRuntimeDependency { .. }
                )
            }) =>
        {
            DiscoveryStatus::MissingRuntimeDependency
        }
        (None, true) => DiscoveryStatus::InvalidExecutable,
        (None, false) if mode == SelectionMode::Saved => DiscoveryStatus::SavedUnavailable,
        (None, false) => DiscoveryStatus::RequestedUnavailable,
    };

    let mut errors = validated
        .iter()
        .filter_map(candidate_error)
        .collect::<Vec<_>>();
    if selected.is_none() {
        match mode {
            SelectionMode::Saved => {
                if let Some(path) = saved {
                    errors.insert(
                        0,
                        OmpDiscoveryError::new(
                            "OMP_SAVED_UNAVAILABLE",
                            format!(
                                "saved OMP installation {} is no longer usable; run `omp-bridge setup --omp-bin <path>` to replace it",
                                path.display()
                            ),
                        ),
                    );
                }
            }
            SelectionMode::Explicit => {
                if let Some(path) = explicit {
                    errors.insert(
                        0,
                        OmpDiscoveryError::new(
                            "OMP_REQUESTED_UNAVAILABLE",
                            format!(
                                "requested OMP executable {} is not usable; fix --omp-bin or OMP_BRIDGE_OMP_BIN",
                                path.display()
                            ),
                        ),
                    );
                }
            }
            SelectionMode::Environment | SelectionMode::Auto => {}
        }
    }

    OmpDiscoveryResult {
        status,
        candidates: validated,
        installations,
        selected,
        searched,
        errors,
    }
}

fn candidate_error(candidate: &OmpCandidate) -> Option<OmpDiscoveryError> {
    match &candidate.status {
        CandidateStatus::Valid { .. } => None,
        CandidateStatus::Missing => Some(OmpDiscoveryError::new(
            "OMP_NOT_FOUND",
            format!("{} does not exist", candidate.path.display()),
        )),
        CandidateStatus::NotAFile => Some(OmpDiscoveryError::new(
            "OMP_INVALID_EXECUTABLE",
            format!("{} is not a regular file", candidate.path.display()),
        )),
        CandidateStatus::NotExecutable => Some(OmpDiscoveryError::new(
            "OMP_NOT_EXECUTABLE",
            format!("{} is not executable", candidate.path.display()),
        )),
        CandidateStatus::VersionFailed { message } => Some(OmpDiscoveryError::new(
            "OMP_VERSION_FAILED",
            format!("{} --version failed: {message}", candidate.path.display()),
        )),
        CandidateStatus::UnsupportedVersion { version, reason } => Some(OmpDiscoveryError::new(
            "OMP_UNSUPPORTED_VERSION",
            format!("{} reports {version}: {reason}", candidate.path.display()),
        )),
        CandidateStatus::MissingRuntimeDependency { message } => Some(OmpDiscoveryError::new(
            "OMP_MISSING_RUNTIME_DEPENDENCY",
            format!("{} cannot run: {message}", candidate.path.display()),
        )),
        CandidateStatus::Timeout => Some(OmpDiscoveryError::new(
            "OMP_VERSION_TIMEOUT",
            format!("{} --version timed out", candidate.path.display()),
        )),
    }
}

pub async fn validate_candidate(path: &Path, version_timeout: Duration) -> CandidateStatus {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return CandidateStatus::Missing;
        }
        Err(error) => {
            return CandidateStatus::VersionFailed {
                message: redact_error(&error.to_string()),
            };
        }
    };
    if !metadata.is_file() {
        return CandidateStatus::NotAFile;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return CandidateStatus::NotExecutable;
        }
    }

    let mut command = Command::new(path);
    command
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let output = match timeout(version_timeout, command.output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => {
            let message = redact_error(&error.to_string());
            if is_runtime_dependency_error(&message) {
                return CandidateStatus::MissingRuntimeDependency { message };
            }
            return CandidateStatus::VersionFailed { message };
        }
        Err(_) => return CandidateStatus::Timeout,
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        let message = redact_error(stderr.trim());
        if is_runtime_dependency_error(&message) {
            return CandidateStatus::MissingRuntimeDependency { message };
        }
        return CandidateStatus::VersionFailed {
            message: format!("exit status {}", output.status),
        };
    }

    let Some(version) = parse_version(&stdout).or_else(|| parse_version(&stderr)) else {
        return CandidateStatus::VersionFailed {
            message: "output did not contain an OMP version".to_owned(),
        };
    };
    match check_version_supported(&version) {
        Ok(()) => CandidateStatus::Valid { version },
        Err(reason) => CandidateStatus::UnsupportedVersion { version, reason },
    }
}

fn is_runtime_dependency_error(message: &str) -> bool {
    let lowered = message.to_ascii_lowercase();
    lowered.contains("not found")
        || lowered.contains("no such file")
        || lowered.contains("glibc")
        || lowered.contains("libc.so")
        || lowered.contains("cannot execute")
        || lowered.contains("exec format error")
}

pub fn parse_version(text: &str) -> Option<String> {
    for token in text.split_whitespace() {
        let token = token.trim_matches(|character: char| {
            !(character.is_ascii_alphanumeric() || character == '.' || character == '/')
        });
        let candidate = token
            .strip_prefix("omp/")
            .or_else(|| token.strip_prefix("omp-v"))
            .or_else(|| token.strip_prefix('v'))
            .unwrap_or(token);
        let mut parts = candidate.split('.');
        let Some(major) = parts.next().and_then(|part| part.parse::<u64>().ok()) else {
            continue;
        };
        let Some(minor) = parts.next().and_then(|part| part.parse::<u64>().ok()) else {
            continue;
        };
        let Some(patch_text) = parts.next() else {
            continue;
        };
        let patch = patch_text
            .split(|character: char| !character.is_ascii_digit())
            .next()
            .and_then(|part| part.parse::<u64>().ok())?;
        return Some(format!("{major}.{minor}.{patch}"));
    }
    None
}

pub fn check_version_supported(version: &str) -> Result<(), String> {
    let parsed =
        parse_version(version).ok_or_else(|| format!("cannot parse OMP version {version:?}"))?;
    let parts: Vec<u64> = parsed
        .split('.')
        .filter_map(|part| part.parse().ok())
        .collect();
    if parts.len() != 3 {
        return Err(format!("cannot parse OMP version {version:?}"));
    }
    let (major, minor, patch) = (parts[0], parts[1], parts[2]);
    let minimum = parse_version(MIN_OMP_VERSION).expect("constant version");
    let minimum: Vec<u64> = minimum
        .split('.')
        .filter_map(|part| part.parse().ok())
        .collect();
    if major != SUPPORTED_OMP_MAJOR {
        return Err(format!(
            "the bridge supports OMP {SUPPORTED_OMP_MAJOR}.x; found {major}.x. Update the bridge only after compatibility testing."
        ));
    }
    if (minor, patch) < (minimum[1], minimum[2]) {
        return Err(format!(
            "OMP {parsed} is older than the tested baseline {MIN_OMP_VERSION}"
        ));
    }
    Ok(())
}

/// Start the real RPC process and require the actual ready/negotiation/state
/// sequence. The process is always shut down before returning.
pub async fn verify_installation(
    installation: &OmpInstallation,
    workspace: &Path,
    model: Option<&str>,
    startup_timeout: Duration,
    request_timeout: Duration,
) -> Result<OmpInstallation, OmpDiscoveryError> {
    let mut omp_args = vec!["--mode".to_owned(), "rpc-ui".to_owned()];
    if let Some(model) = model {
        omp_args.push("--model".to_owned());
        omp_args.push(model.to_owned());
    }
    let engine = Engine::start(EngineConfig {
        omp_bin: installation.path.clone(),
        omp_args,
        workspace: workspace.to_path_buf(),
        startup_timeout,
        request_timeout,
        max_frame_bytes: crate::config::DEFAULT_MAX_FRAME_BYTES,
        max_reassembled_bytes: crate::config::DEFAULT_MAX_REASSEMBLED_BYTES,
    })
    .await;
    let status = engine.status();
    engine.shutdown().await;
    verified_installation(installation, &status)
}

fn verified_installation(
    installation: &OmpInstallation,
    status: &EngineStatus,
) -> Result<OmpInstallation, OmpDiscoveryError> {
    if status.state == EnginePhase::Ready && status.ready {
        let model = status
            .summary
            .as_ref()
            .and_then(|summary| summary.model.as_ref())
            .map(|model| format!("{}/{}", model.provider, model.id));
        return Ok(OmpInstallation {
            path: installation.path.clone(),
            source: installation.source,
            version: installation.version.clone(),
            protocol_version: status.negotiated_protocol_version,
            model,
        });
    }
    let (code, message) = status
        .error
        .as_ref()
        .map(|error| (error.code.clone(), error.message.clone()))
        .unwrap_or_else(|| {
            (
                "RPC_STARTUP_FAILED".to_owned(),
                "OMP did not report ready".to_owned(),
            )
        });
    Err(OmpDiscoveryError::new(code, message))
}

/// Resolve a usable OMP installation from saved configuration or discovery.
/// This validates the version; the bridge start path performs the full RPC
/// probe before reporting success.
pub async fn resolve_installation(
    explicit: Option<&Path>,
    saved: Option<&StoredConfig>,
) -> Result<OmpInstallation, OmpDiscoveryError> {
    let saved_path = saved.map(|config| config.omp_bin.as_path());
    let result = discover(explicit, saved_path, VERSION_TIMEOUT).await;
    result.selected().cloned().ok_or_else(|| {
        result.errors.first().cloned().unwrap_or_else(|| {
            OmpDiscoveryError::new("OMP_NOT_FOUND", "No OMP installation found.")
        })
    })
}

pub fn bridge_config_for(
    stored: &StoredConfig,
    installation: &OmpInstallation,
) -> Result<BridgeConfig, String> {
    let mut config = stored.clone();
    config.omp_bin = installation.path.clone();
    config.omp_version = Some(installation.version.clone());
    config.to_bridge_config().map_err(|error| error.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformInfo {
    pub os: String,
    pub arch: String,
    pub libc: String,
    pub environment: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_target: Option<String>,
}

pub fn platform_info() -> PlatformInfo {
    let arch = std::env::consts::ARCH.to_owned();
    let android_linker = Path::new("/system/bin/linker64").exists();
    let glibc_linker = [
        "/lib/ld-linux-aarch64.so.1",
        "/lib/ld-linux-x86-64.so.2",
        "/lib64/ld-linux-x86-64.so.2",
    ]
    .iter()
    .any(|path| Path::new(path).exists());
    let termux_prefix = std::env::var_os("PREFIX")
        .map(|prefix| prefix.to_string_lossy().contains("com.termux"))
        .unwrap_or(false);
    let (libc, environment) = if android_linker && glibc_linker {
        ("glibc", "proot")
    } else if android_linker && !glibc_linker {
        ("bionic", "termux")
    } else {
        ("glibc", "linux")
    };
    let release_target = match (environment, arch.as_str()) {
        ("termux", _) => None,
        (_, "aarch64") => Some("aarch64-unknown-linux-gnu".to_owned()),
        (_, "x86_64") => Some("x86_64-unknown-linux-gnu".to_owned()),
        _ => None,
    };
    PlatformInfo {
        os: std::env::consts::OS.to_owned(),
        arch,
        libc: libc.to_owned(),
        environment: if termux_prefix && environment == "linux" {
            "termux-prefix".to_owned()
        } else {
            environment.to_owned()
        },
        release_target,
    }
}

pub fn format_discovery(result: &OmpDiscoveryResult) -> String {
    let mut output = String::new();
    output.push_str(&format!("OMP discovery: {:?}\n", result.status));
    for candidate in &result.candidates {
        output.push_str(&format!(
            "  {} [{}] {:?}\n",
            candidate.path.display(),
            candidate.source,
            candidate.status
        ));
    }
    if let Some(selected) = &result.selected {
        output.push_str(&format!(
            "  selected: {} ({}, source {})\n",
            selected.path.display(),
            selected.version,
            selected.source
        ));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_omp_version_formats() {
        assert_eq!(parse_version("omp/18.13.3").as_deref(), Some("18.13.3"));
        assert_eq!(parse_version("omp v18.13.3").as_deref(), Some("18.13.3"));
        assert_eq!(parse_version("v18.13.3\n").as_deref(), Some("18.13.3"));
        assert_eq!(parse_version("18.13.3").as_deref(), Some("18.13.3"));
        assert_eq!(parse_version("not a version"), None);
    }

    #[test]
    fn version_policy_accepts_baseline_and_rejects_old_or_new_major() {
        assert!(check_version_supported("18.13.3").is_ok());
        assert!(check_version_supported("18.14.0").is_ok());
        assert!(check_version_supported("18.13.2").is_err());
        assert!(check_version_supported("19.0.0").is_err());
        assert!(check_version_supported("17.99.99").is_err());
    }

    #[test]
    fn explicit_candidate_wins_over_saved() {
        let (candidates, _) = collect_candidates(
            Some(Path::new("/tmp/explicit-omp")),
            Some(Path::new("/tmp/saved-omp")),
        );
        assert_eq!(candidates[0].source, CandidateSource::Cli);
        assert_eq!(candidates[1].source, CandidateSource::Saved);
    }

    #[test]
    fn known_locations_are_not_recursive() {
        for location in known_locations() {
            assert!(location.is_absolute());
            assert_eq!(
                location.file_name().and_then(|name| name.to_str()),
                Some("omp")
            );
        }
    }

    #[test]
    fn platform_release_target_never_claims_termux() {
        let info = platform_info();
        if info.environment == "termux" {
            assert!(info.release_target.is_none());
        }
    }

    #[test]
    fn verified_installation_requires_ready() {
        let installation = OmpInstallation {
            path: PathBuf::from("/tmp/omp"),
            source: CandidateSource::Cli,
            version: "18.13.3".to_owned(),
            protocol_version: None,
            model: None,
        };
        let failed = EngineStatus::failed("OMP_NOT_FOUND", "missing");
        let error = verified_installation(&installation, &failed).unwrap_err();
        assert_eq!(error.code, "OMP_NOT_FOUND");
    }
}
