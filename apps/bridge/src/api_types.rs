//! Stable DTOs for the versioned bridge API and event envelope.
//!
//! These types intentionally contain no provider credentials, filesystem
//! paths, prompts, or raw `get_state` payloads.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const API_VERSION: &str = "v1";
pub const EVENT_VERSION: u8 = 1;
pub const BRIDGE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HealthResponse {
    pub status: String,
}

impl HealthResponse {
    pub fn ok() -> Self {
        Self {
            status: "ok".to_owned(),
        }
    }
}

/// Unauthenticated identity document used by the mobile client to confirm that
/// a loopback listener is an OMP bridge before pairing.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CapabilitiesResponse {
    pub product: String,
    pub api_version: String,
    pub bridge_version: String,
    pub event_version: u8,
    pub features: Vec<String>,
}

impl CapabilitiesResponse {
    pub fn current() -> Self {
        Self {
            product: "omp-bridge".to_owned(),
            api_version: API_VERSION.to_owned(),
            bridge_version: BRIDGE_VERSION.to_owned(),
            event_version: EVENT_VERSION,
            features: vec![
                "pairing".to_owned(),
                "status".to_owned(),
                "events".to_owned(),
                "token-revoke".to_owned(),
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PairRequest {
    pub code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PairResponse {
    pub token: String,
    pub token_id: String,
    pub expires_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelSummary {
    pub provider: String,
    pub id: String,
}

/// Redacted projection of OMP `get_state`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SafeStateSummary {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_streaming: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_compacting: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_settled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queued_message_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_pending_async_work: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_name: Option<String>,
}

impl SafeStateSummary {
    pub fn from_get_state(data: &Value) -> Self {
        let model = data.get("model").and_then(|model| {
            let provider = model.get("provider")?.as_str()?;
            let id = model.get("id")?.as_str()?;
            Some(ModelSummary {
                provider: provider.to_owned(),
                id: id.to_owned(),
            })
        });
        Self {
            model,
            thinking_level: string_field(data, "thinkingLevel"),
            is_streaming: bool_field(data, "isStreaming"),
            is_compacting: bool_field(data, "isCompacting"),
            is_settled: bool_field(data, "isSettled"),
            message_count: u64_field(data, "messageCount"),
            queued_message_count: u64_field(data, "queuedMessageCount"),
            has_pending_async_work: bool_field(data, "hasPendingAsyncWork"),
            session_id: string_field(data, "sessionId"),
            session_name: string_field(data, "sessionName"),
        }
    }
}

fn string_field(data: &Value, key: &str) -> Option<String> {
    data.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn bool_field(data: &Value, key: &str) -> Option<bool> {
    data.get(key).and_then(Value::as_bool)
}

fn u64_field(data: &Value, key: &str) -> Option<u64> {
    data.get(key).and_then(Value::as_u64)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EngineLimits {
    pub max_frame_bytes: usize,
    pub max_reassembled_frame_bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReadyInfo {
    pub protocol_version: u32,
    pub supported_protocol_versions: Vec<u32>,
    pub limits: EngineLimits,
}

impl ReadyInfo {
    pub fn from_frame(frame: &Value) -> Option<Self> {
        let protocol_version = u32::try_from(frame.get("protocolVersion")?.as_u64()?).ok()?;
        let supported_protocol_versions = frame
            .get("supportedProtocolVersions")?
            .as_array()?
            .iter()
            .map(Value::as_u64)
            .collect::<Option<Vec<_>>>()?
            .into_iter()
            .map(|version| u32::try_from(version).ok())
            .collect::<Option<Vec<_>>>()?;
        let max_frame_bytes = usize::try_from(frame.get("maxFrameBytes")?.as_u64()?).ok()?;
        let max_reassembled_frame_bytes =
            usize::try_from(frame.get("maxReassembledFrameBytes")?.as_u64()?).ok()?;
        Some(Self {
            protocol_version,
            supported_protocol_versions,
            limits: EngineLimits {
                max_frame_bytes,
                max_reassembled_frame_bytes,
            },
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EnginePhase {
    Starting,
    Ready,
    Failed,
    Exited,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EngineErrorInfo {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EngineStatus {
    pub state: EnginePhase,
    pub ready: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ready_info: Option<ReadyInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub negotiated_protocol_version: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<SafeStateSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<EngineErrorInfo>,
}

impl EngineStatus {
    pub fn starting() -> Self {
        Self {
            state: EnginePhase::Starting,
            ready: false,
            ready_info: None,
            negotiated_protocol_version: None,
            summary: None,
            error: None,
        }
    }

    pub fn failed(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            state: EnginePhase::Failed,
            ready: false,
            ready_info: None,
            negotiated_protocol_version: None,
            summary: None,
            error: Some(EngineErrorInfo {
                code: code.into(),
                message: message.into(),
            }),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WebSocketInfo {
    pub path: String,
    pub auth: String,
    pub event_version: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StatusResponse {
    pub api_version: String,
    pub bridge_version: String,
    pub listen: String,
    pub engine: EngineStatus,
    pub websocket: WebSocketInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
    pub request_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ErrorEnvelope {
    pub error: ErrorBody,
}

/// Versioned envelope for every WebSocket message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeEvent {
    pub version: u8,
    #[serde(rename = "type")]
    pub event_type: String,
    pub sequence: u64,
    pub occurred_at: u64,
    #[serde(default, skip_serializing_if = "is_false")]
    pub replay: bool,
    pub payload: Value,
}

impl BridgeEvent {
    pub fn new(event_type: impl Into<String>, sequence: u64, payload: Value) -> Self {
        Self {
            version: EVENT_VERSION,
            event_type: event_type.into(),
            sequence,
            occurred_at: unix_millis(),
            replay: false,
            payload,
        }
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}

pub fn unix_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn health_payload_is_minimal() {
        assert_eq!(
            serde_json::to_value(HealthResponse::ok()).unwrap(),
            json!({"status": "ok"})
        );
    }

    #[test]
    fn ready_frame_parses_advertised_limits() {
        let ready = ReadyInfo::from_frame(&json!({
            "type": "ready",
            "protocolVersion": 1,
            "supportedProtocolVersions": [1, 2],
            "maxFrameBytes": 1048576,
            "maxReassembledFrameBytes": 67108864
        }))
        .unwrap();
        assert_eq!(ready.protocol_version, 1);
        assert_eq!(ready.supported_protocol_versions, vec![1, 2]);
        assert_eq!(ready.limits.max_frame_bytes, 1_048_576);
        assert_eq!(ready.limits.max_reassembled_frame_bytes, 67_108_864);
    }

    #[test]
    fn get_state_projection_excludes_provider_details_and_prompts() {
        let summary = SafeStateSummary::from_get_state(&json!({
            "model": {
                "provider": "anthropic",
                "id": "claude-sonnet-4-5",
                "baseUrl": "https://api.anthropic.com",
                "apiKey": "sk-secret"
            },
            "thinkingLevel": "medium",
            "isStreaming": false,
            "isSettled": true,
            "messageCount": 0,
            "systemPrompt": ["secret prompt"],
            "sessionFile": "/home/user/.omp/session.jsonl",
            "sessionId": "session-1"
        }));
        let encoded = serde_json::to_string(&summary).unwrap();
        assert_eq!(
            summary.model,
            Some(ModelSummary {
                provider: "anthropic".to_owned(),
                id: "claude-sonnet-4-5".to_owned()
            })
        );
        assert_eq!(summary.message_count, Some(0));
        assert!(!encoded.contains("sk-secret"));
        assert!(!encoded.contains("api.anthropic.com"));
        assert!(!encoded.contains("systemPrompt"));
        assert!(!encoded.contains("session.jsonl"));
    }

    #[test]
    fn event_envelope_uses_stable_camel_case_fields() {
        let event = BridgeEvent::new("omp.event", 7, json!({"rawType": "ready"}));
        let encoded = serde_json::to_value(event).unwrap();
        assert_eq!(encoded["version"], 1);
        assert_eq!(encoded["type"], "omp.event");
        assert_eq!(encoded["sequence"], 7);
        assert!(encoded["occurredAt"].as_u64().is_some());
        assert_eq!(encoded["payload"]["rawType"], "ready");
        assert!(encoded.get("replay").is_none());
    }

    #[test]
    fn failed_engine_status_never_claims_ready() {
        let status = EngineStatus::failed("OMP_NOT_FOUND", "OMP binary was not found");
        assert!(!status.ready);
        assert_eq!(status.state, EnginePhase::Failed);
        assert_eq!(status.error.unwrap().code, "OMP_NOT_FOUND");
    }
}
