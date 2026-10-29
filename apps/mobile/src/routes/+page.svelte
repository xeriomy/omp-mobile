<svelte:head>
  <meta
    name="description"
    content="OMP Client mobile shell. Android build smoke test; no bridge connection is active."
  />
</svelte:head>

<main class="shell">
  <header class="topbar">
    <div class="brand">
      <span class="mark" aria-hidden="true">OMP</span>
      <div>
        <h1>OMP Client</h1>
        <p class="subtitle">Android build smoke test</p>
      </div>
    </div>
  </header>

  <section class="content" aria-labelledby="status-heading">
    <div class="status-card">
      <div class="status-row">
        <span class="dot" aria-hidden="true"></span>
        <span id="status-heading" class="status">Bridge not connected</span>
      </div>
      <p>
        This APK verifies that the Tauri 2 Android application shell compiles and starts. It does
        not contact OMP, the bridge, or any network service.
      </p>
    </div>

    <dl class="facts" aria-label="Build facts">
      <div>
        <dt>Application</dt>
        <dd>OMP Client</dd>
      </div>
      <div>
        <dt>Shell</dt>
        <dd>Tauri 2</dd>
      </div>
      <div>
        <dt>Interface</dt>
        <dd>Svelte + TypeScript</dd>
      </div>
      <div>
        <dt>Build</dt>
        <dd>Android debug</dd>
      </div>
    </dl>

    <p class="note">
      Engine state, sessions, models, and connection status will appear only after a real bridge
      connection is implemented and verified. Nothing on this screen is simulated.
    </p>
  </section>

  <footer class="footer">
    <span>Build smoke test only</span>
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

  .status-card {
    padding: 1.125rem;
    border: 1px solid var(--border);
    border-radius: 1.125rem;
    background: var(--surface);
    box-shadow: var(--shadow);
  }

  .status-row {
    display: flex;
    align-items: center;
    gap: 0.625rem;
  }

  .dot {
    width: 0.625rem;
    height: 0.625rem;
    border-radius: 999px;
    background: var(--warning);
    box-shadow: 0 0 0 0.25rem var(--warning-soft);
  }

  .status {
    font-weight: 650;
    font-size: 1rem;
  }

  .status-card p {
    margin: 0.75rem 0 0;
    color: var(--muted);
    font-size: 0.9375rem;
    line-height: 1.55;
  }

  .facts {
    margin: 0;
    border: 1px solid var(--border);
    border-radius: 1.125rem;
    background: var(--surface);
    overflow: hidden;
  }

  .facts div {
    display: flex;
    justify-content: space-between;
    gap: 1rem;
    padding: 0.875rem 1.125rem;
  }

  .facts div + div {
    border-top: 1px solid var(--border);
  }

  dt {
    color: var(--muted);
    font-size: 0.875rem;
  }

  dd {
    margin: 0;
    font-size: 0.875rem;
    font-weight: 600;
    text-align: right;
  }

  .note {
    margin: 0;
    padding: 0.875rem 1rem;
    border-radius: 0.875rem;
    background: var(--accent-soft);
    color: var(--text);
    font-size: 0.875rem;
    line-height: 1.55;
  }

  .footer {
    display: flex;
    justify-content: space-between;
    gap: 1rem;
    color: var(--muted);
    font-size: 0.75rem;
    padding: 0.25rem 0.125rem;
  }

  .version {
    font-variant-numeric: tabular-nums;
  }

  @media (min-width: 48rem) {
    .shell {
      padding-top: 2rem;
      gap: 1.75rem;
    }

    .content {
      gap: 1.25rem;
    }
  }
</style>
