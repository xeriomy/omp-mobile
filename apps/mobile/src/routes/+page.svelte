<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import {
    BridgeError,
    BridgeSocket,
    clearSavedBridgeUrl,
    discoverBridge,
    fetchStatus,
    loadSavedBridgeUrl,
    normalizeBridgeUrl,
    pair,
    probeBridge,
    revokeToken,
    saveBridgeUrl,
    type BridgeCapabilities,
    type BridgeEvent,
    type EnginePhase,
    type StatusResponse,
  } from '$lib/bridge';

  type Phase = 'idle' | 'connecting' | 'connected' | 'error';
  type SocketState = 'closed' | 'connecting' | 'live' | 'paused';
  type Detection = 'idle' | 'searching' | 'found' | 'not-found';

  const MAX_EVENTS = 20;

  let bridgeUrl = $state('http://127.0.0.1:8765');
  let pairingCode = $state('');
  let baseUrl = $state('');
  let eventUrl = $state('');
  let token = $state('');
  let tokenId = $state('');
  let tokenExpiresAt = $state(0);
  let phase = $state<Phase>('idle');
  let socketState = $state<SocketState>('closed');
  let detection = $state<Detection>('idle');
  let capabilities = $state<BridgeCapabilities | null>(null);
  let status = $state<StatusResponse | null>(null);
  let events = $state<BridgeEvent[]>([]);
  let message = $state('');
  let busy = $state(false);
  let detecting = $state(false);
  let pollTimer: ReturnType<typeof setInterval> | undefined;
  let socket: BridgeSocket | null = null;

  const hasSession = $derived(token.length > 0);
  const engine = $derived(status?.engine);
  const summary = $derived(engine?.summary);

  function describeError(error: unknown): string {
    if (error instanceof BridgeError) {
      return error.message;
    }
    if (error instanceof Error && error.message) {
      return error.message;
    }
    return 'Unexpected bridge failure.';
  }

  function stopPolling(): void {
    if (pollTimer !== undefined) {
      clearInterval(pollTimer);
      pollTimer = undefined;
    }
  }

  async function refreshStatus(): Promise<void> {
    if (!token || !baseUrl) {
      return;
    }
    try {
      status = await fetchStatus(baseUrl, token);
    } catch (error) {
      if (error instanceof BridgeError && error.status === 401) {
        dropSession('The bridge no longer accepts this token. Pair again with a new code.');
        return;
      }
      message = describeError(error);
    }
  }

  function startPolling(): void {
    stopPolling();
    pollTimer = setInterval(() => {
      void refreshStatus();
    }, 5000);
  }

  function handleEvent(event: BridgeEvent): void {
    events = [event, ...events].slice(0, MAX_EVENTS);
    if (event.type === 'bridge.status') {
      status = event.payload as StatusResponse;
      return;
    }
    if (event.type === 'bridge.resync') {
      const payload = event.payload as { status?: StatusResponse };
      if (payload.status) {
        status = payload.status;
      }
      return;
    }
    if (
      event.type === 'engine.ready' ||
      event.type === 'engine.failed' ||
      event.type === 'process.exited'
    ) {
      void refreshStatus();
    }
  }

  function openSocket(activeToken: string): void {
    socket?.close();
    socketState = 'connecting';
    socket = new BridgeSocket(eventUrl, activeToken, {
      onEvent: handleEvent,
      onState: (state) => {
        socketState = state;
      },
      onUnauthorized: () => {
        dropSession('The bridge rejected this token. Pair again with a new code.');
      },
      onError: (error) => {
        message = error;
      },
    });
    socket.connect();
  }

  async function detect(): Promise<void> {
    if (detecting || hasSession) {
      return;
    }
    detecting = true;
    detection = 'searching';
    message = '';
    try {
      const saved = loadSavedBridgeUrl();
      let found: Awaited<ReturnType<typeof probeBridge>> | null = null;
      if (saved) {
        try {
          found = await probeBridge(saved);
        } catch {
          // Saved URL no longer answers; fall through to loopback discovery.
        }
      }
      found ??= await discoverBridge();
      bridgeUrl = found.baseUrl;
      baseUrl = found.baseUrl;
      eventUrl = found.eventUrl;
      capabilities = found.capabilities;
      detection = 'found';
      message = `Bridge ${found.capabilities.bridgeVersion} detected. Enter the pairing code printed by omp-bridge.`;
    } catch (error) {
      detection = 'not-found';
      capabilities = null;
      message = describeError(error);
    } finally {
      detecting = false;
    }
  }

  async function connect(): Promise<void> {
    if (busy) {
      return;
    }
    busy = true;
    message = '';
    phase = 'connecting';
    try {
      const endpoints = normalizeBridgeUrl(bridgeUrl);
      baseUrl = endpoints.baseUrl;
      eventUrl = endpoints.eventUrl;
      const probed = await probeBridge(baseUrl);
      capabilities = probed.capabilities;
      detection = 'found';

      let activeToken = token;
      if (!activeToken) {
        if (!pairingCode.trim()) {
          throw new BridgeError(
            'Enter the single-use pairing code printed by omp-bridge.',
            'CODE_REQUIRED',
          );
        }
        const issued = await pair(baseUrl, pairingCode);
        activeToken = issued.token;
        token = issued.token;
        tokenId = issued.tokenId;
        tokenExpiresAt = issued.expiresAt;
        pairingCode = '';
      }

      status = await fetchStatus(baseUrl, activeToken);
      saveBridgeUrl(baseUrl);
      phase = 'connected';
      openSocket(activeToken);
      startPolling();
    } catch (error) {
      message = describeError(error);
      phase = token ? 'error' : 'idle';
      socketState = 'paused';
    } finally {
      busy = false;
    }
  }

  function dropSession(reason: string): void {
    socket?.close();
    socket = null;
    socketState = 'closed';
    stopPolling();
    token = '';
    tokenId = '';
    tokenExpiresAt = 0;
    status = null;
    events = [];
    phase = 'idle';
    message = reason;
  }

  async function forget(): Promise<void> {
    const activeToken = token;
    const activeBase = baseUrl;
    dropSession('Disconnected. Pair again to reconnect.');
    if (activeToken && activeBase) {
      try {
        await revokeToken(activeBase, activeToken);
      } catch {
        // Revocation is best effort; a bridge restart also drops the token.
      }
    }
  }

  function pairAgain(): void {
    dropSession('Enter a new pairing code from the running bridge.');
  }

  function forgetSavedUrl(): void {
    clearSavedBridgeUrl();
    detection = 'idle';
    message = 'Saved bridge URL cleared. Enter a URL manually or detect again.';
  }

  function engineLabel(state: EnginePhase | undefined): string {
    switch (state) {
      case 'ready':
        return 'Ready';
      case 'starting':
        return 'Starting';
      case 'failed':
        return 'Failed';
      case 'exited':
        return 'Exited';
      default:
        return 'Unknown';
    }
  }

  function engineTone(state: EnginePhase | undefined): string {
    switch (state) {
      case 'ready':
        return 'ok';
      case 'starting':
        return 'warn';
      case 'failed':
      case 'exited':
        return 'bad';
      default:
        return 'muted';
    }
  }

  function detectionLabel(): string {
    switch (detection) {
      case 'searching':
        return 'Searching';
      case 'found':
        return 'Detected';
      case 'not-found':
        return 'Not found';
      default:
        return 'Not checked';
    }
  }

  function detectionTone(): string {
    switch (detection) {
      case 'found':
        return 'ok';
      case 'searching':
        return 'warn';
      case 'not-found':
        return 'bad';
      default:
        return 'muted';
    }
  }

  function socketLabel(): string {
    switch (socketState) {
      case 'live':
        return 'Live';
      case 'connecting':
        return 'Connecting';
      case 'paused':
        return 'Paused';
      default:
        return 'Closed';
    }
  }

  function modelLabel(): string {
    if (!summary?.model) {
      return 'Not reported';
    }
    return `${summary.model.provider}/${summary.model.id}`;
  }

  function valueOrDash(value: string | number | undefined): string {
    return value === undefined || value === '' ? '—' : String(value);
  }

  function eventRecord(event: BridgeEvent): Record<string, unknown> | null {
    return event.payload && typeof event.payload === 'object'
      ? (event.payload as Record<string, unknown>)
      : null;
  }

  function eventLabel(event: BridgeEvent): string {
    if (event.type === 'omp.event') {
      const rawType = eventRecord(event)?.rawType;
      return typeof rawType === 'string' ? `omp · ${rawType}` : 'omp event';
    }
    return event.type;
  }

  function eventDetail(event: BridgeEvent): string {
    const payload = eventRecord(event);
    if (!payload) {
      return '';
    }
    if (event.type === 'bridge.warning') {
      const code = typeof payload.code === 'string' ? payload.code : '';
      const text = typeof payload.message === 'string' ? payload.message : '';
      return [code, text].filter(Boolean).join(': ');
    }
    if (event.type === 'process.exited') {
      const code = payload.code;
      return code === null || code === undefined ? 'exit code unknown' : `exit code ${code}`;
    }
    if (event.type === 'engine.failed' && payload.error) {
      const error = payload.error as { code?: string; message?: string };
      return [error.code, error.message].filter(Boolean).join(': ');
    }
    return '';
  }

  function timeLabel(milliseconds: number): string {
    return new Date(milliseconds).toLocaleTimeString();
  }

  onMount(() => {
    const saved = loadSavedBridgeUrl();
    if (saved) {
      bridgeUrl = saved;
    }
    void detect();
  });

  onDestroy(() => {
    stopPolling();
    socket?.close();
  });
</script>

<svelte:head>
  <meta
    name="description"
    content="OMP Client bridge connection and live diagnostics. Displays only real bridge and OMP state."
  />
</svelte:head>

<main class="shell">
  <header class="topbar">
    <div class="brand">
      <span class="mark" aria-hidden="true">OMP</span>
      <div>
        <h1>OMP Client</h1>
        <p class="subtitle">Local bridge connection</p>
      </div>
    </div>
    <span class="pill {phase === 'connected' ? 'ok' : phase === 'connecting' ? 'warn' : 'muted'}">
      {phase === 'connected' ? 'Connected' : phase === 'connecting' ? 'Connecting' : 'Disconnected'}
    </span>
  </header>

  <div class="content">
    {#if !hasSession}
      <section class="card" aria-labelledby="connect-heading">
        <div class="section-head">
          <h2 id="connect-heading">Connect to the bridge</h2>
          <span class="pill {detectionTone()}">{detectionLabel()}</span>
        </div>
        <ol class="steps">
          <li>Install <code>omp-bridge</code> once, then run <code>omp-bridge setup</code>.</li>
          <li>
            Start it with <code>omp-bridge start</code>. The app probes loopback automatically.
          </li>
          <li>Enter the single-use pairing code printed by the bridge.</li>
        </ol>

        <div class="actions detect-actions">
          <button type="button" onclick={() => void detect()} disabled={detecting || busy}>
            {detecting ? 'Searching loopback…' : 'Detect bridge'}
          </button>
          {#if loadSavedBridgeUrl()}
            <button type="button" class="link inline" onclick={forgetSavedUrl} disabled={busy}>
              Clear saved URL
            </button>
          {/if}
        </div>

        {#if capabilities}
          <p class="note" role="status">
            Detected OMP Bridge {capabilities.bridgeVersion} · API {capabilities.apiVersion} · event version
            {capabilities.eventVersion}
          </p>
        {/if}

        <form
          onsubmit={(event) => {
            event.preventDefault();
            void connect();
          }}
        >
          <label class="field" for="bridge-url">
            <span>Bridge URL (manual fallback)</span>
            <input
              id="bridge-url"
              name="bridge-url"
              type="url"
              inputmode="url"
              autocomplete="off"
              autocapitalize="off"
              spellcheck="false"
              bind:value={bridgeUrl}
              placeholder="http://127.0.0.1:8765"
              disabled={busy}
            />
          </label>

          <label class="field" for="pairing-code">
            <span>Pairing code</span>
            <input
              id="pairing-code"
              name="pairing-code"
              type="text"
              autocomplete="off"
              autocapitalize="characters"
              spellcheck="false"
              maxlength="10"
              bind:value={pairingCode}
              placeholder="10 characters"
              disabled={busy}
            />
          </label>

          <button class="primary" type="submit" disabled={busy}>
            {busy ? 'Connecting…' : 'Pair and connect'}
          </button>
        </form>

        <p class="note">
          The pairing code is single use and expires. The session token stays in memory only; it is
          never stored on disk or placed in a URL. Only the bridge URL is remembered.
        </p>

        {#if message}
          <p class="error" role="alert">{message}</p>
        {/if}
      </section>
    {:else}
      <section class="card" aria-labelledby="connection-heading">
        <div class="section-head">
          <h2 id="connection-heading">Connection</h2>
          <span class="pill {socketState === 'live' ? 'ok' : 'warn'}">{socketLabel()}</span>
        </div>
        <dl class="facts">
          <div>
            <dt>Bridge</dt>
            <dd>{status?.listen ?? baseUrl}</dd>
          </div>
          <div>
            <dt>API</dt>
            <dd>{status?.apiVersion ?? 'v1'}</dd>
          </div>
          <div>
            <dt>Bridge version</dt>
            <dd>{status?.bridgeVersion ?? '—'}</dd>
          </div>
          <div>
            <dt>Event stream</dt>
            <dd>{status?.websocket.path ?? '/api/v1/events'}</dd>
          </div>
          <div>
            <dt>Token ID</dt>
            <dd>{tokenId || '—'}</dd>
          </div>
        </dl>
        <div class="actions">
          <button type="button" onclick={() => void refreshStatus()} disabled={busy}>Refresh</button
          >
          <button
            type="button"
            onclick={() => void connect()}
            disabled={busy || socketState === 'live'}
          >
            Reconnect
          </button>
          <button class="danger" type="button" onclick={() => void forget()} disabled={busy}>
            Forget
          </button>
        </div>
        <button class="link" type="button" onclick={pairAgain} disabled={busy}>
          Pair with a new code instead
        </button>
        {#if message}
          <p class="error" role="alert">{message}</p>
        {/if}
        {#if socketState === 'paused'}
          <p class="note warn" role="status">
            Live updates are paused. State shown below may be stale until you reconnect.
          </p>
        {/if}
      </section>

      <section class="card" aria-labelledby="engine-heading">
        <div class="section-head">
          <h2 id="engine-heading">Engine</h2>
          <span class="pill {engineTone(engine?.state)}" aria-live="polite">
            {engineLabel(engine?.state)}
          </span>
        </div>

        {#if status}
          <dl class="facts">
            <div>
              <dt>Model</dt>
              <dd>{modelLabel()}</dd>
            </div>
            <div>
              <dt>Protocol</dt>
              <dd>
                {engine?.negotiatedProtocolVersion ? `v${engine.negotiatedProtocolVersion}` : '—'}
              </dd>
            </div>
            <div>
              <dt>Session</dt>
              <dd>{summary?.sessionName ?? summary?.sessionId ?? 'No session reported'}</dd>
            </div>
            <div>
              <dt>Messages</dt>
              <dd>{valueOrDash(summary?.messageCount)}</dd>
            </div>
            <div>
              <dt>Streaming</dt>
              <dd>{summary?.isStreaming ? 'Yes' : 'No'}</dd>
            </div>
            <div>
              <dt>Settled</dt>
              <dd>{summary?.isSettled === undefined ? '—' : summary.isSettled ? 'Yes' : 'No'}</dd>
            </div>
            <div>
              <dt>Thinking</dt>
              <dd>{valueOrDash(summary?.thinkingLevel)}</dd>
            </div>
          </dl>
          {#if engine?.error}
            <p class="error" role="alert">
              <strong>{engine.error.code}</strong>: {engine.error.message}
            </p>
          {/if}
        {:else}
          <p class="loading" role="status">Loading authoritative bridge state…</p>
        {/if}

        <p class="note">
          Values come from the bridge's <code>get_state</code> response. Unreported fields stay empty;
          this screen never substitutes demo data.
        </p>
      </section>

      <section class="card" aria-labelledby="events-heading">
        <div class="section-head">
          <h2 id="events-heading">Live events</h2>
          <span class="count">{events.length}</span>
        </div>
        {#if events.length === 0}
          <p class="empty">No events received since this connection opened.</p>
        {:else}
          <ol class="events">
            {#each events as event (event.sequence)}
              <li>
                <div class="event-row">
                  <span class="seq">#{event.sequence}</span>
                  <span class="event-type">{eventLabel(event)}</span>
                  <time datetime={new Date(event.occurredAt).toISOString()}
                    >{timeLabel(event.occurredAt)}</time
                  >
                </div>
                {#if eventDetail(event)}
                  <p class="event-detail">{eventDetail(event)}</p>
                {/if}
              </li>
            {/each}
          </ol>
        {/if}
      </section>
    {/if}
  </div>

  <footer class="footer">
    <span>Bridge diagnostics only · no prompts sent</span>
    <span class="version">v0.1.0</span>
  </footer>
</main>

<style>
  :global(:root) {
    color-scheme: light dark;
    --bg: #f4f6f8;
    --surface: #ffffff;
    --surface-muted: #eef2f6;
    --text: #101828;
    --muted: #5b6779;
    --border: #d8e0e9;
    --accent: #0f766e;
    --accent-soft: #d9f3ef;
    --warning: #b45309;
    --warning-soft: #fef3c7;
    --danger: #b42318;
    --danger-soft: #fee4e2;
    --shadow: 0 18px 40px rgb(15 23 42 / 8%);
    font-family:
      Inter,
      ui-sans-serif,
      system-ui,
      -apple-system,
      'Segoe UI',
      sans-serif;
    font-size: 16px;
  }

  :global(:root[data-theme='dark']) {
    --bg: #0b1220;
    --surface: #121c2d;
    --surface-muted: #18243a;
    --text: #edf2f7;
    --muted: #9aa8bb;
    --border: #263650;
    --accent: #5eead4;
    --accent-soft: #123a38;
    --warning: #fbbf24;
    --warning-soft: #3a2a0c;
    --danger: #fda29b;
    --danger-soft: #3b1715;
    --shadow: 0 18px 40px rgb(0 0 0 / 32%);
  }

  @media (prefers-color-scheme: dark) {
    :global(:root:not([data-theme='light'])) {
      --bg: #0b1220;
      --surface: #121c2d;
      --surface-muted: #18243a;
      --text: #edf2f7;
      --muted: #9aa8bb;
      --border: #263650;
      --accent: #5eead4;
      --accent-soft: #123a38;
      --warning: #fbbf24;
      --warning-soft: #3a2a0c;
      --danger: #fda29b;
      --danger-soft: #3b1715;
      --shadow: 0 18px 40px rgb(0 0 0 / 32%);
    }
  }

  :global(*) {
    box-sizing: border-box;
  }

  :global(html) {
    background: var(--bg);
  }

  :global(body) {
    margin: 0;
    min-width: 320px;
    background: var(--bg);
    color: var(--text);
    -webkit-font-smoothing: antialiased;
    text-rendering: optimizeLegibility;
  }

  :global(code) {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 0.9em;
  }

  .shell {
    min-height: 100svh;
    padding: max(1rem, env(safe-area-inset-top)) 1rem max(1rem, env(safe-area-inset-bottom));
    display: flex;
    flex-direction: column;
    gap: 1.25rem;
    max-width: 44rem;
    margin: 0 auto;
  }

  .topbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
    padding-top: 0.5rem;
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 0.875rem;
  }

  .mark {
    display: grid;
    place-items: center;
    width: 3rem;
    height: 3rem;
    border-radius: 0.875rem;
    background: var(--accent);
    color: var(--bg);
    font-size: 0.8125rem;
    font-weight: 800;
    letter-spacing: 0.04em;
    box-shadow: var(--shadow);
  }

  h1 {
    margin: 0;
    font-size: 1.375rem;
    line-height: 1.2;
    letter-spacing: -0.02em;
  }

  .subtitle {
    margin: 0.2rem 0 0;
    color: var(--muted);
    font-size: 0.875rem;
  }

  .content {
    display: flex;
    flex-direction: column;
    gap: 1rem;
    flex: 1;
  }

  .card {
    padding: 1.125rem;
    border: 1px solid var(--border);
    border-radius: 1.125rem;
    background: var(--surface);
    box-shadow: var(--shadow);
  }

  h2 {
    margin: 0;
    font-size: 1.0625rem;
    letter-spacing: -0.01em;
  }

  .section-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.75rem;
    margin-bottom: 0.875rem;
  }

  .pill {
    display: inline-flex;
    align-items: center;
    gap: 0.375rem;
    padding: 0.3rem 0.625rem;
    border-radius: 999px;
    background: var(--surface-muted);
    color: var(--muted);
    font-size: 0.75rem;
    font-weight: 700;
    letter-spacing: 0.02em;
    white-space: nowrap;
  }

  .pill::before {
    content: '';
    width: 0.4rem;
    height: 0.4rem;
    border-radius: 999px;
    background: currentColor;
  }

  .pill.ok {
    background: var(--accent-soft);
    color: var(--accent);
  }

  .pill.warn {
    background: var(--warning-soft);
    color: var(--warning);
  }

  .pill.bad {
    background: var(--danger-soft);
    color: var(--danger);
  }

  .steps {
    margin: 0.875rem 0 1rem;
    padding-left: 1.2rem;
    color: var(--muted);
    font-size: 0.9375rem;
    line-height: 1.6;
  }

  .steps li + li {
    margin-top: 0.3rem;
  }

  form {
    display: flex;
    flex-direction: column;
    gap: 0.875rem;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 0.375rem;
    font-size: 0.875rem;
    font-weight: 650;
  }

  .field input {
    width: 100%;
    min-height: 2.875rem;
    padding: 0.75rem 0.875rem;
    border: 1px solid var(--border);
    border-radius: 0.75rem;
    background: var(--surface-muted);
    color: var(--text);
    font: inherit;
    font-weight: 500;
  }

  .field input:focus-visible,
  button:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  .field input::placeholder {
    color: var(--muted);
  }

  button {
    min-height: 2.75rem;
    padding: 0.7rem 1rem;
    border: 1px solid var(--border);
    border-radius: 0.75rem;
    background: var(--surface);
    color: var(--text);
    font: inherit;
    font-size: 0.9375rem;
    font-weight: 650;
    cursor: pointer;
  }

  button:disabled {
    cursor: not-allowed;
    opacity: 0.55;
  }

  button.primary {
    min-height: 3rem;
    border-color: transparent;
    background: var(--accent);
    color: var(--bg);
  }

  button.danger {
    border-color: var(--danger);
    color: var(--danger);
    background: transparent;
  }

  button.link {
    min-height: 2.5rem;
    margin-top: 0.625rem;
    padding: 0.25rem;
    border: 0;
    background: transparent;
    color: var(--accent);
    font-size: 0.875rem;
    text-align: left;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
    margin-top: 0.875rem;
  }

  .detect-actions {
    margin-top: 0;
    margin-bottom: 0.875rem;
  }

  button.link.inline {
    min-height: 2.75rem;
    margin: 0;
  }

  .facts {
    margin: 0;
    border: 1px solid var(--border);
    border-radius: 0.875rem;
    overflow: hidden;
  }

  .facts div {
    display: flex;
    justify-content: space-between;
    gap: 1rem;
    padding: 0.75rem 0.875rem;
  }

  .facts div + div {
    border-top: 1px solid var(--border);
  }

  dt {
    flex: 0 0 auto;
    color: var(--muted);
    font-size: 0.8125rem;
  }

  dd {
    margin: 0;
    min-width: 0;
    font-size: 0.8125rem;
    font-weight: 600;
    text-align: right;
    overflow-wrap: anywhere;
  }

  .note {
    margin: 0.875rem 0 0;
    padding: 0.75rem 0.875rem;
    border-radius: 0.75rem;
    background: var(--accent-soft);
    color: var(--text);
    font-size: 0.8125rem;
    line-height: 1.55;
  }

  .note.warn {
    background: var(--warning-soft);
  }

  .error {
    margin: 0.875rem 0 0;
    padding: 0.75rem 0.875rem;
    border-radius: 0.75rem;
    background: var(--danger-soft);
    color: var(--danger);
    font-size: 0.8125rem;
    line-height: 1.55;
    overflow-wrap: anywhere;
  }

  .loading,
  .empty {
    margin: 0;
    color: var(--muted);
    font-size: 0.875rem;
    line-height: 1.55;
  }

  .count {
    color: var(--muted);
    font-size: 0.75rem;
    font-weight: 700;
  }

  .events {
    margin: 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  .events li {
    padding: 0.625rem 0.75rem;
    border: 1px solid var(--border);
    border-radius: 0.75rem;
    background: var(--surface-muted);
  }

  .event-row {
    display: flex;
    align-items: baseline;
    gap: 0.5rem;
    font-size: 0.75rem;
  }

  .seq {
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }

  .event-type {
    flex: 1;
    min-width: 0;
    font-weight: 700;
    overflow-wrap: anywhere;
  }

  time {
    flex: 0 0 auto;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }

  .event-detail {
    margin: 0.35rem 0 0;
    color: var(--muted);
    font-size: 0.75rem;
    line-height: 1.45;
    overflow-wrap: anywhere;
  }

  .footer {
    display: flex;
    justify-content: space-between;
    gap: 1rem;
    padding: 0.25rem 0.25rem 0;
    color: var(--muted);
    font-size: 0.75rem;
  }

  .version {
    font-variant-numeric: tabular-nums;
  }

  @media (min-width: 44rem) {
    .shell {
      padding-left: 1.5rem;
      padding-right: 1.5rem;
    }
  }
</style>
