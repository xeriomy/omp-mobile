# Roadmap

Status legend: **Not started**, **Active**, **Blocked**, **Done**. Do not mark items Done without test/build/runtime evidence. Update this roadmap whenever a phase changes.

## Phase 0 — Audit and proof of life
**Status: Active — OMP round trip proven; Android build gate blocked**

- [x] Audit repository/toolchain/Git state.
- [x] Record OMP version/path and relevant help output in `Checkpoint/CURRENT.md`.
- [x] Read current OMP RPC/CLI docs and verify `rpc-ui` launch.
- [x] Run a minimal real `ready` → protocol negotiation (if advertised) → `get_state` exchange.
- [x] Verify child stdout/stderr separation and graceful shutdown.
- [ ] Build a minimal Tauri 2 Android project or confirm an existing one builds. **Blocked:** no JDK/Android SDK/NDK; current aarch64 PRoot host cannot run x86_64 `aapt2`. Requires x86_64 build host/CI.
- [ ] Verify app-to-Termux loopback HTTP + WebSocket and actual app origin on device. **Blocked:** no APK, `adb`, emulator, or device test.
- **Exit:** documented live round trip and a decision for unresolved integration risks. OMP side met; Android side open.

## Phase 1 — Rust bridge foundation
**Status: Not started**

- [ ] Rust workspace and CLI/config.
- [ ] `omp-bridge doctor` with redacted diagnostics.
- [ ] Pairing and token issue/revoke.
- [ ] Loopback-only HTTP server and versioned health/capabilities endpoints.
- [ ] OMP child-process lifecycle and JSONL RPC client.
- [ ] Runtime state snapshot and bounded WebSocket event stream.
- [ ] OpenAPI/event contracts and auth/protocol tests.
- **Exit:** authenticated test client controls a real OMP runtime.

## Phase 2 — Tauri/Svelte app shell
**Status: Not started**

- [ ] Tauri 2 + Svelte + TypeScript scaffold and pinned toolchain.
- [ ] Theme tokens, mobile navigation, phone/tablet layouts.
- [ ] Connection profile and pairing flow.
- [ ] Health, capability, workspace and runtime state screens.
- [ ] API version/unsupported-feature handling.
- **Exit:** debug APK on device connects to bridge and displays real state.

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

- [ ] Device smoke test and Android lifecycle/keyboard handling.
- [ ] Crash/restart/network loss recovery.
- [ ] Security review and required tests.
- [ ] CI for Rust, TypeScript/Svelte, contracts, and Android build where practical.
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
Provision an x86_64 Linux Android build host or CI runner (official JDK, Android SDK build-tools/platform, NDK, Rust Android targets, Tauri 2 CLI) and build a minimal debug APK. Do not start visual-polish work until the OMP runtime and app-to-bridge connection are proven.
