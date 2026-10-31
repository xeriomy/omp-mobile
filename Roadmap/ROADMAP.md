# Roadmap

Status legend: **Not started**, **Active**, **Blocked**, **Done**. Do not mark items Done without test/build/runtime evidence. Update this roadmap whenever a phase changes.

## Phase 0 — Audit and proof of life
**Status: Done — OMP round trip, Android CI APK, and on-device bridge round trip proven**

- [x] Audit repository/toolchain/Git state.
- [x] Record OMP version/path and relevant help output in `Checkpoint/CURRENT.md`.
- [x] Read current OMP RPC/CLI docs and verify `rpc-ui` launch.
- [x] Run a minimal real `ready` → protocol negotiation (if advertised) → `get_state` exchange.
- [x] Verify child stdout/stderr separation and graceful shutdown.
- [x] Build a minimal Tauri 2 Android project or confirm an existing one builds. **Done in CI:** GitHub Actions run `40948549197` built and verified an arm64-v8a debug APK; see `Checkpoint/CURRENT.md`.
- [x] Install the CI APK on the Android 15 arm64 device and launch it. **Done:** user-provided screenshot shows the WebView smoke screen; no `adb logcat` captured.
- [x] Verify app-to-PRoot loopback HTTP + WebSocket and actual app origin on device. **Done 2026-10-30:** bridge-connected APK paired from Android 15, showed engine `Ready` with real model/protocol, and received a live `bridge.status` WebSocket event. `adb logcat` was not captured.
- **Exit:** documented live round trip and a decision for unresolved integration risks. **Met** on 2026-10-30.

## Phase 1 — Rust bridge foundation
**Status: Done — bridge implemented, tested against real OMP, and proven from the Android app; OpenAPI contracts remain open**

- [x] Rust workspace and CLI/config. **Done:** root workspace with `apps/bridge`, `crates/omp-rpc`, and the Phase 0 harness.
- [x] `omp-bridge doctor` with redacted diagnostics. **Done:** validates binary, workspace, listener, origins, limits; runs a real RPC startup in Phase 1.5.
- [x] Pairing and token issue/revoke. **Done:** single-use 10-character code, hashed 24-hour bearer tokens, revoke endpoint, rate limit, expiry tests.
- [x] Loopback-only HTTP server and versioned health/status endpoints. **Done:** non-loopback bind rejected; `/healthz`, `/api/v1/capabilities`, `/api/v1/status`, `/api/v1/pair`, `/api/v1/tokens/revoke`.
- [x] OMP child-process lifecycle and JSONL RPC client. **Done:** real ready/v2/`get_state`, request-ID correlation, bounded frames, stderr redaction, process group, clean shutdown.
- [x] Runtime state snapshot and bounded WebSocket event stream. **Done:** safe `get_state` projection, first-message WS auth, versioned envelopes, bounded broadcast, resync, fake-fixture and real-OMP tests.
- [ ] OpenAPI/event contracts and generated-type drift checks. **Partial:** exact v1 contract is documented in `Specs/BRIDGE_API_SPEC.md`; no `packages/contracts` or codegen exists.
- **Exit:** authenticated test client controls a real OMP runtime. **Partial:** real status/WS/auth/shutdown proven on host and from the Android app; prompt/control actions remain open.

## Phase 1.5 — Bridge install, discovery, and lifecycle
**Status: Done on host — installer scripts, discovery, config, background CLI, capabilities, and mobile detection implemented and tested; device/release execution open**

- [x] Persistent config with atomic private writes and schema migration. **Done:** `config_store.rs`, `0600`/`0700` permissions, no secrets persisted, tests.
- [x] Bounded OMP discovery and version policy. **Done:** explicit/env/saved/`PATH`/known locations, executable and `--version` checks, `18.x`/`>=18.13.3`, real RPC verification.
- [x] Background start/stop/restart/status/logs/doctor/config/update/uninstall CLI. **Done:** `setsid`, private handshake, `flock`, PID identity, stale recovery, log rotation, tests.
- [x] Unauthenticated non-secret capabilities endpoint. **Done:** product/API/event versions and feature names; HTTP test asserts no path/token/workspace leakage.
- [x] Release installer and workflow. **Done at script/workflow level:** SHA-256 verification, root/platform/running-bridge refusal, aarch64/x86_64 glibc targets, installer tests. GitHub Actions execution not yet observed.
- [x] Mobile loopback auto-detection and explicit pairing. **Done at code/test level:** saved URL probe, `8765`–`8767` loopback discovery, capabilities validation, manual fallback, memory-only token, Node tests. Not installed on a device.
- [x] Documentation and evidence. **Done:** lifecycle spec, API/security updates, README, ADRs, risks, checkpoint.
- **Exit:** install bridge once, run `setup`, `start`, open app, auto-detect, pair, and see real state. **Partial:** host CLI and real-OMP flow pass; device UI and release download not yet executed.

## Phase 2 — Tauri/Svelte app shell
**Status: Done — debug APK installed on device, pairs with the real bridge, and displays real state**

- [x] Tauri 2 + Svelte + TypeScript scaffold and pinned toolchain. **Done:** `apps/mobile`, verified in CI run `40948549197`.
- [x] Restrictive WebView CSP (ADR-012). **Done:** CI run `41110700795`; loopback-amended CSP rendered and connected on the Android 15 device on 2026-10-30.
- [x] Theme tokens, mobile-first layout, safe-area spacing, dark/light support. **Done at code level:** CSS tokens and responsive phone/tablet container; Svelte checks and build pass.
- [x] Connection profile and pairing flow. **Done at code level:** loopback-only URL validation, pairing, token-in-memory session, revoke/forget, reconnect, polling. Phase 1.5 adds saved URL and auto-detection.
- [x] Health and runtime state screen. **Done at code level:** real bridge/engine/status data, live WebSocket events, explicit stale/error/empty states. Workspace/capability screens are later.
- [x] API version/unsupported-feature handling. **Done:** API v1 and capabilities are validated before pairing; rejected tokens/network errors are explicit.
- **Exit:** debug APK on device connects to bridge and displays real state. **Met 2026-10-30** by user screenshot (`Connected`/`Live`, engine `Ready`, real model/protocol, live event). Phase 1.5 detection UI is not device-verified.

## Phase 3 — Chat and RPC UI
**Status: Not started**

- [ ] Message timeline and bounded rendering.
- [ ] Prompt streaming, `prompt_result`, final-state handling.
- [ ] Send, abort, queue/steer where supported.
- [ ] Extension UI request handlers and explicit user answers.
- [ ] Tool progress/results, error and disconnected states.
- [ ] Reconnect/resync and duplicate-prompt protection.
- **Exit:** safe real prompt lifecycle works from phone.

## Phase 4 — Sessions and model controls
**Status: Not started**

- [ ] Verify session catalog source for installed OMP version.
- [ ] List/open/new/switch/rename sessions where supported.
- [ ] Fork/branch where supported and tested.
- [ ] Model catalog and model selection.
- [ ] Thinking-level controls and session statistics.
- [ ] Subagent status/events and command palette where justified.
- **Exit:** session continuity and model control work against real OMP.

## Phase 5 — Files and Git inspection
**Status: Not started**

- [ ] Configured workspace picker.
- [ ] Safe file tree and file reader.
- [ ] Read-only Git status and per-file diff.
- [ ] Path traversal and symlink escape tests.
- [ ] Large/binary file handling.
- **Exit:** accurate project inspection against a real workspace.

## Phase 6 — Reliability, packaging and polish
**Status: Not started**

- [ ] Device smoke test and Android lifecycle/keyboard handling. **Partial:** pairing, status, and WebSocket verified on device; `adb logcat`, restart, lifecycle, and keyboard open.
- [ ] Crash/restart/network loss recovery.
- [ ] Security review and required tests. **Partial:** Phase 1/1.5 controls and tests pass; prompt/approval paths remain.
- [ ] CI for Rust, TypeScript/Svelte, contracts, and Android build where practical. **Partial:** Android APK, Rust checks, frontend checks, installer tests, and a bridge release workflow exist; contracts and release execution are open.
- [ ] Termux install/start/update instructions. **Partial:** glibc installer and lifecycle CLI documented; native Termux is rejected until built and tested.
- [ ] Release APK build and signing documentation.
- **Exit:** reproducible debug/release build, documented limitations and acceptance evidence.

## Phase 7 — Optional remote connections
**Status: Deferred**

- [ ] Threat model and ADR.
- [ ] Per-device pairing and revocation.
- [ ] Secure private-network/VPN/TLS story.
- [ ] Network reconnect and origin/transport tests.
- **Exit:** no public unprotected bridge; remote mode is opt-in and security reviewed.

## Immediate next action
Install the Phase 1.5 APK on the Android 15 device and verify auto-detection, capabilities validation, manual URL fallback, and pairing against a real background bridge. Then add the OpenAPI/event contract package with generated types and a drift check before starting the Phase 3 prompt path.
