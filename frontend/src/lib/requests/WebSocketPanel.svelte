<script>
  import Modal from '../components/Modal.svelte';
  import { toast } from '../stores.js';
  import { createEventDispatcher, onDestroy, tick } from 'svelte';

  const dispatch = createEventDispatcher();
  const URL_KEY = 'og_testdesk_ws_url';

  let url = localStorage.getItem(URL_KEY) || 'wss://ws.postman-echo.com/raw';
  let ws = null;
  let connected = false;
  let connecting = false;
  let draft = '';
  let sendAsJson = false;
  let messages = []; // {dir: 'sent'|'recv'|'system', text, at}
  let logEl;

  function log(dir, text) {
    messages = [...messages, { dir, text, at: Date.now() }];
    tick().then(() => {
      if (logEl) logEl.scrollTop = logEl.scrollHeight;
    });
  }

  function connect() {
    if (connected || connecting) return;
    if (!url.trim()) return;
    try {
      localStorage.setItem(URL_KEY, url);
    } catch {}
    connecting = true;
    try {
      ws = new WebSocket(url.trim());
    } catch (e) {
      connecting = false;
      toast(`Couldn't open WebSocket: ${e.message || e}`, 'error', 3000);
      return;
    }
    ws.onopen = () => {
      connecting = false;
      connected = true;
      log('system', `Connected to ${url}`);
    };
    ws.onmessage = (e) => log('recv', typeof e.data === 'string' ? e.data : '[binary frame]');
    ws.onerror = () => log('system', 'Connection error');
    ws.onclose = (e) => {
      connected = false;
      connecting = false;
      log('system', `Closed (code ${e.code}${e.reason ? `: ${e.reason}` : ''})`);
    };
  }

  function disconnect() {
    ws?.close();
  }

  function send() {
    if (!connected || !draft.trim()) return;
    let payload = draft;
    if (sendAsJson) {
      try {
        JSON.parse(draft);
      } catch {
        toast('Not valid JSON', 'error', 2000);
        return;
      }
    }
    ws.send(payload);
    log('sent', payload);
    draft = '';
  }

  function clearLog() {
    messages = [];
  }

  function onKeydown(e) {
    if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') send();
  }

  onDestroy(() => ws?.close());
</script>

<Modal title="WebSocket" width="620px" on:close>
  <div class="conn-bar">
    <input class="input mono" bind:value={url} placeholder="wss://echo.example.com" disabled={connected || connecting} />
    {#if connected || connecting}
      <button class="btn danger sm" on:click={disconnect}>{connecting ? 'Cancel' : 'Disconnect'}</button>
    {:else}
      <button class="btn primary sm" on:click={connect}>Connect</button>
    {/if}
  </div>
  <div class="status-row">
    <span class="dot" class:on={connected} class:connecting />
    <span class="status-text">{connected ? 'Connected' : connecting ? 'Connecting…' : 'Disconnected'}</span>
    <span style="flex:1" />
    <button class="btn ghost sm" on:click={clearLog} disabled={!messages.length}>Clear log</button>
  </div>

  <div class="log" bind:this={logEl}>
    {#each messages as m}
      <div class="log-row {m.dir}">
        <span class="tag">{m.dir === 'sent' ? '→' : m.dir === 'recv' ? '←' : '·'}</span>
        <span class="text">{m.text}</span>
      </div>
    {/each}
    {#if !messages.length}
      <div class="empty">Connect, then send a message to see it here.</div>
    {/if}
  </div>

  <div class="send-bar">
    <label class="json-toggle"><input type="checkbox" bind:checked={sendAsJson} /> JSON</label>
    <textarea
      class="input mono"
      rows="2"
      bind:value={draft}
      placeholder="Message to send — ⌘/Ctrl+Enter to send"
      disabled={!connected}
      on:keydown={onKeydown}
    />
    <button class="btn primary sm" on:click={send} disabled={!connected || !draft.trim()}>Send</button>
  </div>

  <svelte:fragment slot="footer">
    <span style="flex:1" />
    <button class="btn" on:click={() => dispatch('close')}>Close</button>
  </svelte:fragment>
</Modal>

<style>
  .conn-bar {
    display: flex;
    gap: 8px;
    margin-bottom: 8px;
  }
  .conn-bar .input {
    flex: 1;
  }
  .status-row {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-bottom: 8px;
    font-size: 11px;
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--text-muted);
  }
  .dot.on {
    background: var(--ok);
  }
  .dot.connecting {
    background: var(--warn);
  }
  .status-text {
    color: var(--text-secondary);
  }
  .log {
    height: 260px;
    overflow-y: auto;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 6px;
    font-family: var(--font-mono);
    font-size: 11.5px;
    margin-bottom: 8px;
  }
  .empty {
    color: var(--text-muted);
    text-align: center;
    padding: 20px;
    font-family: var(--font-sans);
  }
  .log-row {
    display: flex;
    gap: 6px;
    padding: 2px 0;
    word-break: break-word;
  }
  .log-row.sent .tag {
    color: var(--tool-requests-text);
  }
  .log-row.recv .tag {
    color: var(--ok);
  }
  .log-row.system {
    color: var(--text-muted);
    font-style: italic;
  }
  .tag {
    flex-shrink: 0;
    font-weight: 700;
  }
  .send-bar {
    display: flex;
    align-items: flex-end;
    gap: 8px;
  }
  .send-bar textarea {
    flex: 1;
    resize: vertical;
  }
  .json-toggle {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 11px;
    color: var(--text-secondary);
    white-space: nowrap;
    padding-bottom: 6px;
  }
</style>
