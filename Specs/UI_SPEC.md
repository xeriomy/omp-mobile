# UI / UX Specification

## Design intent

Create a refined, mobile-first agent workspace inspired by OpenChamber's information architecture, with original implementation, project branding, and assets. Avoid a generic “chat app skin”: this is a coding-agent control surface with session state, tools, files, and changes.

## Layout model

### Phone
- Top app bar: connection/workspace/session, status, model menu.
- Main pane: chat timeline.
- Composer fixed above the keyboard/safe area; handles multiline input, send, stop, and queue/steer state.
- Left drawer/bottom sheet: recent sessions and workspace switching.
- Right workspace panel via bottom sheet or full-screen route: Files / Changes / subagents when needed.
- Modal dialogs for OMP extension UI requests, confirmations and secret inputs.

### Tablet / wide viewport
- Sessions navigation can remain in a persistent resizable side panel.
- Chat remains readable in the center.
- Right-side workspace panel appears only where the available width supports readable chat and panel content.
- Use responsive container/short-side breakpoints rather than hardcoding device names.

## Visual system

- Use CSS custom properties for color, typography, spacing, radii, borders, elevations, motion, and z-index.
- Support dark and light themes; dark-first is acceptable for initial mock-up but both themes should remain technically possible.
- Use a restrained palette, clear contrast, quiet borders, generous content spacing, and consistent iconography.
- Prefer subtle transitions, not distracting animated backgrounds.
- Avoid copying OpenChamber's logos, trademarks, or shipped assets. The interaction model can be studied; source/assets require license review before reuse.

## Chat timeline

- Each displayed item has a stable identity and type.
- Differentiate user, assistant, tool activity, extension UI, system notice, and errors.
- Stream updates without rebuilding the entire transcript or losing scroll position.
- Keep code blocks readable, horizontally scrollable when needed, with copy controls.
- Markdown rendering must sanitize raw HTML and avoid executing content from the model.
- Tool calls show state, duration where reliably available, summary and expandable output.
- Large outputs should be bounded/collapsible and accessible on mobile.
- New content should not yank a user away from a prior scroll position. Provide a “jump to latest” affordance when appropriate.

## Composer behavior

- Preserve draft text through transient disconnects and app backgrounding.
- Disable only actions that cannot be performed safely; explain why.
- Show the current OMP queue/steering mode and use supported behavior only.
- Separate “message accepted” from “agent completed” in statuses.
- Stop button maps to the verified OMP abort operation; no false success toast.
- File/image attachment support is feature-gated by the actual RPC schema and size constraints.

## States that every feature must design

- Initial / not configured.
- Connecting / pairing.
- Connected but no workspace/session.
- Loading.
- Empty result.
- Active / streaming.
- Waiting for user approval/input.
- Reconnecting / stale state.
- Recoverable error with retry.
- Fatal configuration/protocol error with diagnostics.
- Unsupported capability in current OMP version.

## Interaction / accessibility

- Touch targets at least roughly 44–48 CSS px where practical.
- Respect Android safe areas, status/navigation bars, IME/keyboard, back navigation and rotation.
- Keep keyboard focus in composer after sending unless the UI requires a dialog.
- Use labeled icon buttons; do not convey important state by color alone.
- Support screen readers, visible focus, text scaling, and keyboard navigation for desktop development.
- Test a physical phone, not only a desktop browser resized to mobile width.
