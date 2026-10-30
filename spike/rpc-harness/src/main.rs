//! Live Phase 0 probe: spawn OMP, negotiate RPC v2, request state, shut down.
//!
//! Usage:
//!   OMP_BIN=/path/to/omp OMP_MODEL=anthropic/claude-sonnet-4-5 \
//!     cargo run -- /tmp/opencode/phase0-workspace
//!
//! stdout carries only the redacted summary. Child stderr is diagnostics.

use omp_rpc::{
    ChunkAssembler, DEFAULT_MAX_REASSEMBLED_BYTES, ResponseCorrelator, parse_jsonl_line,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::env;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const FRAME_TIMEOUT: Duration = Duration::from_secs(60);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(30);

fn main() {
    if let Err(error) = run() {
        eprintln!("probe failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let omp = env::var("OMP_BIN").unwrap_or_else(|_| "omp".to_owned());
    let workspace = env::args()
        .nth(1)
        .unwrap_or_else(|| env::current_dir().unwrap().display().to_string());
    let model = env::var("OMP_MODEL").ok();

    let mut command = Command::new(&omp);
    command
        .arg("--mode")
        .arg("rpc-ui")
        .current_dir(&workspace)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(model) = &model {
        command.arg("--model").arg(model);
    }
    if let Ok(home) = env::var("OMP_HOME") {
        command.env("HOME", home);
    }

    let mut child = command
        .spawn()
        .map_err(|error| format!("cannot spawn {omp} (OMP_BIN, cwd {workspace}): {error}"))?;
    let stdin = child.stdin.take();
    let stdout = child.stdout.take().ok_or("no stdout pipe")?;
    let stderr = child.stderr.take().ok_or("no stderr pipe")?;

    let (tx, rx) = mpsc::channel::<String>();
    let stdout_reader = thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            match line {
                Ok(line) => {
                    if tx.send(line).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let stderr_lines = Arc::new(Mutex::new(Vec::<String>::new()));
    let stderr_sink = Arc::clone(&stderr_lines);
    let stderr_reader = thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if let Ok(mut lines) = stderr_sink.lock()
                && lines.len() < 50
            {
                lines.push(redact(&line));
            }
        }
    });

    let mut correlator = ResponseCorrelator::default();
    let mut assembler = ChunkAssembler::new(DEFAULT_MAX_REASSEMBLED_BYTES);
    let mut event_counts = BTreeMap::<String, u64>::new();
    let mut summary = json!({
        "command": {
            "executable": omp,
            "args": ["--mode", "rpc-ui"],
            "model": model,
            "cwd": workspace,
        }
    });

    let ready = wait_for_frame(&rx, Duration::from_secs(90))?.ok_or("no ready frame within 90s")?;
    if ready.get("type").and_then(Value::as_str) != Some("ready") {
        return Err(format!("first frame is not ready: {ready}"));
    }
    summary["ready"] = json!({
        "protocolVersion": ready.get("protocolVersion"),
        "supportedProtocolVersions": ready.get("supportedProtocolVersions"),
        "maxFrameBytes": ready.get("maxFrameBytes"),
        "maxReassembledFrameBytes": ready.get("maxReassembledFrameBytes"),
    });

    let versions = ready
        .get("supportedProtocolVersions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if versions.iter().any(|version| version.as_u64() == Some(2)) {
        correlator.register("probe-negotiate");
        send(
            stdin.as_ref(),
            &json!({"id": "probe-negotiate", "type": "negotiate_protocol", "protocolVersion": 2}),
        )?;
        let response = wait_for_response(
            &rx,
            &mut correlator,
            &mut assembler,
            &mut event_counts,
            "probe-negotiate",
        )?;
        summary["negotiate"] = json!({
            "id": response.get("id"),
            "command": response.get("command"),
            "success": response.get("success"),
            "error": response.get("error"),
        });
        if response.get("success").and_then(Value::as_bool) != Some(true) {
            return Err(format!("protocol v2 negotiation failed: {response}"));
        }
    }

    correlator.register("probe-state");
    send(
        stdin.as_ref(),
        &json!({"id": "probe-state", "type": "get_state"}),
    )?;
    let response = wait_for_response(
        &rx,
        &mut correlator,
        &mut assembler,
        &mut event_counts,
        "probe-state",
    )?;
    let data = response.get("data").cloned().unwrap_or(Value::Null);
    summary["get_state"] = json!({
        "id": response.get("id"),
        "command": response.get("command"),
        "success": response.get("success"),
        "error": response.get("error"),
        "model": {
            "provider": data.pointer("/model/provider"),
            "id": data.pointer("/model/id"),
        },
        "thinkingLevel": data.get("thinkingLevel"),
        "isStreaming": data.get("isStreaming"),
        "isSettled": data.get("isSettled"),
        "messageCount": data.get("messageCount"),
        "hasPendingAsyncWork": data.get("hasPendingAsyncWork"),
        "sessionId": data.get("sessionId"),
        "keys": data.as_object().map(|object| object.keys().cloned().collect::<Vec<_>>()),
    });
    if response.get("success").and_then(Value::as_bool) != Some(true) {
        return Err(format!("get_state failed: {response}"));
    }

    summary["events_before_state"] = json!(event_counts);
    summary["shutdown"] = json!(shutdown(&mut child, stdin, stdout_reader, stderr_reader));
    summary["stderr_lines"] = json!(
        stderr_lines
            .lock()
            .map(|lines| lines.clone())
            .unwrap_or_default()
    );

    println!("{}", serde_json::to_string_pretty(&summary).unwrap());
    Ok(())
}

fn send(stdin: Option<&ChildStdin>, frame: &Value) -> Result<(), String> {
    let mut stdin = stdin.ok_or("stdin is closed")?;
    let mut line = serde_json::to_string(frame).map_err(|error| error.to_string())?;
    line.push('\n');
    stdin
        .write_all(line.as_bytes())
        .and_then(|_| stdin.flush())
        .map_err(|error| format!("cannot write command: {error}"))
}

fn wait_for_response(
    rx: &Receiver<String>,
    correlator: &mut ResponseCorrelator,
    assembler: &mut ChunkAssembler,
    event_counts: &mut BTreeMap<String, u64>,
    id: &str,
) -> Result<Value, String> {
    let deadline = Instant::now() + FRAME_TIMEOUT;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(format!("timed out waiting for response {id}"));
        }
        let frame = wait_for_frame(rx, remaining)?
            .ok_or_else(|| format!("stdout closed before response {id}"))?;
        let frame = if frame.get("type").and_then(Value::as_str) == Some("rpc_chunk") {
            match assembler.push(&frame) {
                Ok(Some(frame)) => frame,
                Ok(None) => continue,
                Err(error) => return Err(format!("bad chunk frame: {error}")),
            }
        } else if assembler.is_active() {
            return Err("interleaved non-chunk frame during chunk sequence".to_owned());
        } else {
            frame
        };
        if let Some(response) = correlator.accept(&frame) {
            return Ok(response);
        }
        let kind = frame
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("<missing type>")
            .to_owned();
        *event_counts.entry(kind).or_insert(0) += 1;
    }
}

fn wait_for_frame(rx: &Receiver<String>, timeout: Duration) -> Result<Option<Value>, String> {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(None);
        }
        match rx.recv_timeout(remaining) {
            Ok(line) => match parse_jsonl_line(&line) {
                Ok(Some(frame)) => return Ok(Some(frame)),
                Ok(None) => continue,
                Err(error) => return Err(format!("invalid stdout frame: {error}")),
            },
            Err(RecvTimeoutError::Timeout) => return Ok(None),
            Err(RecvTimeoutError::Disconnected) => {
                return Err("stdout reader stopped".to_owned());
            }
        }
    }
}

fn shutdown(
    child: &mut Child,
    stdin: Option<ChildStdin>,
    stdout_reader: thread::JoinHandle<()>,
    stderr_reader: thread::JoinHandle<()>,
) -> Value {
    drop(stdin);
    let deadline = Instant::now() + SHUTDOWN_TIMEOUT;
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
            Ok(None) => {
                timed_out = true;
                let _ = child.kill();
                break child.wait().ok();
            }
            Err(_) => break None,
        }
    };
    let _ = stdout_reader.join();
    let _ = stderr_reader.join();
    json!({
        "exit_code": status.and_then(|status| status.code()),
        "timed_out": timed_out,
    })
}

fn redact(line: &str) -> String {
    let lowered = line.to_ascii_lowercase();
    if ["api_key", "apikey", "token", "secret", "authorization"]
        .iter()
        .any(|needle| lowered.contains(needle))
    {
        return "<redacted diagnostic line>".to_owned();
    }
    line.chars().take(300).collect()
}
