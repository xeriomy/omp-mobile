# Security Specification and Threat Model

## Assets

- OMP provider credentials and configuration.
- Workspace source code and local files.
- Ability to run OMP tools and child commands with the Termux account's permissions.
- Session history, prompts, tool output, client tokens and pairing secrets.

## Threats

- Another local app or web page attempts to invoke the bridge.
- Accidental LAN/public binding exposes the ability to control OMP.
- Token leakage through logs, URLs, crash reports, backups or exported diagnostics.
- Path traversal, symlink escape, malformed paths, large-file/memory exhaustion.
- Malformed RPC frames or compromised child output exhaust memory or corrupt state.
- WebSocket unauthenticated event access, cross-origin request abuse or replay.
- Duplicate prompt after reconnect creates unintended repeated work.
- Agent output contains hostile HTML/Markdown/script-like content.
- A compromised or malicious coding agent tries to persuade the user to approve unsafe operations; the client weakens OMP approvals unintentionally.

## Required controls

1. Loopback bind (`127.0.0.1`) by default; explicit opt-in and warnings for any wider bind.
2. Pairing secret is short-lived and single-use. Client tokens are revocable, generated with a cryptographically secure RNG, and never included in URL query strings.
3. Store tokens only in an Android-secure storage option verified to work with the selected Tauri version, or require repairing until such storage is implemented. Never store tokens in plain localStorage.
4. Authenticate all functional HTTP endpoints. Authenticate WebSocket before emitting any private event; close on timeout/failure.
5. Use exact tested Origin allowlisting and Host validation. Never fix CORS by adding `*`.
6. Request/body/frame/output limits, bounded channels and rate limits.
7. Canonicalize workspaces and all file paths; reject traversal, NUL, absolute client paths and symlink escape.
8. Spawn child programs with explicit arguments and current directory; do not interpolate shell commands.
9. Redact credentials and content from logs. Diagnostics must be opt-in and sanitized.
10. Validate incoming and outgoing DTOs; treat OMP stdout as input even though it is a child process.
11. Do not send host-tool or URI responses until allowed by a documented feature and appropriate user consent.
12. No silent tool approval; OMP's own approval model remains authoritative.
13. Security tests are mandatory for auth, revoke, origin, bind, path safety, message limits and process lifecycle.

## Local bridge boundary

Loopback does not replace authentication. Other local processes can often reach loopback listeners. The first pairing step must be user initiated and must not expose an unauthenticated “pair any device” route to every local caller. If the platform requires an exceptional bootstrapping endpoint, constrain it to a one-time high-entropy secret and a short time window.

## Remote access

Remote access is not enabled in the first milestone. Before adding it:
- write an explicit ADR and updated threat model;
- require TLS or a documented private encrypted transport;
- require per-device tokens with revocation;
- rate-limit pairing and API calls;
- add audit-friendly connection status without logging payloads;
- test token rotation, expiry and revocation;
- warn users not to forward an unprotected port directly to the public internet.

## Security acceptance tests

- No token: all functional endpoints reject.
- Invalid/revoked token: HTTP and WS rejected.
- WS sends no event before auth success.
- Pairing code expires and can only be used once.
- Wrong Origin/Host rejected under the policy chosen for actual Tauri origins.
- Default listener is loopback; wide bind requires explicit opt-in.
- Workspace traversal and symlink escape rejected.
- Oversized request/RPC/event is rejected without unbounded allocation.
- Logs do not contain pairing secrets, bearer tokens, provider keys, prompts or file contents by default.
- OMP crash/shutdown does not leave an orphan child process.
