# Bridge Lifecycle and Installation Specification (Phase 1.5)

This document defines the implemented `omp-bridge` lifecycle, discovery,
configuration, and release behavior. It is the acceptance contract for Phase
1.5. The HTTP/WebSocket contract remains in
[`BRIDGE_API_SPEC.md`](BRIDGE_API_SPEC.md).

## 1. Scope

Phase 1.5 covers:

- one-time bridge installation from GitHub Releases;
- OMP discovery and compatibility checks;
- persistent bridge configuration;
- background start/stop/restart/status/logs;
- safe update and uninstall behavior;
- mobile loopback auto-detection and explicit pairing.

It does not add chat, prompts, sessions, model switching, files, Git, remote
access, or automatic OMP installation/update/configuration.

## 2. Commands

| Command | Behavior |
|---|---|
| `omp-bridge setup` | Explicitly discover OMP, run a real RPC startup check, and save configuration. Optional: `start` performs the same steps on first run. |
| `omp-bridge start` | Load the saved configuration, or on first run initialize it with the `setup` pipeline (discover, verify, workspace defaults to the current directory, atomic save), then start detached in the background. A stored-but-invalid configuration is always reported, never rewritten. |
| `omp-bridge start --foreground` | Same initialization behavior, then stay attached; `Ctrl+C` stops the bridge and its OMP child. |
| `omp-bridge stop` | Stop the recorded bridge process and clean up an orphaned OMP child. |
| `omp-bridge restart` | Stop, then start in the background. |
| `omp-bridge status [--json]` | Report phase, PID, listener, engine, OMP, model, workspace, log path, and platform. |
| `omp-bridge logs [--lines N] [--follow]` | Read the bridge log, optionally follow it. |
| `omp-bridge doctor` | Check platform, paths, discovery, saved config, and a real RPC startup. |
| `omp-bridge config [--json] [--path]` | Show effective saved configuration and file locations. |
| `omp-bridge update` | Print the verified upgrade instructions; it does not self-update. |
| `omp-bridge uninstall [--purge] [--keep-binary] [--dry-run] [--yes]` | Remove runtime/log files and the binary. Config is kept unless `--purge`. `--yes` is required unless `--dry-run`. |

The hidden `run` subcommand is internal to background start. It is not a public
interface and must not be scripted.

Legacy compatibility: running `omp-bridge` with no subcommand still starts the
bridge in the foreground using saved configuration or explicit flags.

## 3. Configuration

### 3.1 Locations

| Purpose | Default | Override |
|---|---|---|
| Config home | `$XDG_CONFIG_HOME/omp-bridge` or `$HOME/.config/omp-bridge` | `OMP_BRIDGE_HOME` |
| Config file | `<config-home>/config.json` | same as config home |
| Runtime | `$XDG_RUNTIME_DIR/omp-bridge`, else `<config-home>/run` | `OMP_BRIDGE_RUNTIME_DIR` |
| Logs | `<config-home>/logs/bridge.log` | `OMP_BRIDGE_LOG_DIR` |

Directories are created with mode `0700` where supported. The config, PID,
runtime-state, and startup-handshake files use mode `0600`. Writes are atomic
through a temporary file plus rename.

### 3.2 Precedence

For each setting:

1. explicit command-line flag;
2. `OMP_BRIDGE_*` environment variable;
3. saved `config.json`;
4. built-in default.

Clap resolves a flag over its bound environment variable. `setup` saves the
resolved values; `start`, `restart`, `doctor`, and the foreground path apply
temporary overrides without rewriting `config.json`. Background start forwards
overrides to the hidden child so parent and child use the same effective
configuration.

### 3.3 Persisted fields

`config.json` stores the OMP path and last verified version, optional model,
workspace, loopback listener, allowed origins, timeouts, limits, and log size.
It never stores bearer tokens, pairing codes, provider keys, prompts, session
content, or file content. A future schema version is rejected; version `0` is
migrated to version `1` by filling missing defaults.

## 4. OMP discovery and compatibility

Candidate priority:

1. `--omp-bin`;
2. `OMP_BRIDGE_OMP_BIN`;
3. saved `ompBin`;
4. every `omp` on `PATH`;
5. documented user-writable locations (`~/.local/bin`, `~/.omp/bin`,
   `~/bin`, `~/.local/share/omp/bin`, `$PREFIX/bin`, `/usr/local/bin`,
   `/usr/bin`).

Discovery never walks a filesystem tree and never invokes a shell. Duplicate
candidates are removed by canonical path.

Selection is deterministic:

- an explicit `--omp-bin` or `OMP_BRIDGE_OMP_BIN` must be usable; it never falls
  back to `PATH` or to a saved installation;
- a valid saved `ompBin` is kept and is never silently replaced;
- if the saved executable disappears, `setup` fails with `OMP_SAVED_UNAVAILABLE`
  and keeps the saved config until the user runs `setup --omp-bin`;
- with neither an override nor saved config, the first valid `PATH` or
  documented-location candidate wins. The selection is not alphabetical.

`setup` and `doctor` print the selected path, version, and source so the choice
is visible.

A candidate is usable only when:

- it is a regular executable file;
- `omp --version` completes within 10 seconds;
- the parsed version is OMP `18.x` at or above `18.13.3`;
- `setup` or `doctor` can start `omp --mode rpc-ui`, receive the real `ready`
  frame, negotiate a supported protocol, and complete `get_state`.

A version outside the policy is reported, not silently accepted. `18.14.0` is
verified above the pinned `18.13.3` baseline. Other majors require a bridge
compatibility update and new evidence.

`doctor` distinguishes missing executable, non-executable file, version
failure, unsupported version, missing runtime dependency, timeout, and RPC
startup failure. It prints no provider keys, prompts, or file contents.

## 5. Background lifecycle

`start` performs this sequence:

1. Load and validate saved configuration plus overrides.
2. Refuse to start when a recorded bridge process is still alive.
3. Prepare a private log file and a random startup ID.
4. Spawn `omp-bridge run` with `setsid`, stdin closed, stdout/stderr redirected
   to the log, and the effective configuration.
5. Wait for a private `0600` startup handshake or child exit.
6. Verify handshake schema, startup ID, bridge PID, loopback listener, engine
   readiness, and pairing code presence.
7. Print the loopback API/WebSocket URLs and the single-use pairing code.
8. Delete the handshake file. The pairing code is not written to config or logs.

The background child holds an exclusive `flock` on the runtime lock. It records
a PID file containing the PID, executable path, and `/proc/<pid>/stat` start
time. Status and stop require the recorded PID identity to match; a stale or
reused PID is not signalled.

`stop` sends `SIGTERM`, waits up to 10 seconds, then sends `SIGKILL` if needed.
The bridge closes OMP stdin and lets the engine perform its bounded graceful
shutdown. If the bridge is already gone, `stop` removes stale runtime state and
attempts to clean up the OMP child recorded in the runtime state.

`status` probes `/healthz` on the recorded loopback listener and reports
`not_configured`, `stopped`, `starting`, `running`, `engine_ready`,
`engine_failed`, `omp_exited`, or `bridge_unhealthy`.

Logs are appended with mode `0600`. When the log reaches `logMaxBytes`
(default 1 MiB), it is rotated to `bridge.log.1` at the next start. Logs contain
lifecycle diagnostics only: no pairing codes, tokens, prompts, provider keys, or
file content.

## 6. Installer and release

The supported release targets are:

- `aarch64-unknown-linux-gnu`;
- `x86_64-unknown-linux-gnu`.

Native Termux (`bionic`), musl, and unknown architectures are rejected with an
explicit message. Native Termux support requires real build and runtime evidence
before it is advertised.

The installer (`scripts/install-bridge.sh`):

- downloads `omp-bridge-<target>` and its `.sha256` from GitHub Releases;
- verifies the SHA-256 before replacing anything;
- installs to `~/.local/bin` by default or `OMP_BRIDGE_INSTALL_DIR`;
- never uses `sudo`;
- distinguishes PRoot from ordinary root. PRoot markers
  (`container=proot-distro`, `PROOT_L2S_DIR`, `PROOT_TMP_DIR`, `PROOT_LOADER`)
  allow a root install only under `$HOME`, with a printed warning and no `..`
  in the path. Ordinary root is still refused unless `OMP_BRIDGE_ALLOW_ROOT=1`,
  which prints its own warning;
- refuses to replace a bridge that reports a running phase;
- prints the installed version and the next `setup`/`start` commands.

PRoot detection is not general root authorization. It exists because
`proot-distro` reports `id -u` as `0` while the install is confined to the
container user's home.

`update` is informational only. It prints the official installer path and the
`stop` command; it never downloads, replaces, or restarts a running binary. The
installer is the only upgrade path. `uninstall` refuses while the bridge is
running, requires `--yes` unless `--dry-run`, keeps configuration by default,
and can keep the binary with `--keep-binary`.

The release workflow validates the requested tag first, checks out that exact
tag with full history, requires a detached HEAD matching the tag and tag commit,
verifies the workspace version, builds both targets with `--locked`, strips the
binaries, records the source commit SHA in `dist/BUILD_INFO.txt`, release notes,
and the run summary, publishes SHA-256 files, and attaches them to a `v*`
GitHub Release. A tag must match the workspace version.

## 7. Mobile detection and connection

On launch, the app:

1. probes a saved bridge URL if present;
2. otherwise probes `127.0.0.1` and `localhost` on ports `8765`, `8766`, `8767`;
3. accepts a listener only when `GET /api/v1/capabilities` returns
   `product: "omp-bridge"`, `apiVersion: "v1"`, `eventVersion: 1`, a bridge
   version, and the `pairing` feature;
4. never scans the LAN or accepts a non-loopback URL;
5. shows the detected bridge version and requires the single-use pairing code;
6. keeps the token in memory only and persists only the non-secret bridge URL;
7. offers a manual loopback URL fallback and a way to clear the saved URL.

Auto-detection does not pair automatically and does not approve anything.

## 8. Acceptance evidence

Automated:

- `cargo test -p omp-bridge --lib` covers configuration migration, permissions,
  discovery order/version policy, PID identity, handshake schema, and logs.
- `cargo test -p omp-bridge --test cli_lifecycle` covers setup, background
  start, status, logs, second-start rejection, stop, uninstall dry-run and
  confirmation, config JSON/path, doctor RPC, non-loopback rejection, explicit
  `--omp-bin` without fallback, saved-OMP disappearance without switching,
  explicit replacement of a saved installation, first-valid `PATH` selection,
  and informational `update` output.
- `cargo test -p omp-bridge --test http_api` covers the unauthenticated
  capabilities identity document and the Phase 1 auth/origin/event controls.
- Mobile Node tests cover capability validation, loopback-only discovery, and
  URL rejection.
- `scripts/test-install-bridge.sh` covers target detection, checksum failure,
  ordinary-root refusal, PRoot root install with warning, PRoot `$HOME`
  confinement, `PROOT_L2S_DIR` detection, explicit override warning, and refusal
  to replace a running bridge.
- `scripts/test-release-workflow.sh` asserts the exact-tag checkout ordering and
  build metadata in `.github/workflows/bridge-release.yml`.

Manual, on the host:

- `setup` discovers real OMP `18.14.0` and verifies protocol v2 plus
  `get_state`;
- `start`, `status`, `capabilities`, `logs`, and `stop` work against real OMP;
- the pairing code appears only in the start output and is absent from config
  and logs.

Not covered by this evidence: Android device install of this Phase 1.5 UI,
native Termux, GitHub Actions execution, and remote access.
