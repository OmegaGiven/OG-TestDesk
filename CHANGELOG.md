# Changelog

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
versioning follows [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.2.0-beta.1] - 2026-09-14

Beta. Full Postman/Postico-parity pass since 0.1.x:

- **Requests**: OAuth 2.0 (Client Credentials + Auth Code/PKCE), Digest,
  AWS SigV4 auth; pre-request & test scripts (`pm.*` sandbox); a real
  cookie jar; multipart/binary/GraphQL body types; proxy/custom CA/
  per-host client certs (mTLS); code snippet generation (cURL, Python,
  JS, Node); a local mock server; a WebSocket tester; gRPC via server
  reflection.
- **SQL**: real row editing (UPDATE/INSERT/DELETE from a plain
  `SELECT *`, with a preview), graphical table structure editor + DDL
  view, CSV import wizard, foreign-key picker, scheduled queries.
- **UI**: full icon system replaced with a clean, consistent Lucide-based
  set across the entire app; new app icon.
- Project site published at https://omegagiven.github.io/OG-TestDesk/.

## [0.1.0] - 2026-09-13

Initial beta. SQL client (Postgres/MySQL/SQLite), HTTP request client,
shared JSON inspector, split-screen SQL editing, saved queries/requests,
environments, an MCP server (OAuth-capable) so an AI assistant can open
tabs/run queries with the human watching live, an in-app error log and
app-state debugger, and a themed dialog system throughout.
