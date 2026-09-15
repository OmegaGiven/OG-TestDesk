# Next steps

MVP is built and has grown well past MVP — see `../CLAUDE.md` for the
current feature set (MCP server w/ OAuth, on-disk history, saved-query
folders, themes, split-screen SQL, error log, app-state debugger,
etc.). This file tracks what's still rough or missing before a wider
release, roughly in priority order.

## Before a beta release

- [ ] Result grid virtualization — still renders every row into the
      DOM. Fine to ~10k rows; windowing matters for larger sets.
- [ ] `docs/design-decisions.md` — reread and update against what
      actually shipped; some early decisions (e.g. the old tool-group
      top-bar design) have since been superseded.
- [x] macOS code signing / notarization — all 6 `APPLE_*` repo secrets
      are set (`APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`,
      `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD`,
      `APPLE_TEAM_ID`); `v0.2.0-beta.1`+ ship signed and notarized.
      Note if regenerating the cert: export the `.p12` with
      `openssl pkcs12 -export -legacy ...` — OpenSSL 3.x's default
      cipher isn't one macOS's `security import` can read.
- [ ] Windows code signing — no cert configured; SmartScreen will warn.
      Needs a code-signing cert (or Azure Trusted Signing) wired into
      `release.yml` the same way.
- [x] SSH tunnel is no longer a Mac App Store sandbox blocker —
      `core/src/drivers/tunnel.rs` reimplemented on `russh`/`russh-keys`
      (pure Rust, no `Command::spawn`), same behavior as before (key or
      ssh-agent auth, known_hosts TOFU matching
      `StrictHostKeyChecking=accept-new`, one session multiplexing many
      local connections like `ssh -L`). Verified end-to-end against a
      real local sshd. The `pre_connect_cmd` feature (arbitrary shell
      exec for IAM-style tokens) is a separate, deliberate feature and
      is still not sandbox-compatible — disable it in the MAS build
      rather than trying to sandbox arbitrary shell exec.
- [ ] Mac App Store submission — remaining work, now that the SSH
      blocker is gone:
      - **Apple Distribution cert** (not Developer ID) + an **App ID**
        with App Sandbox capability + a provisioning profile.
      - **Entitlements**: `com.apple.security.network.server` for the
        local MCP server's port; sandboxed file access (native file
        picker + security-scoped bookmarks) in place of the mock
        server's/CSV import's arbitrary paths; disable
        `pre_connect_cmd` (see above) for this build target.
      - A second Tauri bundle target (entitlements.plist wired in) —
        the direct-download build stays full-featured/unsandboxed.
      - App Store Connect: app record, bundle ID, screenshots, privacy
        nutrition label, export-compliance questionnaire (app uses
        XChaCha20-Poly1305 + TLS — likely exempt but must be declared).
      - Review-notes heads-up: a reviewer will likely ask about the
        local MCP server binding a port — have a plain-English answer
        ready (single-user local app, off by default, explicit
        per-connection ACLs).
- [ ] `cargo fmt` / `cargo clippy` are wired into CI as informational
      only (`continue-on-error`) since the codebase isn't currently
      clean under either — decide whether to actually run
      `cargo fmt --all` once and make it a real gate, or leave it loose.

## Drivers

- [ ] `NUMERIC`/`DECIMAL` precision: currently parsed to JS number when
      it round-trips, else kept as string. Consider always-string for
      money.
- [ ] Postgres arrays / composite types fall back to `<TYPE>` — decode
      the common ones (`_int4`, `_text`, `_uuid`).
- [ ] Connection pool eviction on connection edit/delete (pool cache in
      `core/src/drivers/pool.rs` keys on the conn string, so a changed
      password makes a new pool but the old one lingers until process
      exit — acceptable, revisit if it matters).
- [ ] `EXPLAIN`/`ANALYZE` result rendering (plain text panel).

## Requests

- [ ] Response body: syntax highlight for XML/HTML, image preview.
- [ ] Import from `curl` (Postman collection import already exists).
- [ ] Per-request environment override + variable autocomplete.

## Inspector

- [ ] Search prev/next navigation (match count is shown; no jump yet).
- [ ] Tree virtualization for very large payloads.
- [ ] "Diff two payloads" mode.

## MCP

- [ ] `release.yml`/distribution note: the OAuth flow's "Approve
      access?" page has no branding beyond plain text — fine
      functionally, worth a pass once the app has real visual identity
      to reuse there.
