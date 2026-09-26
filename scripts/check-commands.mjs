// Cross-checks the three places a Tauri command name has to agree:
//   frontend/src/lib/api.js      — what the UI calls
//   src-tauri/src/main.rs        — what generate_handler! actually registers
//   frontend/src/lib/mockTauri.js — what the browser mock answers
// The mock answers anything it knows about, so a command the UI calls but
// Rust never registered works in dev/tests/the web demo and only breaks in
// the real app. That's the drift this catches.
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const read = (p) => readFileSync(join(root, p), 'utf8');

const api = new Set([...read('frontend/src/lib/api.js').matchAll(/call\(\s*'([a-z0-9_]+)'/g)].map((m) => m[1]));

const mainRs = read('src-tauri/src/main.rs');
const start = mainRs.indexOf('generate_handler![');
if (start < 0) throw new Error('generate_handler! not found in src-tauri/src/main.rs');
const body = mainRs.slice(start + 'generate_handler!['.length, mainRs.indexOf(']', start)).replace(/\/\/.*$/gm, '');
const rust = new Set(body.split(',').map((s) => s.trim().split('::').pop()).filter(Boolean));

const mockSrc = read('frontend/src/lib/mockTauri.js');
const hStart = mockSrc.indexOf('const handlers = {');
if (hStart < 0) throw new Error('`const handlers = {` not found in mockTauri.js');
const mock = new Set([...mockSrc.slice(hStart).matchAll(/^ {4}'?([a-z0-9_]+)'?\s*:/gm)].map((m) => m[1]));

const missing = (a, b) => [...a].filter((x) => !b.has(x)).sort();
const problems = [];
const notRegistered = missing(api, rust);
if (notRegistered.length) problems.push(`Called from api.js but NOT registered in main.rs generate_handler! (breaks the real app):\n  ${notRegistered.join('\n  ')}`);
const notMocked = missing(api, mock);
if (notMocked.length) problems.push(`Called from api.js but missing from mockTauri.js handlers (breaks dev/E2E tests/web demo):\n  ${notMocked.join('\n  ')}`);

const unused = missing(rust, api);
console.log(`api.js: ${api.size} commands · main.rs: ${rust.size} registered · mock: ${mock.size} handlers`);
if (unused.length) console.log(`(info) registered in Rust but not called from api.js — fine if used elsewhere/by MCP only: ${unused.join(', ')}`);

if (problems.length) {
  console.error('\n' + problems.join('\n\n'));
  process.exit(1);
}
console.log('OK — every api.js command is registered in Rust and handled by the mock.');
