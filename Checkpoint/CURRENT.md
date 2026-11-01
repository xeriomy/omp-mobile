# Current Checkpoint

> Phase 1.5 bridge installer, OMP discovery, persistent configuration, background lifecycle, and mobile loopback auto-detection are implemented and tested on the host against real OMP `18.14.0`. PRoot-aware root handling, exact-tag release checkout, and no-silent-fallback discovery are in place. PR #3 merged and release `v0.0.1` is published with verified checksums. The Phase 1.5 APK is built by CI but not yet installed on a device. Measured on 2026-10-31.

## Snapshot

- **Project:** OMP Client
- **Milestone:** Phase 1.5 — bridge install, discovery, lifecycle, and mobile auto-detection
- **Status:** Host lifecycle **PASS** with real OMP. Rust/frontend/installer/release-workflow checks **PASS**. Android CI **PASS**. PR #3 **merged**. Release `v0.0.1` **published** and verified over HTTPS, including `setup`/`start`/`status`/`capabilities`/`stop`. Physical-device APK verification remains open.
- **Branch:** `Rust-Bridge-and-Android-Loopback` (merged into `main`).
- **Merge commit:** `05e291eaabfa115f32fead2c72dd2a3a56a609b9` on `main`.
- **Code commit:** `eb76441` (`fix(ci): avoid shellcheck SC2015 in release assertions`), after `6f38b61` (`fix(release): harden PRoot install, tag checkout, discovery, and update`).
- **Version:** workspace and mobile app `0.0.1`; tag and release `v0.0.1` match.
- **Release:** [`v0.0.1`](https://github.com/xeriomy/omp-mobile/releases/tag/v0.0.1), published 2026-10-31 by run `41253077815`.
- **Pull request:** [#3](https://github.com/xeriomy/omp-mobile/pull/3), merged to `main`.
- **Working tree:** clean after the release-evidence docs commit.
- **Last updated:** 2026-10-31

## Environment (measured)

| Item | Value |
|---|---|
| Local host | Ubuntu 26.04.1 LTS, aarch64, PRoot distro on Android 15 (`arm64-v8a`, model `23021RAAEG`) |
| Rust | `rustc 1.91.1`, `cargo 1.91.1` |
| Node / npm | Node `v26.13.0`, npm `11.20.0` |
| OMP | `omp/18.14.0`, `/root/.local/bin/omp` |
| Pinned tested baseline | OMP `18.13.3`; `18.x` at or above the baseline is accepted |
| Android `adb` | unavailable: bundled `platform-tools/adb` is x86_64 ELF on an aarch64 host; no Termux `adb` in PATH |
| Browser automation | unavailable: OpenChamber browser control needs the desktop app; Playwright is not installed |

## Implemented

### Persistent configuration (`config_store.rs`)

- Config home defaults to `$XDG_CONFIG_HOME/omp-bridge` or `$HOME/.config/omp-bridge`; override `OMP_BRIDGE_HOME`.
- Runtime defaults to `$XDG_RUNTIME_DIR/omp-bridge`, else `<config-home>/run`; override `OMP_BRIDGE_RUNTIME_DIR`.
- Logs default to `<config-home>/logs/bridge.log`; override `OMP_BRIDGE_LOG_DIR`.
- Directories use mode `0700`; config, runtime state, PID, and startup handshake use `0600`.
- Atomic write through a temporary file plus rename.
- Schema version `1`; version `0` migrates by filling defaults; a future version is rejected.
- Stores no bearer tokens, pairing codes, provider keys, prompts, session content, or file content.
- Precedence: explicit flag, environment, saved config, built-in default. One-run overrides are not persisted by `start`, `restart`, or `doctor`.

### OMP discovery (`discovery.rs`)

- Candidate order: `--omp-bin`, `OMP_BRIDGE_OMP_BIN`, saved `ompBin`, every `omp` on `PATH`, documented user-writable locations.
- Documented locations include `~/.local/bin`, `~/.omp/bin`, `~/bin`, `~/.local/share/omp/bin`, `$PREFIX/bin`, `/usr/local/bin`, and `/usr/bin`.
- Never walks a filesystem tree and never invokes a shell.
- Duplicate candidates are removed by canonical path.
- Selection policy: an explicit `--omp-bin`/`OMP_BRIDGE_OMP_BIN` must be usable and never falls back to PATH; a valid saved installation is kept and never silently replaced; if the saved executable disappears, `setup` fails with `OMP_SAVED_UNAVAILABLE` and keeps the saved config until the user reruns `setup --omp-bin`. With neither override nor saved config, the first valid `PATH`/known-location candidate wins deterministically.
- Validation: regular executable file, bounded `omp --version`, `18.x` at or above `18.13.3`, then a real `ready`/protocol negotiation/`get_state` startup.
- `setup` and `doctor` print classified failures without provider keys, prompts, or file contents.

### Background lifecycle (`process_manager.rs`, `main.rs`)

- Commands: `setup`, `start`, `start --foreground`, `stop`, `restart`, `status [--json]`, `logs [--lines N] [--follow]`, `doctor`, `config [--json] [--path]`, `update`, `uninstall [--purge] [--keep-binary] [--dry-run] [--yes]`.
- Hidden `run` child is internal to background start.
- `start` spawns the child with `setsid`, stdin closed, stdout/stderr redirected to a private log, and a random startup ID.
- The child holds an exclusive `flock`; PID files record the executable path and `/proc/<pid>/stat` start time.
- Status and stop require the recorded PID identity to match; stale or reused PIDs are not signalled.
- The pairing code is delivered through a private `0600` handshake file, printed once, and the handshake is deleted. It is not written to config or logs.
- `stop` sends `SIGTERM`, waits up to 10 seconds, then `SIGKILL`. An orphaned OMP child recorded in runtime state is cleaned up.
- Logs rotate at `logMaxBytes` (default 1 MiB) on the next start.
- `update` prints verified upgrade instructions only; the installer performs upgrades.
- `uninstall` refuses while running, requires `--yes` unless `--dry-run`, keeps config unless `--purge`, and can keep the binary.

### API and mobile

- Added unauthenticated `GET /api/v1/capabilities` returning only `product`, `apiVersion`, `bridgeVersion`, `eventVersion`, and feature names.
- HTTP test asserts the response excludes token, pairing code, OMP path, and workspace fields.
- Mobile client adds `fetchCapabilities`, `assertBridgeCapabilities`, `probeBridge`, `discoverBridge`, saved non-secret URL helpers, auto-detection state, manual loopback URL fallback, and a clear-saved-URL action.
- Detection probes a saved URL first, then `127.0.0.1` and `localhost` on ports `8765`–`8767`. It never scans the LAN.
- Pairing remains explicit; the token stays in memory and only the non-secret bridge URL is persisted.

### Installer and release

- `scripts/install-bridge.sh` downloads `omp-bridge-<target>` and `.sha256` from GitHub Releases, verifies SHA-256 before replacement, installs to `~/.local/bin` by default, and never uses `sudo`.
- Root handling distinguishes PRoot from ordinary root. PRoot markers (`container=proot-distro`, `PROOT_L2S_DIR`, `PROOT_TMP_DIR`, `PROOT_LOADER`) allow a root install only with a printed warning and only under `$HOME`. Ordinary root is still refused unless `OMP_BRIDGE_ALLOW_ROOT=1`, which prints its own warning.
- Rejects native Termux/musl/unknown architectures, and refuses to replace a running bridge.
- `OMP_BRIDGE_RELEASE_BASE_URL` supports local `file://` installer tests.
- `.github/workflows/bridge-release.yml` validates the requested tag first (`^v<semver>$`), checks out that exact tag, requires a detached HEAD matching the tag and tag commit, verifies the workspace version, records the source commit SHA in `dist/BUILD_INFO.txt`, release notes, and the run summary, and builds both glibc targets with `--locked` plus SHA-256 files.
- Android CI runs `shellcheck`, `bash -n`, the installer tests, and `scripts/test-release-workflow.sh` assertions for exact-tag ordering and metadata.

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
- `cargo test --workspace`: **83 passed, 0 failed, 1 ignored**.
  - omp-bridge unit: 40 passed.
  - CLI lifecycle integration: 11 passed.
  - engine lifecycle integration: 11 passed.
  - HTTP/WebSocket integration: 10 passed.
  - omp-rpc unit: 11 passed.
  - harness and doc tests: 0 tests.
  - `real_omp`: 1 ignored by default.

The CLI lifecycle tests use the labeled fake OMP fixture and cover setup, config shape/permissions, background start, status JSON, second-start rejection, logs, stop, uninstall dry-run, `--yes` refusal, doctor RPC, `config --path`/`--json`, non-loopback rejection, explicit `--omp-bin` without fallback, saved-OMP disappearance without switching, explicit replacement of a saved installation, first-valid `PATH` selection, and informational `update` output.

HTTP tests additionally cover the capabilities identity document and assert it does not leak token, pairing, path, or workspace data.

### Real OMP lifecycle (host)

Fresh run with an isolated config home (this session):

```sh
export OMP_BRIDGE_HOME=/tmp/opencode/phase15-real/config
export HOME=/tmp/opencode/phase15-real/home
export PATH="/root/.local/bin:/usr/bin:/bin"
./target/debug/omp-bridge setup --workspace /tmp/opencode/phase15-real/workspace \
  --listen 127.0.0.1:8865 --model opencode-zen/beam-free
./target/debug/omp-bridge start
./target/debug/omp-bridge status
curl -fsS http://127.0.0.1:8865/api/v1/capabilities
./target/debug/omp-bridge logs --lines 5
./target/debug/omp-bridge stop
```

Key output (pairing code intentionally omitted):

```text
OMP discovery: Found
  /root/.local/bin/omp [PATH] Valid { version: "18.14.0" }
  selected: /root/.local/bin/omp (18.14.0, source PATH)
  OMP:       /root/.local/bin/omp (18.14.0)
  Protocol:  v2
  Model:     opencode-zen/beam-free
  Saved:     /tmp/opencode/phase15-real/config/config.json

OMP Bridge status: engine ready
  Listen:     127.0.0.1:8865
  OMP version:18.14.0
  Engine:     ready (ready)
  Model:      opencode-zen/beam-free
  Platform:   linux aarch64 / glibc / proot

{"product":"omp-bridge","apiVersion":"v1","bridgeVersion":"0.0.1","eventVersion":1,"features":["pairing","status","events","token-revoke"]}

2026-10-31T18:53:29Z INFO  starting bridge pid 31821 on 127.0.0.1:8865
2026-10-31T18:53:34Z INFO  pairing code delivered through the private startup handshake

Stopped bridge PID 31821.
OMP Bridge status: stopped
```

Setup here used no `--omp-bin`: real OMP `18.14.0` was found on `PATH`. A
missing model produced the explicit `OMP_MISSING_MODEL` failure before the
model override was supplied.

Permission and secret checks:

```text
700 /tmp/opencode/phase15-real/config
600 /tmp/opencode/phase15-real/config/config.json
700 /tmp/opencode/phase15-real/config/logs
600 /tmp/opencode/phase15-real/config/logs/bridge.log
600 /tmp/opencode/phase15-real/config/run/state.json
```

The runtime lock file is mode `0644`; it contains no secrets.

### Real OMP end-to-end test

Command:

```sh
OMP_BIN=/root/.local/bin/omp \
OMP_WORKSPACE=/tmp/opencode/real-omp-workspace \
cargo test -p omp-bridge --test real_omp -- --ignored --nocapture
```

Result: **PASS** in 18.01 s.

```text
real OMP ready: opencode-zen/beam-free
test real_omp_boots_negotiates_and_serves_authenticated_status ... ok
```

The test starts real OMP through `RunningBridge`, asserts engine ready, protocol v2, a real model, authenticated HTTP status, absence of `systemPrompt`/`sessionFile`, and clean shutdown. No prompt was sent.

### Installer, release-workflow, shell, and manifest checks

```sh
shellcheck scripts/*.sh
sh scripts/test-install-bridge.sh
sh scripts/test-release-workflow.sh
node scripts/validate-manifest.mjs
```

Results:

- `shellcheck`: exit 0.
- Installer tests: `installer tests passed (aarch64-unknown-linux-gnu)`. Covers target detection, successful local install, ordinary-root refusal, PRoot root install with warning, PRoot `$HOME` confinement, `PROOT_L2S_DIR` detection, explicit container override with warning, bad-checksum refusal without replacing the existing binary, and running-bridge refusal.
- Release workflow assertions: passed. Verified the resolve-tag → exact-tag checkout → verify/version/commit → build ordering plus `BUILD_INFO.txt` and summary metadata.
- Manifest: `PACK_MANIFEST.json OK: 68 files present, no prompt-pack references.`

### Frontend checks

Commands run from `apps/mobile`:

```sh
npm run format:check
npm run test
npm run check
npm run build
```

Results:

- `format:check`: PASS.
- `test`: PASS, 6 tests.
- `check`: PASS, 0 errors, 0 warnings.
- `build`: PASS.

Node tests cover URL normalization/rejection, stable error codes, capability validation, loopback-only discovery, and refusal of non-bridge listeners.

### Android CI

GitHub Actions run [`41248850886`](https://github.com/xeriomy/omp-mobile/actions/runs/41248850886) on commit `eb76441`: **PASS** in 5m50s. The workflow ran the root Rust `fmt`/`clippy`/`test` checks, installer `shellcheck` and tests, release-workflow assertions, frontend format/test/check/build, and the arm64-v8a Android debug APK build. The artifact was downloaded here but not installed on a device. The prior run `41231632979` failed on shellcheck `SC2015` in `scripts/test-release-workflow.sh` and was fixed in `eb76441`. Older recorded runs (`41225948350`, `41226578478`, `41229498072`, `41230036404`) are superseded.

Documentation-only commit `2a07d53` also passed the same workflow in run [`41250496939`](https://github.com/xeriomy/omp-mobile/actions/runs/41250496939); no code changed after `eb76441`.

### Android APK artifact (Phase 1.5 build)

Downloaded from run [`41248850886`](https://github.com/xeriomy/omp-mobile/actions/runs/41248850886):

| Field | Value |
|---|---|
| Artifact | `omp-client-android-debug-00a11463231efeceb419c6bfcff28cf9b9384658` |
| File | `app-universal-debug.apk` |
| Size | 121,156,785 bytes |
| SHA-256 | `c98a748613f113b882bd54190df4895d2a140e8cffb6e095b2d96354628dd7ac` |
| App version | `0.0.1` (`apps/mobile/src-tauri/tauri.conf.json`) |
| Commit | `eb76441` |
| ABI | `arm64-v8a` (universal debug APK) |
| Signing | Android debug certificate, not a release key |

Physical-device install, loopback detection, capabilities validation, pairing, saved URL behavior, and manual fallback are **not yet verified**; the user must install this APK.

### Bridge release and HTTPS installer

PR [#3](https://github.com/xeriomy/omp-mobile/pull/3) merged to `main` at `05e291eaabfa115f32fead2c72dd2a3a56a609b9`. Tag `v0.0.1` points at that commit.

Release [v0.0.1](https://github.com/xeriomy/omp-mobile/releases/tag/v0.0.1) was published by run [`41253077815`](https://github.com/xeriomy/omp-mobile/actions/runs/41253077815): **PASS**.

| Asset | Size | SHA-256 |
|---|---:|---|
| `omp-bridge-aarch64-unknown-linux-gnu` | 3,741,104 bytes | `c795ec19e606834259389eeeb748ff297173880fc45e7416dafd7e035527ccd0` |
| `omp-bridge-x86_64-unknown-linux-gnu` | 4,123,320 bytes | `1aa7cc7e41b56e4ebdedc218924fc8d791e0c284229abbe4f4fd6b8b76da11ca` |
| `BUILD_INFO.txt` | 73 bytes | tag/version/commit metadata |

`sha256sum -c` passed for both assets. `BUILD_INFO.txt` contains `tag=v0.0.1`, `version=0.0.1`, `commit=05e291eaabfa115f32fead2c72dd2a3a56a609b9`.

The published installer was exercised over HTTPS from `main` into a temporary PRoot `$HOME`:

```sh
HOME=/tmp/opencode/proot-v001b/home curl -fsSL https://raw.githubusercontent.com/xeriomy/omp-mobile/main/scripts/install-bridge.sh | HOME=/tmp/opencode/proot-v001b/home sh
/tmp/opencode/proot-v001b/home/.local/bin/omp-bridge --version
```

Result: `warning: detected PRoot; root is the PRoot container user, not Android device root.`, checksum `OK`, installed `omp-bridge 0.0.1`.

Lifecycle against real OMP `18.14.0` with the installed `0.0.1` binary and an isolated config home:

```sh
omp-bridge setup --workspace /tmp/opencode/proot-v001/workspace --listen 127.0.0.1:8867 --model opencode-zen/beam-free
omp-bridge start
omp-bridge status
curl -fsS http://127.0.0.1:8867/api/v1/capabilities
omp-bridge stop
```

Result: discovery selected `/root/.local/bin/omp` (18.14.0, source `PATH`), protocol v2; `status` reported `engine ready`, bridge `0.0.1`, platform `linux aarch64 / glibc / proot`; capabilities returned `bridgeVersion: "0.0.1"`; `stop` exited cleanly.

The superseded test release [v0.1.0](https://github.com/xeriomy/omp-mobile/releases/tag/v0.1.0) remains published. It predates the version policy change and exact-tag checkout fix.

## Security review

- Listener is loopback-only; `0.0.0.0` fails before OMP starts.
- Pairing codes and tokens are never logged or placed in URLs. Only SHA-256 hashes are retained. Tokens are memory-only in the app.
- The pairing code appears only in `start` output; config and logs contain no copy.
- Config, state, PID, and handshake files are private; directories are `0700`; writes are atomic.
- Status and stop verify executable path plus process start time before signalling; stale state is removed.
- Functional HTTP endpoints require bearer auth. WebSocket rejects unauth/garbage/late clients and sends no event first.
- Host must be `127.0.0.1`, `localhost`, or `[::1]` with the configured port. Non-empty Origin must exactly match the allowlist; CORS is never wildcard.
- Capabilities is unauthenticated by design but exposes only product/version/feature metadata.
- Discovery is bounded and never uses a shell; OMP arguments are passed as an explicit array.
- Installer verifies SHA-256, never uses `sudo`, rejects root by default, and refuses to replace a running bridge.
- `get_state` is projected to a safe summary; prompts, session files, provider URLs, and credentials are excluded.
- Child stdout/stderr are drained independently; stderr is bounded and redacted.
- No provider credentials, prompts, or tokens were committed.

## Known issues and blockers

- **Phase 1.5 UI not device-verified.** The auto-detection screen, manual fallback, saved-URL behavior, and capabilities validation pass code tests only. The previous Phase 1 APK evidence predates this UI.
- **No working `adb`.** On-device install, `adb logcat`, restart, lifecycle, and keyboard behavior cannot be captured here.
- **Superseded test release.** `v0.1.0` remains published as an installer-test artifact; `v0.0.1` is the official Phase 1.5 release.
- **Native Termux unsupported.** Release binaries are glibc-only; the installer rejects Termux/bionic, musl, and unknown architectures. Termux support needs a real build and runtime test.
- **No OpenAPI contract package.** The v1 subset is documented in `Specs/BRIDGE_API_SPEC.md`; `packages/contracts` and generated-type drift checks do not exist.
- **No prompt/control API.** Chat, abort, sessions, model selection, files, Git, and extension UI remain future phases.
- **No Android secure storage.** The token is memory-only, so app restart requires pairing again.
- **No automatic WebSocket retry.** The UI offers manual reconnect and shows a stale/paused state.
- **OMP raw events are not normalized.** `omp.event` carries `rawType` and the raw frame; stable typed UI events are future work.

## Next best action

Install APK `omp-client-android-debug-00a11463231efeceb419c6bfcff28cf9b9384658` (SHA-256 `c98a748613f113b882bd54190df4895d2a140e8cffb6e095b2d96354628dd7ac`) on the Android 15 device against the `v0.0.1` bridge. Verify loopback discovery, capabilities identity, pairing, saved URL, manual URL fallback, clear-saved-URL, and rejection of arbitrary listeners. Record device evidence separately from host tests and CI. Do not start the Chat UI before that evidence exists.
