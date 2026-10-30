# Architecture

## 1. Product shape

OMP Client is a mobile control surface for the existing OMP coding agent. Its main pattern is similar to OpenChamber's mobile architecture: the mobile application is a client UI; a separate host-side service owns the runtime and exposes a protected API. Here, the runtime adapter is specific to OMP.

### Components

1. **Mobile client (`apps/mobile`)**
   - Tauri 2 shell with a Svelte + TypeScript UI.
   - Owns presentation, local preferences, connection profiles, and transient UI state.
   - Does not spawn OMP and does not store provider credentials.

2. **Tauri native shell (`apps/mobile/src-tauri`)**
   - Minimal platform integration, lifecycle integration, secure token storage adapter, share/open-file integrations if needed.
   - Avoid duplicating business logic already owned by the bridge.

3. **OMP Bridge (`apps/bridge`)**
   - Standalone Rust HTTP + WebSocket service, normally launched from Termux on the same Android device for local use.
   - Owns OMP child processes, stdin/stdout/stderr, RPC protocol handling, session/runtime lifecycle, scoped workspace/file access, and Git read operations.
   - Provides a versioned API for the client.

4. **OMP executable**
   - The real engine, installed and configured in the chosen host environment.
   - First launch candidate: `omp --mode rpc-ui`, started with the workspace as the process working directory.
   - Provider configuration, API keys, extensions, and engine-specific tools remain in OMP's own environment.

5. **Contracts (`packages/contracts`)**
   - Versioned OpenAPI specification and WebSocket event schemas.
   - TypeScript types should be generated from the API contract where practical. Rust DTOs must be tested against the same contract.

## 2. Runtime topology

### Initial local topology

```text
┌──────────────────── Android device ──────────────────────┐
│                                                          │
│  OMP Client APK (Tauri WebView + Rust native shell)       │
│        │ HTTP + WebSocket, authenticated                  │
│        ▼                                                 │
│  127.0.0.1:<configured-port>                              │
│        ▲                                                 │
│  omp-bridge (Rust, launched from Termux)                  │
│        │ stdin/stdout JSONL, stderr diagnostics           │
│        ▼                                                 │
│  omp --mode rpc-ui (real OMP process)                     │
│        │                                                 │
│        ▼                                                 │
│  OMP session / providers / tools / workspace              │
└──────────────────────────────────────────────────────────┘
```

The bridge must be started explicitly from Termux for the first milestone. Do not promise that an Android app can arbitrarily launch or control a different app's process across sandbox boundaries. Improve startup ergonomics later only through a tested, supported integration.

### Later remote topology

A client can connect to a bridge on another host over a private network/VPN or an explicitly secured remote transport. Remote/LAN bind is not enabled by default. Never expose the bridge to a public interface without authentication, transport protection, revocation, and a reviewed threat model.

## 3. OMP process and RPC loop

- Launch with an explicit executable path, explicit argument list, and `current_dir(workspace)`; do not build a shell command string.
- Default candidate is `omp --mode rpc-ui`. Verify this exact command against the installed version during Phase 0.
- Pipe stdin, stdout, and stderr. Start independent tasks to drain stdout and stderr concurrently, and a single serialized writer for stdin.
- Parse one JSON object per line. Treat blank lines and invalid frames according to the upstream protocol; do not let a single bad input silently corrupt parser state.
- On a `ready` frame, inspect `supportedProtocolVersions`, limits, and negotiate protocol v2 when advertised and implemented.
- Validate and reassemble `rpc_chunk` frames only within advertised bounded limits. Never trust `count`/`byteLength` blindly.
- Correlate responses by `id`. Preserve asynchronous events independently of request-response matching.
- Track `prompt` admission/ack separately from `prompt_result` and agent/session lifecycle events.
- Handle `extension_ui_request`, `host_tool_call`, host URI requests, cancellation messages, `available_commands_update`, and subagent events. If a host request type is not implemented, fail closed and display a clear diagnostic rather than silently approving or dropping a potentially important interaction.
- Keep stdout protocol-only. Do not print logging, progress text, or panic messages to the child's stdout stream.
- On bridge shutdown: stop accepting commands, cancel/close WebSocket subscribers, close OMP stdin, keep draining stdout until exit, then terminate after a bounded grace period if needed. Avoid orphan processes.

## 4. Bridge modules (implemented Phase 1)

```text
Cargo.toml                  # workspace: omp-bridge, omp-rpc, rpc-harness
crates/omp-rpc/src/lib.rs   # bounded JSONL parsing + rpc_chunk reassembly
apps/bridge/src/
├── main.rs                 # CLI, doctor, startup/shutdown
├── config.rs               # loopback/origin/limit/TTL validation
├── api.rs                  # Axum routes, Host/Origin guard, WS auth/fan-out
├── api_types.rs            # versioned DTOs, safe state projection, envelopes
├── auth.rs                 # pairing, hashed expiring tokens, revoke
├── engine.rs               # OMP process, RPC correlation, status, events
└── server.rs               # RunningBridge bind/shutdown wrapper
```

The bridge intentionally stays in a few modules until later features have
callers. Workspace, files, Git, runtime multiplexing, and normalized typed OMP
events are not implemented yet.

## 5. Frontend boundaries

```text
apps/mobile/src/
├── app/                    # app shell, routing, stores
├── components/             # shared UI primitives
├── features/
│   ├── connections/
│   ├── chat/
│   ├── sessions/
│   ├── workspace/
│   ├── files/
│   ├── changes/
│   └── settings/
├── lib/api/                # typed HTTP client + WebSocket client
├── lib/state/              # shared state and lifecycle
├── styles/                 # tokens, themes, responsive rules
└── test/
```

The frontend must not reimplement RPC framing or access OMP's stdin/stdout. It consumes stable bridge DTOs and normalized events.

## 6. API shape

The implemented Phase 1 subset is documented exactly in
[`Specs/BRIDGE_API_SPEC.md`](Specs/BRIDGE_API_SPEC.md): `/healthz`,
`POST /api/v1/pair`, authenticated `GET /api/v1/status`,
`POST /api/v1/tokens/revoke`, and the first-message-authenticated
`WS /api/v1/events`. The bridge owns one OMP child process; there is no runtime
multiplexing or prompt action yet.

All endpoints except a minimal, non-sensitive health check require authentication. Prefix REST with `/api/v1`. The following is the planned expansion, not current behavior:

- `GET /healthz` — minimal readiness status; no secret or workspace details.
- `GET /api/v1/capabilities` — bridge/API version and supported UI features.
- `GET /api/v1/workspaces` — configured allowed workspaces only.
- `POST /api/v1/runtimes` — create/start an OMP runtime for an allowed workspace.
- `GET /api/v1/runtimes/{runtimeId}/state` — authoritative state snapshot.
- `POST /api/v1/runtimes/{runtimeId}/actions` — validated actions such as prompt, abort, new session, model selection, queue behavior.
- `GET /api/v1/runtimes/{runtimeId}/entries` — bounded/paginated entries.
- `GET /api/v1/workspaces/{workspaceId}/files` — list a safe workspace-relative directory.
- `GET /api/v1/workspaces/{workspaceId}/file` — read a safe file with explicit size limits.
- `GET /api/v1/workspaces/{workspaceId}/git/status` and `/git/diff` — read-only Git views in the initial file/Git phase.
- `WS /api/v1/events` — authenticated runtime event stream; authenticate before sending any data.

Do not hard-code the sketch above without documenting it in `packages/contracts/openapi.yaml` and validating it. Keep routes narrow and versioned. Never add a generic “execute arbitrary command” endpoint as a shortcut.

## 7. Security boundary

- Bind to `127.0.0.1` by default; reject accidental `0.0.0.0`/LAN binding unless the user explicitly enables a reviewed remote mode.
- Use a one-time pairing secret or similarly explicit first-run trust flow. Issue a revocable client token; store only a hash server-side where feasible. Never place long-lived tokens in query parameters.
- HTTP requests use an authorization header. WebSockets authenticate in the first frame with a short deadline, then receive no application events until accepted; do not put secrets in URL query strings.
- Restrict origins to the actual Tauri app origins established by tests; never use wildcard CORS as a fix.
- Reject untrusted Host values and malformed route/path parameters.
- Limit request bodies, WebSocket frame sizes, event queue sizes, file response sizes, RPC frame sizes, and in-flight operations.
- Scope file access to configured workspace roots and canonicalize paths, including symlink handling.
- No tokens or full prompts in logs. Use redacted structured events.
- Preserve OMP's own tool approval semantics. `rpc-ui` is required for dialogs/UI messages; do not add `--no-ui` without proving that no required approval flow is lost.

## 8. Error model and observability

- Return stable, documented error codes from the bridge: `UNAUTHORIZED`, `PAIRING_REQUIRED`, `INVALID_INPUT`, `NOT_FOUND`, `WORKSPACE_FORBIDDEN`, `OMP_NOT_FOUND`, `OMP_START_FAILED`, `RPC_TIMEOUT`, `RPC_PROTOCOL_ERROR`, `RUNTIME_BUSY`, `RUNTIME_EXITED`, `RATE_LIMITED`, `INTERNAL`.
- Keep user-facing messages useful but do not expose internal paths/secrets by default.
- Include a request/correlation ID in logs and API error bodies.
- Provide a diagnostic command (`omp-bridge doctor`) that checks config, binary lookup, version/command availability, workspace roots, and bind settings without displaying provider secrets.

## 9. Decisions resolved and still open

Resolved:

- OMP `18.13.3`, `--mode rpc-ui`, ready frame, protocol v2, and bounded chunk reassembly are verified (ADR-009).
- Axum 0.8 + Tokio implement the loopback HTTP/WebSocket bridge (ADR-013).
- The packaged Tauri Android origin is `http://tauri.localhost`; the development origin is `http://localhost:1420`. Both are exact allowlist entries.
- The CSP now allows the loopback HTTP/WebSocket sources (ADR-012, amended by ADR-013).
- Tokens stay in memory until a verified Android secure-storage adapter exists.

Still open:

- Android WebView loopback/CORS/cleartext behavior on a real installed build; no working `adb` here.
- Session enumeration and stable public interfaces for historical sessions.
- The Termux installation/build route for the standalone bridge beyond this PRoot host.
- OpenAPI/event schema generation and drift checks.

Record outcomes in `Plan/DECISIONS.md`. Do not convert assumptions into facts just because a compile succeeds.
