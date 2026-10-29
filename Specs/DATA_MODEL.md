# Data Model (Draft)

Keep public API models distinct from internal runtime implementation details.

## ConnectionProfile (client-side)

- `id`: stable local UUID.
- `label`: user-editable display name.
- `baseUrl`: bridge address; default local loopback endpoint.
- `tokenReference`: opaque key to platform secure storage, never the token itself in ordinary app state.
- `createdAt`, `lastConnectedAt`.
- Optional user-selected display preferences; no provider credentials.

## Bridge configuration

- `listenHost`: defaults to `127.0.0.1`.
- `listenPort`: configurable and validated.
- `ompExecutable`: explicit path or safely resolved executable name.
- `workspaceRoots`: named canonical roots allowed for browsing/runtimes.
- `tokenStorePath`: protected state location appropriate to Termux.
- `requestTimeoutMs`, `maxRequestBytes`, `maxRpcFrameBytes`, `maxReassembledFrameBytes`, `maxEventQueue`.
- Optional `allowLan`: false by default, requires explicit secure-mode settings.

Do not expose the full config to the frontend. Redact secrets and filesystem details from capabilities.

## RuntimeSummary

- `runtimeId`: opaque bridge identifier, not an OS PID.
- `workspaceId`: opaque configured workspace identifier.
- `processState`: starting, ready, running, stopping, exited, failed.
- `sessionId` / `sessionPath`: opaque engine-defined value only if returned by OMP.
- `agentState`: idle, running, awaiting_input, aborting, unknown.
- `model`: provider/model identifiers as reported by OMP, optional.
- `lastError`: stable error code and safe message, optional.
- `startedAt`, `lastEventSequence`, capabilities.

## UiRequest

- `requestId`, `runtimeId`, `method`, validated method-specific payload, `expiresAt`, `status`.
- Secret input metadata must be retained so UI masks the value.
- Unknown methods must not be converted into auto-approval.

## BridgeEvent

- `version`, `type`, `runtimeId`, monotonically increasing `sequence`, `occurredAt`, typed `payload`.
- Do not promise exactly-once delivery over a reconnecting WebSocket; support gap detection and resync.

## Path rules

Workspace API paths are relative strings, normalized and validated on the bridge. Never treat an opaque `workspaceId` or `runtimeId` as a filesystem path. Resolve configured roots at startup; verify canonical paths again before access if a symlink/config change can alter the result.
