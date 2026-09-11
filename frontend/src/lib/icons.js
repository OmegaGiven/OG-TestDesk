// Central icon registry. Every glyph-icon button in the app pulls its
// character from here rather than hardcoding it in markup — so the Help
// modal's icon reference (HelpModal.svelte, "Icon reference" section)
// renders straight from this same object. Change a glyph here and it
// changes everywhere it's used, docs included, in one edit — the two
// can never drift out of sync.
//
// `where` is shown in the Help table as a plain-English location. Keep
// entries in roughly the order a user would meet them.
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
  primaryKey: { glyph: '🔑', label: 'Primary key column', where: 'Schema tree' },
  insertName: { glyph: '↵', label: 'Insert name into the editor', where: 'Schema tree' },
  refresh: { glyph: '⟳', label: 'Refresh', where: 'Schema tree, saved queries' },

  newQuery: { glyph: '+', label: 'New query', where: 'Saved queries sidebar' },
  newFolder: { glyph: '📁+', label: 'New folder', where: 'Saved queries sidebar' },
  newSubfolder: { glyph: '📁+', label: 'New subfolder', where: 'Saved queries folder row' },
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

  connOk: { glyph: '●', label: 'Connection reachable', where: 'Connections dropdown', colorVar: '--ok' },
  connErr: { glyph: '●', label: 'Connection failed', where: 'Connections dropdown', colorVar: '--danger' },

  importPostman: { glyph: '⇩', label: 'Import a Postman collection / environment', where: 'Requests sidebar' },
  exportPostman: { glyph: '⇧', label: 'Export everything as a Postman collection', where: 'Requests sidebar' }
};
