//! Minimal OMP JSONL framing and response correlation.
//!
//! This is a Phase 0 spike. It is intentionally small; promote the proven parts
//! into `apps/bridge` and delete the rest once the bridge foundation exists.

use base64::Engine;
use serde_json::Value;
use std::collections::HashSet;

pub const DEFAULT_MAX_REASSEMBLED_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameError {
    InvalidJson(String),
    InvalidChunk(String),
    OversizedFrame { limit: usize },
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidJson(message) => write!(f, "invalid JSON frame: {message}"),
            Self::InvalidChunk(message) => write!(f, "invalid rpc_chunk: {message}"),
            Self::OversizedFrame { limit } => write!(f, "frame exceeds {limit} bytes"),
        }
    }
}

impl std::error::Error for FrameError {}

/// Parse one physical stdout line. Blank lines are ignored.
pub fn parse_jsonl_line(line: &str) -> Result<Option<Value>, FrameError> {
    let line = line.trim();
    if line.is_empty() {
        return Ok(None);
    }
    serde_json::from_str(line)
        .map(Some)
        .map_err(|error| FrameError::InvalidJson(error.to_string()))
}

#[derive(Debug)]
struct ActiveChunk {
    id: String,
    count: usize,
    byte_length: usize,
    next_index: usize,
    data: Vec<u8>,
}

/// Reassemble protocol v2 `rpc_chunk` frames with bounded, strict validation.
#[derive(Debug)]
pub struct ChunkAssembler {
    limit: usize,
    active: Option<ActiveChunk>,
}

impl ChunkAssembler {
    pub fn new(limit: usize) -> Self {
        Self {
            limit,
            active: None,
        }
    }

    /// True while an unfinished chunk sequence is buffered.
    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }

    /// Feed one `rpc_chunk` frame. Returns a completed logical frame, or
    /// `None` while the sequence is still in progress. Ordinary frames are
    /// not consumed; callers must reject them while [`Self::is_active`].
    pub fn push(&mut self, frame: &Value) -> Result<Option<Value>, FrameError> {
        if frame.get("type").and_then(Value::as_str) != Some("rpc_chunk") {
            return Ok(None);
        }

        let id = required_str(frame, "chunkId")?.to_owned();
        if id.is_empty() {
            return Err(FrameError::InvalidChunk("empty chunkId".into()));
        }
        let index = required_usize(frame, "index")?;
        let count = required_usize(frame, "count")?;
        let byte_length = required_usize(frame, "byteLength")?;
        if count == 0 || index >= count {
            return Err(FrameError::InvalidChunk("index/count out of range".into()));
        }
        if count > self.limit || byte_length > self.limit {
            return Err(FrameError::OversizedFrame { limit: self.limit });
        }
        let encoded = required_str(frame, "data")?;
        let data = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|error| FrameError::InvalidChunk(format!("bad base64: {error}")))?;

        if self.active.is_none() {
            if index != 0 {
                return Err(FrameError::InvalidChunk(
                    "chunk sequence must start at 0".into(),
                ));
            }
            if data.len() > self.limit {
                return Err(FrameError::OversizedFrame { limit: self.limit });
            }
            if count == 1 {
                return finish_chunk(id, byte_length, data).map(Some);
            }
            self.active = Some(ActiveChunk {
                id,
                count,
                byte_length,
                next_index: 1,
                data,
            });
            return Ok(None);
        }

        let active = self.active.as_mut().expect("checked above");
        if active.id != id {
            return Err(FrameError::InvalidChunk("interleaved chunkId".into()));
        }
        if active.count != count || active.byte_length != byte_length {
            return Err(FrameError::InvalidChunk(
                "chunk metadata changed mid-sequence".into(),
            ));
        }
        if index != active.next_index {
            return Err(FrameError::InvalidChunk("unexpected chunk index".into()));
        }
        if active.data.len() + data.len() > self.limit {
            return Err(FrameError::OversizedFrame { limit: self.limit });
        }
        active.data.extend_from_slice(&data);
        active.next_index += 1;
        if active.next_index < active.count {
            return Ok(None);
        }

        let active = self.active.take().expect("active chunk");
        finish_chunk(active.id, active.byte_length, active.data).map(Some)
    }
}

fn finish_chunk(id: String, byte_length: usize, data: Vec<u8>) -> Result<Value, FrameError> {
    if data.len() != byte_length {
        return Err(FrameError::InvalidChunk(format!(
            "chunk {id} decoded to {} bytes, expected {byte_length}",
            data.len()
        )));
    }
    let text = String::from_utf8(data)
        .map_err(|error| FrameError::InvalidChunk(format!("chunk {id} is not UTF-8: {error}")))?;
    serde_json::from_str(&text).map_err(|error| FrameError::InvalidJson(error.to_string()))
}

fn required_str<'a>(frame: &'a Value, key: &str) -> Result<&'a str, FrameError> {
    frame
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| FrameError::InvalidChunk(format!("missing string {key}")))
}

fn required_usize(frame: &Value, key: &str) -> Result<usize, FrameError> {
    frame
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| FrameError::InvalidChunk(format!("missing integer {key}")))
}

/// Tracks outbound request IDs and resolves only matching responses.
#[derive(Debug, Default)]
pub struct ResponseCorrelator {
    pending: HashSet<String>,
}

impl ResponseCorrelator {
    pub fn register(&mut self, id: impl Into<String>) {
        self.pending.insert(id.into());
    }

    pub fn accept(&mut self, frame: &Value) -> Option<Value> {
        if frame.get("type").and_then(Value::as_str) != Some("response") {
            return None;
        }
        let id = frame.get("id").and_then(Value::as_str)?;
        self.pending.remove(id).then(|| frame.clone())
    }

    pub fn is_pending(&self, id: &str) -> bool {
        self.pending.contains(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::STANDARD;
    use serde_json::json;

    #[test]
    fn blank_lines_are_ignored() {
        assert_eq!(parse_jsonl_line("  \r\n").unwrap(), None);
    }

    #[test]
    fn valid_line_parses_to_object() {
        let parsed = parse_jsonl_line(r#"{"type":"ready","protocolVersion":1}"#)
            .unwrap()
            .unwrap();
        assert_eq!(parsed["type"], "ready");
        assert_eq!(parsed["protocolVersion"], 1);
    }

    #[test]
    fn malformed_json_is_reported() {
        let error = parse_jsonl_line("{not json").unwrap_err();
        assert!(matches!(error, FrameError::InvalidJson(_)));
    }

    fn chunk(id: &str, index: usize, count: usize, byte_length: usize, data: &[u8]) -> Value {
        json!({
            "type": "rpc_chunk",
            "chunkId": id,
            "index": index,
            "count": count,
            "byteLength": byte_length,
            "data": STANDARD.encode(data),
        })
    }

    #[test]
    fn chunks_reassemble_in_order() {
        let payload = br#"{"type":"response","id":"r1","success":true}"#;
        let (left, right) = payload.split_at(17);
        let mut assembler = ChunkAssembler::new(DEFAULT_MAX_REASSEMBLED_BYTES);

        assert_eq!(
            assembler
                .push(&chunk("c1", 0, 2, payload.len(), left))
                .unwrap(),
            None
        );
        let complete = assembler
            .push(&chunk("c1", 1, 2, payload.len(), right))
            .unwrap()
            .expect("completed frame");
        assert_eq!(complete["type"], "response");
        assert_eq!(complete["id"], "r1");
    }

    #[test]
    fn active_state_tracks_chunk_sequence() {
        let mut assembler = ChunkAssembler::new(DEFAULT_MAX_REASSEMBLED_BYTES);
        assert!(!assembler.is_active());
        assembler.push(&chunk("c1", 0, 2, 2, b"{")).unwrap();
        assert!(assembler.is_active());
        assembler.push(&chunk("c1", 1, 2, 2, b"}")).unwrap();
        assert!(!assembler.is_active());
    }

    #[test]
    fn mismatched_chunk_id_is_rejected() {
        let mut assembler = ChunkAssembler::new(DEFAULT_MAX_REASSEMBLED_BYTES);
        assembler.push(&chunk("c1", 0, 2, 10, b"hello")).unwrap();
        let error = assembler
            .push(&chunk("c2", 1, 2, 10, b"world"))
            .unwrap_err();
        assert!(matches!(error, FrameError::InvalidChunk(_)));
    }

    #[test]
    fn out_of_order_chunk_is_rejected() {
        let mut assembler = ChunkAssembler::new(DEFAULT_MAX_REASSEMBLED_BYTES);
        let error = assembler
            .push(&chunk("c1", 1, 2, 10, b"world"))
            .unwrap_err();
        assert!(matches!(error, FrameError::InvalidChunk(_)));
    }

    #[test]
    fn advertised_length_mismatch_is_rejected() {
        let mut assembler = ChunkAssembler::new(DEFAULT_MAX_REASSEMBLED_BYTES);
        let error = assembler
            .push(&chunk("c1", 0, 1, 99, b"hello"))
            .unwrap_err();
        assert!(matches!(error, FrameError::InvalidChunk(_)));
    }

    #[test]
    fn oversized_reassembly_is_rejected_before_allocating() {
        let mut assembler = ChunkAssembler::new(4);
        let error = assembler.push(&chunk("c1", 0, 1, 5, b"hello")).unwrap_err();
        assert!(matches!(error, FrameError::OversizedFrame { limit: 4 }));
    }

    #[test]
    fn responses_resolve_by_id_regardless_of_order() {
        let mut correlator = ResponseCorrelator::default();
        correlator.register("first");
        correlator.register("second");

        let second = json!({"type":"response","id":"second","success":true});
        let first = json!({"type":"response","id":"first","success":true});

        assert_eq!(correlator.accept(&second).unwrap()["id"], "second");
        assert!(correlator.is_pending("first"));
        assert!(!correlator.is_pending("second"));
        assert_eq!(correlator.accept(&first).unwrap()["id"], "first");
    }

    #[test]
    fn unknown_and_non_response_frames_are_not_resolved() {
        let mut correlator = ResponseCorrelator::default();
        correlator.register("known");
        assert!(
            correlator
                .accept(&json!({"type":"response","id":"other"}))
                .is_none()
        );
        assert!(
            correlator
                .accept(&json!({"type":"message_update","id":"known"}))
                .is_none()
        );
        assert!(correlator.accept(&json!({"type":"response"})).is_none());
        assert!(correlator.is_pending("known"));
    }
}
