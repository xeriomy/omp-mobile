# Tools, Skills, LSP, Debugger, and Validation Matrix

The coding agent must discover which tools are actually available in its environment. This document describes the intended workflow, not a claim that any particular tool is installed.

## Research and discovery tools

- Use repository search and Git history to understand current patterns before edits.
- Use official OMP docs/source for RPC schemas and CLI flags.
- Use official Tauri 2 docs for Android dev/build/security/capabilities.
- Check the selected crate's docs/release notes before relying on behavior.
- Search installed coding skills for Rust, Android/Termux, Svelte/TypeScript, security review, test design and debugging; read applicable skill files.
- If a GitHub connector/repository tool exists, use it for repository operations rather than duplicating state through generic web browsing.

## Code intelligence

| Layer | Preferred tooling | Minimum fallback |
|---|---|---|
| Rust bridge | rust-analyzer, rustfmt, clippy | `cargo check`, `cargo test`, `cargo fmt` |
| Svelte/TypeScript | Svelte language server, TypeScript language service, ESLint | package scripts, `tsc --noEmit`, build |
| API schemas | OpenAPI validator and generated-type drift check | schema parse + contract tests |
| Tauri | Tauri CLI, Android Studio/Gradle where needed | exact CLI build plus Gradle logs |
| Android | `adb`, `logcat`, Chrome `chrome://inspect` for WebView | device UI + captured build/runtime logs |
| Process/RPC | integration harness, JSONL fixtures, structured tracing | manual protocol capture with sensitive content redacted |
| Git | `git diff`, status, log | do not skip source diff review |

## Recommended Rust checks

Run from the Rust workspace root, adjusting paths to the actual repository:

```sh
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

If clippy fails on pre-existing warnings, isolate the baseline, avoid hiding new warnings, and document the exact issue. Do not put secrets or prompts in test fixtures.

## Recommended frontend checks

Use the lockfile's package manager and scripts discovered in `package.json`:

```sh
pnpm install --frozen-lockfile
pnpm lint
pnpm check
pnpm test
pnpm build
```

The exact script names can differ. Inspect `package.json` first; do not run imaginary scripts and report success. Do not regenerate/remove the lockfile casually.

## Tauri / Android checks

- Confirm the documented requirements for the installed Tauri CLI version.
- Run a debug build at the start of the project, not only at release time.
- For a device build, use the project script or documented command equivalent to `tauri android dev` / `tauri android build --apk`.
- Verify on a real device: app opens; loopback request works; WebSocket authenticates; keyboard and safe areas work; background/foreground recovery; bridge restart/reconnect; logs show no leaked credentials.
- Inspect Android logs using `adb logcat` and inspect the WebView with `chrome://inspect` where enabled.
- Keep emulator limitations distinct from physical-device results.

## Security/test checklist for every release candidate

- Auth required for functional HTTP APIs and WebSocket event subscriptions.
- Pairing expiration/replay/revocation.
- Loopback default and explicit wider-bind safeguards.
- Origin/Host policy tested against real Tauri Android build.
- File path traversal and symlink escape blocked.
- RPC size limits, chunk limits, event queue bounds, timeouts and cancellation.
- No duplicate prompt after ambiguous disconnect.
- Process cleanup after bridge stop and app/client disconnect.
- Markdown sanitized; no unsafe HTML execution.
- Secret redaction checked in tests and log samples.
