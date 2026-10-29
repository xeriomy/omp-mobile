# OMP Client — Planning Pack

A planning/specification pack for an Android-first mobile client for **OMP (oh-my-pi)**, inspired by OpenChamber's mobile-client/server pattern and tailored to OMP's native RPC protocol.

## Product decisions already made

- **Mobile shell:** Tauri 2.
- **UI:** Svelte + TypeScript + CSS, designed mobile-first.
- **Bridge/server:** a separate Rust process called `omp-bridge`, intended to run in Termux or another host environment.
- **Engine:** the existing OMP executable. Do not rewrite or embed the agent engine in the APK.
- **Protocol:** spawn OMP with `--mode rpc-ui` and implement its JSONL protocol; use protocol negotiation and frame handling documented by the installed OMP version.
- **Local-first:** first target is the same Android device, with the bridge started from Termux and the Android app connecting to loopback. Remote/LAN access is later, opt-in work.
- **UX reference:** OpenChamber's mobile information architecture and interaction quality. Reimplement the experience; do not copy branding, assets, or source code without confirming their license and compatibility.

## How to use this pack

1. Put these files in the root of a new repository, or merge them into the target repository without overwriting valuable existing project guidance.
2. Read [`AGENTS.md`](AGENTS.md) and [`ARCHITECTURE.md`](ARCHITECTURE.md).
3. Give the coding agent [`prompts/MASTER_IMPLEMENTATION_PROMPT.md`](prompts/MASTER_IMPLEMENTATION_PROMPT.md).
4. Tell the agent to execute [`prompts/PHASE_0_SPIKE_PROMPT.md`](prompts/PHASE_0_SPIKE_PROMPT.md) first. Phase 0 must prove a real OMP RPC round trip before major UI work.
5. Require evidence in [`Checkpoint/CURRENT.md`](Checkpoint/CURRENT.md) at the end of every work session or milestone.

## Suggested repository layout

```text
.
├── AGENTS.md
├── ARCHITECTURE.md
├── README.md
├── apps/
│   ├── mobile/                 # Tauri 2 + Svelte + TypeScript
│   │   ├── src/                # Frontend UI
│   │   └── src-tauri/          # Tauri/Rust native shell
│   └── bridge/                 # Standalone Rust HTTP/WebSocket service
├── packages/
│   └── contracts/              # OpenAPI + WebSocket event schemas
├── Plan/
├── Specs/
├── Roadmap/
├── Checkpoint/
├── docs/
└── prompts/
```

This is a proposed layout, not a claim that code already exists. If the target repository already has a sensible layout, audit it and adapt rather than blindly moving files.

## Non-negotiable engineering rules

- No fake API, fake RPC responses, static demo data presented as real engine output, or UI-only completion claims.
- No raw shell-string interpolation. Spawn processes with explicit argument arrays and a selected working directory.
- Do not assume undocumented OMP commands, filesystem layouts, event shapes, or CLI flags. Verify against the installed executable and current upstream documentation/source.
- Never expose an unauthenticated bridge on LAN or the public internet. Bind to loopback by default.
- Do not work around an OMP approval/permission flow by silently auto-approving tools.
- Use tests, formatter, linter, type checker, LSP diagnostics, and debugger/logcat/browser DevTools where available. If a tool is unavailable, record the limitation and use a verifiable alternative.
- Keep the checkpoint and roadmap truthful. “Implemented” means tested, not merely written.

## Useful upstream references

- OMP repository: https://github.com/can1357/oh-my-pi
- OMP RPC protocol: https://github.com/can1357/oh-my-pi/blob/main/docs/rpc.md
- OMP CLI reference: https://github.com/can1357/oh-my-pi/blob/main/docs/cli-reference.md
- OpenChamber mobile architecture: https://github.com/openchamber/openchamber/blob/main/packages/mobile/README.md
- OpenChamber mobile UX docs: https://github.com/openchamber/openchamber/blob/main/packages/docs/content/docs/mobile.mdx
- Tauri development: https://v2.tauri.app/develop/
- Tauri Android packaging: https://v2.tauri.app/distribute/google-play/

These links are starting points. The agent must verify current behavior and version compatibility while implementing.
