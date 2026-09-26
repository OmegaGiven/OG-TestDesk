import { test } from 'node:test';
import assert from 'node:assert/strict';
import { parseScript, splitStatements } from '../../src/lib/sql/splitSql.js';

test('plain statements split on ;', () => {
  assert.deepEqual(splitStatements('SELECT 1; SELECT 2;\nSELECT 3'), ['SELECT 1', 'SELECT 2', 'SELECT 3']);
});

test('; inside strings and comments does not split', () => {
  const sql = "SELECT 'a;b', \"c;d\", `e;f`; -- x;y\nSELECT 2 /* ; */";
  assert.deepEqual(splitStatements(sql), ["SELECT 'a;b', \"c;d\", `e;f`", '-- x;y\nSELECT 2 /* ; */']);
});

test("escaped quotes don't end a string", () => {
  assert.deepEqual(splitStatements("SELECT 'it''s; fine'; SELECT 'a\\'b;c'"), ["SELECT 'it''s; fine'", "SELECT 'a\\'b;c'"]);
});

test('comment-only chunks are dropped', () => {
  assert.deepEqual(splitStatements('-- just a note;\n;;SELECT 1;'), ['SELECT 1']);
});

test('MySQL DELIMITER keeps a procedure body together and is not sent', () => {
  const sql = [
    'DROP PROCEDURE IF EXISTS add_two;',
    'DELIMITER $$',
    'CREATE PROCEDURE add_two(IN q INT)',
    'BEGIN',
    '  INSERT INTO t (qty) VALUES (q);',
    '  INSERT INTO t (qty) VALUES (q * 10);',
    'END$$',
    'DELIMITER ;',
    'CALL add_two(2);'
  ].join('\n');
  const { statements, hadDelimiter } = parseScript(sql);
  assert.equal(hadDelimiter, true);
  assert.deepEqual(statements, [
    'DROP PROCEDURE IF EXISTS add_two',
    'CREATE PROCEDURE add_two(IN q INT)\nBEGIN\n  INSERT INTO t (qty) VALUES (q);\n  INSERT INTO t (qty) VALUES (q * 10);\nEND',
    'CALL add_two(2)'
  ]);
  assert.ok(statements.every((s) => !/delimiter/i.test(s)));
});

test('DELIMITER // with a trigger, directive lowercase and indented', () => {
  const sql = '  delimiter //\nCREATE TRIGGER bi BEFORE INSERT ON t FOR EACH ROW\nBEGIN\n  SET NEW.a = 1;\nEND //\ndelimiter ;\nSELECT 1';
  assert.deepEqual(splitStatements(sql), [
    'CREATE TRIGGER bi BEFORE INSERT ON t FOR EACH ROW\nBEGIN\n  SET NEW.a = 1;\nEND',
    'SELECT 1'
  ]);
});

test('DELIMITER after a leading comment still counts', () => {
  const { statements, hadDelimiter } = parseScript('-- setup\nDELIMITER $$\nCREATE FUNCTION f() RETURNS INT DETERMINISTIC BEGIN RETURN 1; END$$');
  assert.equal(hadDelimiter, true);
  assert.deepEqual(statements, ['CREATE FUNCTION f() RETURNS INT DETERMINISTIC BEGIN RETURN 1; END']);
});

test('the word delimiter inside a statement is not a directive', () => {
  const { statements, hadDelimiter } = parseScript("SELECT 'delimiter $$' AS x;\nSELECT 2");
  assert.equal(hadDelimiter, false);
  assert.equal(statements.length, 2);
});

test('Postgres $$ and $tag$ bodies stay whole', () => {
  const sql = [
    'CREATE FUNCTION triple(x int) RETURNS int LANGUAGE plpgsql AS $$',
    'DECLARE r int;',
    'BEGIN r := x * 3; RETURN r; END',
    '$$;',
    'CREATE PROCEDURE p() LANGUAGE plpgsql AS $body$ BEGIN PERFORM 1; PERFORM $$x;y$$; END $body$;',
    'SELECT triple(4);'
  ].join('\n');
  const s = splitStatements(sql);
  assert.equal(s.length, 3);
  assert.ok(s[0].endsWith('$$') && s[0].includes('RETURN r;'));
  assert.ok(s[1].includes('PERFORM $$x;y$$; END $body$'));
  assert.equal(s[2], 'SELECT triple(4)');
});

test('$1-style parameters are not dollar quotes', () => {
  assert.deepEqual(splitStatements('SELECT $1; SELECT $2'), ['SELECT $1', 'SELECT $2']);
});
