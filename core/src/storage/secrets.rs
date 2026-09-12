use anyhow::{anyhow, Context, Result};
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    XChaCha20Poly1305, XNonce,
};
use keyring::Entry;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const SERVICE: &str = "OGTestDesk";

/// App data dir for the encrypted-file fallback. Set once at startup via
/// [`SecretsStore::init_fallback`].
static FALLBACK_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Set the first time a keychain call fails for a reason other than
/// "no such entry" — from then on we stop trying the keychain at all for
/// the rest of this run. Some Linux Secret Service setups are lopsided:
/// writes to the default collection succeed silently, but a *read*
/// needs the collection unlocked and there's no prompt agent running to
/// unlock it (a bare/minimal desktop with no `gnome-keyring`/`kwallet`
/// prompter), so every read fails while writes keep quietly "working" —
/// which otherwise means every single connection produces its own
/// keychain error, repeatedly, for the whole session. Once we've seen
/// real evidence the keychain can't be trusted, commit to the file
/// fallback for both reads and writes so things are at least
/// consistent — a password set after that point is reliably readable
/// again, instead of writes and reads silently fighting over two
/// different backends.
static KEYCHAIN_BROKEN: OnceLock<()> = OnceLock::new();

/// Which backend a secret operation actually used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SecretsBackend {
    /// OS keychain (Keychain / Credential Manager / Secret Service).
    Keychain,
    /// Local `secrets/*.bin` files, XChaCha20-Poly1305 encrypted with a
    /// per-install key. Used when no OS keychain / Secret Service daemon
    /// is reachable (e.g. a bare Wayland session with no keyring agent).
    EncryptedFile,
}

/// Thin wrapper over the OS keychain with a transparent encrypted-file
/// fallback. Never put secrets in `MetadataStore` — only a
/// `connection.id` is stored there; the actual password lives here,
/// keyed by that id.
pub struct SecretsStore;

impl SecretsStore {
    /// Point the file fallback at the app data dir. Call once at startup,
    /// before any `set`/`get`/`delete`.
    pub fn init_fallback(dir: PathBuf) {
        let _ = FALLBACK_DIR.set(dir);
    }

    /// Probe which backend is currently usable. Cheap; does a throwaway
    /// keychain read.
    pub fn backend() -> SecretsBackend {
        match Entry::new(SERVICE, "__probe__").and_then(|e| e.get_password()) {
            Ok(_) | Err(keyring::Error::NoEntry) => SecretsBackend::Keychain,
            Err(_) => SecretsBackend::EncryptedFile,
        }
    }

    pub fn set(connection_id: &str, secret: &str) -> Result<()> {
        if KEYCHAIN_BROKEN.get().is_some() {
            return Self::file_set(connection_id, secret);
        }
        match Self::keychain_set(connection_id, secret) {
            Ok(()) => Ok(()),
            Err(e) => {
                let _ = KEYCHAIN_BROKEN.set(());
                Self::file_set(connection_id, secret).with_context(|| {
                    format!("OS keychain unavailable ({e}); encrypted-file fallback also failed")
                })
            }
        }
    }

    pub fn get(connection_id: &str) -> Result<Option<String>> {
        if KEYCHAIN_BROKEN.get().is_some() {
            return Self::file_get(connection_id);
        }
        match Self::keychain_get(connection_id) {
            Ok(pw) => Ok(Some(pw)),
            Err(keyring::Error::NoEntry) => Self::file_get(connection_id),
            Err(e) => {
                // A transient keychain error (the Secret Service session
                // hasn't fully come back yet right after the window
                // regains focus, e.g. after a screen lock, is a common
                // one on Linux) is NOT the same thing as "this connection
                // has no password". Retry once — most of these self-heal
                // immediately — before giving up on the keychain for the
                // rest of this run.
                if let Ok(pw) = Self::keychain_get(connection_id) {
                    return Ok(Some(pw));
                }
                let _ = KEYCHAIN_BROKEN.set(());
                match Self::file_get(connection_id) {
                    // The file fallback only ever has an entry if `set()`
                    // wrote there because the keychain was unavailable at
                    // save time. If it's empty too, this connection's
                    // password was saved to the keychain while it still
                    // worked and is now stuck there — say so plainly
                    // rather than silently returning None and letting the
                    // caller open a passwordless connection that fails
                    // downstream with a confusing "password authentication
                    // failed". Re-entering the password (Edit connection)
                    // writes it to the file store instead from now on.
                    Ok(None) => Err(e).context(
                        "OS keychain unavailable and no local fallback secret found — \
                         re-enter this connection's password (Edit connection) to fix it",
                    ),
                    other => other,
                }
            }
        }
    }

    fn keychain_get(connection_id: &str) -> std::result::Result<String, keyring::Error> {
        Entry::new(SERVICE, connection_id).and_then(|e| e.get_password())
    }

    pub fn delete(connection_id: &str) -> Result<()> {
        // Best-effort: always also try the file store regardless of
        // whether the keychain attempt worked, so a connection deleted
        // (or having its password cleared) while the keychain is
        // unreachable doesn't leave a stale file-fallback copy behind.
        if KEYCHAIN_BROKEN.get().is_none() {
            if let Ok(entry) = Entry::new(SERVICE, connection_id) {
                if let Err(e) = entry.delete_credential() {
                    if !matches!(e, keyring::Error::NoEntry) {
                        let _ = KEYCHAIN_BROKEN.set(());
                    }
                }
            }
        }
        Self::file_delete(connection_id)
    }

    // ---------------------------------------------------------- keychain

    fn keychain_set(connection_id: &str, secret: &str) -> Result<()> {
        Entry::new(SERVICE, connection_id)
            .context("opening keychain entry")?
            .set_password(secret)
            .context("writing secret to keychain")
    }

    // ------------------------------------------------------ file fallback

    fn secrets_dir() -> Result<PathBuf> {
        let d = FALLBACK_DIR
            .get()
            .ok_or_else(|| anyhow!("secrets fallback dir not initialised"))?;
        Ok(d.join("secrets"))
    }

    fn cipher(dir: &Path) -> Result<XChaCha20Poly1305> {
        let key_path = dir.join("key");
        let key = match std::fs::read(&key_path) {
            Ok(b) if b.len() == 32 => {
                let mut k = [0u8; 32];
                k.copy_from_slice(&b);
                k
            }
            _ => {
                let mut k = [0u8; 32];
                getrandom::getrandom(&mut k).map_err(|e| anyhow!("rng failure: {e}"))?;
                write_private(&key_path, &k)?;
                k
            }
        };
        Ok(XChaCha20Poly1305::new((&key).into()))
    }

    fn file_set(connection_id: &str, secret: &str) -> Result<()> {
        let dir = Self::secrets_dir()?;
        let cipher = Self::cipher(&dir)?;
        let mut nonce = [0u8; 24];
        getrandom::getrandom(&mut nonce).map_err(|e| anyhow!("rng failure: {e}"))?;
        let ct = cipher
            .encrypt(XNonce::from_slice(&nonce), secret.as_bytes())
            .map_err(|e| anyhow!("encrypt: {e}"))?;
        let mut blob = Vec::with_capacity(24 + ct.len());
        blob.extend_from_slice(&nonce);
        blob.extend_from_slice(&ct);
        write_private(&dir.join(format!("{}.bin", sanitize(connection_id))), &blob)
    }

    fn file_get(connection_id: &str) -> Result<Option<String>> {
        let dir = Self::secrets_dir()?;
        let path = dir.join(format!("{}.bin", sanitize(connection_id)));
        let blob = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e).context("reading secret file"),
        };
        if blob.len() < 24 {
            return Err(anyhow!("secret file {path:?} is corrupt"));
        }
        let cipher = Self::cipher(&dir)?;
        let pt = cipher
            .decrypt(XNonce::from_slice(&blob[..24]), &blob[24..])
            .map_err(|e| anyhow!("decrypt {path:?}: {e}"))?;
        Ok(Some(String::from_utf8(pt).context("secret is not valid UTF-8")?))
    }

    fn file_delete(connection_id: &str) -> Result<()> {
        let dir = Self::secrets_dir()?;
        match std::fs::remove_file(dir.join(format!("{}.bin", sanitize(connection_id)))) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e).context("deleting secret file"),
        }
    }
}

/// Keep the on-disk filename to a safe charset (connection ids are uuids,
/// but don't trust that).
fn sanitize(id: &str) -> String {
    id.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

fn write_private(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("creating secrets dir")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
        }
    }
    #[cfg(unix)]
    let opts = {
        use std::os::unix::fs::OpenOptionsExt;
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create(true).truncate(true).mode(0o600);
        o
    };
    #[cfg(not(unix))]
    let opts = {
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create(true).truncate(true);
        o
    };
    use std::io::Write;
    opts.open(path)
        .context("opening secret file for write")?
        .write_all(data)
        .context("writing secret file")
}
