// Central icon registry. Every glyph-icon button in the app pulls its
// character from here rather than hardcoding it in markup — so the Help
// modal's icon reference (HelpModal.svelte, "Icon reference" section)
// renders straight from this same object. Change a glyph here and it
// changes everywhere it's used, docs included, in one edit — the two
// can never drift out of sync.
//
// `where` is shown in the Help table as a plain-English location. Keep
// entries in roughly the order a user would meet them.
// A couple of icons need an actual shape (folder, key) rather than a
// character — emoji glyphs (📁, 🔑) render as nothing at all on systems
// with no color-emoji font, which is common on a bare Linux webview.
// These are plain inline SVG (currentColor, so hover/theme color applies
// same as text) instead.
const FOLDER_PLUS_SVG =
  '<svg viewBox="0 0 16 16" width="13" height="13" fill="none" xmlns="http://www.w3.org/2000/svg">' +
  '<path d="M1.5 3.75c0-.69.56-1.25 1.25-1.25h2.94c.3 0 .58.11.8.31L7.7 3.9a.75.75 0 0 0 .5.2h5.05c.69 0 1.25.56 1.25 1.25v6.4c0 .69-.56 1.25-1.25 1.25h-10.5c-.69 0-1.25-.56-1.25-1.25V3.75Z" ' +
  'stroke="currentColor" stroke-width="1.15"/>' +
  '<path d="M8 6.9v3.2M6.4 8.5h3.2" stroke="currentColor" stroke-width="1.25" stroke-linecap="round"/>' +
  '</svg>';
const KEY_SVG =
  '<svg viewBox="0 0 16 16" width="12" height="12" fill="none" xmlns="http://www.w3.org/2000/svg">' +
  '<circle cx="5" cy="5" r="3" stroke="currentColor" stroke-width="1.2"/>' +
  '<path d="M7.1 6.9 13 12.8M13 12.8 11.4 14.4M13 12.8l1.3-1.3" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/>' +
  '</svg>';

export const ICONS = {
  help: { glyph: '?', label: 'Help', where: 'Top bar' },
  history: { glyph: '⏱', label: 'History & schedules', where: 'Top bar' },
  settings: { glyph: '⚙', label: 'Settings', where: 'Top bar' },
  themeDark: { glyph: '☾', label: 'Theme: dark — click to cycle', where: 'Top bar' },
  themeLight: { glyph: '☀', label: 'Theme: light — click to cycle', where: 'Top bar' },
  themeSystem: { glyph: '◐', label: 'Theme: follows system — click to cycle', where: 'Top bar' },
  closeTab: { glyph: '×', label: 'Close tab', where: 'SQL / request tabs' },

  expandOpen: { glyph: '▾', label: 'Expanded — click to collapse', where: 'Folders, schema tree, panel headers' },
  expandClosed: { glyph: '▸', label: 'Collapsed — click to expand', where: 'Folders, schema tree, panel headers' },
  table: { glyph: '▦', label: 'Table', where: 'Schema tree' },
  view: { glyph: '◇', label: 'View', where: 'Schema tree' },
  primaryKey: { svg: KEY_SVG, label: 'Primary key column', where: 'Schema tree' },
  insertName: { glyph: '↵', label: 'Insert name into the editor', where: 'Schema tree' },
  refresh: { glyph: '⟳', label: 'Refresh', where: 'Schema tree, saved queries' },
  relationships: { glyph: '⛓', label: 'Table relationships (foreign keys)', where: 'Schema tree' },

  newQuery: { glyph: '+', label: 'New query', where: 'Saved queries sidebar' },
  newFolder: { svg: FOLDER_PLUS_SVG, label: 'New folder', where: 'Saved queries sidebar' },
  newSubfolder: { svg: FOLDER_PLUS_SVG, label: 'New subfolder', where: 'Saved queries folder row' },
  newQueryHere: { glyph: '+', label: 'New query in this folder', where: 'Saved queries folder row' },
  rename: { glyph: '✎', label: 'Rename / edit', where: 'Saved queries, connections' },
  delete: { glyph: '✕', label: 'Delete / remove', where: 'Saved queries, requests, schedules, environments' },

  columnFilters: { glyph: '⑂', label: 'Per-column filters', where: 'Result grid toolbar' },
  columns: { glyph: '☰', label: 'Show / hide columns', where: 'Result grid toolbar' },
  editCells: { glyph: '✎', label: 'Inline-edit cells (view/export only)', where: 'Result grid toolbar' },
  revert: { glyph: '↺', label: 'Revert / discard', where: 'Result grid edits, custom theme editor' },
  copy: { glyph: '⧉', label: 'Copy to clipboard', where: 'Result grid, response body' },
  sortAsc: { glyph: '▲', label: 'Sorted ascending — click to reverse', where: 'Result grid column header' },
  sortDesc: { glyph: '▼', label: 'Sorted descending — click to clear', where: 'Result grid column header' },
  success: { glyph: '✓', label: 'Statement executed successfully', where: 'Result pane' },
  toInspector: { glyph: '→', label: 'Send to Inspector', where: 'SQL results, request response' },
  saveFile: { glyph: '⇩', label: 'Save this query to a .sql file', where: 'SQL toolbar' },

  connOk: { glyph: '●', label: 'Connection reachable', where: 'Connections dropdown', colorVar: '--ok' },
  connErr: { glyph: '●', label: 'Connection failed', where: 'Connections dropdown', colorVar: '--danger' },

  importPostman: { glyph: '⇩', label: 'Import a Postman collection / environment', where: 'Requests sidebar' },
  exportPostman: { glyph: '⇧', label: 'Export everything as a Postman collection', where: 'Requests sidebar' }
};
