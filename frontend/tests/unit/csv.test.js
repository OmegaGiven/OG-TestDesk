import { test } from 'node:test';
import assert from 'node:assert/strict';
import { parseCsv, detectDelimiter, looksLikeCsv, inferType, csvToObjects, headerKeys } from '../../src/lib/csv.js';

test('quoted fields keep commas, quotes and newlines', () => {
  const rows = parseCsv('id,note\n1,"a, b"\n2,"say ""hi"""\n3,"two\nlines"\n');
  assert.deepEqual(rows, [['id', 'note'], ['1', 'a, b'], ['2', 'say "hi"'], ['3', 'two\nlines']]);
});

test('CRLF endings and an Excel BOM', () => {
  assert.deepEqual(parseCsv('﻿id,name\r\n1,Ava\r\n'), [['id', 'name'], ['1', 'Ava']]);
});

test('a stray quote mid-field is literal, not the start of a quoted field', () => {
  assert.deepEqual(parseCsv('size,item\n5",ruler'), [['size', 'item'], ['5"', 'ruler']]);
});

test('delimiter detection: comma, tab, semicolon, pipe', () => {
  assert.equal(detectDelimiter('a,b,c\n1,2,3'), ',');
  assert.equal(detectDelimiter('a\tb\tc\n1\t2\t3'), '\t');
  assert.equal(detectDelimiter('a;b;c\n1,5;2,5;3'), ';'); // European decimals
  assert.equal(detectDelimiter('a|b\n1|2'), '|');
});

test('looksLikeCsv: CSV yes, JSON (even broken) no', () => {
  assert.equal(looksLikeCsv('id,name\n1,Ava'), true);
  assert.equal(looksLikeCsv('one\ntwo'), true);
  assert.equal(looksLikeCsv('{"a": 1,,}'), false);
  assert.equal(looksLikeCsv('[1, 2'), false);
  assert.equal(looksLikeCsv('hello'), false);
  assert.equal(looksLikeCsv('   '), false);
});

test('type inference never corrupts values', () => {
  assert.equal(inferType(['1', '-2', '30']), 'integer');
  assert.equal(inferType(['1.5', '2', '-3e2']), 'real');
  assert.equal(inferType(['true', 'FALSE']), 'boolean');
  assert.equal(inferType(['01234', '90210']), 'text'); // zip codes keep leading zeros
  assert.equal(inferType(['1234567890123456789']), 'text'); // past 2^53
  assert.equal(inferType(['00.5']), 'text');
  assert.equal(inferType(['', '']), 'text');
});

test('headerKeys dedupes and fills blanks', () => {
  assert.deepEqual(headerKeys(['id', '', 'id', ' name ']), ['id', 'column_2', 'id_2', 'name']);
});

test('csvToObjects: typed values, nulls, ragged rows', () => {
  const { rows, columns, delimiter, types } = csvToObjects('id\tzip\tactive\tnote\n1\t01234\ttrue\t\n2\t90210\tfalse\tok\textra\n3');
  assert.equal(delimiter, '\t');
  assert.deepEqual(columns, ['id', 'zip', 'active', 'note', 'column_5']);
  assert.deepEqual(types, ['integer', 'text', 'boolean', 'text', 'text']);
  assert.deepEqual(rows, [
    { id: 1, zip: '01234', active: true, note: null, column_5: null },
    { id: 2, zip: '90210', active: false, note: 'ok', column_5: 'extra' },
    { id: 3, zip: null, active: null, note: null, column_5: null }
  ]);
});

test('csvToObjects without a header row', () => {
  const { rows, columns } = csvToObjects('1,Ava\n2,Liam', { header: false });
  assert.deepEqual(columns, ['column_1', 'column_2']);
  assert.deepEqual(rows[1], { column_1: 2, column_2: 'Liam' });
});
