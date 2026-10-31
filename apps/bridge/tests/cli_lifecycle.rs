//! CLI lifecycle tests against the labeled fake OMP fixture.
//!
//! These prove setup, background start, status, logs, stop, and safe uninstall
//! behavior without claiming real-OMP evidence.

use serde_json::Value;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

struct TestEnv {
    _dir: TempDir,
    home: PathBuf,
    workspace: PathBuf,
    omp: PathBuf,
    started: bool,
}

impl TestEnv {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("temp dir");
        let home = dir.path().join("home");
        let workspace = dir.path().join("workspace");
        fs::create_dir_all(&workspace).expect("workspace");
        let omp = dir.path().join("fake-omp");
        let script = include_str!("fixtures/fake-omp.sh").replace("__MODE__", "happy");
        fs::write(&omp, script).expect("write fixture");
        fs::set_permissions(&omp, fs::Permissions::from_mode(0o755)).expect("chmod fixture");
        Self {
            _dir: dir,
            home,
            workspace,
            omp,
            started: false,
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_omp-bridge"));
        command
            .env("OMP_BRIDGE_HOME", &self.home)
            .env_remove("OMP_BRIDGE_RUNTIME_DIR")
            .env_remove("OMP_BRIDGE_LOG_DIR")
            .env_remove("OMP_BRIDGE_OMP_BIN")
            .env_remove("OMP_BRIDGE_WORKSPACE")
            .env_remove("OMP_BRIDGE_LISTEN")
            .env_remove("OMP_BRIDGE_MODEL");
        command
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command().args(args).output().expect("run bridge")
    }

    fn setup(&self) {
        let output = self.run(&[
            "setup",
            "--omp-bin",
            self.omp.to_str().unwrap(),
            "--workspace",
            self.workspace.to_str().unwrap(),
            "--listen",
            "127.0.0.1:0",
        ]);
        assert!(
            output.status.success(),
            "setup failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("Next: omp-bridge start"), "{stdout}");
    }

    fn config_path(&self) -> PathBuf {
        self.home.join("config.json")
    }

    fn status_json(&self) -> Value {
        let output = self.run(&["status", "--json"]);
        assert!(
            output.status.success(),
            "status failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).expect("status JSON")
    }

    fn stop(&mut self) {
        let _ = self.run(&["stop"]);
        self.started = false;
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        if self.started {
            let _ = self.run(&["stop"]);
        }
    }
}

fn write_fake_omp(path: &Path) {
    let script = include_str!("fixtures/fake-omp.sh").replace("__MODE__", "happy");
    fs::create_dir_all(path.parent().expect("parent")).expect("fixture dir");
    fs::write(path, script).expect("write fixture");
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("chmod fixture");
}

fn path_with(dir: &Path) -> String {
    format!("{}:/usr/bin:/bin", dir.display())
}

#[test]
fn setup_start_status_logs_stop_and_uninstall_are_safe() {
    let mut env = TestEnv::new();
    env.setup();

    let text = fs::read_to_string(env.config_path()).expect("config");
    let config: Value = serde_json::from_str(&text).expect("config JSON");
    assert_eq!(config["ompVersion"], "18.13.3");
    assert!(config.get("pairingCode").is_none());
    assert!(config.get("token").is_none());
    let mode = fs::metadata(env.config_path())
        .expect("config metadata")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);

    let start = env.run(&["start"]);
    assert!(
        start.status.success(),
        "start failed: {}",
        String::from_utf8_lossy(&start.stderr)
    );
    env.started = true;
    let stdout = String::from_utf8_lossy(&start.stdout);
    assert!(stdout.contains("started"), "{stdout}");
    let pairing = stdout
        .lines()
        .find_map(|line| line.trim().strip_prefix("Pairing:   "))
        .and_then(|rest| rest.split_whitespace().next())
        .expect("pairing code");
    assert_eq!(pairing.len(), 10);

    let status = env.status_json();
    assert_eq!(status["phase"], "engine_ready");
    assert_eq!(status["engineReady"], true);
    assert_eq!(status["ompVersion"], "18.13.3");
    let pid = status["pid"].as_u64().expect("pid");
    assert!(pid > 0);

    let again = env.run(&["start"]);
    assert!(!again.status.success());
    assert!(
        String::from_utf8_lossy(&again.stderr).contains("already running"),
        "{}",
        String::from_utf8_lossy(&again.stderr)
    );

    let logs = env.run(&["logs", "--lines", "50"]);
    assert!(logs.status.success());
    assert!(String::from_utf8_lossy(&logs.stdout).contains("starting bridge"));

    env.stop();
    let stopped = env.status_json();
    assert_eq!(stopped["phase"], "stopped");
    assert!(stopped.get("pid").is_none());

    let dry_run = env.run(&["uninstall", "--dry-run"]);
    assert!(dry_run.status.success());
    assert!(String::from_utf8_lossy(&dry_run.stdout).contains("would remove"));
    assert!(env.config_path().exists(), "dry run removed config");

    let refused = env.run(&["uninstall"]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("--yes"));
}

#[test]
fn doctor_accepts_the_saved_configuration_and_runs_rpc() {
    let env = TestEnv::new();
    env.setup();
    let output = env.run(&["doctor"]);
    assert!(
        output.status.success(),
        "doctor failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("RPC:"), "{stdout}");
    assert!(stdout.contains("doctor: configuration"), "{stdout}");
}

#[test]
fn status_before_setup_is_not_configured() {
    let env = TestEnv::new();
    let status = env.status_json();
    assert_eq!(status["phase"], "not_configured");
}

#[test]
fn explicit_omp_bin_does_not_fall_back_to_path() {
    let env = TestEnv::new();
    let fallback_dir = env.home.parent().expect("test dir").join("fallback-bin");
    write_fake_omp(&fallback_dir.join("omp"));
    let missing = env.home.parent().expect("test dir").join("missing-omp");

    let output = env
        .command()
        .env("PATH", path_with(&fallback_dir))
        .args([
            "setup",
            "--omp-bin",
            missing.to_str().unwrap(),
            "--workspace",
            env.workspace.to_str().unwrap(),
            "--listen",
            "127.0.0.1:0",
        ])
        .output()
        .expect("run setup");
    assert!(
        !output.status.success(),
        "setup silently fell back to PATH: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("--omp-bin"),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(!env.config_path().exists(), "setup saved a fallback path");
}

#[test]
fn saved_omp_disappearance_is_reported_without_switching() {
    let env = TestEnv::new();
    env.setup();
    let saved = env.omp.clone();
    fs::remove_file(&saved).expect("remove saved OMP");
    let fallback_dir = env.home.parent().expect("test dir").join("fallback-bin");
    write_fake_omp(&fallback_dir.join("omp"));

    let output = env
        .command()
        .env("PATH", path_with(&fallback_dir))
        .args(["setup"])
        .output()
        .expect("run setup");
    assert!(
        !output.status.success(),
        "setup silently switched the saved OMP: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(combined.contains("saved"), "{combined}");
    let config: Value =
        serde_json::from_str(&fs::read_to_string(env.config_path()).expect("config"))
            .expect("config JSON");
    assert_eq!(config["ompBin"], saved.to_str().unwrap());
}

#[test]
fn explicit_omp_bin_replaces_the_saved_installation() {
    let env = TestEnv::new();
    env.setup();
    let replacement = env.home.parent().expect("test dir").join("replacement-omp");
    write_fake_omp(&replacement);

    let output = env.run(&["setup", "--omp-bin", replacement.to_str().unwrap()]);
    assert!(
        output.status.success(),
        "setup failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let config: Value =
        serde_json::from_str(&fs::read_to_string(env.config_path()).expect("config"))
            .expect("config JSON");
    assert_eq!(config["ompBin"], replacement.to_str().unwrap());
}

#[test]
fn auto_discovery_uses_the_first_valid_path_entry() {
    let env = TestEnv::new();
    let first = env.home.parent().expect("test dir").join("first-bin");
    let second = env.home.parent().expect("test dir").join("second-bin");
    write_fake_omp(&first.join("omp"));
    write_fake_omp(&second.join("omp"));
    let path = format!("{}:{}", first.display(), second.display());

    let output = env
        .command()
        .env("PATH", format!("{path}:/usr/bin:/bin"))
        .args([
            "setup",
            "--workspace",
            env.workspace.to_str().unwrap(),
            "--listen",
            "127.0.0.1:0",
        ])
        .output()
        .expect("run setup");
    assert!(
        output.status.success(),
        "setup failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let config: Value =
        serde_json::from_str(&fs::read_to_string(env.config_path()).expect("config"))
            .expect("config JSON");
    assert_eq!(config["ompBin"], first.join("omp").to_str().unwrap());
}

#[test]
fn update_is_informational_and_points_to_the_installer() {
    let env = TestEnv::new();
    let output = env.run(&["update"]);
    assert!(
        output.status.success(),
        "update failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("does not update the binary"), "{stdout}");
    assert!(stdout.contains("scripts/install-bridge.sh"), "{stdout}");
    assert!(stdout.contains("omp-bridge stop"), "{stdout}");
    assert!(!stdout.contains("replace the binary"), "{stdout}");
}

#[test]
fn config_path_and_json_are_stable() {
    let env = TestEnv::new();
    env.setup();
    let path = env.run(&["config", "--path"]);
    assert!(path.status.success());
    assert_eq!(
        String::from_utf8_lossy(&path.stdout).trim(),
        env.config_path().display().to_string()
    );
    let json = env.run(&["config", "--json"]);
    assert!(json.status.success());
    let value: Value = serde_json::from_slice(&json.stdout).expect("config JSON");
    assert_eq!(value["configured"], true);
    assert_eq!(value["config"]["ompVersion"], "18.13.3");
}

#[test]
fn setup_overrides_take_precedence_over_saved_timeouts() {
    let env = TestEnv::new();
    env.setup();

    let output = env.run(&[
        "setup",
        "--startup-timeout-secs",
        "7",
        "--request-timeout-secs",
        "8",
    ]);
    assert!(
        output.status.success(),
        "setup failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("up to 7s"), "{stdout}");

    let config: Value =
        serde_json::from_str(&fs::read_to_string(env.config_path()).expect("config"))
            .expect("config JSON");
    assert_eq!(config["startupTimeoutSecs"], 7);
    assert_eq!(config["requestTimeoutSecs"], 8);
}

#[test]
fn non_loopback_listen_is_rejected_before_start() {
    let env = TestEnv::new();
    env.setup();
    let output = env.run(&["start", "--foreground", "--listen", "0.0.0.0:8765"]);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("loopback"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!Path::new(&env.home.join("run/bridge.pid")).exists());

    let doctor = env.run(&["doctor", "--listen", "0.0.0.0:8765"]);
    assert!(!doctor.status.success());
    assert!(
        String::from_utf8_lossy(&doctor.stderr).contains("loopback"),
        "{}",
        String::from_utf8_lossy(&doctor.stderr)
    );
}
