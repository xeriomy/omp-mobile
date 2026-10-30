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
**Status: Active — bridge implemented, tested against real OMP 18.13.3, and proven from the Android app; OpenAPI contracts open**

- [x] Rust workspace and CLI/config. **Done:** root workspace with `apps/bridge`, `crates/omp-rpc`, and the Phase 0 harness.
- [x] `omp-bridge doctor` with redacted diagnostics. **Done:** validates binary, workspace, listener, origins, limits; runs `omp --version`.
- [x] Pairing and token issue/revoke. **Done:** single-use 10-character code, hashed 24-hour bearer tokens, revoke endpoint, rate limit, expiry tests.
- [x] Loopback-only HTTP server and versioned health/status endpoints. **Done:** non-loopback bind rejected; `/healthz`, `/api/v1/status`, `/api/v1/pair`, `/api/v1/tokens/revoke`. `capabilities` is not implemented yet.
- [x] OMP child-process lifecycle and JSONL RPC client. **Done:** real ready/v2/`get_state`, request-ID correlation, bounded frames, stderr redaction, process group, clean shutdown.
- [x] Runtime state snapshot and bounded WebSocket event stream. **Done:** safe `get_state` projection, first-message WS auth, versioned envelopes, bounded broadcast, resync, fake-fixture and real-OMP tests.
- [ ] OpenAPI/event contracts and generated-type drift checks. **Partial:** exact v1 contract is documented in `Specs/BRIDGE_API_SPEC.md`; no `packages/contracts` or codegen exists.
- **Exit:** authenticated test client controls a real OMP runtime. **Partial:** real status/WS/auth/shutdown proven on host and from the Android app; prompt/control actions remain open.

## Phase 2 — Tauri/Svelte app shell
**Status: Done — debug APK installed on device, pairs with the real bridge, and displays real state**

- [x] Tauri 2 + Svelte + TypeScript scaffold and pinned toolchain. **Done:** `apps/mobile`, verified in CI run `40948549197`.
- [x] Restrictive WebView CSP (ADR-012). **Done:** CI run `41110700795`; loopback-amended CSP rendered and connected on the Android 15 device on 2026-10-30.
- [x] Theme tokens, mobile-first layout, safe-area spacing, dark/light support. **Done at code level:** CSS tokens and responsive phone/tablet container; Svelte checks and build pass.
- [x] Connection profile and pairing flow. **Done at code level:** loopback-only URL validation, pairing, token-in-memory session, revoke/forget, reconnect, polling.
- [x] Health and runtime state screen. **Done at code level:** real bridge/engine/status data, live WebSocket events, explicit stale/error/empty states. Workspace/capability screens are later.
- [x] API version/unsupported-feature handling. **Partial:** API v1 is displayed and rejected tokens/network errors are explicit; no capability negotiation because that endpoint does not exist.
- **Exit:** debug APK on device connects to bridge and displays real state. **Met 2026-10-30** by user screenshot (`Connected`/`Live`, engine `Ready`, real model/protocol, live event).

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
- [ ] Security review and required tests.
- [ ] CI for Rust, TypeScript/Svelte, contracts, and Android build where practical. **Partial:** Android APK, Rust spike, and frontend checks run in CI; contracts do not exist yet.
- [ ] Termux install/start/update instructions.
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
Implement the Phase 3 prompt path over the proven bridge transport: request-ID-correlated prompt submission, streamed OMP events, `prompt_result` completion, abort, and approval passthrough. Add the OpenAPI/event contract package with generated types and a drift check in the same or next change.
