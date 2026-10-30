//! Deterministic child-process tests using the explicitly labeled fake OMP
//! fixture in `tests/fixtures/fake-omp.sh`. These tests prove bridge lifecycle
//! behavior; they are not evidence that the real OMP engine works.

use omp_bridge::api_types::EnginePhase;
use omp_bridge::config::{BridgeConfig, DEFAULT_MAX_REASSEMBLED_BYTES};
use omp_bridge::engine::{Engine, EngineConfig};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tempfile::TempDir;

struct Fixture {
    _dir: TempDir,
    script: PathBuf,
    workspace: PathBuf,
}

impl Fixture {
    fn new(mode: &str) -> Self {
        let dir = tempfile::tempdir().expect("temp dir");
        let script = dir.path().join("fake-omp.sh");
        let content = include_str!("fixtures/fake-omp.sh").replace("__MODE__", mode);
        fs::write(&script, content).expect("write fixture");
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).expect("chmod");
        let workspace = dir.path().join("workspace");
        fs::create_dir(&workspace).expect("workspace");
        Self {
            _dir: dir,
            script,
            workspace,
        }
    }

    fn config(&self) -> BridgeConfig {
        BridgeConfig {
            omp_bin: self.script.clone(),
            omp_args: vec!["--mode".to_owned(), "rpc-ui".to_owned()],
            workspace: self.workspace.clone(),
            listen: "127.0.0.1:0".parse().unwrap(),
            allowed_origins: vec!["http://tauri.localhost".to_owned()],
            startup_timeout: Duration::from_secs(3),
            request_timeout: Duration::from_secs(2),
            pairing_ttl: Duration::from_secs(60),
            token_ttl: Duration::from_secs(3600),
            max_frame_bytes: 1024 * 1024,
            max_reassembled_bytes: DEFAULT_MAX_REASSEMBLED_BYTES,
        }
    }

    fn pid_file(&self) -> PathBuf {
        self.script.with_extension("sh.pid")
    }

    fn pid(&self) -> Option<i32> {
        fs::read_to_string(self.pid_file())
            .ok()
            .and_then(|value| value.trim().parse().ok())
    }
}

async fn start_fixture(mode: &str) -> (Fixture, Engine) {
    let fixture = Fixture::new(mode);
    let config = EngineConfig::from(&fixture.config());
    let engine = Engine::start(config).await;
    (fixture, engine)
}

fn process_alive(pid: i32) -> bool {
    // SAFETY: signal 0 performs an existence check for the fixture child.
    unsafe { libc::kill(pid, 0) == 0 }
}

async fn wait_until_gone(pid: i32) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if !process_alive(pid) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("fixture child {pid} was left running");
}

#[tokio::test]
async fn real_ready_negotiation_and_state_are_required_before_ready() {
    let (fixture, engine) = start_fixture("happy").await;
    let status = engine.status();
    assert_eq!(status.state, EnginePhase::Ready);
    assert!(status.ready);
    assert_eq!(status.negotiated_protocol_version, Some(2));
    let summary = status.summary.expect("state summary");
    assert_eq!(summary.model.as_ref().unwrap().provider, "fixture");
    assert_eq!(summary.model.as_ref().unwrap().id, "test-model");
    assert_eq!(summary.message_count, Some(0));

    let pid = fixture.pid().expect("fixture pid");
    engine.shutdown().await;
    wait_until_gone(pid).await;
}

#[tokio::test]
async fn protocol_v1_is_used_when_v2_is_not_advertised() {
    let (fixture, engine) = start_fixture("v1").await;
    let status = engine.status();
    assert_eq!(status.state, EnginePhase::Ready);
    assert_eq!(status.negotiated_protocol_version, Some(1));
    let pid = fixture.pid().expect("fixture pid");
    engine.shutdown().await;
    wait_until_gone(pid).await;
}

#[tokio::test]
async fn malformed_stdout_frame_does_not_hide_a_later_ready_frame() {
    let (fixture, engine) = start_fixture("malformed").await;
    assert_eq!(engine.status().state, EnginePhase::Ready);
    let pid = fixture.pid().expect("fixture pid");
    engine.shutdown().await;
    wait_until_gone(pid).await;
}

#[tokio::test]
async fn real_stdout_events_reach_subscribers() {
    let (fixture, engine) = start_fixture("events").await;
    let mut events = engine.subscribe();
    let event = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let event = events.recv().await.expect("event channel");
            if event.event_type == "omp.event" {
                return event;
            }
        }
    })
    .await
    .expect("fixture event timeout");
    assert_eq!(event.payload["rawType"], "fixture_event");
    let pid = fixture.pid().expect("fixture pid");
    engine.shutdown().await;
    wait_until_gone(pid).await;
}

#[tokio::test]
async fn missing_binary_is_distinct_from_protocol_failure() {
    let config = EngineConfig {
        omp_bin: PathBuf::from("/tmp/definitely-missing-omp-binary"),
        omp_args: vec!["--mode".to_owned(), "rpc-ui".to_owned()],
        workspace: std::env::current_dir().unwrap(),
        startup_timeout: Duration::from_secs(2),
        request_timeout: Duration::from_secs(1),
        max_frame_bytes: 1024 * 1024,
        max_reassembled_bytes: DEFAULT_MAX_REASSEMBLED_BYTES,
    };
    let engine = Engine::start(config).await;
    let status = engine.status();
    assert_eq!(status.state, EnginePhase::Failed);
    assert!(!status.ready);
    assert_eq!(status.error.unwrap().code, "OMP_NOT_FOUND");
    engine.shutdown().await;
}

#[tokio::test]
async fn missing_model_configuration_is_reported_separately() {
    let (fixture, engine) = start_fixture("no-model").await;
    let status = engine.status();
    assert_eq!(status.state, EnginePhase::Failed);
    let error = status.error.expect("startup error");
    assert_eq!(error.code, "OMP_MISSING_MODEL");
    assert!(error.message.contains("configuration error"));
    assert!(!error.message.contains("RPC protocol failure"));
    if let Some(pid) = fixture.pid() {
        wait_until_gone(pid).await;
    }
    engine.shutdown().await;
}

#[tokio::test]
async fn startup_timeout_kills_the_child() {
    let fixture = Fixture::new("timeout");
    let mut config = fixture.config();
    config.startup_timeout = Duration::from_millis(200);
    let engine = Engine::start(EngineConfig::from(&config)).await;
    let status = engine.status();
    assert_eq!(status.state, EnginePhase::Failed);
    assert_eq!(status.error.unwrap().code, "OMP_START_TIMEOUT");
    if let Some(pid) = fixture.pid() {
        wait_until_gone(pid).await;
    }
    engine.shutdown().await;
}

#[tokio::test]
async fn unsupported_protocol_versions_fail_closed() {
    let (fixture, engine) = start_fixture("unsupported").await;
    let status = engine.status();
    assert_eq!(status.state, EnginePhase::Failed);
    assert_eq!(status.error.unwrap().code, "RPC_UNSUPPORTED_PROTOCOL");
    if let Some(pid) = fixture.pid() {
        wait_until_gone(pid).await;
    }
    engine.shutdown().await;
}

#[tokio::test]
async fn rejected_v2_negotiation_fails_closed() {
    let (fixture, engine) = start_fixture("negotiate-fail").await;
    let status = engine.status();
    assert_eq!(status.state, EnginePhase::Failed);
    assert_eq!(status.error.unwrap().code, "RPC_NEGOTIATION_FAILED");
    if let Some(pid) = fixture.pid() {
        wait_until_gone(pid).await;
    }
    engine.shutdown().await;
}

#[tokio::test]
async fn rejected_state_request_fails_closed() {
    let (fixture, engine) = start_fixture("state-fail").await;
    let status = engine.status();
    assert_eq!(status.state, EnginePhase::Failed);
    assert_eq!(status.error.unwrap().code, "RPC_STATE_FAILED");
    if let Some(pid) = fixture.pid() {
        wait_until_gone(pid).await;
    }
    engine.shutdown().await;
}

#[tokio::test]
async fn ready_frame_larger_than_configured_limit_fails_closed() {
    let fixture = Fixture::new("happy");
    let mut config = fixture.config();
    config.max_frame_bytes = 64;
    let engine = Engine::start(EngineConfig::from(&config)).await;
    let status = engine.status();
    assert_eq!(status.state, EnginePhase::Failed);
    assert_eq!(status.error.unwrap().code, "RPC_PROTOCOL_ERROR");
    if let Some(pid) = fixture.pid() {
        wait_until_gone(pid).await;
    }
    engine.shutdown().await;
}

#[allow(dead_code)]
fn assert_file(path: &Path) {
    assert!(path.is_file(), "{} is not a file", path.display());
}
