<script>
  import Modal from './Modal.svelte';
  import { onMount } from 'svelte';
  import { api } from '../api.js';
  import { ICONS } from '../icons.js';
  import {
    connections,
    appearance,
    APPEARANCE_DEFAULT,
    effectiveMode,
    toast,
    toastError
  } from '../stores.js';
  import {
    COLOR_THEMES,
    FONT_SANS,
    FONT_MONO,
    THEME_VARS,
    applyColorTheme,
    readThemeVar,
    normalizeHex
  } from '../themes.js';

  const TABS = [
    ['appearance', 'Appearance'],
    ['theme', 'Colour theme'],
    ['results', 'Query results'],
    ['mcp', 'MCP server'],
    ['access', 'MCP access']
  ];
  let tab = 'appearance';
  try {
    const t = new URLSearchParams(location.search).get('settingstab');
    if (t && TABS.some(([id]) => id === t)) tab = t;
  } catch {}

  const PRESET_KEYS = Object.keys(COLOR_THEMES);
  const SWATCH_BASE = {
    light: { s: '#ffffff', a: '#0c447c', b: '#27500a' },
    dark: { s: '#26262b', a: '#85b7eb', b: '#97c459' }
  };
  function swatchBg(key, i) {
    const mode = effectiveMode();
    const varName = ['--surface-2', '--tool-sql-text', '--tool-requests-text'][i];
    const preset = (COLOR_THEMES[key] || {})[mode] || {};
    if (key === 'custom') return ($appearance.customTheme?.[mode] || {})[varName] || SWATCH_BASE[mode][['s', 'a', 'b'][i]];
    return preset[varName] || SWATCH_BASE[mode][['s', 'a', 'b'][i]];
  }

  // ---- custom theme editor
  let editMode = effectiveMode(); // which slice ('light'|'dark') we're editing
  const GROUPS = [...new Set(THEME_VARS.map((v) => v.group))];
  $: customSlice = $appearance.customTheme?.[editMode] || {};

  function curVal(key) {
    return customSlice[key] || normalizeHex(readThemeVar(key)) || '#888888';
  }
  function setVar(key, value) {
    const ct = $appearance.customTheme || { light: {}, dark: {} };
    $appearance.customTheme = {
      ...ct,
      [editMode]: { ...(ct[editMode] || {}), [key]: value }
    };
    if ($appearance.colorTheme !== 'custom') $appearance.colorTheme = 'custom';
  }
  function clearVar(key) {
    const ct = $appearance.customTheme || {};
    const slice = { ...(ct[editMode] || {}) };
    delete slice[key];
    $appearance.customTheme = { ...ct, [editMode]: slice };
  }
  function seedFrom(key) {
    if (!key) return;
    // temporarily apply the preset, read every var, keep the snapshot
    applyColorTheme(key, editMode);
    const snap = {};
    for (const { key: k } of THEME_VARS) {
      const v = normalizeHex(readThemeVar(k));
      if (v) snap[k] = v;
    }
    const ct = $appearance.customTheme || {};
    $appearance.customTheme = { ...ct, [editMode]: snap };
    $appearance.colorTheme = 'custom'; // re-applies our custom palette
  }
  function copyLightToDark() {
    const ct = $appearance.customTheme || {};
    $appearance.customTheme = { ...ct, dark: { ...(ct.light || {}) } };
  }
  function resetCustom() {
    $appearance.customTheme = { light: {}, dark: {} };
  }

  // ---- MCP
  let cfg = null;
  let status = { running: false, port: null };
  let acls = {};
  let maxRows = 10000;
  let busy = false;

  const ROW_STEPS = [1000, 5000, 10000, 25000, 50000, 100000, 0];
  const rowLabel = (n) => (n === 0 ? 'unlimited (risky)' : n.toLocaleString());

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
  $: claudeCmd = cfg ? `claude mcp add --transport sse og-testdesk "${url}?token=${cfg.token}"` : '';
  $: isCustom = ($appearance.colorTheme || 'default') === 'custom';
</script>

<Modal title="Settings" width="720px" on:close>
  <div class="wrap">
    <nav class="rail">
      {#each TABS as [id, label]}
        <button class:sel={tab === id} on:click={() => (tab = id)}>{label}</button>
      {/each}
    </nav>

    <div class="pane">
      {#if tab === 'appearance'}
        <h3>Appearance</h3>
        <p class="muted">Applies instantly. Stored on this device.</p>

        <div class="row">
          <div class="field">
            <label for="fs">Interface font</label>
            <select id="fs" class="select" bind:value={$appearance.fontSans}>
              {#each FONT_SANS as [k, label]}<option value={k}>{label}</option>{/each}
            </select>
          </div>
          <div class="field">
            <label for="fm">Editor / mono font</label>
            <select id="fm" class="select" bind:value={$appearance.fontMono}>
              {#each FONT_MONO as [k, label]}<option value={k}>{label}</option>{/each}
            </select>
          </div>
        </div>

        <div class="slider">
          <label>Text scale</label>
          <input type="range" min="0.8" max="1.4" step="0.05" bind:value={$appearance.fontScale} />
          <span class="v">{Math.round(($appearance.fontScale ?? 1) * 100)}%</span>
        </div>
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
        <button class="btn sm" on:click={() => appearance.set({ ...APPEARANCE_DEFAULT })}>
          Reset appearance
        </button>
      {/if}

      {#if tab === 'theme'}
        <h3>Colour theme</h3>
        <div class="theme-swatches">
          {#each PRESET_KEYS as key}
            <button
              class="tsw"
              class:sel={($appearance.colorTheme || 'default') === key}
              title={COLOR_THEMES[key].label}
              on:click={() => ($appearance.colorTheme = key)}
            >
              <span class="tsw-p" style="background:{swatchBg(key, 0)}" />
              <span class="tsw-p" style="background:{swatchBg(key, 1)}" />
              <span class="tsw-p" style="background:{swatchBg(key, 2)}" />
              <span class="tsw-label">{COLOR_THEMES[key].label}</span>
            </button>
          {/each}
        </div>

        {#if isCustom}
          <div class="custom">
            <div class="custom-bar">
              <div class="seg">
                <button class:sel={editMode === 'light'} on:click={() => (editMode = 'light')}>
                  Light
                </button>
                <button class:sel={editMode === 'dark'} on:click={() => (editMode = 'dark')}>
                  Dark
                </button>
              </div>
              <select
                class="select sm"
                on:change={(e) => {
                  seedFrom(e.target.value);
                  e.target.value = '';
                }}
              >
                <option value="">Start from…</option>
                {#each PRESET_KEYS.filter((k) => k !== 'custom') as k}
                  <option value={k}>{COLOR_THEMES[k].label}</option>
                {/each}
              </select>
              <button class="btn sm" on:click={copyLightToDark} title="Copy the light values onto the dark slice">
                Light → dark
              </button>
              <button class="btn sm" on:click={resetCustom}>Clear all</button>
            </div>
            {#if editMode !== effectiveMode()}
              <p class="muted note">
                Editing the <b>{editMode}</b> palette — switch the app to {editMode} mode (top-right)
                to preview it live.
              </p>
            {/if}

            {#each GROUPS as g}
              <div class="cgroup">{g}</div>
              {#each THEME_VARS.filter((v) => v.group === g) as v (v.key)}
                <div class="cvar">
                  <label for="cv-{v.key}">{v.label}</label>
                  <input
                    id="cv-{v.key}"
                    type="color"
                    value={curVal(v.key)}
                    on:input={(e) => setVar(v.key, e.target.value)}
                  />
                  <input
                    class="input mono hex"
                    value={curVal(v.key)}
                    on:change={(e) => setVar(v.key, e.target.value.trim())}
                  />
                  {#if customSlice[v.key]}
                    <button class="mini" title="Revert this one" on:click={() => clearVar(v.key)}>{ICONS.revert.glyph}</button>
                  {:else}
                    <span class="mini dim" title="Inherits from the base palette">—</span>
                  {/if}
                </div>
              {/each}
            {/each}
          </div>
        {:else}
          <p class="muted">
            Pick <b>Custom</b> above to build your own palette — every surface, text, border and
            accent colour, for light and dark independently.
          </p>
        {/if}
      {/if}

      {#if tab === 'results'}
        <h3>Query results</h3>
        <p class="muted">
          Results are paged — each SELECT fetches one page and, on the first page, tries a
          time-boxed <code class="mono">COUNT(*)</code> so you see the total. "Load all" pulls the
          whole result (still stopped at the hard cap below to protect RAM). History keeps result
          sets on disk, loaded only when you reopen one.
        </p>
        <div class="slider">
          <label>Page size</label>
          <input type="range" min="100" max="5000" step="100" bind:value={$appearance.pageSize} />
          <span class="v" style="width:auto">{($appearance.pageSize ?? 500).toLocaleString()}</span>
        </div>
        <div class="slider">
          <label>Hard row cap</label>
          <input
            type="range"
            min="0"
            max={ROW_STEPS.length - 1}
            value={Math.max(0, ROW_STEPS.indexOf(maxRows))}
            on:input={(e) => setMaxRows(ROW_STEPS[+e.target.value])}
          />
          <span class="v" style="width:auto">{rowLabel(maxRows)}</span>
        </div>
      {/if}

      {#if tab === 'mcp'}
        {#if cfg}
          <div class="sec-head">
            <h3>MCP server</h3>
            <span class="badge" class:on={status.running}>
              {status.running ? `running · :${status.port}` : 'stopped'}
            </span>
          </div>
          <p class="muted">
            Exposes your connections and saved requests to on-device AI tools. Passwords are never
            sent — the server runs queries and requests itself. Binds to 127.0.0.1 only, requires
            the token below.
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
        {:else}
          <p class="muted">Loading…</p>
        {/if}
      {/if}

      {#if tab === 'access'}
        {#if cfg}
          <h3>Exposed connections</h3>
          <p class="muted">
            A connection is invisible to MCP until exposed. Exposed = read-only unless "writes" is
            also checked here <em>and</em> on the MCP server tab.
          </p>
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

          <h3 style="margin-top:18px">Connect a client</h3>
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
        {:else}
          <p class="muted">Loading…</p>
        {/if}
      {/if}
    </div>
  </div>
</Modal>

<style>
  .wrap {
    display: flex;
    gap: 14px;
    min-height: 340px;
  }
  .rail {
    display: flex;
    flex-direction: column;
    gap: 2px;
    width: 132px;
    flex-shrink: 0;
    border-right: 1px solid var(--border);
    padding-right: 8px;
  }
  .rail button {
    text-align: left;
    background: none;
    border: none;
    cursor: pointer;
    font-size: 12px;
    color: var(--text-secondary);
    padding: 7px 9px;
    border-radius: var(--radius-sm);
  }
  .rail button:hover {
    background: var(--surface-3);
  }
  .rail button.sel {
    background: var(--tool-sql-tint);
    color: var(--tool-sql-text);
    font-weight: 600;
  }
  .pane {
    flex: 1;
    min-width: 0;
  }
  h3 {
    margin: 0 0 6px;
    font-size: 13px;
  }
  .sec-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .muted {
    font-size: 11.5px;
    line-height: 1.5;
    color: var(--text-muted);
    margin: 4px 0 10px;
  }
  .note {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    padding: 6px 8px;
  }
  .row {
    display: flex;
    gap: 10px;
    margin: 8px 0;
  }
  .field {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .field label {
    font-size: 11px;
    color: var(--text-secondary);
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
  .theme-swatches {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    margin-bottom: 6px;
  }
  .tsw {
    display: flex;
    align-items: center;
    gap: 3px;
    padding: 3px 8px 3px 3px;
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    cursor: pointer;
  }
  .tsw.sel {
    border-color: var(--text-primary);
    box-shadow: 0 0 0 1px var(--text-primary);
  }
  .tsw-p {
    width: 12px;
    height: 18px;
    border-radius: 2px;
  }
  .tsw-label {
    font-size: 11px;
    margin-left: 4px;
    color: var(--text-primary);
  }
  .custom {
    margin-top: 12px;
    border-top: 1px solid var(--border);
    padding-top: 10px;
  }
  .custom-bar {
    display: flex;
    gap: 6px;
    align-items: center;
    flex-wrap: wrap;
    margin-bottom: 8px;
  }
  .seg {
    display: flex;
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    overflow: hidden;
  }
  .seg button {
    background: var(--surface-2);
    border: none;
    cursor: pointer;
    font-size: 11px;
    padding: 4px 10px;
    color: var(--text-secondary);
  }
  .seg button.sel {
    background: var(--tool-sql-text);
    color: #fff;
  }
  .cgroup {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--text-muted);
    margin: 12px 0 4px;
    font-weight: 700;
  }
  .cvar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 3px 0;
  }
  .cvar label {
    flex: 1;
    font-size: 12px;
    color: var(--text-secondary);
  }
  .cvar input[type='color'] {
    width: 30px;
    height: 24px;
    padding: 0;
    border: 1px solid var(--border-strong);
    border-radius: 4px;
    background: none;
    cursor: pointer;
  }
  .hex {
    width: 96px;
    font-size: 11px;
    padding: 4px 6px;
  }
  .mini {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    width: 20px;
    height: 20px;
    background: none;
    border: none;
    border-radius: var(--radius-sm);
    color: var(--text-muted);
    cursor: pointer;
    font-size: 12px;
    line-height: 1;
  }
  .mini:hover:not(.dim) {
    background: var(--surface-3);
    color: var(--text-primary);
  }
  .mini.dim {
    cursor: default;
    opacity: 0.4;
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
