// Decodes a JWT's header + payload for inspection. Never verifies the
// signature (no key material available client-side, and that's not the
// point here) — this is a read tool, not an auth check.

function base64UrlDecode(seg) {
  const padded = seg.replace(/-/g, '+').replace(/_/g, '/').padEnd(seg.length + ((4 - (seg.length % 4)) % 4), '=');
  const binary = atob(padded);
  const bytes = Uint8Array.from(binary, (c) => c.charCodeAt(0));
  return new TextDecoder('utf-8').decode(bytes);
}

// Common JWT claims that are Unix timestamps (seconds) — surfaced as a
// human-readable ISO string alongside the raw number, since "1793058000"
// on its own tells you nothing at a glance.
const TIME_CLAIMS = ['exp', 'iat', 'nbf'];

/**
 * @returns {{ header: object, payload: object, signature: string, expired: boolean|null }}
 * @throws if the token isn't a decodable 3-segment JWT
 */
export function decodeJwt(token) {
  const parts = (token || '').trim().split('.');
  if (parts.length !== 3) throw new Error('Not a JWT — expected 3 dot-separated segments (header.payload.signature)');
  const [headerSeg, payloadSeg, signatureSeg] = parts;

  let header, payload;
  try {
    header = JSON.parse(base64UrlDecode(headerSeg));
  } catch (e) {
    throw new Error('Could not decode header: ' + e.message);
  }
  try {
    payload = JSON.parse(base64UrlDecode(payloadSeg));
  } catch (e) {
    throw new Error('Could not decode payload: ' + e.message);
  }

  const readablePayload = { ...payload };
  for (const claim of TIME_CLAIMS) {
    if (typeof payload[claim] === 'number') {
      readablePayload[`${claim}_readable`] = new Date(payload[claim] * 1000).toISOString();
    }
  }

  const expired = typeof payload.exp === 'number' ? Date.now() / 1000 > payload.exp : null;

  return { header, payload: readablePayload, signature: signatureSeg, expired };
}
