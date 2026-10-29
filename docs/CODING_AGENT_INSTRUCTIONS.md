# Coding Agent Instructions

This file complements root `AGENTS.md`. It is the operational procedure for an agent working on this repository.

## Before the first edit

1. Run `git status --short --branch`, `git rev-parse HEAD`, and inspect repository manifests/lockfiles.
2. Read all applicable `AGENTS.md` files, the current checkpoint, roadmap, architecture and specs relevant to the task.
3. Inventory the actual environment: OS/architecture; installed Rust, Node/package manager, Tauri CLI, Java/Android SDK, Termux availability, OMP binary/version.
4. Discover tools/skills available in the coding environment. Use relevant specialized tools (repository search, official-doc lookup, code navigation, test execution, package inspection, Android device/logcat, etc.) where available. Do not assume names or availability; report the exact limitation if missing.
5. If the task depends on a recent API or library behavior, check official current docs and the installed version. Avoid relying solely on old model knowledge.
6. Define acceptance criteria and a minimal reproduction/test before changing core protocol/runtime code.

## Skills and subagents

- Search the installed skill catalogue for relevant Rust, Tauri, Svelte/TypeScript, Android/Termux, security, testing, and Git skills; read the matching skill instructions before using them.
- Use subagents for genuinely parallel research/review (e.g., OMP protocol audit, UI/UX critique, security review). Give each a bounded task, files it may edit, evidence requested, and explicit “do not edit shared files” limits when appropriate.
- The integrating agent owns architecture, resolves conflicts, reruns tests, checks diffs, and updates the checkpoint. Subagent output is not proof until verified.
- Do not invoke tools merely for ceremony. Use tools where they add evidence or reduce risk.

## LSP/code intelligence

- Use Rust Analyzer for Rust and the project's Svelte/TypeScript language tooling when available.
- After structural edits, inspect diagnostics, go-to-definition, references, and type errors for changed symbols. Use safe rename/refactor facilities rather than blind multi-file replacement where possible.
- Do not suppress a diagnostic without understanding it. Distinguish baseline diagnostics from new regressions.
- If LSP is not installed/connected, record that fact and rely on compiler, formatter, linter, type checker, tests, and repository search. Do not claim LSP validation occurred.

## Debugging

- Reproduce the issue with the smallest useful input and add a regression test where possible.
- For bridge/RPC issues, use redacted structured tracing, explicit request IDs, separate stdout/stderr, and captured protocol fixtures. Never log secrets or full prompts by default.
- For Rust native bugs, use the available IDE debugger, `rust-gdb`, or LLDB when supported; otherwise use backtraces, structured logs and failing tests.
- For UI issues, use browser DevTools during desktop development and Android WebView remote inspection on device where available.
- For device/build/lifecycle failures, capture relevant `adb logcat` output and exact build command. Keep logs minimal and scrub credentials/private content.
- A passing build does not prove correct runtime behavior.

## Dependency/tool decisions

- Inspect and respect the existing lockfile. Prefer current maintained dependencies compatible with Tauri 2 Android and the selected Rust toolchain.
- Before adding a dependency, explain the need, license/maintenance/Android compatibility, and test impact. Use the package manager's normal lockfile update process.
- Do not change package managers or perform blanket upgrades as unrelated cleanup.
- Avoid custom crypto. Use established vetted primitives and platform secure storage where applicable.

## Implementation style

- Keep functions and modules cohesive; avoid speculative frameworks or over-generalized abstractions.
- Use typed DTOs/enums for bridge actions/events and validate JSON at the boundaries.
- Keep frontend state transitions explicit. Distinguish optimistic UI updates from authoritative bridge/OMP state.
- Add cancellation/timeouts to async operations and bounded queues for streaming/backpressure.
- Use comments for non-obvious invariants and protocol quirks, not narration of each line.
- Preserve accessibility, mobile keyboard behavior, scroll anchoring, and all empty/loading/error states.

## Mandatory validation cycle

For each slice:
1. Run formatting.
2. Run LSP diagnostics if available.
3. Run lint and type checks for touched layers.
4. Run unit tests and relevant integration tests.
5. Build the touched target.
6. For RPC or Android networking changes, perform a real integration/device check when the environment allows it.
7. Run `git diff --check`; inspect the full diff and search for secrets, unsafe binds, wildcard CORS, shell interpolation, TODOs masking missing behavior, and unbounded buffers.
8. Update specs/roadmap/checkpoint truthfully.

## When blocked

Do not silently replace a missing real dependency with mocks and call the feature done. State the blocker, add the narrowest testable seam/mock only for unit testing, mark integration as BLOCKED, and identify the next action required to prove the real path.
