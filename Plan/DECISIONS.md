# Architecture Decision Log

Use one entry per meaningful decision. Update status when evidence changes the decision.

## ADR-001 — Tauri 2 for the mobile shell

- **Status:** Accepted
- **Decision:** Use Tauri 2, with Svelte + TypeScript + CSS for the UI and Rust for the native shell.
- **Context:** The user selected Tauri to make a highly customized, OpenChamber-inspired interface while keeping Rust in the application stack.
- **Alternatives:** Native Kotlin UI; Dioxus; Capacitor.
- **Consequences:** Android build/toolchain integration must be tested early. UI uses web technologies inside a native shell; test touch, keyboard, WebView networking, lifecycle and inspection on a real device.

## ADR-002 — Separate Rust bridge in the host environment

- **Status:** Accepted
- **Decision:** Build `omp-bridge` as a standalone Rust service normally started from Termux, separate from the APK.
- **Context:** The service needs to spawn and manage the installed OMP process and access host workspaces/configuration.
- **Alternatives:** Embed OMP in the APK; make the frontend itself spawn shell commands; reuse OpenChamber's OpenCode server unchanged.
- **Consequences:** The user must start/install the bridge in the host environment for the first version. Do not assume the Android app can directly control another app's process.

## ADR-003 — Use OMP RPC UI mode

- **Status:** Accepted; Phase 0 runtime evidence recorded 2026-10-29
- **Decision:** Start with `omp --mode rpc-ui` and speak the OMP JSONL protocol through stdin/stdout.
- **Context:** RPC exposes state, prompts, session control, models, events, and the extension/tool UI sub-protocol. OMP 18.13.3 was launched from the target Android/PRoot environment; the ready frame, v2 negotiation, and a correlated `get_state` succeeded.
- **Alternatives:** Parse human CLI output; invent an unrelated wrapper protocol; use `--mode rpc` and omit UI features.
- **Consequences:** Correct request correlation, protocol negotiation, `rpc_chunk` handling, UI-request responses, approvals, and event lifecycle are required. Do not pass `--no-ui` unless its effects are explicitly understood and tested. OMP exits with code 1 before `ready` when no model is configured; `doctor` must report that distinctly from protocol failure.

## ADR-004 — HTTP + WebSocket between mobile UI and bridge

- **Status:** Accepted as initial design; validate on device
- **Decision:** Versioned REST endpoints for requests/state plus WebSocket for live events.
- **Context:** It separates UI from engine process management and is easy to inspect/test.
- **Alternatives:** Only WebSocket; direct OMP process access from Tauri; SSH.
- **Consequences:** Test Android WebView loopback networking, origin/CORS, cleartext behavior, authentication, reconnect and event resync before relying on it.

## ADR-005 — Loopback-only by default

- **Status:** Accepted
- **Decision:** Bind the bridge to `127.0.0.1` by default. Remote/LAN access must be explicit, authenticated, and separately reviewed.
- **Context:** The bridge controls an agent that can modify project files and invoke tools.
- **Consequences:** Do not make `0.0.0.0` a convenient default. Do not provide unsafe public access instructions.

## ADR-006 — OpenChamber as UX/architecture reference, not a dependency

- **Status:** Accepted
- **Decision:** Recreate relevant mobile information architecture and quality independently; keep OMP-specific semantics and original project identity.
- **Context:** OpenChamber's engine and server APIs are designed for OpenCode, not OMP.
- **Consequences:** Do not import OpenCode-specific APIs or assume OpenChamber backend compatibility. Do not copy branding/assets/source unless license and attribution requirements have been checked.

## ADR-007 — Contracts-first bridge API

- **Status:** Accepted, contract details pending Phase 1
- **Decision:** Maintain versioned HTTP/OpenAPI and WebSocket event schemas in `packages/contracts`.
- **Context:** Rust and TypeScript must agree on payloads and errors.
- **Consequences:** Generate frontend API types or add drift checks; keep event versioning backward compatible where feasible.

## ADR-008 — Workspace-scoped file access

- **Status:** Accepted
- **Decision:** File/Git APIs receive workspace-relative paths. The bridge resolves/canonicalizes paths and rejects traversal and symlink escapes.
- **Context:** The service runs with filesystem permissions of the Termux account.
- **Consequences:** Do not expose a general arbitrary-path file API. Explicitly configured roots define what the client can browse.

## ADR-009 — Pin OMP 18.13.3 as the Phase 1 protocol baseline

- **Status:** Accepted
- **Decision:** Develop and test the bridge against OMP `18.13.3`; accept protocol v1 frames, immediately negotiate v2 when `supportedProtocolVersions` advertises it, enforce the advertised 1 MiB physical-frame and 64 MiB reassembled-frame limits, and reassemble validated `rpc_chunk` sequences.
- **Context:** The official `omp-linux-arm64` 18.13.3 release was run in the target environment. Its ready frame advertised `protocolVersion: 1`, `supportedProtocolVersions: [1,2]`, `maxFrameBytes: 1048576`, and `maxReassembledFrameBytes: 67108864`. Negotiation returned success for id-correlated request `phase0-negotiate-1`; `get_state` returned success with the same id. No credentials or prompts were used.
- **Alternatives:** Target documentation without a pinned version; stay on v1 only; accept arbitrary chunk metadata.
- **Consequences:** The bridge needs a version/limit check in `doctor`, bounded v2 reassembly, and a v1 fallback. Re-run the live probe against a newer OMP release before widening the supported range. Prompt/extension/approval behavior still needs live tests with a configured model.

## ADR-010 — Do not build Android APKs on this aarch64 PRoot host

- **Status:** Accepted
- **Decision:** Build the Tauri Android app on an x86_64 Linux host or CI runner with the official JDK, Android SDK/build-tools, NDK, Rust Android targets, and Tauri 2 CLI. Do not treat the current aarch64 PRoot environment as a supported APK build environment.
- **Context:** The current host is an Android 15 aarch64 device running Ubuntu 26.04 under PRoot. It has no JDK, Android SDK/NDK, Gradle, `adb`, or Tauri CLI. Google's `build-tools_r37_linux.zip` ships ELF64 x86-64 `aapt2` and `zipalign`; no `qemu-x86_64` or `box64` is installed to run them.
- **Alternatives:** Substitute Termux-native build tools; run x86_64 tools under emulation; build on-device with an unofficial toolchain. None is tested or supported here.
- **Consequences:** Phase 0's Android build moved to an x86_64 CI gate and now succeeds; on-device WebView/loopback checks remain open. This does not invalidate the bridge architecture. Device installation/smoke tests still require a real device or emulator.

## ADR-011 — GitHub Actions is the Android APK build gate

- **Status:** Accepted; verified 2026-10-29
- **Decision:** Build the debug APK on GitHub Actions `ubuntu-24.04` with pinned toolchains: Node 24, Temurin JDK 17, Rust 1.91.1, Android cmdline-tools 19.0, platform `android-37.0`, Build Tools `36.0.0`, NDK `29.0.13846066`, and the `aarch64-linux-android` Rust target. Build only `arm64-v8a`.
- **Context:** The local host is aarch64 and Google's Linux Android build tools are x86_64 ELF binaries. The project needs a reproducible build path before on-device testing.
- **Alternatives:** Keep trying to run x86_64 tools under emulation on the device; use an unofficial Termux Android toolchain; rely on a developer's unrecorded local machine.
- **Consequences:** The workflow downloads and checksums cmdline-tools, verifies generated Gradle/AGP/SDK pins, runs frontend and Rust checks, builds the APK, validates package/ABI/minSdk/targetSdk/size/SHA-256, and uploads only a verified artifact. Action references are pinned to commit SHAs. CI success proves compilation and packaging, not runtime behavior. The first verified run is `40948549197`; see `Checkpoint/CURRENT.md`.

## ADR-012 — Disable CSP only for the smoke shell

- **Status:** Accepted as a temporary smoke-build exception; revisit before real data
- **Decision:** Set `app.security.csp` to `null` in `apps/mobile/src-tauri/tauri.conf.json` for the current build-smoke app.
- **Context:** SvelteKit's inline hydration scripts and a strict CSP were not runtime-verified on Android. The smoke screen has no remote content, no user data, no bridge, and no engine connection.
- **Alternatives:** Ship an untested strict CSP; add nonces/hashes without a device test; block the APK build until CSP is solved.
- **Consequences:** The debug smoke APK has no CSP protection. Before adding remote content, user data, bridge calls, or release packaging, define a CSP and verify the WebView renders with it. Do not treat this setting as a production security decision.

## Open decisions (must be resolved with evidence)

- Which HTTP/WebSocket Rust framework best fits the project's chosen toolchain and Termux build constraints.
- Which Tauri-compatible Android secure-storage plugin is maintained and suitable; do not store long-lived tokens in plain preferences.
- Whether all required session discovery operations are available through public OMP APIs/commands or require a documented, tested read-only session catalog adapter.
- Exact Tauri Android origin and permissions needed for local HTTP/WebSocket.
- How to make `tauri icon` output deterministic in CI, or whether to stop regenerating icons during the APK build.
- Which CSP SvelteKit/Tauri needs on Android before the app handles real bridge data.
- How to run repeatable on-device install, launch, screenshot, and `adb logcat` checks in this environment.
