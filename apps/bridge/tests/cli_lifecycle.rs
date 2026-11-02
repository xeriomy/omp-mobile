//! CLI lifecycle tests against the labeled fake OMP fixture.
//!
//! These prove setup, background start, status, logs, stop, and safe uninstall
//! behavior without claiming real-OMP evidence.

use serde_json::Value;
use std::fs;
use std::io::BufRead;
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
            // Discovery's known-locations fallback resolves against the real
            // HOME; pin HOME to the (empty) test home so a host-installed OMP
            // cannot leak into the tests.
            .env("HOME", &self.home)
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

fn bin_dir(dir: &Path, name: &str) -> PathBuf {
    let bin = dir.join(name);
    fs::create_dir_all(&bin).expect("bin dir");
    bin
}

/// First-run initialization inside `start`: discovery, validation, atomic save,
/// and a background start without running `setup` first.
#[test]
fn first_run_start_discovers_omp_saves_config_and_starts() {
    let mut env = TestEnv::new();
    let bin = bin_dir(env.home.parent().expect("test dir"), "first-run-bin");
    write_fake_omp(&bin.join("omp"));

    let output = env
        .command()
        .current_dir(&env.workspace)
        .env("PATH", path_with(&bin))
        .args(["start", "--listen", "127.0.0.1:0"])
        .output()
        .expect("run first-run start");
    assert!(
        output.status.success(),
        "first-run start failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("First run detected"), "{stdout}");
    assert!(stdout.contains("Initializing bridge"), "{stdout}");
    assert!(
        stdout.contains(bin.join("omp").to_str().unwrap()),
        "{stdout}"
    );
    let pairing = stdout
        .lines()
        .find_map(|line| line.trim().strip_prefix("Pairing:   "))
        .and_then(|rest| rest.split_whitespace().next())
        .expect("pairing code");
    assert_eq!(pairing.len(), 10);
    env.started = true;

    let saved = fs::read_to_string(env.config_path()).expect("config saved");
    let config: Value = serde_json::from_str(&saved).expect("config JSON");
    assert_eq!(
        config["ompBin"],
        bin.join("omp").to_str().unwrap().to_owned()
    );
    assert_eq!(
        config["workspace"],
        env.workspace.canonicalize().unwrap().to_str().unwrap()
    );
    assert!(config.get("model").is_none(), "model must not be invented");
    assert!(config.get("pairingCode").is_none());
    assert!(config.get("token").is_none());
    let mode = fs::metadata(env.config_path())
        .expect("config metadata")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);

    let status = env.status_json();
    assert_eq!(status["phase"], "engine_ready");

    // A second start must not repeat first-run initialization.
    let again = env.run(&["start", "--listen", "127.0.0.1:0"]);
    assert!(!again.status.success());
    assert!(
        String::from_utf8_lossy(&again.stderr).contains("already running"),
        "{}",
        String::from_utf8_lossy(&again.stderr)
    );
    assert!(
        !String::from_utf8_lossy(&again.stdout).contains("First run detected"),
        "{}",
        String::from_utf8_lossy(&again.stdout)
    );
}

#[test]
fn first_run_after_stop_reuses_the_saved_configuration() {
    let mut env = TestEnv::new();
    let bin = bin_dir(env.home.parent().expect("test dir"), "reuse-bin");
    write_fake_omp(&bin.join("omp"));

    let first = env
        .command()
        .current_dir(&env.workspace)
        .env("PATH", path_with(&bin))
        .args(["start", "--listen", "127.0.0.1:0"])
        .output()
        .expect("first start");
    assert!(
        first.status.success(),
        "first start failed: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    env.started = true;
    env.stop();

    let second = env
        .command()
        .current_dir(&env.workspace)
        .env("PATH", path_with(&bin))
        .args(["start", "--listen", "127.0.0.1:0"])
        .output()
        .expect("second start");
    assert!(
        second.status.success(),
        "second start failed: {}",
        String::from_utf8_lossy(&second.stderr)
    );
    env.started = true;
    let stdout = String::from_utf8_lossy(&second.stdout);
    assert!(!stdout.contains("First run detected"), "{stdout}");
    let status = env.status_json();
    assert_eq!(status["phase"], "engine_ready");

    // `restart` also goes through the saved configuration.
    env.stop();
    let third = env
        .command()
        .current_dir(&env.workspace)
        .env("PATH", path_with(&bin))
        .args(["restart", "--listen", "127.0.0.1:0"])
        .output()
        .expect("restart");
    assert!(
        third.status.success(),
        "restart failed: {}",
        String::from_utf8_lossy(&third.stderr)
    );
    env.started = true;
    let stdout = String::from_utf8_lossy(&third.stdout);
    assert!(!stdout.contains("First run detected"), "{stdout}");
    let status = env.status_json();
    assert_eq!(status["phase"], "engine_ready");
}

#[test]
fn first_run_foreground_initializes_serves_and_shuts_down() {
    let env = TestEnv::new();
    let bin = bin_dir(env.home.parent().expect("test dir"), "fg-bin");
    write_fake_omp(&bin.join("omp"));

    let mut child = env
        .command()
        .current_dir(&env.workspace)
        .env("PATH", path_with(&bin))
        .args(["start", "--foreground", "--listen", "127.0.0.1:0"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn foreground bridge");
    let stdout = child.stdout.take().expect("foreground stdout");
    let mut lines = std::io::BufReader::new(stdout).lines();

    let mut saw_first_run = false;
    let mut saw_pairing = false;
    for line in lines.by_ref() {
        let Ok(line) = line else { break };
        if line.contains("First run detected") {
            saw_first_run = true;
        }
        if line.contains("Pairing:   ") {
            saw_pairing = true;
            break;
        }
    }

    #[cfg(unix)]
    if saw_pairing {
        // SAFETY: normal vt100-style graceful interruption of our own child.
        let sent = unsafe { libc::kill(child.id() as i32, libc::SIGINT) };
        assert_eq!(sent, 0, "cannot interrupt the foreground bridge");
    }
    let status = child.wait().expect("foreground bridge exit");
    assert!(saw_first_run, "foreground did not run first-run init");
    assert!(saw_pairing, "foreground did not serve pairing: {status}");
    assert!(status.success(), "foreground bridge failed: {status}");
    assert!(
        env.config_path().exists(),
        "foreground first run saved no config"
    );
}

#[test]
fn first_run_without_omp_fails_without_saving_a_configuration() {
    let env = TestEnv::new();
    let output = env
        .command()
        .current_dir(&env.workspace)
        .env("PATH", "/usr/bin:/bin")
        .args(["start", "--listen", "127.0.0.1:0"])
        .output()
        .expect("run start without OMP");
    assert!(!output.status.success(), "start succeeded without OMP");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(combined.contains("OMP_NOT_FOUND"), "{combined}");
    assert!(combined.contains("Install OMP"), "{combined}");
    assert!(combined.contains("--omp-bin"), "{combined}");
    assert!(combined.contains("OMP discovery:"), "{combined}");
    assert!(
        !env.config_path().exists(),
        "start without OMP saved a config"
    );
    let status = env.status_json();
    assert_eq!(status["phase"], "not_configured");
}

#[test]
fn first_run_with_a_non_executable_omp_is_reported() {
    let env = TestEnv::new();
    let bin = bin_dir(env.home.parent().expect("test dir"), "noexec-bin");
    write_fake_omp(&bin.join("omp"));
    fs::set_permissions(bin.join("omp"), fs::Permissions::from_mode(0o644)).expect("chmod 644");

    let output = env
        .command()
        .current_dir(&env.workspace)
        .env("PATH", path_with(&bin))
        .args(["start", "--listen", "127.0.0.1:0"])
        .output()
        .expect("run start with non-executable OMP");
    assert!(!output.status.success());

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(combined.contains("OMP_NOT_EXECUTABLE"), "{combined}");
    assert!(
        !env.config_path().exists(),
        "non-executable OMP produced config"
    );
}

#[test]
fn first_run_with_an_unsupported_version_is_reported() {
    let env = TestEnv::new();
    let bin = bin_dir(env.home.parent().expect("test dir"), "oldversion-bin");
    let omp = bin.join("omp");
    fs::write(&omp, "#!/bin/sh\nprintf '%s\\n' 'omp/17.9.9'\n").expect("fixture");
    fs::set_permissions(&omp, fs::Permissions::from_mode(0o755)).expect("chmod fixture");

    let output = env
        .command()
        .current_dir(&env.workspace)
        .env("PATH", path_with(&bin))
        .args(["start", "--listen", "127.0.0.1:0"])
        .output()
        .expect("run start with unsupported OMP");
    assert!(!output.status.success());

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(combined.contains("OMP_UNSUPPORTED_VERSION"), "{combined}");
    assert!(
        !env.config_path().exists(),
        "unsupported OMP version produced config"
    );
}

#[test]
fn first_run_replying_on_a_missing_model_is_actionable() {
    let env = TestEnv::new();
    let bin = bin_dir(env.home.parent().expect("test dir"), "no-model-bin");
    let omp = bin.join("omp");
    let script = include_str!("fixtures/fake-omp.sh").replace("__MODE__", "no-model");
    fs::write(&omp, script).expect("fixture");
    fs::set_permissions(&omp, fs::Permissions::from_mode(0o755)).expect("chmod fixture");

    let output = env
        .command()
        .current_dir(&env.workspace)
        .env("PATH", path_with(&bin))
        .args(["start", "--listen", "127.0.0.1:0"])
        .output()
        .expect("run start without a model");
    assert!(!output.status.success(), "start succeeded without a model");

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(combined.contains("OMP_MISSING_MODEL"), "{combined}");
    assert!(combined.contains("Configure the model"), "{combined}");
    assert!(
        !env.config_path().exists(),
        "model-less start must not persist configuration"
    );
}

#[test]
fn first_run_explicit_omp_bin_wins_over_path_and_ambiguity_is_deterministic() {
    let mut env = TestEnv::new();
    let explicit_dir = bin_dir(env.home.parent().expect("test dir"), "explicit-bin");
    write_fake_omp(&explicit_dir.join("omp"));
    let first_dir = bin_dir(env.home.parent().expect("test dir"), "path-first-bin");
    write_fake_omp(&first_dir.join("omp"));
    let second_dir = bin_dir(env.home.parent().expect("test dir"), "path-second-bin");
    write_fake_omp(&second_dir.join("omp"));

    let path = format!(
        "{}:{}:/usr/bin:/bin",
        first_dir.display(),
        second_dir.display()
    );
    let output = env
        .command()
        .current_dir(&env.workspace)
        .env("PATH", path_with(&explicit_dir))
        .args([
            "start",
            "--omp-bin",
            explicit_dir.join("omp").to_str().unwrap(),
            "--listen",
            "127.0.0.1:0",
        ])
        .output()
        .expect("run explicit first-run start");
    assert!(
        output.status.success(),
        "explicit first-run start failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    env.started = true;
    let config: Value =
        serde_json::from_str(&fs::read_to_string(env.config_path()).expect("config"))
            .expect("config JSON");
    assert_eq!(
        config["ompBin"],
        explicit_dir.join("omp").to_str().unwrap().to_owned()
    );

    let other = TestEnv::new();
    let mut env = other;
    let output = env
        .command()
        .current_dir(&env.workspace)
        .env("PATH", path)
        .args(["start", "--listen", "127.0.0.1:0"])
        .output()
        .expect("run ambiguous first-run start");
    assert!(
        output.status.success(),
        "ambiguous first-run start failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    env.started = true;
    let config: Value =
        serde_json::from_str(&fs::read_to_string(env.config_path()).expect("config"))
            .expect("config JSON");
    assert_eq!(
        config["ompBin"],
        first_dir.join("omp").to_str().unwrap().to_owned()
    );
}

#[test]
fn corrupt_saved_configuration_is_reported_and_never_replaced() {
    let env = TestEnv::new();
    fs::create_dir_all(&env.home).expect("home dir");
    fs::write(env.config_path(), "{ this is not json").expect("corrupt config");
    let before = fs::read_to_string(env.config_path()).expect("config bytes");

    let bin = bin_dir(env.home.parent().expect("test dir"), "corrupt-bin");
    write_fake_omp(&bin.join("omp"));

    let output = env
        .command()
        .env("PATH", path_with(&bin))
        .args(["start", "--listen", "127.0.0.1:0"])
        .output()
        .expect("run start over corrupt config");
    assert!(!output.status.success());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("invalid"), "{stderr}");
    assert!(stderr.contains("omp-bridge setup"), "{stderr}");
    assert_eq!(
        fs::read_to_string(env.config_path()).expect("config bytes"),
        before,
        "corrupt configuration was replaced"
    );
}

#[test]
fn concurrent_first_run_starts_exactly_one_daemon() {
    let env = TestEnv::new();
    let bin = bin_dir(env.home.parent().expect("test dir"), "race-bin");
    write_fake_omp(&bin.join("omp"));

    let spawn_one = |env: &TestEnv| {
        env.command()
            .current_dir(&env.workspace)
            .env("PATH", path_with(&bin))
            .args(["start", "--listen", "127.0.0.1:0"])
            .output()
            .expect("spawn first-run start")
    };
    let (left, right) = std::thread::scope(|scope| {
        let left = scope.spawn(|| spawn_one(&env));
        let right = scope.spawn(|| spawn_one(&env));
        (
            left.join().expect("left thread"),
            right.join().expect("right thread"),
        )
    });

    let successes = [left, right]
        .into_iter()
        .filter(|output| output.status.success())
        .count();
    let mut env = env;
    assert_eq!(
        successes, 1,
        "concurrent first-run starts produced {} daemons",
        successes
    );
    let status = env.status_json();
    assert_eq!(status["phase"], "engine_ready");
    env.started = true;
    env.stop();
    let stopped = env.status_json();
    assert_eq!(stopped["phase"], "stopped");
}
