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
- [x] Mac App Store: cert + App ID + entitlements + build config — done.
      - **Apple Distribution cert**: `APPLE_DIST_CERTIFICATE`,
        `APPLE_DIST_CERTIFICATE_PASSWORD`, `APPLE_DIST_SIGNING_IDENTITY`
        repo secrets set (not wired into `release.yml` yet — that's a
        manual/local build for now, see below).
      - **App ID** registered: `com.omegagiven.ogtestdesk` (matches
        `tauri.conf.json`'s `identifier`), Team `FGFW7ZAJUC`.
      - Re-audited the actual sandbox concerns instead of assuming: CSV
        import already uses a native `<input type="file">` (sandbox-safe
        by construction — the OS grants access to whatever the user
        picks), the MCP server's `save_sql_file` is already confined to
        `app_data_dir/exports` (sandbox-safe automatically, no code
        change needed), and the mock server has **no filesystem access
        at all** — canned responses live in the metadata DB. The only
        real blocker was `pre_connect_cmd`'s shell exec (SSH tunnel was
        the other one, already fixed above).
      - `core`'s new `app-store` Cargo feature (`src-tauri` passes it
        through) disables `pre_connect_cmd` at compile time with a clear
        runtime error if a connection still has one configured; a new
        `app_capabilities` command + `ConnectionModal.svelte` check
        hides that form field in that build instead of showing one that
        errors. Verified both feature states (`cargo check` with and
        without `--features app-store`) compile clean.
      - `src-tauri/entitlements-appstore.plist`: `app-sandbox`,
        `network.client` (outbound DB/HTTP/SSH/gRPC),
        `network.server` (MCP + mock server's local ports),
        `files.user-selected.read-write` + `files.downloads.read-write`
        (CSV import's file picker, "Save .sql file"'s blob download).
      - `src-tauri/tauri.macos-appstore.conf.json`: a config override
        (deep-merges over the base config) wiring the entitlements file,
        the Apple Distribution signing identity, and `bundle.targets:
        ["app"]` (no dmg/updater for this target). Build with:
        `cargo tauri build --config tauri.macos-appstore.conf.json --features app-store`.
      - [x] **Mac Installer Distribution cert** — set (`APPLE_INSTALLER_*`
        secrets), `productbuild` wraps the signed `.app` into a `.pkg`.
      - [x] **Provisioning profile** — `OG TestDesk Mac App Store`,
        embedded via `bundle.macOS.files` (`embedded.provisionprofile`
        inside `Contents/`, not just installed to the OS profile dir —
        that alone wasn't enough, Apple's validation rejected it).
      - [x] `bundle.category: "DeveloperTool"` in the base
        `tauri.conf.json` — Apple's server-side validation rejected the
        first real upload attempt over a missing
        `LSApplicationCategoryType` in Info.plist.
      - [x] `.github/workflows/appstore.yml` now uploads straight to App
        Store Connect via `xcrun altool --upload-app`, authenticated
        with an API key (`APPLE_API_KEY`/`_ID`/`_ISSUER` secrets) — runs
        entirely on GitHub's macOS runner, no Mac hardware anywhere.
        `HAS_APPSTORE_UPLOAD` repo variable gates it on.
      - [x] The MAS build's `version` is overridden to a plain `0.2.0`
        in `tauri.macos-appstore.conf.json` — `CFBundleShortVersionString`
        can't carry a semver pre-release suffix; the real workspace
        version (GitHub releases, changelog) is untouched.
      - [x] `docs/img/appstore/icon-1024.png` — the 1024×1024, no-alpha
        App Store icon (Apple's separate requirement from the in-app
        icon set; flattened from the same source, no code change).
      - [x] `docs/privacy.html` — privacy policy page (no telemetry, no
        account, nothing leaves the machine except what the user
        explicitly configures), linked from the landing page footer;
        URL: https://omegagiven.github.io/OG-TestDesk/privacy.html
      - [x] Export compliance: answered "uses standard encryption
        algorithms" (TLS + XChaCha20-Poly1305, not proprietary, not
        solely Apple's OS crypto) — qualifies for the standard
        exemption, no CCATS/self-classification report needed.
      - **Submitted for App Review 2026-09-15.** First real build
        (`0.2.0`) uploaded clean — `UPLOAD SUCCEEDED with no errors,
        1 warning` (the one warning is TestFlight-only, doesn't block
        App Store review). Watch for a reviewer question about the
        local MCP server binding a port — answer ready: single-user
        local app, off by default, explicit per-connection ACLs, binds
        127.0.0.1 only (not reachable from the network).
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
