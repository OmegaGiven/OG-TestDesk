//! SSH tunnels (shells out to the system `ssh` binary — not an embedded
//! client, so it picks up the user's own `~/.ssh/config`, known_hosts, and
//! agent) and pre-connect shell commands (a configured command whose
//! stdout becomes the password for that connect, for IAM-style short-lived
//! tokens). Both spawn a process on the user's behalf using a connection
//! profile the user themselves configured — the same shell access the
//! human already has, not a new trust boundary.

use super::ConnConfig;
use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::net::TcpListener;
use std::process::Stdio;
use std::sync::Mutex;
use std::time::Duration;
use tokio::process::{Child, Command};

struct TunnelHandle {
    child: Child,
    local_port: u16,
}

static TUNNELS: Mutex<Option<HashMap<String, TunnelHandle>>> = Mutex::new(None);

fn pick_free_port() -> Result<u16> {
    let l = TcpListener::bind(("127.0.0.1", 0)).context("couldn't allocate a local port")?;
    Ok(l.local_addr()?.port())
}

async fn port_is_open(port: u16) -> bool {
    tokio::net::TcpStream::connect(("127.0.0.1", port)).await.is_ok()
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
    let real_host = cfg.host.as_deref().unwrap_or("localhost");
    let real_port = cfg.port.unwrap_or(5432);

    {
        let mut guard = TUNNELS.lock().unwrap();
        let map = guard.get_or_insert_with(HashMap::new);
        if let Some(handle) = map.get_mut(&cfg.id) {
            if handle.child.try_wait().ok().flatten().is_none() {
                return Ok(handle.local_port);
            }
            map.remove(&cfg.id); // died — fall through and restart it
        }
    }

    let local_port = pick_free_port()?;
    let ssh_port = cfg.ssh_port.unwrap_or(22);
    let ssh_user = cfg.ssh_user.as_deref().unwrap_or("root");

    let mut cmd = Command::new("ssh");
    cmd.arg("-N")
        .arg("-o")
        .arg("StrictHostKeyChecking=accept-new")
        .arg("-o")
        .arg("ExitOnForwardFailure=yes")
        .arg("-o")
        .arg("ServerAliveInterval=15")
        .arg("-p")
        .arg(ssh_port.to_string())
        .arg("-L")
        .arg(format!("{local_port}:{real_host}:{real_port}"));
    if let Some(key) = cfg.ssh_key_path.as_deref().filter(|k| !k.is_empty()) {
        cmd.arg("-i").arg(key);
    }
    cmd.arg(format!("{ssh_user}@{ssh_host}"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = cmd
        .spawn()
        .context("couldn't start `ssh` — is it installed and on PATH?")?;

    // Give it a few seconds to establish the forward before trusting it.
    let mut up = false;
    for _ in 0..50 {
        if let Some(status) = child.try_wait()? {
            let mut stderr = String::new();
            if let Some(mut e) = child.stderr.take() {
                use tokio::io::AsyncReadExt;
                let _ = e.read_to_string(&mut stderr).await;
            }
            bail!("ssh tunnel exited immediately ({status}): {}", stderr.trim());
        }
        if port_is_open(local_port).await {
            up = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    if !up {
        let _ = child.kill().await;
        bail!("ssh tunnel to {ssh_host} didn't come up within 5s");
    }

    let mut guard = TUNNELS.lock().unwrap();
    guard
        .get_or_insert_with(HashMap::new)
        .insert(cfg.id.clone(), TunnelHandle { child, local_port });
    Ok(local_port)
}

/// Stops a connection's tunnel, if any — call when a connection is
/// deleted or its SSH settings change, so a stale tunnel isn't left
/// running (or a new one silently keeps using an old port assignment).
pub fn stop_tunnel(conn_id: &str) {
    if let Some(map) = TUNNELS.lock().unwrap().as_mut() {
        if let Some(mut handle) = map.remove(conn_id) {
            let _ = handle.child.start_kill();
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
pub async fn resolve_password(cfg: &ConnConfig, stored: Option<&str>) -> Result<Option<String>> {
    let Some(cmd_text) = cfg.pre_connect_cmd.as_deref().filter(|c| !c.trim().is_empty()) else {
        return Ok(stored.map(|s| s.to_string()));
    };
    let fut = async {
        #[cfg(windows)]
        let out = Command::new("cmd").arg("/C").arg(cmd_text).output().await?;
        #[cfg(not(windows))]
        let out = Command::new("sh").arg("-c").arg(cmd_text).output().await?;
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
