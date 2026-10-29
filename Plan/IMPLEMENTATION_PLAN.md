# Implementation Plan

## Guiding principle
Prove the riskiest integration first: a real OMP process must launch from the target environment and complete a protocol exchange through the planned Rust bridge. Build the polished client after the contract is grounded in runtime evidence.

## Workstream A — repository and environment audit

- Determine whether the repository already contains a Tauri, Svelte, or Rust project.
- Inspect Git status, branch, existing changes, license, build scripts, package manager, Rust toolchain, target device, Android SDK/JDK, and Termux environment.
- Locate `omp`; record `omp --version`, relevant `omp --help` output, and the exact behavior of `omp --mode rpc-ui`.
- Read current OMP RPC and CLI references. Capture the ready frame and run a minimal `get_state`/response exchange.
- Confirm whether extension/tool approval dialogs are required for the chosen RPC UI mode.
- Check Tauri 2 Android requirements and test a minimal package/build early.

**Exit criteria:** a reproducible audit note with exact versions, commands, outputs, and blockers; no unsupported assumptions.

## Workstream B — RPC spike

- Create the smallest Rust child-process harness (temporary spike is acceptable) that launches OMP in an explicit working directory.
- Drain stdout and stderr independently; keep logs off stdout.
- Parse the `ready` frame, negotiate protocol v2 only if advertised and the implementation supports it, request state, parse the response, and shut down cleanly.
- Test malformed frames, process exit, timeouts, big frames/chunks, and request correlation.
- Verify one user prompt completes through the full lifecycle in a disposable workspace. Do not run a destructive or privileged prompt as a test.

**Exit criteria:** repeatable live round trip with retained, redacted logs; automated tests for parser and lifecycle logic.

## Workstream C — bridge foundation

- Create Rust workspace and `omp-bridge` binary.
- Add validated configuration for host, port, OMP executable, allowed workspace roots, logs, and token storage.
- Implement loopback-only service and minimal health/capabilities endpoints.
- Implement pairing/token flow before any functional endpoint is exposed.
- Implement runtime manager, OMP process lifecycle, RPC client, bounded event broadcaster, and state resynchronization.
- Define API/event contracts; add OpenAPI validation and integration tests.
- Add `doctor` command and actionable error codes.

**Exit criteria:** authenticated test client can create a runtime and receive real state/events; unauthorized tests fail closed.

## Workstream D — mobile shell and connection flow

- Scaffold Tauri 2 + Svelte + TypeScript using a pinned package manager/toolchain.
- Build mobile-first shell, design tokens, responsive breakpoints, empty/loading/error states, and persistent connection profile.
- Connect to a local bridge URL, validate API version, pair through explicit user action, and show health/runtime state.
- Test WebView origin, local HTTP/WebSocket, Android cleartext rules, app resume, network errors, and token persistence on a real device.

**Exit criteria:** installable debug APK connects to a real bridge and shows real bridge/OMP state.

## Workstream E — chat and OMP UI protocol

- Render message timeline using stable IDs, virtualize only if evidence shows a performance need.
- Implement streamed message updates and final message transitions.
- Distinguish prompt acknowledgement, prompt completion, session settlement, and process errors.
- Implement send, stop/abort, queue/steer behavior, model and thinking selectors based on runtime-discovered capabilities.
- Implement extension UI requests: select, confirm, input, editor, ask, cancel, notify, status, widget; mask secret input when the event requests it.
- Handle host tool/URI requests only according to documented capabilities and user consent; unsupported requests must fail explicitly.

**Exit criteria:** real session can complete prompts from the phone; tool approval/dialog paths do not silently fail or auto-approve.

## Workstream F — sessions and model controls

- Build session history/catalog after identifying supported enumeration sources in the installed OMP version.
- Use official RPC commands for new/open/switch/fork/branch/name operations where supported.
- Implement pagination for long history and recovery via `get_state`/entries after reconnect.
- Add model/thinking discovery and selection, surfacing server rejection reasons accurately.

**Exit criteria:** user can create, resume, fork/branch when supported, and rename sessions without losing state.

## Workstream G — project files and Git

- Add workspace picker limited to configured roots.
- Add file tree/listing and text-file viewer with bounded reads, encoding errors, and binary-file handling.
- Add Git status and one-file diff at a time. Do not implement destructive Git actions in the initial read-only milestone.
- Add path traversal, symlink escape, large-file, invalid UTF-8, and non-repository tests.

**Exit criteria:** app can inspect a real project safely, and Git output matches CLI ground truth.

## Workstream H — hardening and release

- Device smoke test on supported Android version(s); inspect logcat and WebView DevTools.
- Test app restart, bridge restart, OMP exit, process leaks, network drop, empty/bad config, invalid/expired/revoked token, and rate limits.
- Run Rust/TS/Svelte check suites and release build.
- Add install/update instructions for Termux and APK signing notes.
- Complete threat-model review before optional LAN/remote mode.

**Exit criteria:** defined MVP acceptance criteria are satisfied with captured evidence, known limitations, and reproducible build steps.

## Agent operating loop

For every task:
1. State intended files and acceptance criteria.
2. Read relevant spec and current checkpoint.
3. Use LSP for navigation/diagnostics and read official docs for uncertain APIs.
4. Implement the smallest slice.
5. Add/update tests before considering the task done.
6. Run formatter, linter, type checker, unit/integration tests, and build relevant to the change.
7. Inspect `git diff --check` and the full diff.
8. Update the roadmap/checkpoint with exact command results and the next best action.
