//! OMP child-process lifecycle and bounded JSONL RPC client.
//!
//! The engine owns exactly one real `omp --mode rpc-ui` process. It never
//! fabricates readiness: the status becomes `Ready` only after the real ready
//! frame, protocol negotiation (when applicable), and `get_state` succeed.

use crate::api_types::{
    BridgeEvent, EngineErrorInfo, EngineLimits, EnginePhase, EngineStatus, ReadyInfo,
    SafeStateSummary, unix_millis,
};
use futures_util::StreamExt;
use omp_rpc::{ChunkAssembler, parse_jsonl_line};
use serde_json::{Value, json};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, Command};
use tokio::sync::{broadcast, mpsc, oneshot, watch};
use tokio::time::timeout;
use tokio_util::codec::{FramedRead, LinesCodec};

const EVENT_CAPACITY: usize = 256;
const RECENT_EVENT_CAPACITY: usize = 12;
const RECENT_EVENT_MAX_BYTES: usize = 64 * 1024;
const STDERR_MAX_LINES: usize = 80;
const SHUTDOWN_GRACE: Duration = Duration::from_secs(3);
const TASK_JOIN_GRACE: Duration = Duration::from_secs(2);

#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub omp_bin: PathBuf,
    pub omp_args: Vec<String>,
    pub workspace: PathBuf,
    pub startup_timeout: Duration,
    pub request_timeout: Duration,
    pub max_frame_bytes: usize,
    pub max_reassembled_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestError {
    NotRunning,
    Closed,
    Timeout,
    Encode(String),
}

impl std::fmt::Display for RequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotRunning => write!(f, "OMP process is not running"),
            Self::Closed => write!(f, "OMP process closed the connection"),
            Self::Timeout => write!(f, "OMP request timed out"),
            Self::Encode(message) => write!(f, "cannot encode OMP request: {message}"),
        }
    }
}

enum WriterMessage {
    Line(String),
    Close,
}

struct Inner {
    status: RwLock<EngineStatus>,
    events: broadcast::Sender<BridgeEvent>,
    recent: Mutex<VecDeque<BridgeEvent>>,
    pending: Mutex<HashMap<String, oneshot::Sender<Value>>>,
    stdin_tx: Mutex<Option<mpsc::Sender<WriterMessage>>>,
    sequence: AtomicU64,
    next_id: AtomicU64,
    physical_limit: AtomicUsize,
    exit_rx: watch::Receiver<Option<i32>>,
    reader: Mutex<Option<tokio::task::JoinHandle<()>>>,
    writer: Mutex<Option<tokio::task::JoinHandle<()>>>,
    waiter: Mutex<Option<tokio::task::JoinHandle<()>>>,
    stderr: Mutex<Option<tokio::task::JoinHandle<()>>>,
    stderr_buffer: Mutex<VecDeque<String>>,
    pid: AtomicU64,
}

#[derive(Clone)]
pub struct Engine {
    inner: Arc<Inner>,
}

struct StartupFailure {
    code: &'static str,
    message: String,
}

impl Engine {
    pub async fn start(config: EngineConfig) -> Self {
        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        let (exit_tx, exit_rx) = watch::channel(None);
        let inner = Arc::new(Inner {
            status: RwLock::new(EngineStatus::starting()),
            events,
            recent: Mutex::new(VecDeque::new()),
            pending: Mutex::new(HashMap::new()),
            stdin_tx: Mutex::new(None),
            sequence: AtomicU64::new(0),
            next_id: AtomicU64::new(1),
            physical_limit: AtomicUsize::new(config.max_frame_bytes),
            exit_rx,
            reader: Mutex::new(None),
            writer: Mutex::new(None),
            waiter: Mutex::new(None),
            stderr: Mutex::new(None),
            stderr_buffer: Mutex::new(VecDeque::new()),
            pid: AtomicU64::new(0),
        });
        let engine = Self { inner };
        if let Err(mut failure) = engine.start_inner(config, exit_tx).await {
            engine.shutdown().await;
            let tail = engine.stderr_tail();
            let joined = tail.join(" ").to_ascii_lowercase();
            if joined.contains("no default model selected") {
                failure = StartupFailure {
                    code: "OMP_MISSING_MODEL",
                    message: "OMP is running without a configured model. Select a model in OMP or pass --model. This is a configuration error, not an RPC failure.".to_owned(),
                };
            } else if failure.code == "OMP_START_FAILED"
                && let Some(last) = tail.last()
            {
                failure.message = format!("{} ({last})", failure.message);
            }
            engine.set_failed(failure.code, failure.message);
        }
        engine
    }

    async fn start_inner(
        &self,
        config: EngineConfig,
        exit_tx: watch::Sender<Option<i32>>,
    ) -> Result<(), StartupFailure> {
        let mut command = Command::new(&config.omp_bin);
        command
            .args(&config.omp_args)
            .current_dir(&config.workspace)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // Own process group, so a hung child and its descendants can be
            // killed together without touching the bridge process group.
            command.as_std_mut().process_group(0);
        }

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                let code = if error.kind() == std::io::ErrorKind::NotFound {
                    "OMP_NOT_FOUND"
                } else {
                    "OMP_START_FAILED"
                };
                let message = if code == "OMP_NOT_FOUND" {
                    format!(
                        "OMP executable was not found: {}. Set --omp-bin or OMP_BRIDGE_OMP_BIN.",
                        config.omp_bin.display()
                    )
                } else {
                    format!(
                        "OMP could not be started from {}: {}",
                        config.omp_bin.display(),
                        redact_error(&error.to_string())
                    )
                };
                return Err(StartupFailure { code, message });
            }
        };

        let pid = child.id();
        if let Some(pid) = pid {
            self.inner.pid.store(u64::from(pid), Ordering::SeqCst);
        }
        let stdin = child.stdin.take().ok_or_else(|| StartupFailure {
            code: "OMP_START_FAILED",
            message: "OMP stdin pipe was unavailable".to_owned(),
        })?;
        let stdout = child.stdout.take().ok_or_else(|| StartupFailure {
            code: "OMP_START_FAILED",
            message: "OMP stdout pipe was unavailable".to_owned(),
        })?;
        let stderr = child.stderr.take().ok_or_else(|| StartupFailure {
            code: "OMP_START_FAILED",
            message: "OMP stderr pipe was unavailable".to_owned(),
        })?;

        let stderr_task = spawn_stderr_reader(stderr, Arc::clone(&self.inner));
        *self.inner.stderr.lock().expect("stderr lock") = Some(stderr_task);

        let inner = Arc::clone(&self.inner);
        let waiter = tokio::spawn(async move {
            let status = child.wait().await;
            let code = status.ok().and_then(|status| status.code());
            let _ = exit_tx.send(Some(code.unwrap_or(-1)));
            inner.publish(
                "process.exited",
                json!({ "code": code, "message": "OMP process exited" }),
            );
            let mut current = inner.status.write().expect("status lock");
            if matches!(current.state, EnginePhase::Starting | EnginePhase::Ready) {
                current.state = EnginePhase::Exited;
                current.ready = false;
                current.error = Some(EngineErrorInfo {
                    code: "OMP_EXITED".to_owned(),
                    message: format!(
                        "OMP process exited with code {}",
                        code.map_or("unknown".to_owned(), |value| value.to_string())
                    ),
                });
            }
        });
        *self.inner.waiter.lock().expect("waiter lock") = Some(waiter);

        let (writer_tx, writer_rx) = mpsc::channel(32);
        *self.inner.stdin_tx.lock().expect("stdin lock") = Some(writer_tx);
        let writer_inner = Arc::clone(&self.inner);
        let writer = tokio::spawn(writer_task(stdin, writer_rx, writer_inner));
        *self.inner.writer.lock().expect("writer lock") = Some(writer);

        let (ready_tx, ready_rx) = oneshot::channel::<Result<ReadyInfo, String>>();
        let reader_inner = Arc::clone(&self.inner);
        let physical_limit = config.max_frame_bytes;
        let reader = tokio::spawn(reader_task(stdout, reader_inner, ready_tx, physical_limit));
        *self.inner.reader.lock().expect("reader lock") = Some(reader);

        let mut exit_rx = self.inner.exit_rx.clone();
        let startup = timeout(config.startup_timeout, async {
            tokio::select! {
                ready = ready_rx => match ready {
                    Ok(Ok(ready)) => Ok(ready),
                    Ok(Err(message)) => Err(StartupFailure {
                        code: "RPC_PROTOCOL_ERROR",
                        message,
                    }),
                    Err(_) => Err(StartupFailure {
                        code: "RPC_PROTOCOL_ERROR",
                        message: "OMP reader stopped before emitting a ready frame".to_owned(),
                    }),
                },
                _ = wait_for_exit(&mut exit_rx) => Err(StartupFailure {
                    code: "OMP_START_FAILED",
                    message: "OMP exited before emitting a ready frame".to_owned(),
                }),
            }
        })
        .await;

        let ready = match startup {
            Ok(result) => result?,
            Err(_) => {
                return Err(StartupFailure {
                    code: "OMP_START_TIMEOUT",
                    message: format!(
                        "OMP did not emit a ready frame within {} seconds",
                        config.startup_timeout.as_secs()
                    ),
                });
            }
        };

        if !ready
            .supported_protocol_versions
            .contains(&ready.protocol_version)
            && ready.supported_protocol_versions.is_empty()
        {
            return Err(StartupFailure {
                code: "RPC_UNSUPPORTED_PROTOCOL",
                message: "OMP advertised no supported protocol versions".to_owned(),
            });
        }

        let physical_limit = physical_limit.min(ready.limits.max_frame_bytes).max(1);
        self.inner
            .physical_limit
            .store(physical_limit, Ordering::SeqCst);

        let negotiated = if ready.supported_protocol_versions.contains(&2) {
            let response = self
                .request(
                    json!({"type": "negotiate_protocol", "protocolVersion": 2}),
                    config.request_timeout,
                )
                .await
                .map_err(|error| StartupFailure {
                    code: "RPC_NEGOTIATION_FAILED",
                    message: format!("protocol v2 negotiation failed: {error}"),
                })?;
            if response.get("success").and_then(Value::as_bool) != Some(true) {
                return Err(StartupFailure {
                    code: "RPC_NEGOTIATION_FAILED",
                    message: format!(
                        "protocol v2 negotiation was rejected: {}",
                        redact_error(&response_error(&response))
                    ),
                });
            }
            2
        } else if ready.supported_protocol_versions.contains(&1) {
            1
        } else {
            return Err(StartupFailure {
                code: "RPC_UNSUPPORTED_PROTOCOL",
                message: format!(
                    "OMP supports protocol versions {:?}; the bridge supports 1 and 2",
                    ready.supported_protocol_versions
                ),
            });
        };

        let response = self
            .request(json!({"type": "get_state"}), config.request_timeout)
            .await
            .map_err(|error| StartupFailure {
                code: "RPC_STATE_FAILED",
                message: format!("get_state failed: {error}"),
            })?;
        if response.get("success").and_then(Value::as_bool) != Some(true) {
            return Err(StartupFailure {
                code: "RPC_STATE_FAILED",
                message: format!(
                    "get_state was rejected: {}",
                    redact_error(&response_error(&response))
                ),
            });
        }
        let data = response.get("data").cloned().unwrap_or(Value::Null);
        let summary = SafeStateSummary::from_get_state(&data);

        let status = EngineStatus {
            state: EnginePhase::Ready,
            ready: true,
            ready_info: Some(ReadyInfo {
                limits: EngineLimits {
                    max_frame_bytes: physical_limit,
                    max_reassembled_frame_bytes: ready
                        .limits
                        .max_reassembled_frame_bytes
                        .min(config.max_reassembled_bytes),
                },
                ..ready
            }),
            negotiated_protocol_version: Some(negotiated),
            summary: Some(summary),
            error: None,
        };
        *self.inner.status.write().expect("status lock") = status.clone();
        self.inner.publish(
            "engine.ready",
            serde_json::to_value(status).unwrap_or(Value::Null),
        );
        let _ = pid;
        Ok(())
    }

    pub fn status(&self) -> EngineStatus {
        self.inner.status.read().expect("status lock").clone()
    }

    /// PID of the managed OMP child, when it has been spawned.
    pub fn pid(&self) -> Option<u32> {
        let pid = self.inner.pid.load(Ordering::SeqCst);
        u32::try_from(pid).ok().filter(|pid| *pid != 0)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<BridgeEvent> {
        self.inner.events.subscribe()
    }

    pub fn recent_events(&self) -> Vec<BridgeEvent> {
        self.inner
            .recent
            .lock()
            .expect("recent lock")
            .iter()
            .cloned()
            .collect()
    }

    pub fn make_event(&self, event_type: &str, payload: Value) -> BridgeEvent {
        self.inner.make_event(event_type, payload)
    }

    pub(crate) async fn request(
        &self,
        mut frame: Value,
        request_timeout: Duration,
    ) -> Result<Value, RequestError> {
        let id = format!(
            "bridge-{}",
            self.inner.next_id.fetch_add(1, Ordering::SeqCst)
        );
        frame["id"] = Value::String(id.clone());
        let encoded = serde_json::to_string(&frame)
            .map_err(|error| RequestError::Encode(error.to_string()))?;
        let (response_tx, response_rx) = oneshot::channel();
        self.inner
            .pending
            .lock()
            .expect("pending lock")
            .insert(id.clone(), response_tx);

        let sender = self.inner.stdin_tx.lock().expect("stdin lock").clone();
        let Some(sender) = sender else {
            self.inner.pending.lock().expect("pending lock").remove(&id);
            return Err(RequestError::NotRunning);
        };
        if sender.send(WriterMessage::Line(encoded)).await.is_err() {
            self.inner.pending.lock().expect("pending lock").remove(&id);
            return Err(RequestError::Closed);
        }

        match timeout(request_timeout, response_rx).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(_)) => Err(RequestError::Closed),
            Err(_) => {
                self.inner.pending.lock().expect("pending lock").remove(&id);
                Err(RequestError::Timeout)
            }
        }
    }

    pub async fn shutdown(&self) {
        let sender = self.inner.stdin_tx.lock().expect("stdin lock").take();
        if let Some(sender) = sender {
            let _ = sender.send(WriterMessage::Close).await;
        }

        let mut exit_rx = self.inner.exit_rx.clone();
        if timeout(SHUTDOWN_GRACE, wait_for_exit(&mut exit_rx))
            .await
            .is_err()
        {
            self.kill_child();
            let _ = timeout(TASK_JOIN_GRACE, wait_for_exit(&mut exit_rx)).await;
        }

        let handles: Vec<_> = [
            self.inner.reader.lock().expect("reader lock").take(),
            self.inner.writer.lock().expect("writer lock").take(),
            self.inner.waiter.lock().expect("waiter lock").take(),
            self.inner.stderr.lock().expect("stderr lock").take(),
        ]
        .into_iter()
        .flatten()
        .collect();
        for handle in handles {
            let _ = timeout(TASK_JOIN_GRACE, handle).await;
        }
        self.inner.pending.lock().expect("pending lock").clear();
    }

    fn kill_child(&self) {
        #[cfg(unix)]
        let pid = self.inner.pid.load(Ordering::SeqCst);
        if pid != 0 && pid <= i32::MAX as u64 {
            // SAFETY: the child leads its own process group (pid == pgid), so
            // a negative pid targets only that group after the grace window.
            unsafe {
                libc::kill(-(pid as libc::pid_t), libc::SIGKILL);
            }
        }
    }

    fn stderr_tail(&self) -> Vec<String> {
        self.inner
            .stderr_buffer
            .lock()
            .expect("stderr buffer lock")
            .iter()
            .cloned()
            .collect()
    }

    fn set_failed(&self, code: &'static str, message: String) {
        let mut status = self.inner.status.write().expect("status lock");
        if status.state == EnginePhase::Ready {
            return;
        }
        status.state = EnginePhase::Failed;
        status.ready = false;
        status.error = Some(EngineErrorInfo {
            code: code.to_owned(),
            message,
        });
        let payload = serde_json::to_value(status.clone()).unwrap_or(Value::Null);
        drop(status);
        self.inner.publish("engine.failed", payload);
    }

    pub fn status_summary_for_log(&self) -> String {
        let status = self.status();
        match status.error {
            Some(error) => format!("{}: {}", error.code, error.message),
            None => format!("{:?}", status.state),
        }
    }
}

impl Inner {
    fn make_event(&self, event_type: &str, payload: Value) -> BridgeEvent {
        let sequence = self.sequence.fetch_add(1, Ordering::SeqCst) + 1;
        BridgeEvent {
            version: crate::api_types::EVENT_VERSION,
            event_type: event_type.to_owned(),
            sequence,
            occurred_at: unix_millis(),
            replay: false,
            payload,
        }
    }

    fn publish(&self, event_type: &str, payload: Value) {
        let event = self.make_event(event_type, payload);
        if event.event_type == "omp.event"
            && let Ok(encoded) = serde_json::to_vec(&event.payload)
            && encoded.len() <= RECENT_EVENT_MAX_BYTES
        {
            let mut recent = self.recent.lock().expect("recent lock");
            while recent.len() >= RECENT_EVENT_CAPACITY {
                recent.pop_front();
            }
            recent.push_back(event.clone());
        }
        let _ = self.events.send(event);
    }
}

async fn writer_task(
    mut stdin: ChildStdin,
    mut receiver: mpsc::Receiver<WriterMessage>,
    inner: Arc<Inner>,
) {
    while let Some(message) = receiver.recv().await {
        match message {
            WriterMessage::Line(line) => {
                let mut bytes = line.into_bytes();
                bytes.push(b'\n');
                if stdin.write_all(&bytes).await.is_err() || stdin.flush().await.is_err() {
                    inner.publish(
                        "bridge.warning",
                        json!({
                            "code": "OMP_STDIN_CLOSED",
                            "message": "Cannot write to the OMP process"
                        }),
                    );
                    break;
                }
            }
            WriterMessage::Close => break,
        }
    }
    let _ = stdin.shutdown().await;
}

fn spawn_stderr_reader(
    stderr: tokio::process::ChildStderr,
    inner: Arc<Inner>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let mut redacted = redact_error(&line);
            if redacted.is_empty() {
                continue;
            }
            if line
                .to_ascii_lowercase()
                .contains("no default model selected")
            {
                // Safe constant: the raw line may mention credentials.
                redacted = "no default model selected".to_owned();
            }
            let mut buffer = inner.stderr_buffer.lock().expect("stderr buffer lock");
            while buffer.len() >= STDERR_MAX_LINES {
                buffer.pop_front();
            }
            buffer.push_back(redacted.chars().take(400).collect());
        }
    })
}

async fn reader_task(
    stdout: tokio::process::ChildStdout,
    inner: Arc<Inner>,
    ready_tx: oneshot::Sender<Result<ReadyInfo, String>>,
    configured_limit: usize,
) {
    let mut ready_tx = Some(ready_tx);
    let mut assembler: Option<ChunkAssembler> = None;
    let mut reassembly_limit = crate::config::DEFAULT_MAX_REASSEMBLED_BYTES;
    let mut ready_seen = false;
    let mut lines = FramedRead::new(
        stdout,
        LinesCodec::new_with_max_length(configured_limit.max(1)),
    );

    while let Some(item) = lines.next().await {
        let line = match item {
            Ok(line) => line,
            Err(error) => {
                inner.publish(
                    "bridge.warning",
                    json!({
                        "code": "RPC_FRAME_ERROR",
                        "message": format!("OMP stdout frame rejected: {error}")
                    }),
                );
                if let Some(sender) = ready_tx.take() {
                    let _ = sender.send(Err(format!("OMP stdout frame rejected: {error}")));
                }
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        if line.len() > inner.physical_limit.load(Ordering::SeqCst) {
            inner.publish(
                "bridge.warning",
                json!({
                    "code": "RPC_FRAME_TOO_LARGE",
                    "message": "OMP stdout frame exceeded the negotiated physical limit"
                }),
            );
            continue;
        }
        let frame = match parse_jsonl_line(&line) {
            Ok(Some(frame)) => frame,
            Ok(None) => continue,
            Err(error) => {
                inner.publish(
                    "bridge.warning",
                    json!({
                        "code": "RPC_MALFORMED_FRAME",
                        "message": format!("Ignored malformed OMP stdout frame: {error}")
                    }),
                );
                continue;
            }
        };

        if frame.get("type").and_then(Value::as_str) == Some("ready") && !ready_seen {
            match ReadyInfo::from_frame(&frame) {
                Some(ready) => {
                    ready_seen = true;
                    inner.physical_limit.store(
                        ready.limits.max_frame_bytes.min(configured_limit).max(1),
                        Ordering::SeqCst,
                    );
                    reassembly_limit = ready
                        .limits
                        .max_reassembled_frame_bytes
                        .clamp(1, crate::config::DEFAULT_MAX_REASSEMBLED_BYTES);
                    assembler = Some(ChunkAssembler::new(reassembly_limit));
                    if let Some(sender) = ready_tx.take() {
                        let _ = sender.send(Ok(ready));
                    }
                    inner.publish("omp.event", json!({"rawType": "ready", "frame": frame}));
                }
                None => {
                    if let Some(sender) = ready_tx.take() {
                        let _ = sender.send(Err(
                            "OMP ready frame is missing protocol or frame-limit fields".to_owned(),
                        ));
                    }
                    break;
                }
            }
            continue;
        }

        if frame.get("type").and_then(Value::as_str) == Some("rpc_chunk") {
            let result = match assembler.as_mut() {
                Some(assembler) => assembler.push(&frame),
                None => continue,
            };
            match result {
                Ok(Some(frame)) => route_frame(&inner, frame),
                Ok(None) => {}
                Err(error) => {
                    inner.publish(
                        "bridge.warning",
                        json!({
                            "code": "RPC_CHUNK_ERROR",
                            "message": format!("Rejected OMP chunk sequence: {error}")
                        }),
                    );
                    assembler = Some(ChunkAssembler::new(reassembly_limit));
                }
            }
            continue;
        }

        if let Some(assembler) = assembler.as_ref()
            && assembler.is_active()
        {
            inner.publish(
                "bridge.warning",
                json!({
                    "code": "RPC_CHUNK_INTERRUPTED",
                    "message": "Dropped a frame interleaved into an OMP chunk sequence"
                }),
            );
            continue;
        }

        route_frame(&inner, frame);
    }
}

fn route_frame(inner: &Arc<Inner>, frame: Value) {
    if frame.get("type").and_then(Value::as_str) == Some("response") {
        let Some(id) = frame.get("id").and_then(Value::as_str) else {
            inner.publish(
                "bridge.warning",
                json!({
                    "code": "RPC_UNMATCHED_RESPONSE",
                    "message": "Ignored an OMP response without an id"
                }),
            );
            return;
        };
        let sender = inner.pending.lock().expect("pending lock").remove(id);
        if let Some(sender) = sender {
            let _ = sender.send(frame);
        } else {
            inner.publish(
                "bridge.warning",
                json!({
                    "code": "RPC_UNMATCHED_RESPONSE",
                    "message": "Ignored an OMP response with an unknown request id"
                }),
            );
        }
        return;
    }

    let event_type = frame
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("<missing>");
    inner.publish(
        "omp.event",
        json!({ "rawType": event_type, "frame": frame }),
    );
}

async fn wait_for_exit(exit_rx: &mut watch::Receiver<Option<i32>>) {
    if exit_rx.borrow().is_some() {
        return;
    }
    while exit_rx.changed().await.is_ok() {
        if exit_rx.borrow().is_some() {
            return;
        }
    }
}

fn response_error(response: &Value) -> String {
    response
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or("no error detail")
        .to_owned()
}

pub fn redact_error(message: &str) -> String {
    let lowered = message.to_ascii_lowercase();
    if [
        "api_key",
        "apikey",
        "api key",
        "authorization",
        "bearer ",
        "token",
        "secret",
        "password",
    ]
    .iter()
    .any(|needle| lowered.contains(needle))
    {
        return "<redacted diagnostic line>".to_owned();
    }
    message.chars().take(400).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_with_credentials_are_redacted() {
        assert_eq!(
            redact_error("Authorization: Bearer sk-secret"),
            "<redacted diagnostic line>"
        );
        assert_eq!(
            redact_error("provider api_key=abc"),
            "<redacted diagnostic line>"
        );
        assert_eq!(
            redact_error("plain startup failure"),
            "plain startup failure"
        );
    }
}
