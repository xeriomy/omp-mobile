# Product Specification

## 1. Purpose

Provide a polished Android client for controlling the real OMP coding agent in a host environment such as Termux. The UX should feel deliberate and mobile-first, using OpenChamber's mobile layout as a reference while implementing OMP-specific behavior.

## 2. Target user and primary workflow

The target user already has OMP installed and configured in Termux or another host. They want to open a mobile app, connect to the bridge, choose a project/session, ask OMP to work, follow progress, inspect results, and continue the task without using a terminal for routine operations.

Primary flow:
1. Install/start `omp-bridge` in the host environment.
2. Open OMP Client and connect/pair with the local bridge.
3. Select an allowed workspace.
4. Create or resume a session.
5. Send a prompt and observe live agent events/tool activity.
6. Answer any OMP UI/approval dialogs explicitly.
7. Stop, queue, or steer follow-up work.
8. Inspect session history, files, and Git changes.

## 3. MVP features

### P0 — Required for first useful release
- Connection onboarding and health state.
- Clear bridge/OMP errors and diagnostics.
- Workspace selection constrained to configured roots.
- Real OMP runtime start/stop and state snapshot.
- Chat timeline with streaming updates and final results.
- Send prompt and abort current work.
- Extension/tool UI requests exposed by `rpc-ui` are rendered and answered correctly.
- Reconnect and state resynchronization.
- Secure token pairing and local token revocation/forget flow.

### P1 — Core agent controls
- New/open/switch sessions based on verified OMP capabilities.
- Session history, titles, fork/branch when supported.
- Runtime-discovered model list and model switching.
- Thinking-level selection where supported by selected model.
- Queue/follow-up/steer behavior where supported by OMP.
- Session stats and available slash commands.
- Subagent status and events.

### P2 — Project inspection
- Workspace file tree and bounded text viewer.
- Git status and read-only diffs.
- Search within a workspace if performance and safety tests pass.
- Copy/share output and files using supported Android integrations.

### Later, not MVP
- Git writes/commit/checkout/push.
- Arbitrary terminal/PTY management.
- Multi-user authorization and cloud accounts.
- Public internet exposure, custom relay infrastructure, or a general SSH manager.
- Syncing provider credentials to the mobile application.
- Automated approval of agent tools.

## 4. Non-functional requirements

- **Correctness:** events must be mapped to stable state and not be lost silently. A reconnect must resynchronize from OMP.
- **Security:** loopback by default, authenticated API and WebSocket, scoped filesystem access, secret redaction, no auto-approval.
- **Usability:** touch targets, safe-area handling, mobile keyboard support, scroll anchoring, loading/empty/error states, readable code blocks, no horizontal overflow in ordinary layouts.
- **Responsiveness:** UI remains interactive during streaming and large tool output; long histories/files are bounded or paginated.
- **Reliability:** process termination and bridge restart are handled; no orphaned OMP process after managed shutdown.
- **Maintainability:** Rust and TypeScript are separated through documented contracts; tests cover protocol parsing and lifecycle.
- **Accessibility:** visible focus states, sensible screen-reader labels, dynamic text sizing where practicable, accessible dialog controls.

## 5. UX principles

- Use a clean, dark-first optional theme with restrained color, clear typography, and strong content hierarchy. Theme must be configurable; don't hardcode dark only.
- Phone layout: a sessions drawer, a focused chat/workspace, and a workspace panel/bottom sheet for Files and Changes. Do not squeeze three columns onto a phone.
- Tablet layout: let the sessions list persist as a side rail and display a right workspace panel only when width allows; use responsive size classes rather than device-model checks.
- Use bottom sheets for mobile selection/confirmation when appropriate, anchored popovers on wide layouts.
- Tool activity should be visible but not overwhelm the conversation. Show status and expandable details.
- Keep text entry usable while the agent works. Preserve draft content through connection retries.
- Do not automatically scroll the user away from a historical position when new content arrives.
- All actions have explicit pending/success/error states. No fake optimistic success for engine operations.

## 6. Core screens

1. **Connect / Instances:** saved bridge profiles, health, pair/connect, error diagnostics.
2. **Workspace selector:** allowed roots only, recent projects, current path.
3. **Sessions drawer:** recent sessions and new session, only once supported listing works.
4. **Chat:** user/assistant timeline, streaming content, tool activity, controls, queued work, dialog UI.
5. **Model/settings menu:** available providers/models/thinking levels from OMP, not a hard-coded catalog.
6. **Files:** safe tree/list and text viewer.
7. **Changes:** Git status and one-file-at-a-time diff.
8. **Settings:** connection, local security, appearance, diagnostics, about/version.

## 7. Success criteria

- A new install can connect to a running bridge with explicit pairing.
- The app can start a real OMP runtime in an allowed workspace and display real state.
- A prompt from the app leads to visible streaming and a terminal completion/error state.
- Abort works and is reflected by real OMP state/events.
- Required interactive RPC UI requests can be completed without automatic approval.
- App and bridge restart recover through state resync without duplicate prompts.
- Unauthorized requests and unsafe file paths are rejected by tests.
- Android debug APK can be installed and smoke-tested on a real device.
