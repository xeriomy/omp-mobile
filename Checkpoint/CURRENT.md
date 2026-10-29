# Current Checkpoint

> Phase 0 OMP RPC proof, the first successful Android debug APK build in GitHub Actions, and an on-device install/WebView smoke render. Measured on 2026-10-29. Bridge, loopback, and `adb logcat` tests remain unrun.

## Snapshot

- **Project:** OMP Client
- **Milestone:** Phase 0 — proof of life and Android build gate
- **Status:** OMP RPC round trip **PASS**. Android debug APK CI build **PASS**. On-device install and WebView smoke render **PASS** (visual evidence). Bridge/loopback **NOT RUN**.
- **Branch:** `ci-apk-build`
- **Code/evidence commit:** `d8ca5a8` (`ci: skip docs-only runs; record on-device smoke render`)
- **Pull request:** https://github.com/xeriomy/omp-mobile/pull/1
- **Working tree:** docs-only checkpoint update in progress; generated `apps/mobile/src-tauri/gen/` is intentionally untracked.
- **Last updated:** 2026-10-29

## Environment (measured)

| Item | Value |
|---|---|
| Local host | Ubuntu 26.04.1 LTS, aarch64, PRoot distro on Android 15 (`arm64-v8a`, model `23021RAAEG`) |
| Test device | Android 15, `arm64-v8a`, model `23021RAAEG` |
| Local Rust | `rustc 1.91.1`, `cargo 1.91.1`; `aarch64-unknown-linux-gnu` host target |
| Local Node / npm | Node `v26.13.0`, npm `11.20.0` |
| Local Java | OpenJDK `17.0.20.1` |
| Local Android SDK | cmdline-tools 19.0 and Build Tools `37.0.0` are x86_64 ELF; cannot execute on this aarch64 host |
| CI runner | GitHub Actions `ubuntu-24.04`, x86_64 |
| CI Node / Java / Rust | Node `24`, Temurin JDK `17`, Rust `1.91.1` |
| CI Android | cmdline-tools `19.0`, platform `android-37.0`, Build Tools `36.0.0`, NDK `29.0.13846066` |
| OMP | `omp/18.13.3`, official `omp-linux-arm64` release asset |
| OMP checksum | SHA-256 `981f08df6c6344356b31aa696fdd810b8b5fa8c8851ba926d3940e793d47cc3a`, matches release `SHA256SUMS.txt` |

## OMP protocol evidence

### Version and flags

```text
$ omp --version
omp/18.13.3
```

`omp --help` confirms `--mode` accepts `text`, `json`, `rpc`, `rpc-ui`; it also documents `--no-ui` as making extensions headless while `rpc-ui` tool UI remains enabled. No `--no-ui` was used.

### Exact candidate command without a model

Command, cwd `/tmp/opencode/phase0-workspace` with isolated `HOME=/tmp/opencode/phase0-home`:

```sh
omp --mode rpc-ui < /dev/null
```

Result: exit code `1`, zero bytes on stdout, and this stderr:

```text
No default model selected. Use /login, set an API key environment variable, or select a local model with /model or --model.
```

This is an environment/configuration failure, not an RPC protocol failure. The isolated HOME had no provider credentials or model configuration.

### Live protocol exchange

The same launch with an explicit catalog model completed the round trip:

```sh
omp --mode rpc-ui --model anthropic/claude-sonnet-4-5
```

Exact ready frame (also at `/tmp/opencode/evidence/ready.jsonl`):

```json
{"type":"ready","protocolVersion":1,"supportedProtocolVersions":[1,2],"maxFrameBytes":1048576,"maxReassembledFrameBytes":67108864}
```

Negotiation request:

```json
{"id":"phase0-negotiate-1","type":"negotiate_protocol","protocolVersion":2}
```

Response was `success: true`, `command: "negotiate_protocol"`, same `id`. A `get_state` request with id `phase0-state-1` returned `success: true`, `command: "get_state"`, same `id`, with `isStreaming: false`, `isSettled: true`, `messageCount: 0`, and model `anthropic/claude-sonnet-4-5`. Full selected fields are in `/tmp/opencode/evidence/get_state.redacted.json`.

Shutdown: stdin closed, remaining stdout drained, child exited with code `0`; stderr was empty. The Python probe observed one post-shutdown stdout frame. The Rust harness independently reproduced the same sequence (ready → v2 → get_state → exit 0).

No real prompt was sent. This host had no provider credentials, so prompt acknowledgement versus `prompt_result`, streaming, abort, extension dialogs, and approval behavior remain **not runtime-tested**. Do not claim them from this checkpoint.

## Android debug APK CI evidence

### Run

- Workflow: `.github/workflows/android-debug-apk.yml`
- Run: https://github.com/xeriomy/omp-mobile/actions/runs/40948549197
- Trigger: `pull_request` for PR #1
- Head SHA: `287f49119bccf138b25e4e2e5482bab5a4dcbf48`
- Result: **success**, job `Build debug APK (arm64-v8a)` in 4m29s
- All steps passed: frontend checks, Rust spike checks, manifest validation, Android init, generated toolchain pins, APK build, APK verification, artifact upload.
- Final workflow version with the docs-only path filter also passed: run `40950435431`, same package/ABI/size/SDK levels, SHA-256 `98996d96d6dcdb544881c6e863d4187b7ac208c184fcb7b41a4e81b926efe07c`. Build output is not byte-identical across runs.

### Artifact

| Field | Value |
|---|---|
| CI path | `apps/mobile/src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk` |
| Size | `121146141` bytes |
| SHA-256 | `6c650a9ac8039ba57ef980c104a4184b13d3ac802fc02b9f7a6ca024e63ee342` |
| Package | `com.ompclient.mobile` |
| Version | `0.1.0` (`versionCode 1000`) |
| minSdk / targetSdk | `24` / `37` |
| compileSdk | `37` |
| ABI | `arm64-v8a` |
| Native library | `lib/arm64-v8a/libomp_client_lib.so`, `114058800` bytes |
| Debug signer | V2, `CN=Android Debug`, cert SHA-256 `abaa17c972fd220b7c16e14f9c78d2acd3aaa92b0957354d3c0e614d9a0e6302` |
| Uploaded artifact | `omp-client-android-debug-1363b30f13537a67033baa1b1468055590a11b43`, ID `12931160546`, `36927322` bytes zipped, 14-day retention |

The artifact was downloaded and re-checked locally: size and SHA-256 match the CI values. `apksigner verify --print-certs` passes and reports the Android debug certificate. `unzip -l` confirms exactly one ABI (`arm64-v8a`) and the native library. This is a debug-signed build, not a release artifact.

### On-device evidence

A user installed the CI APK on the target Android 15 device (`arm64-v8a`, model `23021RAAEG`) and supplied a screenshot of the running app on 2026-10-29. The screen shows:

- `OMP Client` and `Android build smoke test`;
- the `Bridge not connected` panel with the statement that the APK does not contact OMP, the bridge, or any network service;
- `Tauri 2`, `Svelte + TypeScript`, `Android debug`, and `v0.1.0`.

This verifies installation, process launch, WebView rendering, and the static smoke UI on a real arm64 device. It does **not** verify `adb logcat`, lifecycle/keyboard behavior, loopback networking, bridge connectivity, or any OMP interaction. No runtime logs were captured.

### What the APK is

The app is a minimal Tauri 2 + SvelteKit shell. It shows a build-smoke screen and states that the bridge is not connected. It does not contain a bridge, does not spawn OMP, and does not claim engine state.

### What was not tested

- `adb logcat`, keyboard/lifecycle behavior, rotation, or process death/recovery.
- Tauri WebView origin inspection or Chrome DevTools inspection.
- Loopback HTTP/WebSocket access from the app to a bridge.
- OMP prompt, streaming, abort, extension UI, or approval paths.
- Release signing, Play Store packaging, or AAB output.

## Validation evidence

| Check | Command / environment | Result |
|---|---|---|
| Local frontend install | `cd apps/mobile && npm ci --no-audit --no-fund` | PASS |
| Local frontend format | `npm run format:check` | PASS |
| Local frontend types/Svelte | `npm run check` | PASS, 0 errors, 0 warnings |
| Local frontend build | `npm run build` | PASS |
| Local manifest | `node scripts/validate-manifest.mjs` | PASS, 43 files present, no prompt-pack references |
| Local workflow lint | `actionlint -no-color -oneline .github/workflows/android-debug-apk.yml` | PASS, exit 0 |
| Local YAML parse | `python3 -c 'import yaml; yaml.safe_load(...)'` | PASS |
| Local Rust spike format | `cargo fmt --all -- --check` | PASS |
| Local Rust spike lint | `cargo clippy --all-targets -- -D warnings` | PASS |
| Local Rust spike tests | `cargo test` | PASS, 11 passed, 0 failed |
| Local app Rust format | `cargo fmt --manifest-path apps/mobile/src-tauri/Cargo.toml --all -- --check` | PASS |
| Local app lockfile | `cargo metadata --manifest-path apps/mobile/src-tauri/Cargo.toml --locked --no-deps` | PASS |
| Local APK build | `tauri android build --debug --apk` on aarch64 host | NOT RUN — x86_64-only Android build tools |
| CI APK build | GitHub Actions run `40948549197` | PASS |
| CI APK verification | `aapt2 dump badging`, `unzip -l`, size/hash assertions | PASS |
| CI artifact re-check | downloaded artifact, `sha256sum`, `apksigner verify` | PASS |
| Device install/launch/WebView render | user-installed CI APK on Android 15 arm64; screenshot | PASS (visual evidence only) |
| Device logs/lifecycle | `adb logcat`, keyboard, rotation, process death | NOT RUN |
| Bridge/loopback/WebSocket | app plus bridge | NOT RUN |

## Files changed in this milestone

- `.github/workflows/android-debug-apk.yml` — pinned x86_64 Android debug APK workflow; docs-only changes skip, manual `workflow_dispatch` always runs.
- `apps/mobile/` — minimal Tauri 2 + SvelteKit app, pinned frontend and Rust dependencies, icon, capabilities.
- `scripts/validate-manifest.mjs` — manifest and stale-reference validation.
- `PACK_MANIFEST.json` — removed `prompts/` entries, added the app, workflow, and validator.
- `README.md` — current status, build instructions, and evidence boundaries.
- `Plan/DECISIONS.md` — CI build and smoke-CSP decisions.
- `Plan/RISKS_AND_OPEN_QUESTIONS.md` — updated Android/WebView status.
- `Roadmap/ROADMAP.md` — Phase 0 build gate checked, on-device work remains.
- `Checkpoint/CURRENT.md` — this checkpoint.

No secrets, signing keys, provider credentials, prompts, or private workspace contents were committed.

## Known issues / blockers

- No `adb logcat`, WebView origin inspection, keyboard/rotation test, or process-death recovery test. Installation, launch, and the static smoke render are verified only by the supplied screenshot.
- No bridge implementation, so loopback HTTP/WebSocket and authentication tests cannot run yet.
- Prompt acknowledgement versus `prompt_result`, streaming, abort, extension UI, and approvals remain runtime-untested without provider credentials.
- The CI APK is a 121 MB debug/universal build with an unstripped native library. It is not a release or size-optimized artifact.
- `tauri icon` regenerates `src-tauri/icons/icon.icns` and `src-tauri/icons/ios/` during CI; the `Check repository diff` step reports these as modified/untracked. This does not fail the build but should be made deterministic or removed before the icons become a maintained asset.
- Session enumeration remains unverified. Do not infer it from undocumented files.
- The no-model OMP launch exits before the `ready` frame; bridge `doctor` must distinguish missing model configuration from OMP launch/protocol failure.

## Security review

- No HTTP/WebSocket listener was created; bridge authentication/origin/path tests remain Phase 1 work.
- OMP was spawned with an explicit argument list and working directory, no shell interpolation.
- The RPC probe used an isolated HOME, a disposable workspace, no provider credentials, and no prompts. No secrets, prompts, or workspace file contents were written to the repository.
- stdout remained protocol-only; stderr was captured separately, bounded, and redacted before summary output.
- The harness enforces advertised frame limits and rejects malformed/oversized/interleaved chunk sequences.
- The APK is debug-signed only. No release key or signing material is present in the repository or CI.
- The smoke app sets `app.security.csp` to `null` because SvelteKit inline hydration with a strict CSP was not runtime-verified. This is a smoke-build exception, not a production security decision; define and test a CSP before any real data or remote content is added.

## Next best action

Capture `adb logcat` for a clean launch and lifecycle check, then implement the Phase 1 bridge and test loopback HTTP/WebSocket from the installed app. The smoke screen render is no longer the blocker.
