# Current Checkpoint

> Phase 1 loopback bridge implemented and tested against real OMP `18.13.3`; bridge-connected mobile client implemented and browser-independent client behavior exercised against the live bridge. Android device loopback remains unverified. Measured on 2026-10-30.

## Snapshot

- **Project:** OMP Client
- **Milestone:** Phase 1 — authenticated loopback bridge and connection diagnostics
- **Status:** Bridge **PASS** on host with real OMP. Rust/frontend automated checks **PASS**. Live HTTP/WS/revoke through the mobile client module **PASS**. Android device bridge connection **NOT RUN**.
- **Branch:** `Rust-Bridge-and-Android-Loopback` (placeholder branch from `main`)
- **Base commit:** `663aed1` (`docs: record merged main CSP build run 41113969884`)
- **Code commits:** `625a823` (bridge and shared RPC crate), `88c886e` (mobile client).
- **Pull request:** [#3](https://github.com/xeriomy/omp-mobile/pull/3), open against `main`; CI result pending.
- **Working tree:** clean after the docs commit that records these SHAs.
- **Last updated:** 2026-10-30

## Environment (measured)

| Item | Value |
|---|---|
| Local host | Ubuntu 26.04.1 LTS, aarch64, PRoot distro on Android 15 (`arm64-v8a`, model `23021RAAEG`) |
| Rust | `rustc 1.91.1`, `cargo 1.91.1` |
| Node / npm | Node `v26.13.0`, npm `11.20.0` |
| OMP | `omp/18.13.3`, `/tmp/opencode/omp-bin/omp-linux-arm64` |
| OMP SHA-256 | `981f08df6c6344356b31aa696fdd810b8b5fa8c8851ba926d3940e793d47cc3a` |
| Android `adb` | unavailable: bundled `platform-tools/adb` is x86_64 ELF on an aarch64 host; no Termux `adb` in PATH |
| Browser automation | unavailable: OpenChamber browser control needs the desktop app; Playwright is not installed |

## Implemented

### Bridge (`apps/bridge`)

- Axum 0.8 + Tokio loopback HTTP/WebSocket service.
- Validated config rejects non-loopback bind, wildcard/invalid origins, missing workspace, oversized limits, and zero timeouts.
- `omp-bridge doctor` validates config and runs `omp --version` without starting OMP.
- One real `omp --mode rpc-ui` child process, explicit argument array, selected working directory, inherited environment, own process group.
- Concurrent stdout/stderr drain. stdout is protocol-only; stderr is bounded and redacted.
- Ready-frame validation, advertised physical-frame limit, v2 negotiation, id-correlated `get_state`.
- `ready: true` only after real ready + negotiation + successful state.
- Startup failures are classified: `OMP_NOT_FOUND`, `OMP_START_FAILED`, `OMP_MISSING_MODEL`, `OMP_START_TIMEOUT`, `RPC_PROTOCOL_ERROR`, `RPC_UNSUPPORTED_PROTOCOL`, `RPC_NEGOTIATION_FAILED`, `RPC_STATE_FAILED`, `OMP_EXITED`.
- Pairing: 10-character code from `ABCDEFGHJKLMNPQRSTUVWXYZ23456789`, single use, 10-minute TTL, five wrong attempts, hashed at rest.
- Tokens: 32 random bytes, hashed at rest, 24-hour TTL, revocable, memory-only, printed only once to the pairing client.
- Exact Host and Origin guard; narrow CORS; 16 KiB request-body limit; 5-second WS auth deadline; no event before auth.
- Versioned WebSocket envelopes; bounded broadcast; lag emits `bridge.resync`; WS send timeout.
- Shutdown closes stdin, waits 3 seconds, then SIGKILLs the child process group; tasks joined and pending requests cleared.

### Shared RPC crate (`crates/omp-rpc`)

Moved the Phase 0 framing/correlation code out of `spike/rpc-harness/src/lib.rs` so the bridge and harness share the same bounded JSONL parser and v2 chunk assembler. The harness remains a workspace member.

### Mobile (`apps/mobile`)

- Replaced the smoke screen with a real pairing/connection/diagnostics screen.
- Loopback-only URL validation (`127.0.0.1`, `localhost`, `[::1]`), no path/credentials/query.
- Pairing code entry; token and token ID kept in memory only; expiry displayed; revoke/forget.
- Real `/healthz`, `/api/v1/pair`, `/api/v1/status`, `/api/v1/tokens/revoke`, and WebSocket first-message auth.
- Displays engine state, model, protocol, session, message count, streaming/settled flags, spurious-free error states, last 20 live events, and explicit paused/stale state.
- Manual reconnect and 5-second status polling; no fake model/session data and no prompt action.
- CSP extended with loopback HTTP/WS sources.
- Added Node tests for URL normalization and rejection rules.

## Evidence

### Rust checks

Commands run from the repository root:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Results:

- `cargo fmt`: exit 0.
- `cargo clippy -D warnings`: exit 0.
- `cargo test --workspace`: **54 passed, 0 failed, 1 ignored**.
  - omp-bridge unit: 23 passed.
  - engine lifecycle integration: 11 passed.
  - HTTP/WebSocket integration: 9 passed.
  - omp-rpc unit: 11 passed.
  - harness unit: 0 tests.
  - doc tests: 0 tests.
  - `real_omp`: 1 ignored by default.

Engine lifecycle tests cover real-ready ordering, v1 fallback, malformed frame tolerance, missing binary, missing model, startup timeout, unsupported versions, rejected negotiation/state, oversized frames, real stdout event delivery, and child cleanup.

HTTP/WS tests cover minimal health, single-use pairing, missing/revoked/expired auth, pairing rate limit, safe state projection, exact Origin/Host rejection, first-message WS auth, bad token/garbage close `4401`, disallowed WS Origin, live event delivery, and token expiry metadata.

### Real OMP end-to-end

Command:

```sh
OMP_BIN=/tmp/opencode/omp-bin/omp-linux-arm64 \
OMP_MODEL=anthropic/claude-sonnet-4-5 \
OMP_WORKSPACE=/tmp/opencode/real-omp-workspace \
cargo test -p omp-bridge --test real_omp -- --ignored --nocapture
```

Result: **PASS** in 5.89 s.

```text
real OMP ready: anthropic/claude-sonnet-4-5
test real_omp_boots_negotiates_and_serves_authenticated_status ... ok
```

This test starts the real OMP child through `RunningBridge`, asserts engine `ready`, protocol v2, a real model in `get_state`, authenticates HTTP with a paired token, fetches `/api/v1/status`, checks that `systemPrompt` and `sessionFile` are absent, and shuts down. No prompt was sent.

CLI evidence:

```text
$ cargo run -q -p omp-bridge -- --omp-bin /tmp/opencode/omp-bin/omp-linux-arm64 \
    --workspace /tmp/opencode/real-omp-workspace --doctor
omp-bridge doctor
  OMP binary: /tmp/opencode/omp-bin/omp-linux-arm64
  Workspace:  /tmp/opencode/real-omp-workspace
  Listen:     127.0.0.1:8765
  Origins:    http://tauri.localhost, https://tauri.localhost, tauri://localhost, http://localhost:1420
  Limits:     1048576 byte frames, 67108864 byte reassembled
  Version:    omp/18.13.3
doctor: configuration looks usable; no OMP process was started.
```

Non-loopback rejection:

```text
$ cargo run -q -p omp-bridge -- --listen 0.0.0.0:8765 ...
omp-bridge: refusing non-loopback listen address 0.0.0.0:8765; remote mode is not implemented
exit=1
```

Live bridge startup output (single-use pairing code intentionally not recorded):

```text
OMP Bridge 0.1.0
  API:       http://127.0.0.1:8765/api/v1
  WebSocket: ws://127.0.0.1:8765/api/v1/events
  OMP:       /tmp/opencode/omp-bin/omp-linux-arm64 --mode rpc-ui --model anthropic/claude-sonnet-4-5 (cwd /tmp/opencode/real-omp-workspace)
  Engine:    ready, protocol v2, model anthropic/claude-sonnet-4-5
  Pairing:   <single-use code> (single use, expires in 10 minutes)
Press Ctrl+C to stop the bridge and its OMP child process.
```

### Mobile client behavior against the live bridge

The built Svelte page could not be driven in a browser here: OpenChamber's browser panel cannot control pages in this environment and Playwright is not installed. Instead, the committed TypeScript client module was imported with Node 26 and exercised against the live real-OMP bridge:

```json
{
  "api": "v1",
  "engine": "ready",
  "ready": true,
  "protocol": 2,
  "model": { "provider": "anthropic", "id": "claude-sonnet-4-5" },
  "tokenExpires": true
}
```

```json
{ "event": "bridge.status", "version": 1, "sequence": 6, "engine": "ready" }
```

```text
revoked token rejected: 401
```

This proves the client's URL normalization, health check, pairing request, bearer status request, WebSocket first-message auth, event envelope parsing, and revoke path against the real bridge. It does **not** prove DOM rendering, CSP behavior, Android WebView fetch/WS, or touch interaction.

### Frontend checks

Commands run from `apps/mobile`:

```sh
npm ci --no-audit --no-fund
npm run format:check
npm run test
npm run check
npm run build
```

Results:

- `format:check`: PASS, all files match Prettier.
- `test`: PASS, 3 tests.
- `check`: PASS, 0 errors, 0 warnings.
- `build`: PASS, adapter-static wrote `build/`.

Repository metadata:

```sh
node scripts/validate-manifest.mjs
```

Result: `PASS: 60 files present, no prompt-pack references.`

CI workflow now runs frontend format/test/check/build and root-workspace Rust fmt/clippy/test before the Android build. Run `41130961933` failed because the new root workspace implicitly claimed `apps/mobile/src-tauri`, whose own locked metadata check then refused to run. Commit `414c640` excludes that crate; the next run passed.

### Android APK build (CI)

Run [`41131544289`](https://github.com/xeriomy/omp-mobile/actions/runs/41131544289) on `414c640`: **PASS** in 5m03s. The workflow ran the Rust workspace checks, frontend checks, Android debug build, `aapt2` badging assertions, and APK content checks.

Downloaded artifact:

| Field | Value |
|---|---|
| Artifact | `omp-client-android-debug-fdb21e6a277c48403835492543deb9428c9c7e4c` |
| APK | `app-universal-debug.apk` |
| Size | `121155701` bytes |
| SHA-256 | `3f81e99dc05a0a0221478ed58cc7eb6eb96de6867cb1ea255b358f1404e65e61` |
| Native lib | `lib/arm64-v8a/libomp_client_lib.so` present |
| Package / version | `com.ompclient.mobile` / `0.1.0` |
| Embedded CSP | `connect-src` includes loopback HTTP/WS sources from ADR-013 |

This proves the bridge-connected client compiles, packages, and carries the loopback CSP. It does not prove the app runs or connects on a device.

## Security review

- Listener is loopback-only; `0.0.0.0` fails before OMP starts.
- Pairing codes and tokens are never logged or placed in URLs. Only SHA-256 hashes are retained. Tokens are memory-only in the app.
- Functional HTTP endpoints require bearer auth. WebSocket rejects unauth/garbage/late clients with `4401`/`4408` and sends no event first.
- Host must be `127.0.0.1`, `localhost`, or `[::1]` with the configured port. Non-empty Origin must exactly match the allowlist; CORS is never wildcard.
- `get_state` is projected to a safe summary; prompts, session files, provider URLs, and credentials are excluded from status.
- Child stdout/stderr are drained independently; stderr is bounded, redacted, and only surfaced after startup failure classification.
- Request and frame sizes are bounded; broadcast is bounded; lag resynchronizes.
- OMP runs in its own process group and is killed as a group only after graceful shutdown times out.
- No provider credentials, prompts, or tokens were committed. The real-OMP test used the existing host configuration and did not send a model prompt.

## Known issues and blockers

- **Android device not verified.** No working `adb`; APK install and WebView loopback/CORS/cleartext behavior remain open. Do not claim success from the host-only test.
- **No browser DOM/CSP runtime test.** OpenChamber browser control and Playwright were unavailable. Svelte compile/type/build checks pass, but CSP behavior on Android remains unverified.
- **No OpenAPI contract package.** The exact v1 subset is documented in `Specs/BRIDGE_API_SPEC.md`; `packages/contracts` and generated-type drift checks do not exist.
- **No prompt/control API.** Chat, abort, sessions, model selection, files, Git, and extension UI remain future phases.
- **No Android secure storage.** Token is memory-only, so app restart requires pairing again.
- **No automatic WebSocket retry.** The UI offers manual reconnect and shows a stale/paused state.
- **OMP raw events are not normalized.** `omp.event` carries `rawType` and the raw frame; stable typed UI events are future work.
- **Termux deployment unverified.** The bridge ran in this Ubuntu/PRoot environment, not a plain Termux toolchain.
- **CI artifact not installed.** The bridge-connected APK exists and was inspected, but no device has run it.

## Next best action

Install `app-universal-debug.apk` (SHA-256 `3f81e99d…65e61`) on the target Android 15 device while `omp-bridge` runs in Termux, then verify pairing, real status, live events, revoke, WebView origin/CORS, and `adb logcat`. Device access is blocked on this host, so the next engineering task is the OpenAPI/event contract package with generated types and a drift check.
