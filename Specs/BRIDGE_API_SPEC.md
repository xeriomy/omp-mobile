# Bridge API Specification (Draft v1)

This document defines the proposed client-facing contract. Implement it in `packages/contracts/openapi.yaml` and a WebSocket events schema before claiming API completeness.

## 1. Rules

- Base path: `/api/v1`.
- JSON request/response bodies use UTF-8.
- All functional endpoints require a valid client token.
- `GET /healthz` may be unauthenticated but returns only `{ "status": "ok" }` and no runtime/workspace details.
- Every error follows the same envelope: `{ "error": { "code": "...", "message": "...", "requestId": "..." } }`.
- Successful responses use explicit DTOs, not arbitrary raw child-process output.
- Requests have body-size and timeout limits.
- Mutating requests should be idempotent when practical or use a client-generated idempotency key where duplicate submission is dangerous.

## 1a. Implemented Phase 1 subset (2026-10-30)

This is the contract the current bridge and mobile client implement. Later
sections describe planned expansions; do not treat those as implemented.

### Pairing

`POST /api/v1/pair` is unauthenticated and accepts
`{"code":"ABCDEFGHJK"}`. The code is a 10-character random value from an
unambiguous alphabet, single use, valid for 10 minutes by default, and locked
after five wrong attempts. Success returns:

```json
{"token":"<opaque bearer token>","tokenId":"<opaque id>","expiresAt":1790000000000}
```

Only SHA-256 hashes of codes and tokens are retained. Tokens are not persisted
and expire after 24 hours by default. A bridge restart invalidates all tokens.

### Authenticated endpoints

- `GET /api/v1/status` returns the versioned status snapshot:

```json
{
  "apiVersion": "v1",
  "bridgeVersion": "0.1.0",
  "listen": "127.0.0.1:8765",
  "engine": {
    "state": "ready",
    "ready": true,
    "readyInfo": {
      "protocolVersion": 1,
      "supportedProtocolVersions": [1, 2],
      "limits": {"maxFrameBytes": 1048576, "maxReassembledFrameBytes": 67108864}
    },
    "negotiatedProtocolVersion": 2,
    "summary": {
      "model": {"provider": "anthropic", "id": "claude-sonnet-4-5"},
      "thinkingLevel": "off",
      "isStreaming": false,
      "isSettled": true,
      "messageCount": 0,
      "sessionId": "<opaque>"
    }
  },
  "websocket": {"path": "/api/v1/events", "auth": "first-message", "eventVersion": 1}
}
```

  `state` is one of `starting`, `ready`, `failed`, `exited`. The bridge reports
  `ready: true` only after the real `ready` frame, protocol negotiation, and a
  successful `get_state`. `summary` excludes `systemPrompt`, `sessionFile`,
  provider `baseUrl`, and credentials.
- `POST /api/v1/tokens/revoke` revokes the bearer token used for the request and
  returns `204 No Content`.

### Errors

Every HTTP error uses:

```json
{"error":{"code":"UNAUTHORIZED","message":"A valid bearer token is required.","requestId":"<opaque>"}}
```

Implemented HTTP codes: `UNAUTHORIZED`, `INVALID_INPUT`, `INVALID_HOST`,
`INVALID_ORIGIN`, `RATE_LIMITED`. Engine startup failures appear in
`engine.error.code`: `OMP_NOT_FOUND`, `OMP_START_FAILED`, `OMP_MISSING_MODEL`,
`OMP_START_TIMEOUT`, `RPC_PROTOCOL_ERROR`, `RPC_UNSUPPORTED_PROTOCOL`,
`RPC_NEGOTIATION_FAILED`, `RPC_STATE_FAILED`, `OMP_EXITED`.

### WebSocket events

After upgrade, the client sends
`{"type":"authenticate","token":"<token>"}` within 5 seconds. Invalid or late
authentication closes with code `4401` (unauthorized) or `4408` (timeout); no
event is sent before acceptance. Every server message uses:

```json
{"version":1,"type":"bridge.status","sequence":1,"occurredAt":1790000000000,"payload":{}}
```

`occurredAt` is Unix milliseconds. `sequence` increases per bridge process.
Implemented event types:

- `bridge.status` — first event; payload is the status snapshot above.
- `engine.ready`, `engine.failed`, `process.exited` — engine status payloads.
- `omp.event` — `{"rawType":"<OMP frame type>","frame":{...}}`; raw OMP frames
  remain an extensible diagnostic field, not a stable typed UI contract yet.
- `bridge.warning` — `{"code","message"}` for rejected frames or chunk errors.
- `bridge.resync` — `{"skipped", "status"}` after broadcast lag.

If a client sees a sequence gap, it must fetch `/api/v1/status` again. No prompt
actions or history replay exist in this subset.

## 2. Initial endpoints


### Pairing and authorization

A first-run local pairing flow must be designed before feature endpoints are enabled. It should require a one-time secret shown/created by an explicit Termux command or a similarly explicit user action. Pairing tokens expire and can be revoked. Do not print the long-lived token into ordinary logs.

- `POST /api/v1/pair` — exchange a short-lived pairing secret for a client token.
- `GET /api/v1/capabilities` — return API version, bridge version, OMP detection status, feature flags, and supported transport/event versions after authentication.
- `POST /api/v1/tokens/revoke` — revoke the current token (or later, a specifically selected token).

Token storage and pairing UX require a documented security review. WebSocket credentials must not appear in URL query strings.

### Workspaces

- `GET /api/v1/workspaces` — only explicitly configured workspace roots and their human-readable labels.
- The bridge must not expose arbitrary filesystem paths to the client just because the bridge process can access them.

### Runtime

- `POST /api/v1/runtimes` — start a runtime for a selected `workspaceId` and approved runtime options.
- `GET /api/v1/runtimes/{runtimeId}/state` — authoritative snapshot for connection, process, session, agent activity, model, pending UI request and feature support.
- `POST /api/v1/runtimes/{runtimeId}/actions` — typed allowlisted action, not raw arbitrary command execution.
- `GET /api/v1/runtimes/{runtimeId}/entries?cursor=...&limit=...` — bounded entry/history page where supported.
- `DELETE /api/v1/runtimes/{runtimeId}` — request graceful runtime shutdown; the bridge drains remaining stdout before closing.

Initial action types should include only those verified in the installed OMP RPC schema, such as `prompt`, `abort`, `new_session`, `open_session`/`switch_session`, `set_model`, and `set_thinking_level`. Use typed action objects and validate required fields by action. Do not create the runtime API around an unrestricted `{ type: string, payload: any }` that forwards user content blindly.

### Files and Git (P2)

- `GET /api/v1/workspaces/{workspaceId}/files?path=relative/path` — list one safe directory.
- `GET /api/v1/workspaces/{workspaceId}/file?path=relative/file` — read a bounded text file; identify binary/too-large files explicitly.
- `GET /api/v1/workspaces/{workspaceId}/git/status` — read-only status.
- `GET /api/v1/workspaces/{workspaceId}/git/diff?path=relative/file` — read-only diff for one file at a time.

Paths are workspace-relative. Reject `..` traversal, absolute paths, encoded traversal, NUL, paths escaping the root through symlinks, and invalid workspace IDs. Never trust a frontend path check by itself.

## 3. WebSocket events

Endpoint: `WS /api/v1/events`.

- Authenticate immediately using an initial message with a short timeout. The server sends no runtime content before authentication succeeds.
- Never pass a bearer token via query parameter.
- Use a versioned envelope, for example:

```json
{
  "version": 1,
  "type": "runtime.event",
  "runtimeId": "opaque-runtime-id",
  "sequence": 42,
  "occurredAt": "2026-01-01T00:00:00Z",
  "payload": { "kind": "agent_start" }
}
```

The example is illustrative; actual payload types must be specified and validated. Sequence numbers are scoped to a runtime/stream and help clients detect gaps. If a gap is detected, fetch a fresh state snapshot and the missing history if the API supports it.

Define explicit bridge event types for at least:
- runtime state changed / process exit;
- raw OMP event normalized to a versioned client event;
- streamed message update and message completion;
- prompt result;
- extension UI request and resolution state;
- available commands update;
- subagent lifecycle/progress/events;
- bridge warning/error;
- reconnect/resync marker.

Do not expose unbounded raw JSON blobs as the permanent client API. The bridge may carry an extensible `rawType`/diagnostic subfield, but stable UI behavior must use typed documented fields.

## 4. Request correlation and duplicate protection

- Use unique string IDs for each OMP RPC command.
- For prompt submission, client request acceptance and agent completion are different lifecycle states.
- Return the bridge action ID promptly, then send completion events through WebSocket.
- Avoid replaying a prompt after reconnect unless the client can prove the request never reached OMP. Show an uncertain status and resynchronize rather than duplicate destructive work.

## 5. Compatibility

- `GET /api/v1/capabilities` advertises bridge/API versions, OMP version if detected, and supported features.
- The frontend must degrade gracefully when optional features are unsupported.
- Breaking changes require `/api/v2` or a compatible version negotiation policy.
- Add API contract tests for every documented route, response, error and authorization requirement.
