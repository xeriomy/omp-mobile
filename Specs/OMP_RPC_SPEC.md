# OMP RPC Integration Spec

## Purpose

Define the required behavior of the bridge's client for OMP's native JSONL RPC transport. This document is a compatibility contract; verify it against the exact OMP version used for tests.

## Verified baseline (2026-10-29)

Tested with the official OMP `18.13.3` `omp-linux-arm64` release in the target Android/PRoot environment:

```json
{"type":"ready","protocolVersion":1,"supportedProtocolVersions":[1,2],"maxFrameBytes":1048576,"maxReassembledFrameBytes":67108864}
```

`negotiate_protocol` with `protocolVersion: 2` returned a matching-id success response, and `get_state` returned a matching-id success response before a clean stdin-close shutdown (exit code `0`, stderr empty). A launch without any configured model exits `1` before emitting `ready`; that is a configuration error, not a protocol failure. Prompt, abort, extension-dialog, and approval paths were not exercised because no provider credentials were available. Re-test them before treating those parts of this spec as verified.

### Bridge implementation evidence (2026-10-30)

The `apps/bridge` engine implements the following against OMP `18.13.3`:

- spawn with an explicit argument array, selected working directory, piped stdin/stdout/stderr, and its own process group;
- concurrent bounded stderr drain, redaction, and a bounded in-memory tail used once to classify startup failure;
- real `ready` parsing, advertised-limit enforcement, v2 negotiation, and id-correlated `get_state`;
- status becomes `ready` only after all three succeed; otherwise it is `failed` with a typed code;
- a process that exits before `ready` is classified as `OMP_MISSING_MODEL` when the redacted stderr tail reports no selected model, not as an RPC failure;
- graceful stdin-close shutdown, then process-group SIGKILL after a 3-second grace period;
- real stdout events are broadcast to authenticated WebSocket clients with a versioned envelope.

The opt-in `real_omp` test completed the real ready/negotiation/state sequence, authenticated HTTP status, and clean shutdown. The fake fixture tests separately cover v1 fallback, malformed frames, unsupported versions, rejected negotiation/state, oversized frames, and startup timeout. No prompt or approval frame was sent.

## Launch

Initial candidate:

```text
omp --mode rpc-ui
```

Use the configured executable path and set `current_dir` to the selected workspace. Do not pass workspace path or user prompt through shell interpolation. Verify CLI flags and environment with `omp --help` and `omp launch --help`/relevant reference for the installed version. Do not add `--no-ui` by default: it changes extension UI and approval behavior. If headless mode is explored, record a decision and test every approval/dialog path.

## Framing and negotiation

- Input and output are JSON Lines: one JSON object per line, followed by newline.
- This is OMP's custom RPC protocol, not JSON-RPC 2.0.
- Expect an initial `ready` frame with protocol/version/limit information.
- If `supportedProtocolVersions` advertises v2 and the bridge implements its rules, send `{"id":"...","type":"negotiate_protocol","protocolVersion":2}` and wait for its success response before using v2 framing.
- Validate `rpc_chunk` metadata (`chunkId`, `index`, `count`, `byteLength`), enforce advertised maximum reassembly sizes, reject interrupted/interleaved/oversized chunks, decode strict UTF-8, then parse the reconstructed JSON object.
- Continue reading stdout after closing stdin during shutdown so queued output is drained and the process can exit.
- Keep stderr separate and bounded in log output; never merge it into the JSONL reader.

## Request correlation and prompt lifecycle

- Every issued command should have a unique `id` where the schema supports it.
- Resolve responses by `id`; concurrent calls may emit responses in a different order.
- Do not assume a successful `prompt` response means the model finished. It can mean the prompt was admitted. Track later `prompt_result`, agent/message events, and session-settled events according to the protocol contract.
- Do not duplicate prompts automatically after connection loss. Re-read state/entries and ask for user intervention if delivery status cannot be established.

## Minimum supported operations

Implement only after verifying the schema in the versioned upstream RPC docs/source:
- protocol negotiation;
- `get_state`;
- `prompt`, `abort`, `abort_and_prompt` and any agreed queue/steer operations;
- `get_entries` and session transitions supported by that version;
- `get_available_models`, `set_model`;
- `get_available_thinking_levels`, `set_thinking_level`;
- `get_available_commands`;
- `set_subagent_subscription`, `get_subagents` and available subagent events;
- extension UI request/response (`select`, `confirm`, `input`, `editor`, `ask`, `cancel`, plus presentation methods);
- host-tool and host-URI request/cancellation frames if the chosen OMP features use them.

## UI protocol requirements

- Render extension UI requests as native-looking app dialogs/sheets, but keep the protocol payload exact.
- If `secret: true` is present for an input, mask user entry.
- Enforce request IDs and timeouts; send cancellation when the UI is dismissed only if the upstream schema permits it.
- Do not auto-confirm dangerous actions. Preserve OMP's permission and tool-approval semantics.
- If a request type is unknown or cannot be represented, show an actionable unsupported-interaction error and respond according to documented protocol requirements; never silently approve.

## Session model

- Treat session file paths/IDs as engine-defined opaque values. Do not synthesize session IDs.
- The runtime's `get_state` is the source of truth for current session identity after any session transition.
- Session listing may require a separate, version-specific catalog adapter. Verify a stable documented source before implementing it; avoid brittle parsing of undocumented files.
- Use pagination for history; do not depend on one giant response.

## Testing

Unit tests:
- line parser, malformed input, blank line handling;
- request ID matching and out-of-order response;
- ready/negotiate behavior;
- v2 chunk reconstruction and all malformed chunk cases;
- prompt acknowledgement vs final result;
- unknown events and commands;
- timeout, process exit, stdin close, stdout drain, stderr redaction.

Integration tests (use real OMP when available):
- startup and `get_state`;
- one harmless prompt in a disposable workspace;
- abort a long-running but safe prompt;
- one tool/extension UI interaction;
- session transition and post-transition state refresh;
- shutdown without orphan process.

References:
- https://github.com/can1357/oh-my-pi/blob/main/docs/rpc.md
- https://github.com/can1357/oh-my-pi/blob/main/docs/cli-reference.md
