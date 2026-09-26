// Splits an editor script into the statements to send to the server, one
// at a time. Understands:
//   - line (--) and block (/* */) comments
//   - '…', "…" and `…` quoting, with doubled-quote and backslash escapes
//   - Postgres dollar quoting ($$…$$, $body$…$body$), so a function body
//     full of `;` stays one statement
//   - MySQL `DELIMITER xx` lines, the client-side convention every MySQL
//     tool uses for procedure/trigger bodies. The directive itself is never
//     sent (the server doesn't know it); it just changes the terminator.

const DELIMITER_RE = /^[ \t]*delimiter[ \t]+(\S+)[ \t]*(?:\r?\n|$)/i;
const DOLLAR_RE = /^\$([A-Za-z_][A-Za-z0-9_]*)?\$/;

/** @returns {{ statements: string[], hadDelimiter: boolean }} */
export function parseScript(sql) {
  const statements = [];
  let cur = '';
  let hasCode = false; // anything besides whitespace/comments in `cur`
  let delim = ';';
  let hadDelimiter = false;
  let i = 0;
  const n = sql.length;

  const flush = () => {
    if (hasCode && cur.trim()) statements.push(cur.trim());
    cur = '';
    hasCode = false;
  };

  while (i < n) {
    if (!hasCode && (i === 0 || sql[i - 1] === '\n')) {
      const m = DELIMITER_RE.exec(sql.slice(i, i + 200));
      if (m) {
        delim = m[1];
        hadDelimiter = true;
        cur = '';
        i += m[0].length;
        continue;
      }
    }

    if (sql.startsWith(delim, i)) {
      flush();
      i += delim.length;
      continue;
    }

    const c = sql[i];
    if (c === '-' && sql[i + 1] === '-') {
      const nl = sql.indexOf('\n', i);
      const end = nl === -1 ? n : nl;
      cur += sql.slice(i, end);
      i = end;
      continue;
    }
    if (c === '/' && sql[i + 1] === '*') {
      const close = sql.indexOf('*/', i + 2);
      const end = close === -1 ? n : close + 2;
      cur += sql.slice(i, end);
      i = end;
      continue;
    }
    if (c === "'" || c === '"' || c === '`') {
      let j = i + 1;
      while (j < n) {
        if (sql[j] === c) {
          if (sql[j + 1] === c) {
            j += 2;
            continue;
          }
          j++;
          break;
        }
        if (sql[j] === '\\' && c !== '`') {
          j += 2;
          continue;
        }
        j++;
      }
      cur += sql.slice(i, j);
      hasCode = true;
      i = j;
      continue;
    }
    if (c === '$') {
      const m = DOLLAR_RE.exec(sql.slice(i, i + 100));
      if (m) {
        const tag = m[0];
        const close = sql.indexOf(tag, i + tag.length);
        const end = close === -1 ? n : close + tag.length;
        cur += sql.slice(i, end);
        hasCode = true;
        i = end;
        continue;
      }
    }

    cur += c;
    if (!/\s/.test(c)) hasCode = true;
    i++;
  }
  flush();
  return { statements, hadDelimiter };
}

export function splitStatements(sql) {
  return parseScript(sql).statements;
}
