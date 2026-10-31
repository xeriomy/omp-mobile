//! Opt-in end-to-end test against the real OMP executable.
//!
//! Run explicitly:
//!   OMP_BIN=/path/to/omp OMP_MODEL=provider/model \
//!     cargo test -p omp-bridge --test real_omp -- --ignored --nocapture
//!
//! `OMP_MODEL` is optional when the host already has a default model.

use omp_bridge::RunningBridge;
use omp_bridge::api_types::EnginePhase;
use omp_bridge::config::{BridgeConfig, DEFAULT_MAX_REASSEMBLED_BYTES};
use reqwest::{Client, StatusCode};
use serde_json::Value;
use std::path::PathBuf;
use std::time::Duration;

#[tokio::test]
#[ignore = "requires the real OMP binary and a configured model"]
async fn real_omp_boots_negotiates_and_serves_authenticated_status() {
    let omp_bin = std::env::var_os("OMP_BIN").expect("set OMP_BIN to the real OMP executable");
    let mut omp_args = vec!["--mode".to_owned(), "rpc-ui".to_owned()];
    if let Some(model) = std::env::var_os("OMP_MODEL") {
        omp_args.push("--model".to_owned());
        omp_args.push(model.to_string_lossy().into_owned());
    }
    let workspace = std::env::var_os("OMP_WORKSPACE")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().expect("cwd"));

    let config = BridgeConfig {
        omp_bin: PathBuf::from(omp_bin),
        omp_args,
        workspace,
        listen: "127.0.0.1:0".parse().unwrap(),
        allowed_origins: vec!["http://tauri.localhost".to_owned()],
        startup_timeout: Duration::from_secs(90),
        request_timeout: Duration::from_secs(30),
        pairing_ttl: Duration::from_secs(120),
        token_ttl: Duration::from_secs(3600),
        max_frame_bytes: 1024 * 1024,
        max_reassembled_bytes: DEFAULT_MAX_REASSEMBLED_BYTES,
    }
    .validate()
    .expect("valid bridge config");

    let bridge = RunningBridge::start(config).await.expect("bridge start");
    let status = bridge.engine.status();
    assert_eq!(status.state, EnginePhase::Ready, "status: {status:?}");
    assert!(status.ready);
    assert_eq!(status.negotiated_protocol_version, Some(2));
    assert!(status.summary.is_some(), "get_state summary missing");
    let model = status
        .summary
        .as_ref()
        .and_then(|summary| summary.model.as_ref())
        .expect("real OMP reported no model");
    println!("real OMP ready: {}/{}", model.provider, model.id);

    let client = Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("client");
    let pair: Value = client
        .post(format!("{}/api/v1/pair", bridge.base_url()))
        .json(&serde_json::json!({ "code": bridge.pairing_code }))
        .send()
        .await
        .expect("pair")
        .json()
        .await
        .expect("pair body");
    let token = pair["token"].as_str().expect("token").to_owned();

    let response = client
        .get(format!("{}/api/v1/status", bridge.base_url()))
        .bearer_auth(&token)
        .send()
        .await
        .expect("status");
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = response.json().await.expect("status body");
    assert_eq!(body["engine"]["state"], "ready");
    assert_eq!(body["engine"]["ready"], true);
    assert_eq!(body["engine"]["negotiatedProtocolVersion"], 2);
    assert!(!body.to_string().contains("systemPrompt"));
    assert!(!body.to_string().contains("sessionFile"));

    bridge.shutdown().await;
}
