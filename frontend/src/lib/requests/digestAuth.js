// HTTP Digest auth (RFC 2617 — still the version essentially everything
// implements in practice; RFC 7616's SHA-256 variant is rare in the
// wild). Needs MD5, which Web Crypto doesn't provide (SubtleCrypto only
// does the SHA family), so this bundles a small public-domain MD5
// implementation rather than pulling in a dependency for one function.
// Unlike Bearer/Basic, a Digest response can't be precomputed — it's
// only knowable after the server's 401 challenge — so this doesn't fit
// the reactive auth-header composer; the caller does the two-request
// dance (send once, see the 401, ask this module for the header, resend).

// --- MD5 (public domain, adapted from the widely-used compact JS
// implementation by Joseph Myers / danschumann) -----------------------
function md5cycle(x, k) {
  let [a, b, c, d] = x;
  const ff = (a, b, c, d, x, s, t) => {
    a = (a + ((b & c) | (~b & d)) + x + t) | 0;
    return ((a << s) | (a >>> (32 - s))) + b | 0;
  };
  const gg = (a, b, c, d, x, s, t) => {
    a = (a + ((b & d) | (c & ~d)) + x + t) | 0;
    return ((a << s) | (a >>> (32 - s))) + b | 0;
  };
  const hh = (a, b, c, d, x, s, t) => {
    a = (a + (b ^ c ^ d) + x + t) | 0;
    return ((a << s) | (a >>> (32 - s))) + b | 0;
  };
  const ii = (a, b, c, d, x, s, t) => {
    a = (a + (c ^ (b | ~d)) + x + t) | 0;
    return ((a << s) | (a >>> (32 - s))) + b | 0;
  };
  a = ff(a, b, c, d, k[0], 7, -680876936); d = ff(d, a, b, c, k[1], 12, -389564586);
  c = ff(c, d, a, b, k[2], 17, 606105819); b = ff(b, c, d, a, k[3], 22, -1044525330);
  a = ff(a, b, c, d, k[4], 7, -176418897); d = ff(d, a, b, c, k[5], 12, 1200080426);
  c = ff(c, d, a, b, k[6], 17, -1473231341); b = ff(b, c, d, a, k[7], 22, -45705983);
  a = ff(a, b, c, d, k[8], 7, 1770035416); d = ff(d, a, b, c, k[9], 12, -1958414417);
  c = ff(c, d, a, b, k[10], 17, -42063); b = ff(b, c, d, a, k[11], 22, -1990404162);
  a = ff(a, b, c, d, k[12], 7, 1804603682); d = ff(d, a, b, c, k[13], 12, -40341101);
  c = ff(c, d, a, b, k[14], 17, -1502002290); b = ff(b, c, d, a, k[15], 22, 1236535329);
  a = gg(a, b, c, d, k[1], 5, -165796510); d = gg(d, a, b, c, k[6], 9, -1069501632);
  c = gg(c, d, a, b, k[11], 14, 643717713); b = gg(b, c, d, a, k[0], 20, -373897302);
  a = gg(a, b, c, d, k[5], 5, -701558691); d = gg(d, a, b, c, k[10], 9, 38016083);
  c = gg(c, d, a, b, k[15], 14, -660478335); b = gg(b, c, d, a, k[4], 20, -405537848);
  a = gg(a, b, c, d, k[9], 5, 568446438); d = gg(d, a, b, c, k[14], 9, -1019803690);
  c = gg(c, d, a, b, k[3], 14, -187363961); b = gg(b, c, d, a, k[8], 20, 1163531501);
  a = gg(a, b, c, d, k[13], 5, -1444681467); d = gg(d, a, b, c, k[2], 9, -51403784);
  c = gg(c, d, a, b, k[7], 14, 1735328473); b = gg(b, c, d, a, k[12], 20, -1926607734);
  a = hh(a, b, c, d, k[5], 4, -378558); d = hh(d, a, b, c, k[8], 11, -2022574463);
  c = hh(c, d, a, b, k[11], 16, 1839030562); b = hh(b, c, d, a, k[14], 23, -35309556);
  a = hh(a, b, c, d, k[1], 4, -1530992060); d = hh(d, a, b, c, k[4], 11, 1272893353);
  c = hh(c, d, a, b, k[7], 16, -155497632); b = hh(b, c, d, a, k[10], 23, -1094730640);
  a = hh(a, b, c, d, k[13], 4, 681279174); d = hh(d, a, b, c, k[0], 11, -358537222);
  c = hh(c, d, a, b, k[3], 16, -722521979); b = hh(b, c, d, a, k[6], 23, 76029189);
  a = hh(a, b, c, d, k[9], 4, -640364487); d = hh(d, a, b, c, k[12], 11, -421815835);
  c = hh(c, d, a, b, k[15], 16, 530742520); b = hh(b, c, d, a, k[2], 23, -995338651);
  a = ii(a, b, c, d, k[0], 6, -198630844); d = ii(d, a, b, c, k[7], 10, 1126891415);
  c = ii(c, d, a, b, k[14], 15, -1416354905); b = ii(b, c, d, a, k[5], 21, -57434055);
  a = ii(a, b, c, d, k[12], 6, 1700485571); d = ii(d, a, b, c, k[3], 10, -1894986606);
  c = ii(c, d, a, b, k[10], 15, -1051523); b = ii(b, c, d, a, k[1], 21, -2054922799);
  a = ii(a, b, c, d, k[8], 6, 1873313359); d = ii(d, a, b, c, k[15], 10, -30611744);
  c = ii(c, d, a, b, k[6], 15, -1560198380); b = ii(b, c, d, a, k[13], 21, 1309151649);
  a = ii(a, b, c, d, k[4], 6, -145523070); d = ii(d, a, b, c, k[11], 10, -1120210379);
  c = ii(c, d, a, b, k[2], 15, 718787259); b = ii(b, c, d, a, k[9], 21, -343485551);
  x[0] = (x[0] + a) | 0; x[1] = (x[1] + b) | 0; x[2] = (x[2] + c) | 0; x[3] = (x[3] + d) | 0;
}
function md51(s) {
  const n = s.length;
  const state = [1732584193, -271733879, -1732584194, 271733878];
  let i;
  for (i = 64; i <= s.length; i += 64) md5cycle(state, md5blk(s.substring(i - 64, i)));
  s = s.substring(i - 64);
  const tail = new Array(16).fill(0);
  for (i = 0; i < s.length; i++) tail[i >> 2] |= s.charCodeAt(i) << ((i % 4) << 3);
  tail[i >> 2] |= 0x80 << ((i % 4) << 3);
  if (i > 55) {
    md5cycle(state, tail);
    tail.fill(0);
  }
  tail[14] = n * 8;
  md5cycle(state, tail);
  return state;
}
function md5blk(s) {
  const md5blks = [];
  for (let i = 0; i < 64; i += 4) {
    md5blks[i >> 2] =
      s.charCodeAt(i) + (s.charCodeAt(i + 1) << 8) + (s.charCodeAt(i + 2) << 16) + (s.charCodeAt(i + 3) << 24);
  }
  return md5blks;
}
function rhex(n) {
  const hexChr = '0123456789abcdef';
  let s = '';
  for (let j = 0; j < 4; j++) s += hexChr[(n >> (j * 8 + 4)) & 0x0f] + hexChr[(n >> (j * 8)) & 0x0f];
  return s;
}
function toUtf8Binary(s) {
  return unescape(encodeURIComponent(s));
}
export function md5(s) {
  return md51(toUtf8Binary(s)).map(rhex).join('');
}

/** Parses a `WWW-Authenticate: Digest ...` header into its directives. */
export function parseDigestChallenge(headerValue) {
  const m = headerValue.match(/^Digest\s+(.*)$/i);
  if (!m) return null;
  const params = {};
  const re = /(\w+)=(?:"([^"]*)"|([^,\s]+))/g;
  let match;
  while ((match = re.exec(m[1]))) {
    params[match[1]] = match[2] !== undefined ? match[2] : match[3];
  }
  return params;
}

/**
 * Builds the `Authorization: Digest ...` header for one request, given
 * the parsed challenge from the server's 401.
 */
export function buildDigestHeader({ username, password, method, uri, challenge }) {
  const realm = challenge.realm || '';
  const nonce = challenge.nonce || '';
  const qop = (challenge.qop || '').split(',')[0].trim() || null;
  const opaque = challenge.opaque;
  const nc = '00000001';
  const cnonce = Array.from(crypto.getRandomValues(new Uint8Array(8)))
    .map((b) => b.toString(16).padStart(2, '0'))
    .join('');

  const ha1 = md5(`${username}:${realm}:${password}`);
  const ha2 = md5(`${method.toUpperCase()}:${uri}`);
  const response = qop
    ? md5(`${ha1}:${nonce}:${nc}:${cnonce}:${qop}:${ha2}`)
    : md5(`${ha1}:${nonce}:${ha2}`);

  const parts = [
    `username="${username}"`,
    `realm="${realm}"`,
    `nonce="${nonce}"`,
    `uri="${uri}"`,
    `response="${response}"`
  ];
  if (qop) {
    parts.push(`qop=${qop}`, `nc=${nc}`, `cnonce="${cnonce}"`);
  }
  if (opaque) parts.push(`opaque="${opaque}"`);
  return 'Digest ' + parts.join(', ');
}
