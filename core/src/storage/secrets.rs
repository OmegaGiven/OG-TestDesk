use anyhow::{Context, Result};
use keyring::Entry;

const SERVICE: &str = "OGTestDesk";

/// Thin wrapper over the OS keychain (Keychain on macOS, Credential
/// Manager on Windows, Secret Service on Linux). Never put secrets in
/// `MetadataStore` — only a `connection.id` is stored there; the
/// actual password lives here, keyed by that id.
pub struct SecretsStore;

impl SecretsStore {
    pub fn set(connection_id: &str, secret: &str) -> Result<()> {
        Entry::new(SERVICE, connection_id)
            .context("opening keychain entry")?
            .set_password(secret)
            .context("writing secret to keychain")
    }

    pub fn get(connection_id: &str) -> Result<Option<String>> {
        let entry = Entry::new(SERVICE, connection_id).context("opening keychain entry")?;
        match entry.get_password() {
            Ok(pw) => Ok(Some(pw)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e).context("reading secret from keychain"),
        }
    }

    pub fn delete(connection_id: &str) -> Result<()> {
        let entry = Entry::new(SERVICE, connection_id).context("opening keychain entry")?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e).context("deleting secret from keychain"),
        }
    }
}
