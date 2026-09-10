// Shared export helpers for SQL results and HTTP responses.

export function downloadText(filename, text, mime = 'text/plain') {
  try {
    const a = document.createElement('a');
    a.href = URL.createObjectURL(new Blob([text], { type: mime }));
    a.download = filename;
    document.body.appendChild(a);
    a.click();
    a.remove();
    setTimeout(() => URL.revokeObjectURL(a.href), 2000);
  } catch (e) {
    // webview blocked the download — fall back to clipboard
    copyText(text);
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

/** cols: [{name}], rows: any[][]. delim ',' or '\t'. */
export function rowsToDelimited(cols, rows, delim = ',') {
  const esc = (s) => {
    s = cell(s);
    if (delim === ',' && /[",\n\r]/.test(s)) return `"${s.replace(/"/g, '""')}"`;
    if (delim === '\t') return s.replace(/[\t\n\r]/g, ' ');
    return s;
  };
  const head = cols.map((c) => esc(c.name)).join(delim);
  const body = rows.map((r) => r.map(esc).join(delim)).join('\n');
  return body ? `${head}\n${body}` : head;
}

/** Array of {colName: value} objects. */
export function rowsToObjects(cols, rows) {
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
