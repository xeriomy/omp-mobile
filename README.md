# OMP Client

Android-first mobile client for **OMP (oh-my-pi)**, inspired by OpenChamber's mobile client/server pattern and built around OMP's native RPC protocol.

## Current status

- **OMP RPC proof:** verified against the real `omp/18.13.3` binary. The ready frame, protocol v2 negotiation, and an id-correlated `get_state` succeeded; the child exited cleanly. See [`Checkpoint/CURRENT.md`](Checkpoint/CURRENT.md).
- **Mobile shell:** minimal Tauri 2 + Svelte + TypeScript + Vite application exists at `apps/mobile`. It shows an honest build-smoke screen and does not contact OMP or the bridge.
- **Android APK:** built by the GitHub Actions workflow on an x86_64 runner. A real APK artifact is not claimed until the workflow completes and the artifact is verified.
- **Bridge:** not implemented yet. `apps/bridge` is planned for Phase 1.
- **On-device runtime:** not tested. WebView origin, loopback HTTP/WebSocket, lifecycle, and `adb logcat` remain open.

## Product decisions

- **Mobile shell:** Tauri 2.
- **UI:** Svelte + TypeScript + CSS, designed mobile-first.
- **Bridge/server:** a separate Rust process called `omp-bridge`, intended to run in Termux or another host environment.
- **Engine:** the existing OMP executable. Do not rewrite or embed the agent engine in the APK.
- **Protocol:** spawn OMP with `--mode rpc-ui` and implement its JSONL protocol; use protocol negotiation and frame handling documented by the installed OMP version.
- **Local-first:** first target is the same Android device, with the bridge started from Termux and the Android app connecting to loopback. Remote/LAN access is later, opt-in work.
- **UX reference:** OpenChamber's mobile information architecture and interaction quality. Reimplement the experience; do not copy branding, assets, or source code without confirming their license and compatibility.

## Repository layout

```text
.
├── AGENTS.md
├── ARCHITECTURE.md
├── README.md
├── apps/
│   └── mobile/                 # Tauri 2 + Svelte + TypeScript app (current)
├── spike/
│   └── rpc-harness/            # Phase 0 Rust OMP RPC harness
├── scripts/
│   └── validate-manifest.mjs   # PACK_MANIFEST.json integrity check
├── .github/workflows/
│   └── android-debug-apk.yml   # x86_64 debug APK build
├── Plan/
├── Specs/
├── Roadmap/
├── Checkpoint/
└── docs/
```

`apps/bridge` and `packages/contracts` are planned but not present yet. Do not treat the layout sketch in [`ARCHITECTURE.md`](ARCHITECTURE.md) as a claim that they exist.

## Build the mobile shell

Requires Node.js 24 and npm 11. The Tauri CLI is installed from the lockfile.

```sh
cd apps/mobile
npm ci
npm run check
npm run build
npm run tauri -- android init --ci --skip-targets-install
npm run tauri -- android build --debug --apk --target aarch64 --ci
```

Android builds require JDK 17, Android SDK platform `android-37.0`, Build Tools `36.0.0`, NDK `29.0.13846066`, and the Rust `aarch64-linux-android` target. The generated `src-tauri/gen/` tree is intentionally untracked and recreated by CI.

The CI workflow is the supported APK build path: it installs the pinned toolchains, runs frontend and Rust checks, generates the Android project, builds the debug APK, verifies its size/hash/ABI, and uploads it as an artifact. A successful compile is not proof of on-device runtime behavior.

## Validate repository metadata

```sh
node scripts/validate-manifest.mjs
```

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
- OpenChamber mobile architecture: https://github.com/openchamber/openchamber/blob/main/packages/mobile/README.md
- OpenChamber mobile UX docs: https://github.com/openchamber/openchamber/blob/main/packages/docs/content/docs/mobile.mdx
- Tauri development: https://v2.tauri.app/develop/
- Tauri Android prerequisites: https://v2.tauri.app/start/prerequisites/#android
- Tauri Android packaging: https://v2.tauri.app/distribute/google-play/

These links are starting points. Verify current behavior and version compatibility while implementing.
