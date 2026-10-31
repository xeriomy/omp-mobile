//! Loopback HTTP/WebSocket tests against the labeled fake OMP fixture.
//!
//! These tests prove authentication, origin/host checks, versioned envelopes,
//! token revocation, and event delivery. They are not real-OMP evidence.

use futures_util::{SinkExt, StreamExt};
use omp_bridge::config::{BridgeConfig, DEFAULT_MAX_REASSEMBLED_BYTES};
use omp_bridge::{RunningBridge, api_types::BridgeEvent};
use reqwest::{Client, StatusCode, header::HOST};
use serde_json::{Value, json};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::Duration;
use tempfile::TempDir;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;

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
            allowed_origins: vec![
                "http://tauri.localhost".to_owned(),
                "https://tauri.localhost".to_owned(),
            ],
            startup_timeout: Duration::from_secs(3),
            request_timeout: Duration::from_secs(2),
            pairing_ttl: Duration::from_secs(120),
            token_ttl: Duration::from_secs(3600),
            max_frame_bytes: 1024 * 1024,
            max_reassembled_bytes: DEFAULT_MAX_REASSEMBLED_BYTES,
        }
    }
}

struct TestBridge {
    _fixture: Fixture,
    bridge: Option<RunningBridge>,
    client: Client,
    base: String,
}

impl TestBridge {
    async fn start(mode: &str) -> Self {
        let fixture = Fixture::new(mode);
        let bridge = RunningBridge::start(fixture.config())
            .await
            .expect("bridge start");
        let base = bridge.base_url();
        let client = Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("client");
        Self {
            _fixture: fixture,
            bridge: Some(bridge),
            client,
            base,
        }
    }

    fn bridge(&self) -> &RunningBridge {
        self.bridge.as_ref().expect("bridge")
    }

    async fn pair(&self, code: &str) -> reqwest::Response {
        self.client
            .post(format!("{}/api/v1/pair", self.base))
            .json(&json!({ "code": code }))
            .send()
            .await
            .expect("pair request")
    }

    async fn token(&self) -> String {
        let response = self.pair(&self.bridge().pairing_code).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body: Value = response.json().await.expect("pair body");
        assert!(body["expiresAt"].as_u64().is_some_and(|value| value > 0));
        body["token"].as_str().expect("token").to_owned()
    }

    async fn status(&self, token: &str) -> reqwest::Response {
        self.client
            .get(format!("{}/api/v1/status", self.base))
            .bearer_auth(token)
            .send()
            .await
            .expect("status request")
    }

    async fn shutdown(mut self) {
        if let Some(bridge) = self.bridge.take() {
            bridge.shutdown().await;
        }
    }
}

async fn next_text(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> String {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(3), socket.next())
            .await
            .expect("websocket frame timeout")
            .expect("websocket closed")
            .expect("websocket error");
        match message {
            Message::Text(text) => return text.to_string(),
            Message::Ping(_) | Message::Pong(_) => continue,
            other => panic!("expected text frame, got {other:?}"),
        }
    }
}

async fn connect(
    bridge: &RunningBridge,
    token: Option<&str>,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let (mut socket, _) = connect_async(format!("ws://{}/api/v1/events", bridge.addr))
        .await
        .expect("websocket connect");
    if let Some(token) = token {
        socket
            .send(Message::Text(
                json!({ "type": "authenticate", "token": token })
                    .to_string()
                    .into(),
            ))
            .await
            .expect("send auth");
    }
    socket
}

#[tokio::test]
async fn health_is_minimal_and_unauthenticated() {
    let bridge = TestBridge::start("happy").await;
    let response = bridge
        .client
        .get(format!("{}/healthz", bridge.base))
        .send()
        .await
        .expect("health request");
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = response.json().await.expect("health body");
    assert_eq!(body, json!({ "status": "ok" }));
    bridge.shutdown().await;
}

#[tokio::test]
async fn capabilities_are_unauthenticated_and_identify_the_bridge() {
    let bridge = TestBridge::start("happy").await;
    let response = bridge
        .client
        .get(format!("{}/api/v1/capabilities", bridge.base))
        .send()
        .await
        .expect("capabilities request");
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = response.json().await.expect("capabilities body");
    assert_eq!(body["product"], "omp-bridge");
    assert_eq!(body["apiVersion"], "v1");
    assert_eq!(body["eventVersion"], 1);
    assert!(
        body["features"]
            .as_array()
            .is_some_and(|features| features.iter().any(|feature| feature == "pairing"))
    );
    let encoded = body.to_string();
    for needle in ["pairingCode", "ompBin", "workspace", "apiKey"] {
        assert!(!encoded.contains(needle), "capabilities leaked {needle}");
    }
    bridge.shutdown().await;
}

#[tokio::test]
async fn pairing_is_single_use_and_issues_a_bearer_token() {
    let bridge = TestBridge::start("happy").await;
    let token = bridge.token().await;
    assert!(!token.is_empty());

    let second = bridge.pair(&bridge.bridge().pairing_code).await;
    assert_eq!(second.status(), StatusCode::UNAUTHORIZED);

    let response = bridge.status(&token).await;
    assert_eq!(response.status(), StatusCode::OK);
    bridge.shutdown().await;
}

#[tokio::test]
async fn status_requires_auth_and_never_leaks_secrets_or_paths() {
    let bridge = TestBridge::start("happy").await;
    let unauthenticated = bridge
        .client
        .get(format!("{}/api/v1/status", bridge.base))
        .send()
        .await
        .expect("status request");
    assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);
    let error: Value = unauthenticated.json().await.expect("error body");
    assert_eq!(error["error"]["code"], "UNAUTHORIZED");
    assert!(
        error["error"]["requestId"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
    );

    let token = bridge.token().await;
    let response = bridge.status(&token).await;
    assert_eq!(response.status(), StatusCode::OK);
    let raw = response.text().await.expect("status body");
    for secret in [
        "must not leak",
        "secret-session.jsonl",
        "systemPrompt",
        "sessionFile",
        "pairing_code",
        "pairingCode",
    ] {
        assert!(!raw.contains(secret), "status leaked {secret:?}: {raw}");
    }
    let body: Value = serde_json::from_str(&raw).expect("status json");
    assert_eq!(body["apiVersion"], "v1");
    assert_eq!(body["engine"]["state"], "ready");
    assert_eq!(body["engine"]["ready"], true);
    assert_eq!(body["engine"]["negotiatedProtocolVersion"], 2);
    assert_eq!(body["engine"]["summary"]["model"]["provider"], "fixture");
    assert_eq!(body["websocket"]["path"], "/api/v1/events");
    assert_eq!(body["websocket"]["auth"], "first-message");
    bridge.shutdown().await;
}

#[tokio::test]
async fn pairing_attempts_are_rate_limited() {
    let bridge = TestBridge::start("happy").await;
    for _ in 0..4 {
        let response = bridge.pair("ZZZZZZZZZZ").await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    let locked = bridge.pair("ZZZZZZZZZZ").await;
    assert_eq!(locked.status(), StatusCode::TOO_MANY_REQUESTS);
    let correct = bridge.pair(&bridge.bridge().pairing_code).await;
    assert_eq!(correct.status(), StatusCode::UNAUTHORIZED);
    bridge.shutdown().await;
}

#[tokio::test]
async fn revoked_token_is_rejected() {
    let bridge = TestBridge::start("happy").await;
    let token = bridge.token().await;
    let response = bridge
        .client
        .post(format!("{}/api/v1/tokens/revoke", bridge.base))
        .bearer_auth(&token)
        .send()
        .await
        .expect("revoke request");
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        bridge.status(&token).await.status(),
        StatusCode::UNAUTHORIZED
    );
    bridge.shutdown().await;
}

#[tokio::test]
async fn exact_origin_allowlist_and_host_guard_are_enforced() {
    let bridge = TestBridge::start("happy").await;
    let allowed = bridge
        .client
        .get(format!("{}/healthz", bridge.base))
        .header("Origin", "http://tauri.localhost")
        .send()
        .await
        .expect("allowed origin");
    assert_eq!(allowed.status(), StatusCode::OK);
    assert_eq!(
        allowed
            .headers()
            .get("access-control-allow-origin")
            .and_then(|value| value.to_str().ok()),
        Some("http://tauri.localhost")
    );

    let denied = bridge
        .client
        .get(format!("{}/healthz", bridge.base))
        .header("Origin", "http://tauri.localhost.evil.example")
        .send()
        .await
        .expect("denied origin");
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    assert!(
        denied
            .headers()
            .get("access-control-allow-origin")
            .is_none()
    );

    let bad_host = bridge
        .client
        .get(format!("{}/healthz", bridge.base))
        .header(HOST, "evil.example")
        .send()
        .await
        .expect("bad host");
    assert_eq!(bad_host.status(), StatusCode::BAD_REQUEST);
    bridge.shutdown().await;
}

#[tokio::test]
async fn websocket_requires_first_message_auth_then_delivers_events() {
    let bridge = TestBridge::start("events").await;
    let token = bridge.token().await;
    let mut socket = connect(bridge.bridge(), Some(&token)).await;

    let first: BridgeEvent =
        serde_json::from_str(&next_text(&mut socket).await).expect("bridge status event");
    assert_eq!(first.event_type, "bridge.status");
    assert_eq!(first.version, 1);
    assert_eq!(first.payload["engine"]["state"], "ready");

    let next: BridgeEvent = serde_json::from_str(&next_text(&mut socket).await).expect("omp event");
    assert_eq!(next.event_type, "omp.event");
    assert_eq!(next.payload["rawType"], "fixture_event");
    assert!(next.sequence > 0);
    let _ = socket.close(None).await;
    bridge.shutdown().await;
}

#[tokio::test]
async fn websocket_rejects_bad_token_and_bad_first_message() {
    let bridge = TestBridge::start("happy").await;
    let mut socket = connect(bridge.bridge(), Some("not-a-token")).await;
    match socket.next().await.expect("close frame").expect("frame") {
        Message::Close(Some(frame)) => {
            assert_eq!(frame.code, CloseCode::Library(4401));
        }
        other => panic!("expected unauthorized close, got {other:?}"),
    }

    let mut socket = connect(bridge.bridge(), None).await;
    socket
        .send(Message::Text("not json".to_owned().into()))
        .await
        .expect("send garbage");
    match socket.next().await.expect("close frame").expect("frame") {
        Message::Close(Some(frame)) => {
            assert_eq!(frame.code, CloseCode::Library(4401));
        }
        other => panic!("expected unauthorized close, got {other:?}"),
    }
    bridge.shutdown().await;
}

#[tokio::test]
async fn websocket_upgrade_rejects_disallowed_origin() {
    let bridge = TestBridge::start("happy").await;
    let mut request = format!("ws://{}/api/v1/events", bridge.bridge().addr)
        .into_client_request()
        .expect("request");
    request
        .headers_mut()
        .insert("origin", "http://evil.example".parse().unwrap());
    let result = connect_async(request).await;
    let error = result.expect_err("origin should be rejected");
    let tokio_tungstenite::tungstenite::Error::Http(response) = error else {
        panic!("expected HTTP rejection, got {error:?}");
    };
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    bridge.shutdown().await;
}
