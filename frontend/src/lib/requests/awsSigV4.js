// AWS Signature Version 4 request signing (https://docs.aws.amazon.com/general/latest/gr/sigv4-signing-process.html).
// Pure Web Crypto (SubtleCrypto) — no dependency, works in the Tauri
// webview same as any browser. Computed fresh per-request (unlike Bearer/
// Basic, the signature depends on the exact method/url/headers/body being
// sent right now, including a timestamp), so this is called from send()
// itself rather than the reactive header-composer used for the simpler
// auth types.

async function sha256Hex(data) {
  const bytes = typeof data === 'string' ? new TextEncoder().encode(data) : data;
  const hash = await crypto.subtle.digest('SHA-256', bytes);
  return [...new Uint8Array(hash)].map((b) => b.toString(16).padStart(2, '0')).join('');
}

async function hmac(key, data) {
  const cryptoKey = await crypto.subtle.importKey(
    'raw',
    typeof key === 'string' ? new TextEncoder().encode(key) : key,
    { name: 'HMAC', hash: 'SHA-256' },
    false,
    ['sign']
  );
  return new Uint8Array(await crypto.subtle.sign('HMAC', cryptoKey, new TextEncoder().encode(data)));
}

function toHex(bytes) {
  return [...bytes].map((b) => b.toString(16).padStart(2, '0')).join('');
}

function amzDate(d) {
  const iso = d.toISOString().replace(/[:-]|\.\d{3}/g, '');
  return { amzDate: iso, dateStamp: iso.slice(0, 8) };
}

// Encodes a path segment the way AWS's canonical URI wants: RFC 3986
// unreserved chars untouched, everything else percent-encoded, `/`
// preserved as a path separator.
function encodeUriSegment(s) {
  return encodeURIComponent(s).replace(/[!'()*]/g, (c) => '%' + c.charCodeAt(0).toString(16).toUpperCase());
}
function canonicalUri(pathname) {
  if (!pathname) return '/';
  return pathname.split('/').map(encodeUriSegment).join('/');
}
function canonicalQuery(search) {
  const params = new URLSearchParams(search);
  const pairs = [...params.entries()].map(([k, v]) => [encodeUriSegment(k), encodeUriSegment(v)]);
  pairs.sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : a[1] < b[1] ? -1 : a[1] > b[1] ? 1 : 0));
  return pairs.map(([k, v]) => `${k}=${v}`).join('&');
}

/**
 * Returns the extra headers to add so the request is AWS SigV4-signed.
 * `req` = { method, url, headers (plain object), body (string|null) }
 * `creds` = { accessKeyId, secretAccessKey, sessionToken?, region, service }
 */
export async function signAwsV4(req, creds) {
  const u = new URL(req.url);
  const { amzDate: amz, dateStamp } = amzDate(new Date());
  const payloadHash = await sha256Hex(req.body || '');

  const headerEntries = [
    ['host', u.host],
    ['x-amz-date', amz],
    ['x-amz-content-sha256', payloadHash]
  ];
  if (creds.sessionToken) headerEntries.push(['x-amz-security-token', creds.sessionToken]);
  const existingContentType = Object.entries(req.headers || {}).find(([k]) => k.toLowerCase() === 'content-type');
  if (existingContentType) headerEntries.push(['content-type', existingContentType[1]]);

  headerEntries.sort((a, b) => (a[0] < b[0] ? -1 : 1));
  const canonicalHeaders = headerEntries.map(([k, v]) => `${k}:${v.trim()}\n`).join('');
  const signedHeaders = headerEntries.map(([k]) => k).join(';');

  const canonicalRequest = [
    req.method.toUpperCase(),
    canonicalUri(u.pathname),
    canonicalQuery(u.search),
    canonicalHeaders,
    signedHeaders,
    payloadHash
  ].join('\n');

  const scope = `${dateStamp}/${creds.region}/${creds.service}/aws4_request`;
  const stringToSign = ['AWS4-HMAC-SHA256', amz, scope, await sha256Hex(canonicalRequest)].join('\n');

  const kDate = await hmac('AWS4' + creds.secretAccessKey, dateStamp);
  const kRegion = await hmac(kDate, creds.region);
  const kService = await hmac(kRegion, creds.service);
  const kSigning = await hmac(kService, 'aws4_request');
  const signature = toHex(await hmac(kSigning, stringToSign));

  const authorization = `AWS4-HMAC-SHA256 Credential=${creds.accessKeyId}/${scope}, SignedHeaders=${signedHeaders}, Signature=${signature}`;

  const extraHeaders = {
    'X-Amz-Date': amz,
    'X-Amz-Content-Sha256': payloadHash,
    Authorization: authorization
  };
  if (creds.sessionToken) extraHeaders['X-Amz-Security-Token'] = creds.sessionToken;
  return extraHeaders;
}
