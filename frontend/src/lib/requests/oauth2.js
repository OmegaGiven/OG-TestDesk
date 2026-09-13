// PKCE helpers for OAuth2 Authorization Code — pure functions (no Tauri/
// network calls) so they're independently testable. The actual flow
// (opening the browser, waiting on the loopback listener, exchanging the
// code) lives in RequestsView.svelte since it's mostly glue around Tauri
// commands.

function base64url(bytes) {
  let bin = '';
  bytes.forEach((b) => (bin += String.fromCharCode(b)));
  return btoa(bin).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

export function randomToken(byteLen = 32) {
  return base64url(crypto.getRandomValues(new Uint8Array(byteLen)));
}

/** RFC 7636 S256 code_challenge from a code_verifier. */
export async function pkceChallengeFromVerifier(verifier) {
  const hash = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(verifier));
  return base64url(new Uint8Array(hash));
}
