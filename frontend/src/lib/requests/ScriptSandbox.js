// Runs pre-request/test scripts in a sandboxed iframe (allow-scripts only —
// no allow-same-origin, so it can't reach the parent's DOM, storage, or
// cookies) rather than eval'ing them in the main page. A minimal, honestly
// scoped subset of Postman's pm.* API: pm.environment/pm.globals/
// pm.variables get/set/unset, pm.request (read/write before sending),
// pm.response (read-only, post-send), pm.test(name, fn), and a small
// chai-like pm.expect(...) covering the assertions actually seen in the
// wild most often. Not a faithful reimplementation of Postman's full
// sandbox — no pm.sendRequest, no external libraries, no async scripts.
const HARNESS_HTML = `<!doctype html><script>
window.addEventListener('message', (ev) => {
  const { id, kind, code, context } = ev.data || {};
  if (!code) { parent.postMessage({ id, results: [], vars: context, error: null }, '*'); return; }
  const vars = {
    environment: { ...(context.environment || {}) },
    globals: { ...(context.globals || {}) }
  };
  const results = [];
  function makeExpect(actual) {
    const fail = (msg) => { throw new Error(msg); };
    const chain = {};
    ['to', 'be', 'have', 'an', 'a', 'been', 'was'].forEach((k) => (chain[k] = chain));
    chain.equal = (exp) => { if (actual !== exp) fail('expected ' + JSON.stringify(actual) + ' to equal ' + JSON.stringify(exp)); };
    chain.eql = (exp) => { if (JSON.stringify(actual) !== JSON.stringify(exp)) fail('expected ' + JSON.stringify(actual) + ' to deeply equal ' + JSON.stringify(exp)); };
    chain.include = (exp) => { if (!(actual && actual.includes && actual.includes(exp))) fail('expected ' + JSON.stringify(actual) + ' to include ' + JSON.stringify(exp)); };
    chain.above = (n) => { if (!(actual > n)) fail('expected ' + actual + ' to be above ' + n); };
    chain.below = (n) => { if (!(actual < n)) fail('expected ' + actual + ' to be below ' + n); };
    chain.ok = () => { if (!actual) fail('expected value to be truthy'); };
    chain.null = () => { if (actual !== null) fail('expected null'); };
    chain.undefined = () => { if (actual !== undefined) fail('expected undefined'); };
    return chain;
  }
  const pm = {
    environment: {
      get: (k) => vars.environment[k],
      set: (k, v) => { vars.environment[k] = String(v); },
      unset: (k) => { delete vars.environment[k]; },
      has: (k) => k in vars.environment
    },
    globals: {
      get: (k) => vars.globals[k],
      set: (k, v) => { vars.globals[k] = String(v); },
      unset: (k) => { delete vars.globals[k]; },
      has: (k) => k in vars.globals
    },
    variables: {
      get: (k) => (k in vars.environment ? vars.environment[k] : vars.globals[k])
    },
    request: context.request ? JSON.parse(JSON.stringify(context.request)) : undefined,
    test: (name, fn) => {
      try { fn(); results.push({ name, passed: true }); }
      catch (e) { results.push({ name, passed: false, error: String((e && e.message) || e) }); }
    },
    expect: makeExpect
  };
  if (context.response) {
    const r = context.response;
    pm.response = {
      code: r.status,
      status: r.status_text,
      responseTime: r.duration_ms,
      responseSize: r.size_bytes,
      headers: r.headers,
      json: () => JSON.parse(r.body),
      text: () => r.body,
      to: { have: { status: (code) => { if (r.status !== code) { throw new Error('expected status ' + code + ' but got ' + r.status); } } } }
    };
  }
  let error = null;
  try {
    // eslint-disable-next-line no-new-func
    const fn = new Function('pm', 'console', code);
    fn(pm, console);
  } catch (e) {
    error = String((e && e.message) || e);
  }
  parent.postMessage({ id, kind, results, vars, error, request: pm.request }, '*');
});
<\/script>`;

let iframeEl = null;
let ready = false;
let seq = 0;
const pending = new Map();

function ensureIframe() {
  if (iframeEl) return;
  iframeEl = document.createElement('iframe');
  iframeEl.sandbox = 'allow-scripts';
  iframeEl.style.display = 'none';
  iframeEl.srcdoc = HARNESS_HTML;
  document.body.appendChild(iframeEl);
  ready = true;
  window.addEventListener('message', (ev) => {
    if (!ev.data || ev.data.id == null) return;
    const resolve = pending.get(ev.data.id);
    if (resolve) {
      pending.delete(ev.data.id);
      resolve(ev.data);
    }
  });
}

/**
 * Runs `code` (a pre-request or test script) in the sandbox.
 * `context` = { environment, globals, request, response? }
 * Resolves { results: [{name, passed, error?}], vars: {environment, globals}, error, request }
 */
export function runScript(kind, code, context, timeoutMs = 5000) {
  ensureIframe();
  const id = ++seq;
  return new Promise((resolve) => {
    const timer = setTimeout(() => {
      pending.delete(id);
      resolve({ results: [], vars: { environment: context.environment, globals: context.globals }, error: 'Script timed out after 5s' });
    }, timeoutMs);
    pending.set(id, (data) => {
      clearTimeout(timer);
      resolve(data);
    });
    iframeEl.contentWindow.postMessage({ id, kind, code, context }, '*');
  });
}
