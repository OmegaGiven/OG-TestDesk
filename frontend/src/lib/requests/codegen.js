// Code snippet generation ("Code" button in Postman) — given a fully
// resolved request (method/url/headers + whichever body shape), produces
// a copy-pasteable snippet in a handful of common languages. Raw and
// x-www-form-urlencoded bodies are fully supported in every language;
// multipart and binary bodies fall back to a comment pointing back at
// the app rather than guessing wrong — generating subtly-incorrect code
// silently would be worse than admitting the gap.

function shq(s) {
  return `'${String(s).replace(/'/g, `'\\''`)}'`;
}
function pyq(s) {
  return `'${String(s).replace(/\\/g, '\\\\').replace(/'/g, "\\'")}'`;
}
function jsq(s) {
  return JSON.stringify(String(s));
}

// Normalizes {bodyMode, body, formFields} from the app's draft shape into
// one of: {kind:'raw', text} | {kind:'form', fields:[{k,v}]} |
// {kind:'unsupported', label}
function describeBody(req) {
  const mode = req.bodyMode || 'raw';
  if (mode === 'raw') {
    return req.body ? { kind: 'raw', text: req.body } : null;
  }
  if (mode === 'urlencoded') {
    const fields = (req.formFields || []).filter((f) => f.k && f.on !== false);
    return fields.length ? { kind: 'form', fields } : null;
  }
  if (mode === 'graphql') {
    return req.graphqlQuery
      ? { kind: 'raw', text: JSON.stringify({ query: req.graphqlQuery, variables: safeParse(req.graphqlVariables) }) }
      : null;
  }
  if (mode === 'multipart') return { kind: 'unsupported', label: 'multipart/form-data' };
  if (mode === 'binary') return { kind: 'unsupported', label: 'a binary file body' };
  return null;
}
function safeParse(s) {
  try {
    return s ? JSON.parse(s) : undefined;
  } catch {
    return undefined;
  }
}

export function toCurlSnippet(req) {
  const parts = [`curl -X ${req.method} ${shq(req.url)}`];
  for (const [k, v] of Object.entries(req.headers || {})) {
    if (k.trim()) parts.push(`  -H ${shq(`${k}: ${v}`)}`);
  }
  const body = describeBody(req);
  if (body?.kind === 'raw') parts.push(`  -d ${shq(body.text)}`);
  else if (body?.kind === 'form') {
    for (const f of body.fields) parts.push(`  --data-urlencode ${shq(`${f.k}=${f.v}`)}`);
  } else if (body?.kind === 'unsupported') {
    parts.push(`  # body: ${body.label} — export not supported here, see the app's Body tab`);
  }
  return parts.join(' \\\n');
}

export function toPythonSnippet(req) {
  const lines = ['import requests', ''];
  const headers = Object.entries(req.headers || {}).filter(([k]) => k.trim());
  if (headers.length) {
    lines.push('headers = {');
    for (const [k, v] of headers) lines.push(`    ${pyq(k)}: ${pyq(v)},`);
    lines.push('}');
  }
  const body = describeBody(req);
  let dataArg = '';
  if (body?.kind === 'raw') {
    lines.push(`data = ${pyq(body.text)}`);
    dataArg = ', data=data';
  } else if (body?.kind === 'form') {
    lines.push('data = {');
    for (const f of body.fields) lines.push(`    ${pyq(f.k)}: ${pyq(f.v)},`);
    lines.push('}');
    dataArg = ', data=data';
  } else if (body?.kind === 'unsupported') {
    lines.push(`# body: ${body.label} — export not supported here, see the app's Body tab`);
  }
  lines.push('');
  lines.push(
    `response = requests.request(${pyq(req.method)}, ${pyq(req.url)}${headers.length ? ', headers=headers' : ''}${dataArg})`
  );
  lines.push('print(response.status_code)');
  lines.push('print(response.text)');
  return lines.join('\n');
}

export function toFetchSnippet(req) {
  const lines = [];
  const headers = Object.entries(req.headers || {}).filter(([k]) => k.trim());
  const body = describeBody(req);
  const opts = [`method: ${jsq(req.method)}`];
  if (headers.length) {
    opts.push(`headers: {\n${headers.map(([k, v]) => `    ${jsq(k)}: ${jsq(v)}`).join(',\n')}\n  }`);
  }
  if (body?.kind === 'raw') {
    opts.push(`body: ${jsq(body.text)}`);
  } else if (body?.kind === 'form') {
    lines.push('const params = new URLSearchParams();');
    for (const f of body.fields) lines.push(`params.append(${jsq(f.k)}, ${jsq(f.v)});`);
    lines.push('');
    opts.push('body: params');
  } else if (body?.kind === 'unsupported') {
    lines.push(`// body: ${body.label} — export not supported here, see the app's Body tab`);
  }
  lines.push(`fetch(${jsq(req.url)}, {\n  ${opts.join(',\n  ')}\n})`);
  lines.push('  .then((res) => res.text())');
  lines.push('  .then(console.log);');
  return lines.join('\n');
}

export function toNodeAxiosSnippet(req) {
  const lines = ["const axios = require('axios');", ''];
  const headers = Object.entries(req.headers || {}).filter(([k]) => k.trim());
  const body = describeBody(req);
  const config = [`method: ${jsq(req.method)}`, `url: ${jsq(req.url)}`];
  if (headers.length) {
    config.push(`headers: {\n    ${headers.map(([k, v]) => `${jsq(k)}: ${jsq(v)}`).join(',\n    ')}\n  }`);
  }
  if (body?.kind === 'raw') {
    config.push(`data: ${jsq(body.text)}`);
  } else if (body?.kind === 'form') {
    lines.push('const qs = require(\'querystring\');');
    config.push(
      `data: qs.stringify({\n    ${body.fields.map((f) => `${jsq(f.k)}: ${jsq(f.v)}`).join(',\n    ')}\n  })`
    );
  } else if (body?.kind === 'unsupported') {
    lines.push(`// body: ${body.label} — export not supported here, see the app's Body tab`);
  }
  lines.push(`axios({\n  ${config.join(',\n  ')}\n})`);
  lines.push('  .then((res) => console.log(res.status, res.data))');
  lines.push('  .catch((err) => console.error(err));');
  return lines.join('\n');
}

export const CODE_GENERATORS = {
  curl: { label: 'cURL', generate: toCurlSnippet },
  python: { label: 'Python (requests)', generate: toPythonSnippet },
  fetch: { label: 'JavaScript (fetch)', generate: toFetchSnippet },
  node: { label: 'Node.js (axios)', generate: toNodeAxiosSnippet }
};
