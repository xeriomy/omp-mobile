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

## 4. Bridge modules (proposed)

```text
apps/bridge/src/
├── main.rs                 # CLI/config/startup
├── config.rs               # validated configuration
├── http/                   # routes, middleware, health
├── ws/                     # auth handshake and event fan-out
├── auth/                   # pairing/token issue, verify, revoke
├── runtime/                # runtime manager + lifecycle
├── omp/
│   ├── process.rs          # spawn, pipes, exit handling
│   ├── framing.rs          # bounded JSONL + rpc_chunk handling
│   ├── client.rs           # request IDs, pending requests
│   ├── events.rs           # normalized event routing
│   └── types.rs            # DTOs from verified RPC contract
├── workspace/              # configured roots and path safety
├── files/                  # read/list initially; edits gated by roadmap
├── git/                    # read-only status/diff initially
├── error.rs
└── telemetry.rs            # structured redacted logs
```

Use a small number of modules initially; do not create abstractions with no caller. Names are guidance, not a mandate to split every file immediately.

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

## 6. API shape (initial proposal)

All endpoints except a minimal, non-sensitive health check require authentication. Prefix REST with `/api/v1`.

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

## 9. Decisions still requiring Phase 0 evidence

- The exact installed OMP binary path and version; support for `--mode rpc-ui`; whether the installed protocol advertises v2 and chunk reassembly.
- Session enumeration behavior and stable public interfaces for opening historical sessions.
- Android WebView/Tauri origin, HTTP loopback/CORS/cleartext behavior on the target device.
- Selected Rust HTTP/WebSocket framework and secure token storage plugin compatibility with Tauri 2 Android.
- The Termux installation/build route for the standalone bridge.

Record outcomes in `Plan/DECISIONS.md`. Do not convert assumptions into facts just because a compile succeeds.
