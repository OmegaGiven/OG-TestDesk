//! SSH tunnels — a pure-Rust client (`russh`), not a shelled-out `ssh`
//! process. The old implementation spawned the system `ssh` binary, which
//! is exactly the kind of arbitrary-process-exec that macOS App Sandbox
//! (required for Mac App Store distribution) forbids; this reimplements
//! the same behavior — key or ssh-agent auth, known_hosts TOFU matching
//! `StrictHostKeyChecking=accept-new`, one SSH session multiplexing many
//! local connections like `ssh -L` does — without ever spawning a process.
//!
//! Also handles pre-connect shell commands (a configured command whose
//! stdout becomes the password for that connect, for IAM-style short-lived
//! tokens) — that one's a deliberate, user-configured shell invocation
//! (the same shell access the human already has), unrelated to this file's
//! sandboxing concern, and stays as a plain `Command`.

use super::ConnConfig;
use anyhow::{anyhow, bail, Context, Result};
use russh::client;
use russh::{ChannelMsg, Disconnect};
use russh_keys::agent::client::AgentClient;
use russh_keys::key::PublicKey;
use russh_keys::{check_known_hosts_path, learn_known_hosts_path, load_secret_key};
use std::collections::HashMap;
use std::net::TcpListener as StdTcpListener;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::process::Command;
use tokio::sync::oneshot;

struct TunnelHandle {
    local_port: u16,
    stop_tx: oneshot::Sender<()>,
}

static TUNNELS: Mutex<Option<HashMap<String, TunnelHandle>>> = Mutex::new(None);

fn pick_free_port() -> Result<u16> {
    let l = StdTcpListener::bind(("127.0.0.1", 0)).context("couldn't allocate a local port")?;
    Ok(l.local_addr()?.port())
}

fn known_hosts_path() -> Result<PathBuf> {
    Ok(dirs::home_dir()
        .ok_or_else(|| anyhow!("no home directory"))?
        .join(".ssh")
        .join("known_hosts"))
}

/// TOFU host-key check, matching the old `StrictHostKeyChecking=accept-new`:
/// a key that matches an existing known_hosts entry is trusted; a host with
/// no entry at all gets its key recorded and trusted; a host whose recorded
/// key doesn't match (a real MITM signal) is refused.
struct Client {
    host: String,
    port: u16,
}

#[async_trait::async_trait]
impl client::Handler for Client {
    type Error = russh::Error;

    async fn check_server_key(&mut self, pk: &PublicKey) -> Result<bool, Self::Error> {
        let Ok(path) = known_hosts_path() else {
            return Ok(false);
        };
        match check_known_hosts_path(&self.host, self.port, pk, &path) {
            Ok(true) => Ok(true),
            Ok(false) => {
                // No entry yet — learn it (accept-new) and trust it now.
                let _ = learn_known_hosts_path(&self.host, self.port, pk, &path);
                Ok(true)
            }
            // Err(KeyChanged { .. }) or a parse error — never silently
            // downgrade this to a trust decision.
            Err(_) => Ok(false),
        }
    }
}

/// Tries the configured key path, then the usual default key files, then
/// falls back to ssh-agent — same order a plain `ssh` invocation would
/// effectively land on for a host with no `~/.ssh/config` entry.
async fn authenticate(
    session: &mut client::Handle<Client>,
    user: &str,
    key_path: Option<&str>,
) -> Result<()> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(k) = key_path {
        candidates.push(PathBuf::from(k));
    } else if let Some(home) = dirs::home_dir() {
        for name in ["id_ed25519", "id_ecdsa", "id_rsa"] {
            candidates.push(home.join(".ssh").join(name));
        }
    }

    for path in &candidates {
        if !path.exists() {
            continue;
        }
        let Ok(key_pair) = load_secret_key(path, None) else {
            // Wrong/no passphrase, unsupported format, etc. — try the next
            // candidate instead of failing the whole connect outright.
            continue;
        };
        if let Ok(true) = session.authenticate_publickey(user, Arc::new(key_pair)).await {
            return Ok(());
        }
    }

    // ssh-agent fallback — covers passphrase-protected keys transparently,
    // same as a plain `ssh` invocation would.
    if let Ok(mut agent) = AgentClient::connect_env().await {
        if let Ok(identities) = agent.request_identities().await {
            for pubkey in identities {
                let (returned_agent, res) = session.authenticate_future(user, pubkey, agent).await;
                agent = returned_agent;
                if matches!(res, Ok(true)) {
                    return Ok(());
                }
            }
        }
    }

    bail!(
        "no SSH key worked for {user} — checked {} key file(s) and ssh-agent",
        candidates.len()
    );
}

/// Ensures a live SSH tunnel exists for this connection, starting one if
/// needed, and returns the local port to connect through in place of the
/// real host/port. Reuses an already-running tunnel for the same
/// connection id so the pool cache (keyed by connection URL) doesn't
/// accumulate a new entry on every call.
pub async fn ensure_tunnel(cfg: &ConnConfig) -> Result<u16> {
    let ssh_host = cfg.ssh_host.as_deref().filter(|h| !h.is_empty());
    let Some(ssh_host) = ssh_host else {
        bail!("ensure_tunnel called without ssh_host set");
    };
    let real_host = cfg.host.as_deref().unwrap_or("localhost").to_string();
    let real_port = cfg.port.unwrap_or(5432);

    {
        let mut guard = TUNNELS.lock().unwrap();
        let map = guard.get_or_insert_with(HashMap::new);
        if let Some(handle) = map.get(&cfg.id) {
            if !handle.stop_tx.is_closed() {
                return Ok(handle.local_port);
            }
            map.remove(&cfg.id); // the tunnel task died — fall through and restart it
        }
    }

    let local_port = pick_free_port()?;
    let ssh_port = cfg.ssh_port.unwrap_or(22);
    let ssh_user = cfg.ssh_user.as_deref().unwrap_or("root").to_string();
    let ssh_key_path = cfg.ssh_key_path.clone();
    let ssh_host_owned = ssh_host.to_string();

    let config = Arc::new(client::Config::default());
    let handler = Client {
        host: ssh_host_owned.clone(),
        port: ssh_port,
    };
    let mut session = client::connect(config, (ssh_host_owned.as_str(), ssh_port), handler)
        .await
        .with_context(|| format!("couldn't connect to SSH host {ssh_host_owned}:{ssh_port}"))?;
    authenticate(&mut session, &ssh_user, ssh_key_path.as_deref()).await?;

    let listener = TcpListener::bind(("127.0.0.1", local_port))
        .await
        .context("couldn't bind local tunnel port")?;
    let (stop_tx, mut stop_rx) = oneshot::channel::<()>();

    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = &mut stop_rx => break,
                accepted = listener.accept() => {
                    let Ok((stream, peer)) = accepted else { continue };
                    let channel = session
                        .channel_open_direct_tcpip(
                            real_host.clone(),
                            real_port as u32,
                            peer.ip().to_string(),
                            peer.port() as u32,
                        )
                        .await;
                    let Ok(channel) = channel else { continue };
                    tokio::spawn(pump(stream, channel));
                }
            }
        }
        let _ = session.disconnect(Disconnect::ByApplication, "", "English").await;
    });

    let mut guard = TUNNELS.lock().unwrap();
    guard
        .get_or_insert_with(HashMap::new)
        .insert(cfg.id.clone(), TunnelHandle { local_port, stop_tx });
    Ok(local_port)
}

/// Bidirectionally pumps bytes between a locally-accepted TCP connection
/// and its SSH direct-tcpip channel, same shape as the upstream `russh`
/// port-forwarding example.
async fn pump(mut stream: tokio::net::TcpStream, mut channel: russh::Channel<client::Msg>) {
    let mut buf = vec![0u8; 65536];
    let mut stream_closed = false;
    loop {
        tokio::select! {
            r = stream.read(&mut buf), if !stream_closed => {
                match r {
                    Ok(0) => {
                        stream_closed = true;
                        if channel.eof().await.is_err() { break; }
                    }
                    Ok(n) => {
                        if channel.data(&buf[..n]).await.is_err() { break; }
                    }
                    Err(_) => break,
                }
            }
            msg = channel.wait() => {
                match msg {
                    Some(ChannelMsg::Data { data }) => {
                        if stream.write_all(&data).await.is_err() { break; }
                    }
                    Some(ChannelMsg::Eof) | None => break,
                    _ => {}
                }
            }
        }
    }
}

/// Stops a connection's tunnel, if any — call when a connection is
/// deleted or its SSH settings change, so a stale tunnel isn't left
/// running (or a new one silently keeps using an old port assignment).
pub fn stop_tunnel(conn_id: &str) {
    if let Some(map) = TUNNELS.lock().unwrap().as_mut() {
        if let Some(handle) = map.remove(conn_id) {
            let _ = handle.stop_tx.send(());
        }
    }
}

/// Resolves the host/port a driver should actually dial: the real
/// host/port normally, or `127.0.0.1`/the tunnel's local port when the
/// connection has an SSH tunnel configured.
pub async fn effective_host_port(cfg: &ConnConfig) -> Result<(String, u16)> {
    if cfg.ssh_host.as_deref().filter(|h| !h.is_empty()).is_some() {
        let port = ensure_tunnel(cfg).await?;
        Ok(("127.0.0.1".to_string(), port))
    } else {
        Ok((
            cfg.host.clone().unwrap_or_else(|| "localhost".to_string()),
            cfg.port.unwrap_or(0),
        ))
    }
}

/// Runs `cfg.pre_connect_cmd` (if set) and returns its trimmed stdout as
/// the password to use, overriding `stored`. Times out after 15s so a
/// misbehaving command can't hang a connect indefinitely.
///
/// This is a deliberate, user-configured shell command (the human's own
/// shell access, for IAM-style short-lived credential fetches) — not the
/// same sandboxing concern as the SSH tunnel above, and stays as a plain
/// `Command`. It's still incompatible with App Sandbox, same as the old
/// SSH exec was; the Mac App Store build disables this feature (see
/// docs/next-steps.md) rather than trying to sandbox arbitrary shell exec.
pub async fn resolve_password(cfg: &ConnConfig, stored: Option<&str>) -> Result<Option<String>> {
    let Some(cmd_text) = cfg.pre_connect_cmd.as_deref().filter(|c| !c.trim().is_empty()) else {
        return Ok(stored.map(|s| s.to_string()));
    };
    let fut = async {
        #[cfg(windows)]
        let out = Command::new("cmd").arg("/C").arg(cmd_text).output().await?;
        #[cfg(not(windows))]
        let out = Command::new("sh").arg("-c").arg(cmd_text).stdin(Stdio::null()).output().await?;
        anyhow::Ok(out)
    };
    let out = tokio::time::timeout(Duration::from_secs(15), fut)
        .await
        .context("pre-connect command timed out after 15s")??;
    if !out.status.success() {
        bail!(
            "pre-connect command exited with {}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    let token = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if token.is_empty() {
        bail!("pre-connect command produced no output");
    }
    Ok(Some(token))
}

/// Convenience wrapper for the common call-site shape: look the stored
/// secret up, then let `resolve_password` decide whether a pre-connect
/// command overrides it. Replaces a bare `SecretsStore::get(&cfg.id)`
/// wherever a connection is about to be dialed.
pub async fn resolve_password_for(cfg: &ConnConfig) -> Result<Option<String>> {
    let stored = crate::storage::secrets::SecretsStore::get(&cfg.id)?;
    resolve_password(cfg, stored.as_deref()).await
}
