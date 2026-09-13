#!/usr/bin/env bash
# Bump the app's version everywhere it needs to live, consistently, in
# one shot: the workspace Cargo.toml (source of truth — src-tauri and
# core inherit it via `version.workspace = true`, and tauri.conf.json
# has no `version` field of its own so Tauri falls back to reading it
# from src-tauri's Cargo.toml too) and the frontend's package.json.
#
# Usage: scripts/bump-version.sh 0.2.0
#
# After running: review the diff, commit, tag (`git tag v0.2.0`), and
# push the tag — pushing a `v*` tag triggers .github/workflows/release.yml.
set -euo pipefail

if [ $# -ne 1 ]; then
  echo "usage: $0 <new-version>  (e.g. 0.2.0 — no leading 'v')" >&2
  exit 1
fi

new_version="$1"
if ! [[ "$new_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[A-Za-z0-9.-]+)?$ ]]; then
  echo "error: '$new_version' doesn't look like a semver version (expected e.g. 1.2.3 or 1.2.3-beta.1)" >&2
  exit 1
fi

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

old_version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
if [ -z "$old_version" ]; then
  echo "error: couldn't find [workspace.package] version in Cargo.toml" >&2
  exit 1
fi

echo "Bumping $old_version -> $new_version"

# Workspace Cargo.toml: only the [workspace.package] version line.
sed -i "0,/^version = \"$old_version\"/s//version = \"$new_version\"/" Cargo.toml

# Frontend package.json version.
sed -i "0,/\"version\": \"$old_version\"/s//\"version\": \"$new_version\"/" frontend/package.json

# Refresh Cargo.lock's recorded versions for the two workspace members.
cargo check --workspace >/dev/null

echo "Done. Review the diff, then:"
echo "  git add -A && git commit -m \"Bump version to $new_version\""
echo "  git tag v$new_version && git push origin main v$new_version"
