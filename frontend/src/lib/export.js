// Shared export helpers for SQL results and HTTP responses.

// Splits a "name.ext" into parts save()'s filters expect.
function splitExt(filename) {
  const i = filename.lastIndexOf('.');
  return i > 0 ? [filename.slice(0, i), filename.slice(i + 1)] : [filename, ''];
}

/** Save `text` to a file the user picks. Native OS save panel inside the
 * real app (works correctly under macOS App Sandbox with only
 * files.user-selected.read-write — no broader Downloads entitlement
 * needed); a plain browser blob-download when previewed outside Tauri
 * (mock IPC / headless screenshotting), since the native plugin isn't
 * available there. Returns true if the file was actually written,
 * false if the user cancelled or it failed (silent to the caller
 * either way is fine for a Save action; callers that want a toast on
 * success should check the return value). */
export async function downloadText(filename, text, mime = 'text/plain') {
  if (!window.__OGTD_MOCK__) {
    try {
      const { save } = await import('@tauri-apps/plugin-dialog');
      const { writeTextFile } = await import('@tauri-apps/plugin-fs');
      const [base, ext] = splitExt(filename);
      const path = await save({
        defaultPath: filename,
        filters: ext ? [{ name: ext.toUpperCase(), extensions: [ext] }] : undefined
      });
      if (!path) return false; // user cancelled
      await writeTextFile(path, text);
      return true;
    } catch (e) {
      // Native path unavailable for some reason — fall through to the
      // browser download below rather than silently doing nothing.
    }
  }
  try {
    const a = document.createElement('a');
    a.href = URL.createObjectURL(new Blob([text], { type: mime }));
    a.download = filename;
    document.body.appendChild(a);
    a.click();
    a.remove();
    setTimeout(() => URL.revokeObjectURL(a.href), 2000);
    return true;
  } catch (e) {
    // webview blocked the download — fall back to clipboard
    copyText(text);
    return false;
  }
}

export async function copyText(text) {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}

function cell(v) {
  if (v === null || v === undefined) return '';
  return typeof v === 'object' ? JSON.stringify(v) : String(v);
}

/** cols: [{name}], rows: any[][]. delim ',' or '\t'. withHeaders=false
 * omits the header row entirely (Postico-style export option). */
export function rowsToDelimited(cols, rows, delim = ',', withHeaders = true) {
  const esc = (s) => {
    s = cell(s);
    if (delim === ',' && /[",\n\r]/.test(s)) return `"${s.replace(/"/g, '""')}"`;
    if (delim === '\t') return s.replace(/[\t\n\r]/g, ' ');
    return s;
  };
  const body = rows.map((r) => r.map(esc).join(delim)).join('\n');
  if (!withHeaders) return body;
  const head = cols.map((c) => esc(c.name)).join(delim);
  return body ? `${head}\n${body}` : head;
}

/** withHeaders=true (default): array of {colName: value} objects.
 * withHeaders=false: plain array of value-arrays, no field names —
 * the JSON equivalent of a headerless CSV. */
export function rowsToObjects(cols, rows, withHeaders = true) {
  if (!withHeaders) return rows;
  return rows.map((r) => Object.fromEntries(cols.map((c, i) => [c.name, r[i]])));
}

/** Rebuild a curl command from a request draft. */
export function toCurl(method, url, headers, body) {
  const parts = [`curl -X ${method} ${shq(url)}`];
  for (const [k, v] of Object.entries(headers || {})) {
    if (k.trim()) parts.push(`  -H ${shq(`${k}: ${v}`)}`);
  }
  if (body && !['GET', 'HEAD'].includes(method)) parts.push(`  -d ${shq(body)}`);
  return parts.join(' \\\n');
}

function shq(s) {
  return `'${String(s).replace(/'/g, `'\\''`)}'`;
}
