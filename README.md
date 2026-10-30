# OMP Client

Android-first mobile client for **OMP (oh-my-pi)**, built around OMP's native JSONL RPC protocol: a standalone Rust bridge owns the real `omp --mode rpc-ui` process, and a Tauri 2 + Svelte app connects to it over authenticated loopback HTTP/WebSocket.

## Current status

- **OMP RPC proof:** verified against the real `omp/18.13.3` binary. The ready frame, protocol v2 negotiation, and an id-correlated `get_state` succeed; the child exits cleanly. See [`Checkpoint/CURRENT.md`](Checkpoint/CURRENT.md).
- **Bridge:** implemented in `apps/bridge`. It binds loopback only, validates Host/Origin, pairs with a single-use code, issues expiring revocable tokens, owns one OMP child process, drains stdout/stderr, correlates responses by ID, enforces frame limits, serves versioned HTTP status, and streams authenticated WebSocket events.
- **Mobile app:** the Tauri 2 + Svelte screen pairs with the bridge and displays only real bridge/OMP state, live events, and explicit disconnected/error states. It never sends prompts yet.
- **Automated evidence:** 23 bridge unit tests, 11 engine lifecycle tests, 9 HTTP/WebSocket integration tests, 11 shared RPC tests, 3 frontend client tests, and an opt-in real-OMP end-to-end test pass locally. The real-OMP test also passed against `omp/18.13.3`.
- **Android APK:** the existing CI workflow builds a debug arm64-v8a APK. A bridge-connected APK build on the target device is **not** verified yet; this host has no usable `adb` (the platform-tools binary is x86_64 on an aarch64 host).
- **Not implemented:** chat/prompts, abort, sessions, model selection, files, Git, Android secure token storage. Tokens stay in memory and must be re-paired after a bridge restart.

## Repository layout

```text
.
├── Cargo.toml                  # workspace: bridge, omp-rpc, Phase 0 harness
├── apps/
│   ├── bridge/                 # omp-bridge binary + HTTP/WS/engine tests
│   └── mobile/                 # Tauri 2 + Svelte + TypeScript app
├── crates/omp-rpc/             # bounded JSONL framing and chunk reassembly
├── spike/rpc-harness/          # Phase 0 live probe (workspace member)
├── Specs/                      # product, API, RPC, UI, security contracts
├── Plan/                       # decisions, risks, implementation plan
├── Roadmap/ROADMAP.md
├── Checkpoint/CURRENT.md
├── scripts/validate-manifest.mjs
└── .github/workflows/android-debug-apk.yml
```

`packages/contracts` is still a planned contract home; no generated OpenAPI client or schema drift check exists yet.

## Start the bridge

Build on the host that will run OMP (Termux/PRoot or a development machine):

```sh
cargo build -p omp-bridge --release
./target/release/omp-bridge \
  --omp-bin /path/to/omp \
  --workspace /path/to/workspace \
  --model provider/model
```

The bridge prints the loopback URL and a random single-use pairing code. The pairing code is valid for 10 minutes by default; a client token is valid for 24 hours and is dropped when the bridge restarts. Press `Ctrl+C` to stop the bridge and its OMP child.

Validate configuration without starting OMP:

```sh
./target/release/omp-bridge --omp-bin /path/to/omp --workspace /path/to/workspace --doctor
```

Useful flags and environment variables (prefix `OMP_BRIDGE_`):

| Flag | Default | Purpose |
|---|---|---|
| `--listen` | `127.0.0.1:8765` | Loopback listener; non-loopback addresses are rejected |
| `--allowed-origins` | Tauri packaged origins + `http://localhost:1420` | Exact HTTP Origin allowlist |
| `--model` | unset | Passed to OMP as `--model` |
| `--startup-timeout-secs` | `90` | Wait for real `ready` + negotiation + `get_state` |
| `--request-timeout-secs` | `60` | Per-RPC response timeout |
| `--pairing-ttl-secs` | `600` | Single-use pairing window |
| `--token-ttl-secs` | `86400` | Client token lifetime |
| `--max-frame-bytes` | `1048576` | Physical stdout frame limit |
| `--max-reassembled-bytes` | `67108864` | v2 chunk reassembly limit |

### API v1

```text
GET  /healthz                    unauthenticated, returns {"status":"ok"}
POST /api/v1/pair                {"code":"..."} -> {"token","tokenId","expiresAt"}
GET  /api/v1/status              authenticated state snapshot
POST /api/v1/tokens/revoke       authenticated, revokes the current token
WS   /api/v1/events              first message: {"type":"authenticate","token":"..."}
```

All functional endpoints require `Authorization: Bearer <token>`. The WebSocket authenticates in its first text message within 5 seconds and sends no event before that succeeds. Tokens never appear in URL query strings. Errors use `{"error":{"code","message","requestId"}}`.

## Run the mobile app

Requires Node.js 24 and npm 11:

```sh
cd apps/mobile
npm ci
npm run test
npm run check
npm run build
npm run tauri -- android init --ci --skip-targets-install
npm run tauri -- android build --debug --apk --target aarch64 --ci
```

For desktop development, the bridge allowlist includes `http://localhost:1420`; start Vite on that exact port. The packaged Android app uses Tauri's `http://tauri.localhost` origin. The CSP allows loopback `http://127.0.0.1:*`, `ws://127.0.0.1:*`, `http://localhost:*`, `ws://localhost:*`, and IPv6 equivalents in `connect-src`.

Android builds require JDK 17, Android SDK platform `android-37.0`, Build Tools `36.0.0`, NDK `29.0.13846066`, and the Rust `aarch64-linux-android` target. Use the GitHub Actions workflow; do not build the APK on this aarch64 PRoot host.

## Validation

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

cd apps/mobile
npm run format:check
npm run test
npm run check
npm run build
```

The real-engine test is ignored by default:

```sh
OMP_BIN=/path/to/omp OMP_MODEL=provider/model \
  cargo test -p omp-bridge --test real_omp -- --ignored --nocapture
```

It proves real `ready`/v2/`get_state`, authenticated HTTP status, and clean shutdown. It does not prove prompts, abort, approvals, or device networking.

## Verified Android artifact (pre-bridge)

The `Android debug APK` workflow on `main` (run [41113969884](https://github.com/xeriomy/omp-mobile/actions/runs/41113969884), merge commit `35e882f`) built and uploaded a debug APK on 2026-10-30. It contains the earlier smoke screen, not this bridge client.

| Field | Value |
|---|---|
| Artifact | `omp-client-android-debug-35e882f475dd38cf2a462fbe4f8821937560f724` |
| Size / SHA-256 | `121146901` bytes / `b6c37dd7245eec6d55766b1991212fd38fd9731135442bf0434a371946df10c1` |
| Package / version | `com.ompclient.mobile` / `0.1.0` |
| minSdk / targetSdk / compileSdk | `24` / `37` / `37` |
| ABI | `arm64-v8a` |
| Signing | Android debug certificate, not a release key |

A user installed an earlier debug build on the target Android 15 arm64 device and supplied a screenshot of the smoke screen. That verifies installation and WebView rendering only. `adb logcat`, lifecycle/keyboard behavior, loopback networking from the app, and a bridge-connected APK remain unverified.

## Non-negotiable engineering rules

- No fake API, fake RPC responses, static demo data presented as real engine output, or UI-only completion claims.
- No raw shell-string interpolation. Spawn processes with explicit argument arrays and a selected working directory.
- Do not assume undocumented OMP commands, filesystem layouts, event shapes, or CLI flags. Verify against the installed executable and current upstream documentation/source.
- Never expose an unauthenticated bridge on LAN or the public internet. Bind to loopback by default.
- Do not work around an OMP approval/permission flow by silently auto-approving tools.
- Use tests, formatter, linter, type checker, LSP diagnostics, and debugger/logcat/browser DevTools where available. If a tool is unavailable, record the limitation and use a verifiable alternative.
- Keep the checkpoint and roadmap truthful. "Implemented" means tested, not merely written.

## Useful upstream references

- OMP repository: https://github.com/can1357/oh-my-pi
- OMP RPC protocol: https://github.com/can1357/oh-my-pi/blob/main/docs/rpc.md
- OMP CLI reference: https://github.com/can1357/oh-my-pi/blob/main/docs/cli-reference.md
- Tauri development: https://v2.tauri.app/develop/
- Tauri Android prerequisites: https://v2.tauri.app/start/prerequisites/#android
- Tauri Android packaging: https://v2.tauri.app/distribute/google-play/
