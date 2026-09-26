// The app version lives in three files that must move together. The App
// Store config's own "version" silently overrides Cargo's for the App
// Store build, so forgetting it ships (or gets rejected as) the old
// version — this has already cost two rejected uploads.
//   Cargo.toml [workspace.package] version   e.g. 0.2.4-beta.1
//   frontend/package.json version            must equal Cargo's exactly
//   src-tauri/tauri.macos-appstore.conf.json must equal Cargo's minus any
//                                            pre-release suffix (Apple only
//                                            accepts plain X.Y.Z)
// Optional: pass a tag (e.g. v0.2.4-beta.1) to also check it matches.
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const read = (p) => readFileSync(join(root, p), 'utf8');

const ws = read('Cargo.toml').match(/\[workspace\.package\][^[]*?\nversion\s*=\s*"([^"]+)"/);
if (!ws) throw new Error('No version under [workspace.package] in Cargo.toml');
const cargo = ws[1];
const pkg = JSON.parse(read('frontend/package.json')).version;
const appstore = JSON.parse(read('src-tauri/tauri.macos-appstore.conf.json')).version;
const plain = cargo.split('-')[0];

const errors = [];
if (pkg !== cargo) errors.push(`frontend/package.json is ${pkg}, Cargo.toml is ${cargo}`);
if (appstore !== plain) errors.push(`src-tauri/tauri.macos-appstore.conf.json is ${appstore}, expected ${plain} (Cargo.toml ${cargo} without the pre-release suffix)`);
if (!/^\d+\.\d+\.\d+$/.test(appstore)) errors.push(`App Store version "${appstore}" must be plain X.Y.Z`);

const tag = process.argv[2];
if (tag && tag.replace(/^v/, '') !== cargo) errors.push(`tag ${tag} doesn't match Cargo.toml version ${cargo}`);

console.log(`Cargo ${cargo} · package.json ${pkg} · App Store ${appstore}${tag ? ` · tag ${tag}` : ''}`);
if (errors.length) {
  console.error('Version mismatch:\n  ' + errors.join('\n  '));
  process.exit(1);
}
console.log('OK — versions in lockstep.');
