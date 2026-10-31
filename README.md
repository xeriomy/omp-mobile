# OMP Client

Android-first mobile client for **OMP (oh-my-pi)**, built around OMP's native JSONL RPC protocol: a standalone Rust bridge owns the real `omp --mode rpc-ui` process, and a Tauri 2 + Svelte app connects to it over authenticated loopback HTTP/WebSocket.

## Current status

- **OMP RPC proof:** verified against real `omp/18.14.0` on this host (pinned tested baseline `18.13.3`). The ready frame, protocol v2 negotiation, and an id-correlated `get_state` succeed; the child exits cleanly. See [`Checkpoint/CURRENT.md`](Checkpoint/CURRENT.md).
- **Bridge:** implemented in `apps/bridge`. It discovers OMP, persists validated configuration, starts detached with a private startup handshake, owns one OMP child process, binds loopback only, validates Host/Origin, pairs with a single-use code, issues expiring revocable tokens, drains stdout/stderr, correlates responses by ID, enforces frame limits, serves versioned HTTP status, and streams authenticated WebSocket events. Lifecycle commands: `setup`, `start`, `stop`, `restart`, `status`, `logs`, `doctor`, `config`, `update`, `uninstall`.
- **Mobile app:** the Tauri 2 + Svelte screen auto-detects a loopback bridge, validates its capabilities before pairing, displays only real bridge/OMP state, live events, and explicit disconnected/error states. It never sends prompts yet.
- **Automated evidence:** 40 bridge unit tests, 6 CLI lifecycle tests, 11 engine lifecycle tests, 10 HTTP/WebSocket integration tests, 11 shared RPC tests, 6 frontend client tests, and installer script tests pass locally. The opt-in real-OMP end-to-end test also passed against `omp/18.14.0`.
- **Android APK:** CI run `41131544289` built the earlier bridge-connected debug APK. The user installed it on the target Android 15 device, paired with a real bridge running in PRoot, and the app showed `Connected`/`Live`, engine `Ready`, model `anthropic/claude-sonnet-4-5`, protocol `v2`, and a live `bridge.status` event. The Phase 1.5 auto-detection UI has not been installed on a device. `adb logcat` is still unavailable here (the platform-tools binary is x86_64 on an aarch64 host).
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
├── Specs/                      # product, API, lifecycle, RPC, UI, security contracts
├── Plan/                       # decisions, risks, implementation plan
├── Roadmap/ROADMAP.md
├── Checkpoint/CURRENT.md
├── scripts/
│   ├── validate-manifest.mjs
│   ├── install-bridge.sh       # GitHub Releases installer with SHA-256 check
│   └── test-install-bridge.sh  # executable installer tests
└── .github/workflows/
    ├── android-debug-apk.yml
    └── bridge-release.yml      # aarch64/x86_64 glibc release assets
```

`packages/contracts` is still a planned contract home; no generated OpenAPI client or schema drift check exists yet.

## Install and start the bridge

Install the release binary for `aarch64-unknown-linux-gnu` or
`x86_64-unknown-linux-gnu` glibc Linux. The installer verifies the published
SHA-256 and never uses `sudo`:

```sh
curl -fsSL https://raw.githubusercontent.com/xeriomy/omp-mobile/main/scripts/install-bridge.sh | sh
```

To pin a version or directory:

```sh
curl -fsSL https://raw.githubusercontent.com/xeriomy/omp-mobile/main/scripts/install-bridge.sh \
  | OMP_BRIDGE_VERSION=v0.0.1 OMP_BRIDGE_INSTALL_DIR="$HOME/.local/bin" sh
```

Native Termux, musl, and unknown architectures are rejected until they are
built and tested. The supported first-release route is glibc Linux, including
Ubuntu under PRoot on Android.

If you are root inside Ubuntu PRoot (the usual Android 15 setup), the installer
detects PRoot environment markers (`container=proot-distro`, `PROOT_*`), warns
that this root is the container user rather than Android device root, and keeps
the install inside `$HOME`:

```sh
curl -fsSL https://raw.githubusercontent.com/xeriomy/omp-mobile/main/scripts/install-bridge.sh | sh
```

If root detection cannot see PRoot (for example a minimal container), opt in
explicitly. Do not use this on a normal multi-user host:

```sh
curl -fsSL https://raw.githubusercontent.com/xeriomy/omp-mobile/main/scripts/install-bridge.sh \
  | OMP_BRIDGE_ALLOW_ROOT=1 sh
```

Then:

```sh
omp-bridge setup    # discover OMP, verify a real RPC startup, save config
omp-bridge start    # start detached and print the pairing code once
omp-bridge status
omp-bridge logs --lines 100
omp-bridge stop
```

`setup` checks `--omp-bin`, `OMP_BRIDGE_OMP_BIN`, saved configuration, `PATH`,
and a fixed list of user install locations, in that order. It accepts OMP `18.x`
at or above `18.13.3` and requires a real `ready`/negotiation/`get_state`
sequence before saving.

For a one-off foreground run:

```sh
omp-bridge start --foreground
```

For a local development build instead of a release asset:

```sh
cargo build -p omp-bridge --release
./target/release/omp-bridge setup --omp-bin /path/to/omp --workspace /path/to/workspace
./target/release/omp-bridge start
```

Configuration precedence is explicit flag, environment, saved config, default.
`start`, `restart`, and `doctor` apply one-run overrides without rewriting the
saved file. `uninstall` requires `--yes` unless `--dry-run`; it keeps
configuration unless `--purge` is passed. `update` only prints verified upgrade
instructions; the installer performs upgrades.

Paths and full lifecycle behavior are in
[`Specs/BRIDGE_LIFECYCLE_SPEC.md`](Specs/BRIDGE_LIFECYCLE_SPEC.md).

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
| `--log-max-bytes` | `1048576` | Rotate `bridge.log` at this size on next start |

`OMP_BRIDGE_HOME`, `OMP_BRIDGE_RUNTIME_DIR`, and `OMP_BRIDGE_LOG_DIR` move the
config, runtime, and log directories.

### API v1

```text
GET  /healthz                    unauthenticated, returns {"status":"ok"}
GET  /api/v1/capabilities        unauthenticated product/version identity
POST /api/v1/pair                {"code":"..."} -> {"token","tokenId","expiresAt"}
GET  /api/v1/status              authenticated state snapshot
POST /api/v1/tokens/revoke       authenticated, revokes the current token
WS   /api/v1/events              first message: {"type":"authenticate","token":"..."}
```

All functional endpoints require `Authorization: Bearer <token>`. The WebSocket authenticates in its first text message within 5 seconds and sends no event before that succeeds. Tokens never appear in URL query strings. Errors use `{"error":{"code","message","requestId"}}`. Capabilities returns only non-secret product/version metadata so the mobile client can reject unrelated loopback listeners before pairing.

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

On launch, the app probes a saved loopback URL, then `127.0.0.1`/`localhost` on
ports `8765`–`8767`. It accepts only a listener that returns the expected
`omp-bridge` capabilities document, then asks for the single-use pairing code.
A manual loopback URL fallback and a clear-saved-URL action are available. The
token stays in memory; only the non-secret URL is saved.

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

cd ../..
node scripts/validate-manifest.mjs
shellcheck scripts/*.sh
sh scripts/test-install-bridge.sh
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

The user installed the bridge-connected build from run `41131544289` and connected it to `omp-bridge` running in the Ubuntu/PRoot environment on the same phone. A user-provided screenshot shows the real paired session: `Connected`/`Live`, API `v1`, engine `Ready`, model `anthropic/claude-sonnet-4-5`, protocol `v2`, one live `bridge.status` event, and a reported session ID. This verifies install, WebView rendering, loopback HTTP/WebSocket, Android cleartext to loopback, the app origin against the bridge allowlist, and the CSP at runtime.

That evidence predates the Phase 1.5 auto-detection UI, lifecycle CLI, and capabilities endpoint. The current APK has not been installed on a device in this session.

Still unverified: `adb logcat`, app restart, Android lifecycle/keyboard behavior, device-initiated revoke, the Phase 1.5 detection UI on device, GitHub Actions release execution, and a native Termux build of the bridge.

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
