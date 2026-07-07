use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::Value;

pub struct DecodedJwt {
    pub header: Value,
    pub payload: Value,
    pub header_pretty: String,
    pub payload_pretty: String,
    pub signature_b64: String,
}

/// Decodes a JWT's header and payload (base64url + JSON), without verifying
/// the signature. Returns `Err` with a human-readable reason for malformed
/// tokens.
pub fn decode(token: &str) -> Result<DecodedJwt, String> {
    let token = token.trim();
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err("Token must have three dot-separated segments (header.payload.signature).".to_string());
    }

    let header = decode_segment(parts[0], "header")?;
    let payload = decode_segment(parts[1], "payload")?;
    let header_pretty = serde_json::to_string_pretty(&header).unwrap_or_default();
    let payload_pretty = serde_json::to_string_pretty(&payload).unwrap_or_default();

    Ok(DecodedJwt {
        header,
        payload,
        header_pretty,
        payload_pretty,
        signature_b64: parts[2].to_string(),
    })
}

fn decode_segment(segment: &str, label: &str) -> Result<Value, String> {
    let bytes = URL_SAFE_NO_PAD
        .decode(segment)
        .map_err(|err| format!("Failed to base64url-decode {label}: {err}"))?;
    serde_json::from_slice(&bytes).map_err(|err| format!("{label} was not valid JSON: {err}"))
}
