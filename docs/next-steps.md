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
- [ ] macOS code signing / notarization — `release.yml`'s macOS job
      already reads 5 repo secrets (`APPLE_CERTIFICATE`,
      `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`,
      `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` — `tauri-action`'s
      standard names) but they aren't set yet, so the build stays
      unsigned and Gatekeeper calls it "damaged". To set up (needs a
      paid Apple Developer account, which is available):
      1. In Xcode (or Keychain Access), create a **Developer ID
         Application** certificate for the team, then export it as a
         `.p12` with a password.
      2. `base64 -i cert.p12 | pbcopy` → repo secret `APPLE_CERTIFICATE`.
         The export password → `APPLE_CERTIFICATE_PASSWORD`.
      3. `APPLE_SIGNING_IDENTITY` = the cert's common name, e.g.
         `Developer ID Application: Name (TEAMID)`.
      4. `APPLE_TEAM_ID` = the 10-char Team ID (Apple Developer
         account → Membership).
      5. `APPLE_ID` = the Apple ID email on that account;
         `APPLE_PASSWORD` = an **app-specific password** for it
         (appleid.apple.com → Sign-In and Security → App-Specific
         Passwords) — not the account password.
      Once all 5 secrets exist, the next `v*` tag push signs + notarizes
      automatically — no workflow change needed.
- [ ] Windows code signing — no cert configured; SmartScreen will warn.
      Needs a code-signing cert (or Azure Trusted Signing) wired into
      `release.yml` the same way.
- [ ] Mac App Store submission — a separate track from the above:
      requires an **App Store Distribution** cert (not Developer ID),
      App Sandbox entitlements enabled (review which features — the
      system-`ssh` tunnel shell-out, arbitrary file paths for the mock
      server/CSV import — need sandboxed alternatives or user-selected
      file access under sandboxing), an App Store Connect API key for
      upload, and an App Store Connect listing + screenshots. Do the
      Developer ID signing/notarization above first; this is follow-up
      work, not blocking the direct-download release.
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
