// Small SQL-text-building helpers shared by anything that generates raw
// SQL client-side (real row editing, CSV import) rather than going through
// a parameterized-query API — consistent with how the rest of the app
// already builds SQL client-side (see SqlView's openRelation()). This is a
// power-user desktop tool where the user already has full raw SQL access
// via the editor; these helpers aren't a new trust boundary, just quoting
// discipline for statements the app generates on the user's behalf.

export function quote(conn, ident) {
  if (conn.kind === 'mysql') return '`' + ident.replace(/`/g, '``') + '`';
  return '"' + ident.replace(/"/g, '""') + '"';
}

export function literal(conn, val) {
  if (val === null || val === undefined) return 'NULL';
  if (typeof val === 'number') return String(val);
  if (typeof val === 'boolean') {
    return conn.kind === 'mysql' ? (val ? '1' : '0') : val ? 'TRUE' : 'FALSE';
  }
  const s = (typeof val === 'object' ? JSON.stringify(val) : String(val)).replace(/'/g, "''");
  return `'${s}'`;
}

export function defaultSchema(conn) {
  if (conn.kind === 'sqlite') return 'main';
  if (conn.kind === 'mysql') return conn.database || '';
  return 'public';
}
