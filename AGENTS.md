# AGENTS.md — Repository-wide coding-agent contract

## Mission
Build an Android-first OMP client with Tauri 2 + Svelte + TypeScript, backed by a standalone Rust `omp-bridge` that communicates with the real OMP process over its JSONL RPC protocol. Preserve native OMP capabilities; do not reimplement the coding agent.

## Required reading before editing
1. `README.md`
2. `ARCHITECTURE.md`
3. `Plan/IMPLEMENTATION_PLAN.md`
4. `Plan/DECISIONS.md`
5. Relevant documents under `Specs/`
6. `Roadmap/ROADMAP.md`
7. `Checkpoint/CURRENT.md`
8. Any more-specific `AGENTS.md` found in the target directory tree

## Work protocol
1. Inspect the repository, Git status, existing conventions, manifests, lockfiles, available scripts, and target platform before making changes.
2. Inventory available tools, installed skills, MCP integrations, LSP servers, debugger options, and project scripts. Use relevant ones; do not invent tool names or claim use without evidence.
3. Read the official documentation and current source for OMP RPC, Tauri 2, and the libraries actually selected. Verify CLI commands with `omp --help` / relevant subcommand help where possible.
4. Work in small, reviewable increments. Prefer a failing test or executable spike before architecture-heavy changes.
5. Use subagents when the environment supports them and independent work can be safely partitioned. Good roles: protocol research, Rust bridge review, frontend UX review, security/test review. Avoid two agents editing the same files concurrently. Integrate and review all subagent output.
6. After edits, inspect the diff and run relevant validation. Never describe a check as passed unless its command completed successfully.
7. Update `Checkpoint/CURRENT.md`, `Roadmap/ROADMAP.md`, and relevant specs whenever the status or an architectural decision changes.

## Architectural constraints
- Mobile app: Tauri 2, Svelte, TypeScript, CSS; design for phone first, then tablet layouts.
- Bridge: standalone Rust service in `apps/bridge` or the repository's existing equivalent; intended to run within Termux or a supported host.
- Engine: spawn the existing `omp` executable with `--mode rpc-ui` unless phase-zero experiments establish a documented, tested reason to choose otherwise.
- The RPC transport is line-delimited JSON over stdin/stdout, not JSON-RPC 2.0. Implement IDs, responses, events, errors, and protocol negotiation correctly.
- Drain stdout and stderr concurrently. stdout is protocol-only; stderr is diagnostics.
- Treat `prompt` acknowledgement and `prompt_result` as different lifecycle events. Correlate all replies by request ID, never by arrival order.
- Keep the frontend disconnected from child-process details. Only the bridge manages OMP processes.
- Version the HTTP API and WebSocket event envelopes. Document any protocol mapping.
- Keep the bridge loopback-only by default. Remote access is a later, explicit, authenticated feature.
- File paths must be workspace-relative at the API boundary and canonicalized before access. Prevent traversal and symlink escapes.
- Do not pass user-supplied strings through a shell. Use process APIs with explicit arguments.
- Never leak model keys, tokens, SSH keys, prompt content, or file contents into logs by default.
- Preserve OMP's safety/approval semantics. Do not add an automatic “approve all” mode.

## Scope control
- Phase 0 proves the real engine launch and RPC round-trip before major feature UI.
- Phase 1 is the narrow, testable bridge foundation.
- Phase 2 is connection/onboarding and real state display.
- Phase 3 is chat, streaming, stop, and tool/extension UI events.
- Sessions, model selection, files, and Git arrive in the order defined by the roadmap.
- Do not silently expand scope to terminal emulation, SSH management, cloud sync, relay infrastructure, arbitrary plugins, or remote internet exposure.

## Quality gates
Run the applicable checks and record exact outcomes:
- Rust: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings` where cleanly supportable, `cargo test --workspace`.
- Frontend: package manager scripts for format/lint/type-check/unit tests/build; use the lockfile-selected package manager.
- Contracts: validate OpenAPI and event schemas; regenerate types and verify no uncommitted drift.
- Tauri: Android debug build and a real device/emulator smoke test at an appropriate milestone.
- Security: tests for unauthorized HTTP/WS access, path traversal, token redaction, process cleanup, and unsafe bind configuration.

Do not force global `-D warnings` onto a legacy baseline without distinguishing pre-existing warnings from new ones. Prefer fixing new warnings and documenting unavoidable baseline issues.

## LSP, debugger, and inspection requirements
- Use the appropriate LSP for Rust and TypeScript/Svelte when available. Check diagnostics, symbol references, and type errors after significant edits.
- Use browser/WebView DevTools for layout, event, and network issues; on Android use `adb logcat` and Tauri's documented device-inspection workflow when available.
- Use `rust-gdb`/LLDB or the IDE debugger for hard-to-reproduce native issues when available. For async/protocol issues, first add a minimal regression test and structured, redacted tracing.
- If LSP/debugger access is unavailable, say so in the checkpoint and use compiler/tests/logs rather than pretending the tool was used.

## Documentation and decision discipline
- Record important choices in `Plan/DECISIONS.md` with context, decision, alternatives, and consequences.
- Use `Specs/` for expected behavior and acceptance criteria, not implementation diaries.
- Keep `Roadmap/ROADMAP.md` synchronized with completed, active, blocked, and next work.
- Each work session ends with a checkpoint containing commit SHA, dirty status, files changed, commands run, outcomes, known issues, and the single next best action.
- If a requested behavior conflicts with the actual OMP protocol, stop and document evidence; do not invent a substitute protocol.

## Definition of done
A feature is done only when its implementation, automated tests, manual validation instructions, error states, and documentation agree. A build alone is not proof of runtime correctness.
