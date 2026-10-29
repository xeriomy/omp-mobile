# Current Checkpoint

> Phase 0 audit and real OMP RPC spike. Measured on 2026-10-29 in the target workspace. Android APK build and on-device loopback test remain blocked by the available toolchain.

## Snapshot

- **Project:** OMP Client
- **Milestone:** Phase 0 — audit and proof of life
- **Status:** Partial. OMP RPC round trip **PASS**. Tauri/Android build **BLOCKED** by missing x86_64 Android toolchain.
- **Branch:** `master`
- **Current commit:** `NONE` — repository has no commits; `git rev-parse HEAD` exits 128 (`unknown revision`).
- **Working tree:** all files untracked (`git status --short --branch` reports `## No commits yet on master`). No reset, stash, checkout, or deletion was performed.
- **Last updated:** 2026-10-29

## Environment (measured)

| Item | Value |
|---|---|
| OS / arch | Ubuntu 26.04.1 LTS (Resolute Raccoon), aarch64, PRoot distro on Android |
| Kernel | Linux 6.17.0-PRoot-Distro aarch64 |
| Android | 15, `arm64-v8a`, device model `23021RAAEG` |
| Termux presence | `/data/data/com.termux` exists; `TERMUX_VERSION` unset |
| Rust | `rustc 1.99.1 (58cf8f9c3 2026-10-13)`, `cargo 1.99.1 (5f94df478 2026-08-27)` |
| Rust components | `rustfmt 1.10.0-stable`, `clippy 0.1.99`; only `aarch64-unknown-linux-gnu` target installed |
| C toolchain | Ubuntu `gcc 15.2.0` at `/usr/bin/gcc`; Termux `cc` earlier in PATH cannot link Ubuntu Rust binaries |
| Node / npm | Node `v26.13.0`, npm `11.20.0`; pnpm, yarn, bun not on PATH |
| Java | Not installed (`java`, `javac` absent); `JAVA_HOME` unset |
| Android SDK | `ANDROID_HOME` and `ANDROID_SDK_ROOT` unset; `sdkmanager`, `adb`, `gradle` absent |
| Tauri CLI | Not installed. npm registry latest `@tauri-apps/cli` is `2.12.4`; not used for a build. |
| OMP | `omp/18.13.3`, official `omp-linux-arm64` release asset at `/tmp/opencode/omp-bin/omp-linux-arm64` |
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

## Rust harness

Location: `spike/rpc-harness` (temporary Phase 0 crate; promote proven parts into `apps/bridge` later).

It spawns OMP with explicit arguments and working directory, drains stdout and stderr on separate threads, parses JSONL, reassembles bounded `rpc_chunk` frames, negotiates v2, correlates responses by ID, requests `get_state`, closes stdin, and requires clean exit.

Unit tests cover blank lines, malformed JSON, in-order chunk reassembly, active-sequence tracking, mismatched chunk IDs, out-of-order chunks, length mismatch, oversized reassembly, out-of-order response correlation, and unknown/non-response frames.

## Tauri / Android evidence

**Not built.** Required toolchain is absent and the host architecture blocks Google's Linux Android build tools:

- no JDK, no Android SDK/NDK, no Gradle, no `adb`, no Tauri CLI;
- only the host Rust target `aarch64-unknown-linux-gnu` is installed;
- Google `build-tools_r37_linux.zip` contains ELF64 **x86-64** `aapt2` and `zipalign` (`readelf -h` reports `Advanced Micro Devices X86-64`);
- host is aarch64; no `qemu-x86_64`, `box64`, `adb`, or emulator is available.

Consequently a debug APK build, install, app-to-Termux loopback HTTP/WebSocket test, real WebView origin check, and `adb logcat` inspection were **NOT RUN**. Nothing in this checkpoint claims they passed. Architecture viability for the OMP/bridge side is unaffected; Android build verification needs an x86_64 Linux host or CI runner with the official Android toolchain.

## Validation evidence

| Check | Command / environment | Result | Evidence |
|---|---|---|---|
| Repository state | `git status --short --branch`; `git rev-parse HEAD` | No commits; all files untracked; HEAD lookup exits 128 | This checkpoint |
| OMP release checksum | `sha256sum omp-linux-arm64` | PASS, matches `SHA256SUMS.txt` | `/tmp/opencode/omp-bin/SHA256SUMS.txt` |
| OMP startup without model | `omp --mode rpc-ui < /dev/null` | FAIL (expected config error), exit 1, no stdout | `/tmp/opencode/evidence/no-model-stderr.txt` |
| OMP ready + v2 + get_state | Python probe, isolated HOME | PASS, exit 0 | `/tmp/opencode/evidence/` |
| OMP ready + v2 + get_state | `cargo run` in `spike/rpc-harness` | PASS, exit 0, empty stderr | `/tmp/opencode/evidence/rust-harness-summary.json` |
| Rust unit tests | `cargo test` | PASS, 11 passed, 0 failed | command output |
| Rust lint | `cargo clippy --all-targets -- -D warnings` | PASS, exit 0 | command output |
| Rust format | `cargo fmt --all -- --check` | PASS, exit 0 | command output |
| Tauri debug Android build | `tauri android build --debug --apk` | NOT RUN — toolchain absent | see above |
| Device loopback/WebSocket | real Tauri app on device | NOT RUN — no APK/device test | see above |

Reproduction notes:

```sh
cd spike/rpc-harness
CC=/usr/bin/gcc PATH=/usr/bin:/root/.cargo/bin:/usr/local/bin:/bin \
OMP_BIN=/tmp/opencode/omp-bin/omp-linux-arm64 \
OMP_MODEL=anthropic/claude-sonnet-4-5 \
OMP_HOME=/tmp/opencode/phase0-home \
cargo run --quiet -- /tmp/opencode/phase0-workspace
```

`CC=/usr/bin/gcc` is required in this environment because Termux's `cc` appears first on PATH and fails to link Ubuntu's Rust runtime (`-lgcc_s` not found).

## Files changed

- `Checkpoint/CURRENT.md` — replaced planning placeholder with measured Phase 0 evidence.
- `Plan/DECISIONS.md` — added verified OMP baseline and Android build-host decision; updated open decisions.
- `Plan/RISKS_AND_OPEN_QUESTIONS.md` — recorded Phase 0 status for the protocol, environment, and Android risks.
- `Roadmap/ROADMAP.md` — Phase 0 status and remaining gates.
- `Specs/OMP_RPC_SPEC.md` — recorded the verified OMP 18.13.3 baseline.
- `spike/rpc-harness/Cargo.toml`, `Cargo.lock`, `src/lib.rs`, `src/main.rs` — Phase 0 Rust harness and tests.

No secrets, provider credentials, prompts, or private workspace contents were written to the repository. `/tmp/opencode/evidence/` is disposable and not committed.

## Known issues / blockers

- Android APK build blocked on this aarch64 PRoot host: no JDK/SDK/NDK, and Google's Linux build-tools are x86_64-only.
- No emulator or `adb`; real Tauri WebView origin, cleartext HTTP, WebSocket auth, lifecycle, and logcat checks remain open.
- No provider credentials; prompt lifecycle, abort, extension UI dialogs, and approvals remain unverified at runtime.
- Session enumeration interface remains unverified. Do not infer it from undocumented files.
- The exact no-model launch exits before the ready frame; bridge `doctor` must distinguish missing model configuration from OMP launch/protocol failure.
- `PACK_MANIFEST.json` and `README.md` reference `prompts/MASTER_IMPLEMENTATION_PROMPT.md` and `prompts/PHASE_0_SPIKE_PROMPT.md`; that directory is absent from this checkout. The Phase 0 instruction was supplied externally.

## Security review

- No HTTP/WebSocket listener was created; bridge authentication/origin/path tests remain Phase 1 work.
- OMP was spawned with an explicit argument list and working directory, no shell interpolation.
- The probe used an isolated HOME, a disposable workspace, no provider credentials, and no prompts. No secrets, prompts, or workspace file contents were written to the repository.
- stdout remained protocol-only; stderr was captured separately, bounded, and redacted before summary output.
- The harness enforces advertised frame limits and rejects malformed/oversized/interleaved chunk sequences.

## Next best action

Provision an x86_64 Linux Android build host or CI runner with the official JDK, Android SDK build-tools/platform, NDK, Rust Android targets, and Tauri 2 CLI, then build a minimal debug APK; this is the only remaining Phase 0 exit.
