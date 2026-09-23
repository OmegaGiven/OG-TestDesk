// Central icon registry. Every icon button in the app pulls its markup
// from here rather than hardcoding it — so the Help tab's icon
// reference (Settings > Help & docs, HelpPane.svelte, "Icon reference"
// section) renders straight from this same object. Change an icon here
// and it changes everywhere it's used, docs included, in one edit — the
// two can never drift out of sync.
//
// Every entry is a real inline SVG (Lucide's icon set, ISC-licensed,
// currentColor stroke so hover/theme color applies same as text would) —
// not a Unicode/emoji glyph. Emoji render as colorful platform-specific
// pictures on systems with a color-emoji font (clashing with an
// otherwise monochrome UI) and as nothing at all on systems without one,
// which is common on a bare Linux webview; plain Unicode symbols (×, ▸,
// ⚙...) render at inconsistent weight/alignment across OS fonts. A single
// vector icon set renders identically everywhere, same as Postman/
// Insomnia/TablePlus all do.
//
// `where` is shown in the Help table as a plain-English location. Keep
// entries in roughly the order a user would meet them.

// width/height in `em` (not px) so each icon scales with whatever
// font-size its container already sets — the same trick icon fonts use,
// and it means no per-call-site sizing logic. `.ico-svg` picks up
// baseline alignment from one shared rule (see ui.css) since an inline
// SVG's default baseline sits noticeably higher than adjacent text.
function svg(inner, viewBox = '0 0 24 24') {
  return (
    `<svg class="ico-svg" viewBox="${viewBox}" width="1em" height="1em" fill="none" stroke="currentColor" ` +
    `stroke-width="2" stroke-linecap="round" stroke-linejoin="round" xmlns="http://www.w3.org/2000/svg">${inner}</svg>`
  );
}

export const ICONS = {
  help: { svg: svg('<circle cx="12" cy="12" r="10"/><path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3"/><path d="M12 17h.01"/>'), label: 'Help', where: 'Top bar' },
  history: { svg: svg('<path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/><path d="M12 7v5l4 2"/>'), label: 'History & schedules', where: 'Top bar' },
  settings: { svg: svg('<path d="M9.671 4.136a2.34 2.34 0 0 1 4.659 0 2.34 2.34 0 0 0 3.319 1.915 2.34 2.34 0 0 1 2.33 4.033 2.34 2.34 0 0 0 0 3.831 2.34 2.34 0 0 1-2.33 4.033 2.34 2.34 0 0 0-3.319 1.915 2.34 2.34 0 0 1-4.659 0 2.34 2.34 0 0 0-3.32-1.915 2.34 2.34 0 0 1-2.33-4.033 2.34 2.34 0 0 0 0-3.831A2.34 2.34 0 0 1 6.35 6.051a2.34 2.34 0 0 0 3.319-1.915"/><circle cx="12" cy="12" r="3"/>'), label: 'Settings', where: 'Top bar' },
  structure: { svg: svg('<path d="M15 3v18"/><rect width="18" height="18" x="3" y="3" rx="2"/><path d="M21 9H3"/><path d="M21 15H3"/>'), label: 'Edit table structure / view DDL', where: 'Schema tree' },
  themeDark: { svg: svg('<path d="M20.985 12.486a9 9 0 1 1-9.473-9.472c.405-.022.617.46.402.803a6 6 0 0 0 8.268 8.268c.344-.215.825-.004.803.401"/>'), label: 'Theme: dark — click to cycle', where: 'Top bar' },
  themeLight: { svg: svg('<circle cx="12" cy="12" r="4"/><path d="M12 2v2"/><path d="M12 20v2"/><path d="m4.93 4.93 1.41 1.41"/><path d="m17.66 17.66 1.41 1.41"/><path d="M2 12h2"/><path d="M20 12h2"/><path d="m6.34 17.66-1.41 1.41"/><path d="m19.07 4.93-1.41 1.41"/>'), label: 'Theme: light — click to cycle', where: 'Top bar' },
  themeSystem: { svg: svg('<path d="M12 2v2"/><path d="M14.837 16.385a6 6 0 1 1-7.223-7.222c.624-.147.97.66.715 1.248a4 4 0 0 0 5.26 5.259c.589-.255 1.396.09 1.248.715"/><path d="M16 12a4 4 0 0 0-4-4"/><path d="m19 5-1.256 1.256"/><path d="M20 12h2"/>'), label: 'Theme: follows system — click to cycle', where: 'Top bar' },
  closeTab: { svg: svg('<path d="M18 6 6 18"/><path d="m6 6 12 12"/>'), label: 'Close tab', where: 'SQL / request tabs' },

  expandOpen: { svg: svg('<path d="m6 9 6 6 6-6"/>'), label: 'Expanded — click to collapse', where: 'Folders, schema tree, panel headers' },
  expandClosed: { svg: svg('<path d="m9 18 6-6-6-6"/>'), label: 'Collapsed — click to expand', where: 'Folders, schema tree, panel headers' },
  table: { svg: svg('<path d="M12 3v18"/><rect width="18" height="18" x="3" y="3" rx="2"/><path d="M3 9h18"/><path d="M3 15h18"/>'), label: 'Table', where: 'Schema tree' },
  view: { svg: svg('<path d="M2.062 12.348a1 1 0 0 1 0-.696 10.75 10.75 0 0 1 19.876 0 1 1 0 0 1 0 .696 10.75 10.75 0 0 1-19.876 0"/><circle cx="12" cy="12" r="3"/>'), label: 'View', where: 'Schema tree' },
  primaryKey: { svg: svg('<path d="M2.586 17.414A2 2 0 0 0 2 18.828V21a1 1 0 0 0 1 1h3a1 1 0 0 0 1-1v-1a1 1 0 0 1 1-1h1a1 1 0 0 0 1-1v-1a1 1 0 0 1 1-1h.172a2 2 0 0 0 1.414-.586l.814-.814a6.5 6.5 0 1 0-4-4z"/><circle cx="16.5" cy="7.5" r=".5" fill="currentColor"/>'), label: 'Primary key column', where: 'Schema tree' },
  insertName: { svg: svg('<path d="M20 4v7a4 4 0 0 1-4 4H4"/><path d="m9 10-5 5 5 5"/>'), label: 'Insert name into the editor', where: 'Schema tree' },
  refresh: { svg: svg('<path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8"/><path d="M21 3v5h-5"/><path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16"/><path d="M8 16H3v5"/>'), label: 'Refresh', where: 'Schema tree, saved queries' },
  relationships: { svg: svg('<path d="M9 17H7A5 5 0 0 1 7 7h2"/><path d="M15 7h2a5 5 0 1 1 0 10h-2"/><line x1="8" x2="16" y1="12" y2="12"/>'), label: 'Table relationships (foreign keys)', where: 'Schema tree' },

  newQuery: { svg: svg('<path d="M5 12h14"/><path d="M12 5v14"/>'), label: 'New query', where: 'Saved queries sidebar' },
  newFolder: { svg: svg('<path d="M12 10v6"/><path d="M9 13h6"/><path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"/>'), label: 'New folder', where: 'Saved queries sidebar' },
  newSubfolder: { svg: svg('<path d="M12 10v6"/><path d="M9 13h6"/><path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"/>'), label: 'New subfolder', where: 'Saved queries folder row' },
  newQueryHere: { svg: svg('<path d="M5 12h14"/><path d="M12 5v14"/>'), label: 'New query in this folder', where: 'Saved queries folder row' },
  rename: { svg: svg('<path d="M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z"/><path d="m15 5 4 4"/>'), label: 'Rename / edit', where: 'Saved queries, connections' },
  delete: { svg: svg('<path d="M10 11v6"/><path d="M14 11v6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6"/><path d="M3 6h18"/><path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>'), label: 'Delete / remove', where: 'Saved queries, requests, schedules, environments' },

  columnFilters: { svg: svg('<path d="M10 20a1 1 0 0 0 .553.895l2 1A1 1 0 0 0 14 21v-7a2 2 0 0 1 .517-1.341L21.74 4.67A1 1 0 0 0 21 3H3a1 1 0 0 0-.742 1.67l7.225 7.989A2 2 0 0 1 10 14z"/>'), label: 'Per-column filters', where: 'Result grid toolbar' },
  columns: { svg: svg('<rect width="18" height="18" x="3" y="3" rx="2"/><path d="M9 3v18"/><path d="M15 3v18"/>'), label: 'Show / hide columns', where: 'Result grid toolbar' },
  editCells: { svg: svg('<path d="M12 3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"/><path d="M18.375 2.625a1 1 0 0 1 3 3l-9.013 9.014a2 2 0 0 1-.853.505l-2.873.84a.5.5 0 0 1-.62-.62l.84-2.873a2 2 0 0 1 .506-.852z"/>'), label: 'Inline-edit cells (view/export only)', where: 'Result grid toolbar' },
  revert: { svg: svg('<path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/>'), label: 'Revert / discard', where: 'Result grid edits, custom theme editor' },
  copy: { svg: svg('<rect width="14" height="14" x="8" y="8" rx="2" ry="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/>'), label: 'Copy to clipboard', where: 'Result grid, response body' },
  sortAsc: { svg: svg('<path d="m5 12 7-7 7 7"/><path d="M12 19V5"/>'), label: 'Sorted ascending — click to reverse', where: 'Result grid column header' },
  sortDesc: { svg: svg('<path d="M12 5v14"/><path d="m19 12-7 7-7-7"/>'), label: 'Sorted descending — click to clear', where: 'Result grid column header' },
  success: { svg: svg('<path d="M20 6 9 17l-5-5"/>'), label: 'Statement executed successfully', where: 'Result pane' },
  toInspector: { svg: svg('<path d="M5 12h14"/><path d="m12 5 7 7-7 7"/>'), label: 'Send to Inspector', where: 'SQL results, request response' },
  saveFile: { svg: svg('<path d="M15.2 3a2 2 0 0 1 1.4.6l3.8 3.8a2 2 0 0 1 .6 1.4V19a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z"/><path d="M17 21v-7a1 1 0 0 0-1-1H8a1 1 0 0 0-1 1v7"/><path d="M7 3v4a1 1 0 0 0 1 1h7"/>'), label: 'Save this query to a .sql file', where: 'SQL toolbar' },
  saveToBookmarks: { svg: svg('<path d="M12 7v6"/><path d="M15 10H9"/><path d="M17 3a2 2 0 0 1 2 2v15a1 1 0 0 1-1.496.868l-4.512-2.578a2 2 0 0 0-1.984 0l-4.512 2.578A1 1 0 0 1 5 20V5a2 2 0 0 1 2-2z"/>'), label: 'Save to Saved Queries', where: 'SQL toolbar' },

  connOk: { svg: svg('<circle cx="12" cy="12" r="8" fill="currentColor" stroke="none"/>'), label: 'Connection reachable', where: 'Connections dropdown', colorVar: '--ok' },
  connErr: { svg: svg('<circle cx="12" cy="12" r="8" fill="currentColor" stroke="none"/>'), label: 'Connection failed', where: 'Connections dropdown', colorVar: '--danger' },

  importPostman: { svg: svg('<path d="M12 15V3"/><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><path d="m7 10 5 5 5-5"/>'), label: 'Import a Postman collection / environment', where: 'Requests sidebar' },
  exportPostman: { svg: svg('<path d="M12 3v12"/><path d="m17 8-5-5-5 5"/><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/>'), label: 'Export everything as a Postman collection', where: 'Requests sidebar' },

  code: { svg: svg('<path d="m16 18 6-6-6-6"/><path d="m8 6-6 6 6 6"/>'), label: 'Generate a code snippet (curl, Python, JS, ...)', where: 'Response panel toolbar' },
  cookie: { svg: svg('<path d="M11 17h.01"/><path d="M11.496 2c.324-.016.558.292.529.615a4 4 0 0 0 4.235 4.368.713.713 0 0 1 .758.757 4 4 0 0 0 4.366 4.237c.323-.03.63.204.614.527a10 10 0 0 1-2.915 6.566A1 1 0 1 1 4.93 4.918 10 10 0 0 1 11.496 2"/><path d="M12 12h.01"/><path d="M16 16h.01"/><path d="M16 3h.01"/><path d="M21 4h.01"/><path d="M21 8h.01"/><path d="M7 14h.01"/><path d="M9 8h.01"/>'), label: 'Cookie manager', where: 'Requests sidebar' },
  network: { svg: svg('<rect x="16" y="16" width="6" height="6" rx="1"/><rect x="2" y="16" width="6" height="6" rx="1"/><rect x="9" y="2" width="6" height="6" rx="1"/><path d="M5 16v-3a1 1 0 0 1 1-1h12a1 1 0 0 1 1 1v3"/><path d="M12 12V8"/>'), label: 'Proxy, custom CA, client certificates', where: 'Requests sidebar' },
  mockServer: { svg: svg('<rect width="20" height="8" x="2" y="2" rx="2" ry="2"/><rect width="20" height="8" x="2" y="14" rx="2" ry="2"/><line x1="6" x2="6.01" y1="6" y2="6"/><line x1="6" x2="6.01" y1="18" y2="18"/>'), label: 'Local mock server', where: 'Requests sidebar' },
  websocket: { svg: svg('<path d="M17 19a1 1 0 0 1-1-1v-2a2 2 0 0 1 2-2h2a2 2 0 0 1 2 2v2a1 1 0 0 1-1 1z"/><path d="M17 21v-2"/><path d="M19 14V6.5a1 1 0 0 0-7 0v11a1 1 0 0 1-7 0V10"/><path d="M21 21v-2"/><path d="M3 5V3"/><path d="M4 10a2 2 0 0 1-2-2V6a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2a2 2 0 0 1-2 2z"/><path d="M7 5V3"/>'), label: 'WebSocket connection tester', where: 'Requests sidebar' },
  grpc: { svg: svg('<path d="M2.97 12.92A2 2 0 0 0 2 14.63v3.24a2 2 0 0 0 .97 1.71l3 1.8a2 2 0 0 0 2.06 0L12 19v-5.5l-5-3-4.03 2.42Z"/><path d="m7 16.5-4.74-2.85"/><path d="m7 16.5 5-3"/><path d="M7 16.5v5.17"/><path d="M12 13.5V19l3.97 2.38a2 2 0 0 0 2.06 0l3-1.8a2 2 0 0 0 .97-1.71v-3.24a2 2 0 0 0-.97-1.71L17 10.5l-5 3Z"/><path d="m17 16.5-5-3"/><path d="m17 16.5 4.74-2.85"/><path d="M17 16.5v5.17"/><path d="M7.97 4.42A2 2 0 0 0 7 6.13v4.37l5 3 5-3V6.13a2 2 0 0 0-.97-1.71l-3-1.8a2 2 0 0 0-2.06 0l-3 1.8Z"/><path d="M12 8 7.26 5.15"/><path d="m12 8 4.74-2.85"/><path d="M12 13.5V8"/>'), label: 'gRPC — reflection-based discovery, unary calls', where: 'Requests sidebar' },

  check: { svg: svg('<path d="M20 6 9 17l-5-5"/>'), label: 'Pass / done / active', where: 'Test results, table structure, primary key' },
  cancel: { svg: svg('<path d="M18 6 6 18"/><path d="m6 6 12 12"/>'), label: 'Fail / remove / cancel', where: 'Test results, table structure, remove-row buttons' },
  radioOn: { svg: svg('<circle cx="12" cy="12" r="8" fill="currentColor" stroke="none"/>'), label: 'Active environment', where: 'Environments modal' },
  radioOff: { svg: svg('<circle cx="12" cy="12" r="10"/>'), label: 'Inactive environment — click to activate', where: 'Environments modal' },
  heart: { svg: svg('<path d="M2 9.5a5.5 5.5 0 0 1 9.591-3.676.56.56 0 0 0 .818 0A5.49 5.49 0 0 1 22 9.5c0 2.29-1.5 4-3 5.5l-5.492 5.313a2 2 0 0 1-3 .019L5 15c-1.5-1.5-3-3.2-3-5.5"/>'), label: 'Sponsor this project', where: 'Help modal' }
};
