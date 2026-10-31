# Current Checkpoint

> Phase 1.5 bridge installer, OMP discovery, persistent configuration, background lifecycle, and mobile loopback auto-detection are implemented and tested on the host against real OMP `18.14.0`. Android device verification of the new UI, GitHub Actions release execution, and native Termux support remain unverified. Measured on 2026-10-31.

## Snapshot

- **Project:** OMP Client
- **Milestone:** Phase 1.5 — bridge install, discovery, lifecycle, and mobile auto-detection
- **Status:** Host lifecycle **PASS** with real OMP. Rust/frontend/installer automated checks **PASS**. Phase 1.5 mobile UI is code/test verified but not installed on a device. Release workflow not executed.
- **Branch:** `Rust-Bridge-and-Android-Loopback` (placeholder branch from `main`)
- **Base commit:** `92acf28` (`docs: record verified Android pairing and live bridge state`)
- **Code commit:** pending in this working tree; recorded in the follow-up checkpoint commit.
- **Pull request:** [#3](https://github.com/xeriomy/omp-mobile/pull/3), open against `main`.
- **Working tree:** Phase 1.5 changes uncommitted at the time of writing.
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

- Candidate order: `--omp-bin`, `OMP_BRIDGE_OMP_BIN`, saved `ompBin`, every `omp` on `PATH`, documented user-writable locations, manual path.
- Documented locations include `~/.local/bin`, `~/.omp/bin`, `~/bin`, `~/.local/share/omp/bin`, `$PREFIX/bin`, `/usr/local/bin`, and `/usr/bin`.
- Never walks a filesystem tree and never invokes a shell.
- Duplicate candidates are removed by canonical path; the first valid candidate wins.
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
- Rejects root unless `OMP_BRIDGE_ALLOW_ROOT=1`, rejects native Termux/musl/unknown architectures, and refuses to replace a running bridge.
- `OMP_BRIDGE_RELEASE_BASE_URL` supports local `file://` installer tests.
- `.github/workflows/bridge-release.yml` builds `aarch64-unknown-linux-gnu` and `x86_64-unknown-linux-gnu` with `--locked`, strips them, publishes SHA-256 files, and checks that a `v*` tag matches the workspace version.
- Android CI now runs `shellcheck`, `bash -n`, and the installer tests.

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
- `cargo test --workspace`: **78 passed, 0 failed, 1 ignored**.
  - omp-bridge unit: 40 passed.
  - CLI lifecycle integration: 6 passed.
  - engine lifecycle integration: 11 passed.
  - HTTP/WebSocket integration: 10 passed.
  - omp-rpc unit: 11 passed.
  - harness and doc tests: 0 tests.
  - `real_omp`: 1 ignored by default.

The CLI lifecycle tests use the labeled fake OMP fixture and cover setup, config shape/permissions, background start, status JSON, second-start rejection, logs, stop, uninstall dry-run, `--yes` refusal, doctor RPC, `config --path`/`--json`, and non-loopback rejection.

HTTP tests additionally cover the capabilities identity document and assert it does not leak token, pairing, path, or workspace data.

### Real OMP lifecycle (host)

Fresh run with an isolated config home:

```sh
export OMP_BRIDGE_HOME=/tmp/opencode/checkpoint-home
omp-bridge setup --workspace /tmp/opencode/real-omp-workspace
omp-bridge start
omp-bridge status
curl -fsS http://127.0.0.1:8765/api/v1/capabilities
omp-bridge logs --lines 6
omp-bridge stop
```

Key output (pairing code intentionally omitted):

```text
OMP discovery: Found
  /root/.local/bin/omp [PATH] Valid { version: "18.14.0" }
  selected: /root/.local/bin/omp (18.14.0, source PATH)
  OMP:       /root/.local/bin/omp (18.14.0)
  Protocol:  v2
  Model:     opencode-zen/beam-free
  Saved:     /tmp/opencode/checkpoint-home/config.json

OMP Bridge status: engine ready
  Listen:     127.0.0.1:8765
  OMP version:18.14.0
  Engine:     ready (ready)
  Model:      opencode-zen/beam-free
  Workspace:  /tmp/opencode/real-omp-workspace
  Platform:   linux aarch64 / glibc / proot

{"product":"omp-bridge","apiVersion":"v1","bridgeVersion":"0.1.0","eventVersion":1,"features":["pairing","status","events","token-revoke"]}

2026-10-31T01:17:31Z INFO  starting bridge pid 8272 on 127.0.0.1:8765
2026-10-31T01:17:31Z INFO  OMP /root/.local/bin/omp (cwd /tmp/opencode/real-omp-workspace)
2026-10-31T01:18:02Z INFO  pairing code delivered through the private startup handshake

Stopped bridge PID 8272.
OMP Bridge status: stopped
```

Permission and secret checks:

```text
700 /tmp/opencode/checkpoint-home
600 /tmp/opencode/checkpoint-home/config.json
700 /tmp/opencode/checkpoint-home/logs
600 /tmp/opencode/checkpoint-home/logs/bridge.log
600 /tmp/opencode/checkpoint-home/run/state.json
grep <pairing code>: 0 matches in config.json and bridge.log
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

### Installer, shell, and manifest checks

```sh
shellcheck scripts/*.sh
sh scripts/test-install-bridge.sh
node scripts/validate-manifest.mjs
```

Results:

- `shellcheck`: exit 0.
- Installer tests: `installer tests passed (aarch64-unknown-linux-gnu)`. Covers target detection, successful local install, root refusal, bad-checksum refusal without replacing the existing binary, and running-bridge refusal.
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
- **Release workflow not executed.** `bridge-release.yml` is unrun on GitHub Actions; no release assets exist yet.
- **Native Termux unsupported.** Release binaries are glibc-only; the installer rejects Termux/bionic, musl, and unknown architectures. Termux support needs a real build and runtime test.
- **Installer download path unproven.** Local `file://` tests pass; the GitHub Releases URL path has not been exercised.
- **No OpenAPI contract package.** The v1 subset is documented in `Specs/BRIDGE_API_SPEC.md`; `packages/contracts` and generated-type drift checks do not exist.
- **No prompt/control API.** Chat, abort, sessions, model selection, files, Git, and extension UI remain future phases.
- **No Android secure storage.** The token is memory-only, so app restart requires pairing again.
- **No automatic WebSocket retry.** The UI offers manual reconnect and shows a stale/paused state.
- **OMP raw events are not normalized.** `omp.event` carries `rawType` and the raw frame; stable typed UI events are future work.

## Next best action

Install the Phase 1.5 APK on the Android 15 device and verify auto-detection, capabilities validation, manual URL fallback, and pairing against a real background bridge. Then run the bridge release workflow for `v0.1.0`, verify the published SHA-256 assets and installer over HTTPS, and add the OpenAPI/event contract package with generated types and a drift check before starting the Phase 3 prompt path.
