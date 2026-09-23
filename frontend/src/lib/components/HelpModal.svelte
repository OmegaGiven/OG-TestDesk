<script>
  import { onMount } from 'svelte';
  import Modal from './Modal.svelte';
  import { ICONS } from '../icons.js';
  import { open as openExternal } from '@tauri-apps/plugin-shell';
  import { api } from '../api.js';

  const ISSUE_REPO = 'OmegaGiven/OG-TestDesk';
  let searchEl;
  onMount(() => searchEl?.focus());

  // Shown at the bottom of the nav so a bug report (or the person
  // reading one) always knows which build they're looking at.
  let appVersion = '';
  onMount(async () => {
    try {
      const { getVersion } = await import('@tauri-apps/api/app');
      appVersion = await getVersion();
    } catch {}
  });

  async function reportIssue() {
    let logTail = '';
    try {
      const errors = await api.errorLogList(5);
      if (errors?.length) {
        logTail =
          '\n\n<details><summary>Last few logged errors</summary>\n\n```\n' +
          errors
            .map((e) => `[${new Date(e.ts * 1000).toISOString()}] ${e.source}: ${e.message}`)
            .join('\n') +
          '\n```\n</details>';
      }
    } catch {}
    const platform =
      typeof navigator !== 'undefined' ? navigator.platform || navigator.userAgent : 'unknown';
    const body =
      `**What happened**\n\n\n**What you expected**\n\n\n**Steps to reproduce**\n\n\n` +
      `---\nVersion: ${appVersion || 'unknown'}\nPlatform: ${platform}${logTail}`;
    const url =
      `https://github.com/${ISSUE_REPO}/issues/new?` +
      `title=${encodeURIComponent('')}&body=${encodeURIComponent(body)}`;
    try {
      await openExternal(url);
    } catch {
      // last resort if the shell plugin isn't available for some reason
      window.open(url, '_blank');
    }
  }

  async function sponsor() {
    const url = 'https://github.com/sponsors/OmegaGiven';
    try {
      await openExternal(url);
    } catch {
      window.open(url, '_blank');
    }
  }

  // Each section: plain text `body` lines (rendered as <p>), `code` blocks,
  // and `steps` (ordered). Searchable over title + keywords + all text.
  const SECTIONS = [
    {
      id: 'overview',
      title: 'Overview',
      keywords: 'start intro what is tools sql requests inspector colors',
      blocks: [
        { p: 'OG TestDesk is three tools in one window: a SQL client, an HTTP request client, and a shared JSON inspector.' },
        { p: 'Each tool has a fixed color — SQL is blue, Requests is green, Inspector is violet — used on its tab, buttons, and selection state so you always know which tool you are looking at.' },
        { p: 'Switch tools from the top bar or with ⌘1 / ⌘2 / ⌘3 (Ctrl on Windows/Linux).' }
      ]
    },
    {
      id: 'connections',
      title: 'Connections & running SQL',
      keywords: 'database postgres mysql sqlite connect query editor results grid schema table columns run',
      blocks: [
        { p: 'Add a connection with "+ New" in the SQL sidebar. Pick Postgres, MySQL/MariaDB, or SQLite, fill host/port/database/user (or a file path for SQLite), and give it an accent color. "Test" checks it before saving. Passwords go to the OS keychain, never the app database.' },
        { p: 'Each connection gets query tabs. Write SQL, then Run (⌘↵) or click ▶ Run. If you select text first, only the selection runs.' },
        { p: 'The schema tree on the left is lazy — expand a table to load its columns. The ↵ icon next to a table opens a "SELECT * FROM …" tab and runs it, paginated — scroll the results to keep pulling more of the table in.' },
        { p: 'Results grid: click a column header to sort, click a cell to select it (⌘C copies), and use → Inspector / CSV / JSON in the toolbar to send or export the result set.' }
      ]
    },
    {
      id: 'sql-vars',
      title: 'SQL variables',
      keywords: 'variable placeholder parameter slot fill substitute {{}} template',
      blocks: [
        { p: 'Put {{name}} anywhere in a query. A "Variables" bar appears above the editor with a slot for each distinct name.' },
        { p: 'Values are substituted into the SQL exactly as typed right before the query runs (you control quoting — wrap string values in quotes yourself). Values are saved per tab and survive a restart.' },
        { p: 'Run is blocked with a warning while any slot is empty.' },
        { code: "SELECT * FROM orders\nWHERE status = '{{status}}' AND placed_at >= '{{since}}';" }
      ]
    },
    {
      id: 'requests',
      title: 'Requests',
      keywords: 'http api rest post get send response headers body json method url collection save',
      blocks: [
        { p: 'Build a request with the method dropdown + URL bar, then Send. The Params tab stays in sync with the query string in the URL. Headers and a JSON body (with a Beautify button) are on their own tabs.' },
        { p: 'Save requests into collections in the sidebar. The response pane shows status, time, and size, with Body and Headers tabs; → Inspector opens a JSON response in the Inspector.' }
      ]
    },
    {
      id: 'env-vars',
      title: 'Environments & variables',
      keywords: 'environment variable globals baseurl token secret active switch {{}} substitution',
      blocks: [
        { p: 'Use {{variable}} in a request URL, headers, or body. Values come from two places, applied in order:' },
        { steps: [
          'Globals — plain variables applied to every request. Edit them from the "Env:" button at the bottom of the Requests sidebar → Globals row.',
          'The active environment — overrides a global of the same name. Only one environment is active at a time; click the ○ next to it to activate.'
        ] },
        { p: 'The MCP server applies the same globals + active environment to requests it sends.' }
      ]
    },
    {
      id: 'postman',
      title: 'Importing from Postman',
      keywords: 'postman import collection environment json export migrate',
      blocks: [
        { p: 'Click the ⇩ button in the Collections header of the Requests sidebar and choose a Postman export file (.json).' },
        { p: 'Collection exports (v2.0 / v2.1) become a collection of saved requests — folders are flattened into the request name. Environment exports become an environment. Postman already uses {{var}} syntax, so variables carry over unchanged.' }
      ]
    },
    {
      id: 'inspector',
      title: 'Inspector',
      keywords: 'json tree table summary search path copy explore drill viewer',
      blocks: [
        { p: 'The Inspector views any JSON — sent from a SQL result or an HTTP response, or pasted directly (Paste JSON button).' },
        { p: 'Tree mode: expand/collapse, "Expand all" / "+1 level", and a search box that highlights matches and shows a count. Table mode renders an array of objects as a grid. Summary mode shows depth, key counts, and type breakdown.' },
        { p: 'Click any node to fill the right-hand panel with its path, type, size, a pretty-printed snippet, and Copy path / value / pretty buttons.' }
      ]
    },
    {
      id: 'mcp',
      title: 'Connect an AI tool (MCP server)',
      keywords: 'mcp ai claude model context protocol server sse token expose read-only connect agent local llm',
      blocks: [
        { p: 'OG TestDesk can run a local MCP server so an AI assistant can list your schemas and run queries / requests — without ever seeing your passwords. The server executes everything itself.' },
        { p: 'Open Settings (⚙ in the top bar) → MCP server:' },
        { steps: [
          'Tick "Enable MCP server". It binds to 127.0.0.1 on the chosen port (default 7788).',
          'In "Exposed connections", tick a connection to make it visible. It is read-only until you also tick "Writes" here and "Allow write statements" above.',
          'Optionally tick "Expose HTTP request tools" to let the AI send saved or ad-hoc HTTP requests.',
          'Click "Apply & restart".',
          'Copy the "claude mcp add" line from "Connect a client" and run it in a terminal.'
        ] },
        { p: 'Manual Claude Code setup:' },
        { code: 'claude mcp add --transport sse og-testdesk \\\n  "http://127.0.0.1:7788/sse?token=YOUR_TOKEN"' },
        { p: 'Claude Desktop — add to its MCP config:' },
        { code: '{\n  "mcpServers": {\n    "og-testdesk": {\n      "transport": "sse",\n      "url": "http://127.0.0.1:7788/sse?token=YOUR_TOKEN"\n    }\n  }\n}' },
        { p: 'Tools exposed: list_connections, list_schemas, list_columns, run_query, and (when enabled) list_saved_requests, run_saved_request, send_request.' },
        { p: 'Security: the token is the only gate. Anyone on this machine who has the token and can reach the port can query your exposed connections. Regenerate the token (Settings → New) to revoke old clients.' }
      ]
    },
    {
      id: 'icons',
      title: 'Icon reference',
      keywords:
        'icon glyph symbol button meaning legend reference ' +
        Object.values(ICONS)
          .map((i) => `${i.label} ${i.where}`)
          .join(' '),
      blocks: [
        { p: 'Every icon button in the app, and where you\'ll run into it. This table reads straight from the app\'s own icon registry — if a glyph changes, this list changes with it.' },
        { iconTable: true }
      ]
    },
    {
      id: 'shortcuts',
      title: 'Keyboard shortcuts',
      keywords: 'keys hotkey shortcut cmd ctrl run save copy switch tool',
      blocks: [
        { steps: [
          '⌘↵ / Ctrl+Enter — run the current query',
          '⌘S / Ctrl+S — save the current query',
          '⌘C — copy the selected result cell',
          '⌘1 / ⌘2 / ⌘3 — switch to SQL / Requests / Inspector'
        ] }
      ]
    },
    {
      id: 'storage',
      title: 'Where your data lives',
      keywords: 'storage file path database keychain secret location backup reset',
      blocks: [
        { p: 'App data (connections, tabs, history, saved queries/requests, environments, settings) is a SQLite file in the OS app-data directory: OGTestDesk/og_testdesk.db. Override with the OGTESTDESK_DB_PATH environment variable.' },
        { p: 'Connection passwords are stored only in the OS keychain (macOS Keychain, Windows Credential Manager, Linux Secret Service), keyed by connection id — never in that database file.' }
      ]
    }
  ];

  let query = '';
  let activeId = SECTIONS[0].id;
  if (typeof location !== 'undefined') {
    const s = new URLSearchParams(location.search).get('helpsection');
    if (s && SECTIONS.some((x) => x.id === s)) activeId = s;
  }

  function sectionText(s) {
    return (
      s.title +
      ' ' +
      s.keywords +
      ' ' +
      s.blocks
        .map((b) => b.p || (b.code || '') + ' ' + (b.steps || []).join(' '))
        .join(' ')
    ).toLowerCase();
  }

  $: q = query.trim().toLowerCase();
  $: matches = q ? SECTIONS.filter((s) => sectionText(s).includes(q)) : SECTIONS;
  $: active = (q ? matches[0] : SECTIONS.find((s) => s.id === activeId)) || SECTIONS[0];

  function select(id) {
    activeId = id;
    query = '';
  }
</script>

<Modal title="Help & documentation" width="760px" on:close>
  <div class="help">
    <aside class="nav">
      <input class="input" placeholder="Search docs…" bind:value={query} bind:this={searchEl} />
      <ul>
        {#each matches as s (s.id)}
          <li>
            <button class:active={active.id === s.id} on:click={() => select(s.id)}>{s.title}</button>
          </li>
        {/each}
        {#if matches.length === 0}
          <li class="none">No matches.</li>
        {/if}
      </ul>
      <div class="nav-footer">
        <button class="report-issue" on:click={reportIssue}>Report an issue on GitHub ↗</button>
        <button class="report-issue sponsor" on:click={sponsor}>{@html ICONS.heart.svg} Sponsor this project</button>
        {#if appVersion}<div class="app-version">OG TestDesk v{appVersion}</div>{/if}
      </div>
    </aside>

    <article class="content">
      <h2>{active.title}</h2>
      {#each active.blocks as b}
        {#if b.p}
          <p>{b.p}</p>
        {:else if b.code}
          <pre>{b.code}</pre>
        {:else if b.steps}
          <ol>
            {#each b.steps as step}<li>{step}</li>{/each}
          </ol>
        {:else if b.iconTable}
          <table class="icon-ref">
            <thead>
              <tr><th class="ic">Icon</th><th>Meaning</th><th>Where</th></tr>
            </thead>
            <tbody>
              {#each Object.values(ICONS) as icon}
                <tr>
                  <td class="ic" style={icon.colorVar ? `color:var(${icon.colorVar})` : ''}>
                    {@html icon.svg}
                  </td>
                  <td>{icon.label}</td>
                  <td class="where">{icon.where}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
      {/each}
    </article>
  </div>
</Modal>

<style>
  .help {
    display: flex;
    gap: 0;
    min-height: 420px;
    max-height: 60vh;
    margin: -14px;
  }
  .nav {
    width: 220px;
    flex-shrink: 0;
    border-right: 1px solid var(--border);
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    overflow: auto;
  }
  .nav ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .nav button {
    width: 100%;
    text-align: left;
    background: none;
    border: none;
    padding: 6px 8px;
    border-radius: var(--radius-sm);
    font-size: 12px;
    color: var(--text-secondary);
    cursor: pointer;
  }
  .nav button:hover {
    background: var(--surface-3);
  }
  .nav-footer {
    margin-top: auto;
  }
  .nav button.report-issue {
    padding-top: 10px;
    border-top: 1px solid var(--border);
    border-radius: 0;
    color: var(--text-muted);
    font-size: 11px;
  }
  .nav button.report-issue.sponsor {
    border-top: none;
    padding-top: 2px;
    color: var(--danger);
    font-weight: 600;
  }
  .app-version {
    padding: 4px 8px 0;
    font-size: 10px;
    color: var(--text-muted);
    text-align: center;
  }
  .nav button.active {
    background: var(--tool-sql-tint);
    color: var(--tool-sql-text);
    font-weight: 600;
  }
  .nav .none {
    font-size: 11px;
    color: var(--text-muted);
    padding: 6px 8px;
  }
  .content {
    flex: 1;
    padding: 16px 20px;
    overflow: auto;
    font-size: 13px;
    line-height: 1.6;
  }
  .content h2 {
    margin: 0 0 12px;
    font-size: 15px;
  }
  .content p {
    margin: 0 0 10px;
    color: var(--text-secondary);
  }
  .content ol {
    margin: 0 0 12px;
    padding-left: 20px;
    color: var(--text-secondary);
  }
  .content ol li {
    margin-bottom: 5px;
  }
  .content pre {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    padding: 10px 12px;
    font-family: var(--font-mono);
    font-size: 11.5px;
    overflow-x: auto;
    margin: 0 0 12px;
    white-space: pre;
  }
  .icon-ref {
    width: 100%;
    border-collapse: collapse;
    font-size: 12px;
    margin-bottom: 12px;
  }
  .icon-ref th {
    text-align: left;
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.03em;
    color: var(--text-muted);
    padding: 4px 8px;
    border-bottom: 1px solid var(--border-strong);
  }
  .icon-ref td {
    padding: 5px 8px;
    border-bottom: 1px solid var(--border);
    color: var(--text-secondary);
    vertical-align: top;
  }
  .icon-ref td.ic {
    width: 40px;
    font-size: 14px;
    text-align: center;
    color: var(--text-primary);
    font-family: var(--font-mono);
  }
  .icon-ref td.where {
    color: var(--text-muted);
    font-size: 11px;
  }
</style>
