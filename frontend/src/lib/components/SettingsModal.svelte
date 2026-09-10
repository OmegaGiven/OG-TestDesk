<script>
  import Modal from './Modal.svelte';
  import { onMount } from 'svelte';
  import { api } from '../api.js';
  import { connections, appearance, toast, toastError } from '../stores.js';

  let cfg = null; // {enabled, port, token, allow_write, allow_http}
  let status = { running: false, port: null };
  let acls = {}; // connId -> {exposed, allow_writes}
  let maxRows = 10000;
  let busy = false;

  const ROW_STEPS = [1000, 5000, 10000, 25000, 50000, 100000, 0];
  function rowLabel(n) {
    return n === 0 ? 'unlimited (risky)' : n.toLocaleString();
  }

  onMount(load);
  async function load() {
    try {
      [cfg, status, acls, maxRows] = await Promise.all([
        api.mcpConfigGet(),
        api.mcpStatus(),
        api.mcpAclsGet(),
        api.queryLimitsGet()
      ]);
    } catch (e) {
      toastError(e);
    }
  }
  async function setMaxRows(n) {
    maxRows = n;
    try {
      await api.queryLimitsSet(n);
    } catch (e) {
      toastError(e);
    }
  }

  async function apply() {
    busy = true;
    try {
      status = await api.mcpConfigSet(cfg);
      toast(cfg.enabled ? `MCP server on :${status.port}` : 'MCP server stopped', 'success');
    } catch (e) {
      toastError(e);
    } finally {
      busy = false;
    }
  }

  function regenToken() {
    cfg.token = crypto.randomUUID().replace(/-/g, '');
  }
  async function copy(t) {
    try {
      await navigator.clipboard.writeText(t);
      toast('Copied', 'success', 1200);
    } catch {}
  }

  async function setAcl(id, patch) {
    const next = { exposed: false, allow_writes: false, ...(acls[id] || {}), ...patch };
    acls = { ...acls, [id]: next };
    try {
      await api.mcpAclSet(id, next);
    } catch (e) {
      toastError(e);
    }
  }

  $: url = cfg ? `http://127.0.0.1:${cfg.port}/sse` : '';
  $: claudeCmd = cfg
    ? `claude mcp add --transport sse og-testdesk "${url}?token=${cfg.token}"`
    : '';
</script>

<Modal title="Settings" width="600px" on:close>
  {#if cfg}
    <section class="sec">
      <h3>Appearance</h3>
      <p class="muted">Applies instantly. Stored on this device.</p>
      <div class="slider">
        <label>Corner radius</label>
        <input type="range" min="0" max="18" bind:value={$appearance.radius} />
        <span class="v">{$appearance.radius}px</span>
      </div>
      <div class="slider">
        <label>Window margin</label>
        <input type="range" min="0" max="24" bind:value={$appearance.gutter} />
        <span class="v">{$appearance.gutter}px</span>
      </div>
      <div class="slider">
        <label>Top-bar padding</label>
        <input type="range" min="0" max="16" bind:value={$appearance.navPad} />
        <span class="v">{$appearance.navPad}px</span>
      </div>
      <div class="slider">
        <label>Top-bar item gap</label>
        <input type="range" min="1" max="14" bind:value={$appearance.navGap} />
        <span class="v">{$appearance.navGap}px</span>
      </div>
      <button
        class="btn sm"
        on:click={() =>
          appearance.set({ radius: 8, gutter: 0, density: 1, navPad: 6, navGap: 5 })}
      >Reset</button>
    </section>

    <section class="sec">
      <h3>Query limits</h3>
      <p class="muted">
        A query returning many rows is streamed and stopped at this cap so a
        <code class="mono">SELECT *</code> on a huge table can't exhaust RAM. History keeps result
        sets on disk (not memory) and loads one only when you reopen it.
      </p>
      <div class="slider">
        <label>Max rows per query</label>
        <input
          type="range"
          min="0"
          max={ROW_STEPS.length - 1}
          value={Math.max(0, ROW_STEPS.indexOf(maxRows))}
          on:input={(e) => setMaxRows(ROW_STEPS[+e.target.value])}
        />
        <span class="v" style="width:auto">{rowLabel(maxRows)}</span>
      </div>
    </section>

    <section class="sec">
      <div class="sec-head">
        <h3>MCP server</h3>
        <span class="badge" class:on={status.running}>
          {status.running ? `running · :${status.port}` : 'stopped'}
        </span>
      </div>
      <p class="muted">
        Exposes your connections and saved requests to on-device AI tools. Passwords are
        never sent — the server runs queries and requests itself. Binds to 127.0.0.1 only,
        requires the token below.
      </p>

      <label class="toggle">
        <input type="checkbox" bind:checked={cfg.enabled} />
        <span>Enable MCP server</span>
      </label>

      <div class="row">
        <div class="field">
          <label for="port">Port</label>
          <input id="port" class="input" type="number" bind:value={cfg.port} />
        </div>
        <div class="field" style="flex:2">
          <label for="tok">Token</label>
          <div class="tok">
            <input id="tok" class="input mono" value={cfg.token} readonly />
            <button class="btn sm" on:click={() => copy(cfg.token)}>Copy</button>
            <button class="btn sm" on:click={regenToken}>New</button>
          </div>
        </div>
      </div>

      <label class="toggle">
        <input type="checkbox" bind:checked={cfg.allow_write} />
        <span>Allow write statements (INSERT/UPDATE/DDL) — per-connection flag still required</span>
      </label>
      <label class="toggle">
        <input type="checkbox" bind:checked={cfg.allow_http} />
        <span>Expose HTTP request tools (<code>send_request</code>, <code>run_saved_request</code>)</span>
      </label>

      <button class="btn primary" on:click={apply} disabled={busy} style="margin-top:8px">
        {busy ? 'Applying…' : 'Apply & restart'}
      </button>
    </section>

    <section class="sec">
      <h3>Exposed connections</h3>
      <p class="muted">A connection is invisible to MCP until exposed. Exposed = read-only unless "writes" is also checked here <em>and</em> above.</p>
      <table class="acl">
        <thead>
          <tr><th>Connection</th><th>Exposed</th><th>Writes</th></tr>
        </thead>
        <tbody>
          {#each $connections as c (c.id)}
            <tr>
              <td>
                <span class="dot" style="background:{c.color || 'var(--conn-slate)'}" />
                {c.nickname} <span class="muted">{c.kind}</span>
              </td>
              <td>
                <input
                  type="checkbox"
                  checked={acls[c.id]?.exposed || false}
                  on:change={(e) => setAcl(c.id, { exposed: e.target.checked })}
                />
              </td>
              <td>
                <input
                  type="checkbox"
                  disabled={!acls[c.id]?.exposed}
                  checked={acls[c.id]?.allow_writes || false}
                  on:change={(e) => setAcl(c.id, { allow_writes: e.target.checked })}
                />
              </td>
            </tr>
          {/each}
          {#if $connections.length === 0}
            <tr><td colspan="3" class="muted">No connections.</td></tr>
          {/if}
        </tbody>
      </table>
    </section>

    <section class="sec">
      <h3>Connect a client</h3>
      <p class="muted">SSE endpoint (token in the query string):</p>
      <div class="snippet">
        <code>{url}?token={cfg.token}</code>
        <button class="btn sm" on:click={() => copy(`${url}?token=${cfg.token}`)}>Copy</button>
      </div>
      <p class="muted" style="margin-top:8px">Claude Code:</p>
      <div class="snippet">
        <code>{claudeCmd}</code>
        <button class="btn sm" on:click={() => copy(claudeCmd)}>Copy</button>
      </div>
    </section>
  {:else}
    <p class="muted">Loading…</p>
  {/if}
</Modal>

<style>
  .sec {
    margin-bottom: 18px;
    padding-bottom: 16px;
    border-bottom: 1px solid var(--border);
  }
  .sec:last-child {
    border-bottom: none;
    margin-bottom: 0;
  }
  .sec-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  h3 {
    margin: 0 0 6px;
    font-size: 13px;
  }
  .muted {
    font-size: 11.5px;
    line-height: 1.5;
    color: var(--text-muted);
    margin: 4px 0 10px;
  }
  .badge {
    font-size: 10px;
    font-weight: 700;
    padding: 2px 7px;
    border-radius: 10px;
    background: var(--surface-3);
    color: var(--text-muted);
  }
  .badge.on {
    background: color-mix(in srgb, var(--ok) 20%, transparent);
    color: var(--ok);
  }
  .toggle {
    display: flex;
    gap: 8px;
    align-items: flex-start;
    font-size: 12px;
    margin: 8px 0;
    color: var(--text-secondary);
  }
  .toggle input {
    margin-top: 2px;
  }
  .tok {
    display: flex;
    gap: 5px;
  }
  .slider {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 8px 0;
    font-size: 12px;
  }
  .slider label {
    width: 110px;
    color: var(--text-secondary);
  }
  .slider input[type='range'] {
    flex: 1;
  }
  .slider .v {
    width: 44px;
    text-align: right;
    color: var(--text-muted);
    font-family: var(--font-mono);
  }
  .acl {
    width: 100%;
    border-collapse: collapse;
    font-size: 12px;
  }
  .acl th,
  .acl td {
    text-align: left;
    padding: 6px 8px;
    border-bottom: 1px solid var(--border);
  }
  .acl th {
    font-size: 10px;
    text-transform: uppercase;
    color: var(--text-muted);
  }
  .acl th:not(:first-child),
  .acl td:not(:first-child) {
    text-align: center;
    width: 70px;
  }
  .dot {
    display: inline-block;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    margin-right: 5px;
  }
  .snippet {
    display: flex;
    gap: 6px;
    align-items: center;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    padding: 7px 9px;
  }
  .snippet code {
    flex: 1;
    font-family: var(--font-mono);
    font-size: 11px;
    word-break: break-all;
  }
</style>
