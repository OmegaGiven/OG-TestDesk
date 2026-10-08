// CSV/TSV parsing shared by the Inspector (open/paste) and the SQL
// "Import CSV" wizard. RFC 4180-ish: quoted fields with "" escapes, commas
// (or another delimiter) and newlines inside quotes, \n and \r\n endings.

const DELIMITERS = [',', '\t', ';', '|'];

/** @returns {string[][]} */
export function parseCsv(text, delimiter = ',') {
  if (text.charCodeAt(0) === 0xfeff) text = text.slice(1); // Excel's UTF-8 BOM
  const out = [];
  let row = [];
  let field = '';
  let inQuotes = false;
  let i = 0;
  const n = text.length;
  while (i < n) {
    const c = text[i];
    if (inQuotes) {
      if (c === '"') {
        if (text[i + 1] === '"') {
          field += '"';
          i += 2;
          continue;
        }
        inQuotes = false;
        i++;
        continue;
      }
      field += c;
      i++;
      continue;
    }
    if (c === '"' && field === '') {
      inQuotes = true;
      i++;
      continue;
    }
    if (c === delimiter) {
      row.push(field);
      field = '';
      i++;
      continue;
    }
    if (c === '\r') {
      i++;
      continue;
    }
    if (c === '\n') {
      row.push(field);
      out.push(row);
      row = [];
      field = '';
      i++;
      continue;
    }
    field += c;
    i++;
  }
  if (field.length || row.length) {
    row.push(field);
    out.push(row);
  }
  return out.filter((r) => !(r.length === 1 && r[0] === ''));
}

/** Picks the delimiter that splits the first lines into the most consistent column count. */
export function detectDelimiter(text) {
  const sample = text.slice(0, 20000);
  let best = ',';
  let bestScore = 0;
  for (const d of DELIMITERS) {
    const rows = parseCsv(sample, d).slice(0, 20);
    if (rows.length === 0) continue;
    const width = rows[0].length;
    if (width < 2) continue;
    const consistent = rows.filter((r) => r.length === width).length;
    const score = (consistent / rows.length) * width;
    if (score > bestScore) {
      best = d;
      bestScore = score;
    }
  }
  return best;
}

/**
 * Guesses whether pasted text is CSV rather than (broken) JSON. Anything
 * starting with { or [ is treated as JSON so a JSON typo still shows the
 * JSON error instead of turning into a one-column table.
 */
export function looksLikeCsv(text) {
  const t = text.trim();
  if (!t || t[0] === '{' || t[0] === '[') return false;
  const rows = parseCsv(t, detectDelimiter(t));
  return rows.length >= 2 || (rows[0]?.length ?? 0) >= 2;
}

const INT_RE = /^-?(0|[1-9]\d*)$/;
const REAL_RE = /^-?(0|[1-9]\d*)?(\.\d+)?([eE][+-]?\d+)?$/;

/** 'integer' | 'real' | 'boolean' | 'text' for a column's raw string values. */
export function inferType(values) {
  const sample = values.filter((v) => v !== '' && v != null).slice(0, 500);
  if (!sample.length) return 'text';
  // Leading zeros (zip codes, account numbers) and integers past 2^53
  // (snowflake IDs) would be corrupted by Number(), so they stay text.
  if (sample.every((v) => INT_RE.test(v) && Number.isSafeInteger(Number(v)))) return 'integer';
  const isReal = (v) =>
    REAL_RE.test(v) &&
    /\d/.test(v) &&
    !/^-?0\d/.test(v) &&
    (/[.eE]/.test(v) || Number.isSafeInteger(Number(v)));
  if (sample.every(isReal)) return 'real';
  if (sample.every((v) => /^(true|false)$/i.test(v))) return 'boolean';
  return 'text';
}

export function coerce(inferred, raw) {
  if (raw === '' || raw == null) return null;
  if (inferred === 'integer' || inferred === 'real') return Number(raw);
  if (inferred === 'boolean') return /^true$/i.test(raw);
  return raw;
}

/** Header cells → unique, non-empty keys ("id", "id_2", "column_3"). */
export function headerKeys(cells) {
  const seen = new Map();
  return cells.map((c, i) => {
    const base = String(c ?? '').trim() || `column_${i + 1}`;
    const n = (seen.get(base) || 0) + 1;
    seen.set(base, n);
    return n === 1 ? base : `${base}_${n}`;
  });
}

/**
 * CSV text → array of row objects with typed values, ready for the
 * Inspector. Short rows are padded with null; extra cells get column_N keys.
 * @returns {{ rows: object[], columns: string[], delimiter: string, types: string[] }}
 */
export function csvToObjects(text, { delimiter = detectDelimiter(text), header = true } = {}) {
  const grid = parseCsv(text, delimiter);
  if (!grid.length) return { rows: [], columns: [], delimiter, types: [] };
  const width = Math.max(...grid.map((r) => r.length));
  const head = header ? grid[0] : [];
  const body = header ? grid.slice(1) : grid;
  const columns = headerKeys(Array.from({ length: width }, (_, i) => head[i] ?? ''));
  const types = columns.map((_, i) => inferType(body.map((r) => r[i])));
  const rows = body.map((r) => Object.fromEntries(columns.map((k, i) => [k, coerce(types[i], r[i] ?? null)])));
  return { rows, columns, delimiter, types };
}

export const delimiterName = (d) => ({ ',': 'comma', '\t': 'tab', ';': 'semicolon', '|': 'pipe' })[d] || d;
