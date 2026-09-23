<script>
  import { onMount, onDestroy, createEventDispatcher } from 'svelte';
  import { EditorView, keymap, lineNumbers, highlightActiveLine } from '@codemirror/view';
  import { EditorState, Compartment } from '@codemirror/state';
  import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands';
  import { sql as sqlLang } from '@codemirror/lang-sql';
  import { json as jsonLang } from '@codemirror/lang-json';
  import { syntaxHighlighting, HighlightStyle, bracketMatching } from '@codemirror/language';
  import { autocompletion, completionKeymap, closeBrackets, closeBracketsKeymap } from '@codemirror/autocomplete';
  import { tags as t } from '@lezer/highlight';

  // Theme-following highlight — colours are CSS vars, so they update live
  // when the app theme changes without rebuilding the editor.
  const appHighlight = HighlightStyle.define([
    { tag: [t.keyword, t.operatorKeyword, t.modifier], color: 'var(--j-key)' },
    { tag: [t.string, t.special(t.string)], color: 'var(--j-string)' },
    { tag: [t.number, t.bool, t.null], color: 'var(--j-number)' },
    { tag: [t.propertyName, t.definition(t.propertyName)], color: 'var(--j-key)' },
    { tag: [t.comment], color: 'var(--text-muted)', fontStyle: 'italic' },
    { tag: [t.function(t.variableName), t.function(t.propertyName)], color: 'var(--tool-sql-text)' },
    { tag: [t.typeName, t.className], color: 'var(--j-bool)' },
    { tag: [t.punctuation, t.separator, t.bracket], color: 'var(--text-secondary)' }
  ]);

  export let value = '';
  export let language = 'sql'; // 'sql' | 'json' | 'text'
  export let readonly = false;
  export let placeholder = '';
  // { tableName: [columnName, ...] } — schema-aware completion for the
  // active connection. Table-name-only (empty arrays) still gets you
  // completion after FROM/JOIN; column arrays add per-table column
  // completion. Optional — plain keyword completion works without it.
  export let schema = null;

  const dispatch = createEventDispatcher();
  let el;
  let view;
  const langComp = new Compartment();
  const roComp = new Compartment();

  // The built-in schema completion (@codemirror/lang-sql) only resolves
  // "ident." when `ident` is a literal table/schema name from `schema` —
  // it has no idea `o.` means `orders` in `FROM orders o`. Aliases are
  // how most real SQL is written, so without this the "." dropdown looks
  // broken for the common case even though it technically works for the
  // rare one (typing the full table name out again). Registered as an
  // *additional* completion source (language.data.of, same mechanism
  // lang-sql itself uses) alongside the built-in one rather than
  // replacing it, so real-table-name and keyword completion are untouched.
  const RESERVED_AFTER_TABLE = new Set([
    'where', 'group', 'order', 'having', 'limit', 'on', 'join', 'inner', 'left',
    'right', 'full', 'outer', 'union', 'set', 'values', 'returning', 'window',
    'select', 'from', 'and', 'or', 'offset'
  ]);
  function aliasSchemaCompletion(tableSchema) {
    return (context) => {
      const word = context.matchBefore(/[A-Za-z_]\w*\.\w*/);
      if (!word) return null;
      const dot = word.text.indexOf('.');
      const ident = word.text.slice(0, dot);
      if (tableSchema[ident]) return null; // real table name — let the built-in source handle it
      const aliasRe = /\b(?:from|join)\s+([A-Za-z_][\w.]*)\s+(?:as\s+)?([A-Za-z_]\w*)\b/gi;
      const doc = context.state.doc.toString();
      let table = null;
      let m;
      while ((m = aliasRe.exec(doc))) {
        if (RESERVED_AFTER_TABLE.has(m[2].toLowerCase())) continue;
        if (m[2].toLowerCase() === ident.toLowerCase()) {
          table = m[1].split('.').pop();
          break;
        }
      }
      const cols = table ? tableSchema[table] : null;
      if (!cols || !cols.length) return null;
      return {
        from: word.from + dot + 1,
        options: cols.map((name) => ({ label: name, type: 'property' })),
        validFor: /^\w*$/
      };
    };
  }

  function langExt() {
    if (language === 'sql') {
      const base = sqlLang(schema ? { schema, upperCaseKeywords: true } : { upperCaseKeywords: true });
      if (!schema) return base;
      return [base, base.language.data.of({ autocomplete: aliasSchemaCompletion(schema) })];
    }
    if (language === 'json') return jsonLang();
    return [];
  }

  onMount(() => {
    const runKey = keymap.of([
      {
        key: 'Mod-Enter',
        run: () => {
          dispatch('run');
          return true;
        },
        preventDefault: true
      },
      // Explicit Ctrl-Enter alongside Mod-Enter: on macOS "Mod" maps to
      // Cmd, so muscle-memory Ctrl+Enter (common coming from
      // Windows/Linux tools) is otherwise never intercepted — and
      // unbound Ctrl+Enter inside a contentEditable is a known WebKit
      // quirk that pops the native right-click/context menu instead of
      // doing nothing. Binding it explicitly (harmless no-op on
      // Windows/Linux, where it's identical to Mod-Enter already) stops
      // that at the source.
      {
        key: 'Ctrl-Enter',
        run: () => {
          dispatch('run');
          return true;
        },
        preventDefault: true
      },
      {
        key: 'Mod-s',
        run: () => {
          dispatch('save');
          return true;
        },
        preventDefault: true
      },
      {
        key: 'Alt-ArrowUp',
        run: () => {
          dispatch('histprev');
          return true;
        }
      },
      {
        key: 'Alt-ArrowDown',
        run: () => {
          dispatch('histnext');
          return true;
        }
      }
    ]);

    view = new EditorView({
      parent: el,
      state: EditorState.create({
        doc: value,
        extensions: [
          lineNumbers(),
          history(),
          bracketMatching(),
          closeBrackets(),
          autocompletion(),
          highlightActiveLine(),
          syntaxHighlighting(appHighlight, { fallback: true }),
          keymap.of([...closeBracketsKeymap, ...completionKeymap, ...defaultKeymap, ...historyKeymap, indentWithTab]),
          runKey,
          langComp.of(langExt()),
          roComp.of(EditorState.readOnly.of(readonly)),
          EditorView.editable.of(!readonly),
          EditorView.lineWrapping,
          EditorView.updateListener.of((u) => {
            if (u.docChanged) {
              value = u.state.doc.toString();
              dispatch('change', value);
            }
          }),
          EditorView.theme({
            '&': {
              height: '100%',
              fontSize: '12.5px',
              color: 'var(--text-primary)',
              backgroundColor: 'var(--surface-2)'
            },
            '.cm-content': { caretColor: 'var(--text-primary)' },
            '.cm-scroller': { fontFamily: 'var(--font-mono)', overflow: 'auto' },
            '&.cm-focused': { outline: 'none' },
            '.cm-cursor, .cm-dropCursor': { borderLeftColor: 'var(--text-primary)' },
            '&.cm-focused .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection': {
              backgroundColor: 'color-mix(in srgb, var(--tool-sql-text) 22%, transparent)'
            },
            '.cm-gutters': {
              background: 'var(--surface-1)',
              color: 'var(--text-muted)',
              border: 'none'
            },
            '.cm-activeLine': {
              background: 'color-mix(in srgb, var(--text-secondary) 8%, transparent)'
            },
            '.cm-activeLineGutter': { background: 'transparent' },
            // CodeMirror's autocomplete tooltip has no default theming of
            // its own — it inherited the page's text color (white, in
            // dark mode) with no matching background, so every row but
            // the selected one (which gets CM's built-in blue highlight)
            // was white text on an effectively-transparent/white
            // background: unreadable.
            '.cm-tooltip': {
              border: '1px solid var(--border-strong, var(--border))',
              backgroundColor: 'var(--surface-1)'
            },
            '.cm-tooltip.cm-tooltip-autocomplete': {
              boxShadow: 'var(--shadow-pop, 0 4px 16px rgba(0, 0, 0, 0.3))'
            },
            '.cm-tooltip.cm-tooltip-autocomplete > ul': {
              backgroundColor: 'var(--surface-1)',
              fontFamily: 'var(--font-mono)',
              maxHeight: '18em'
            },
            '.cm-tooltip.cm-tooltip-autocomplete > ul > li': {
              color: 'var(--text-primary)'
            },
            '.cm-tooltip.cm-tooltip-autocomplete > ul > li[aria-selected]': {
              backgroundColor: 'color-mix(in srgb, var(--tool-sql-text) 30%, var(--surface-2))',
              color: 'var(--text-primary)'
            },
            '.cm-completionLabel': { color: 'var(--text-primary)' },
            '.cm-completionDetail': { color: 'var(--text-muted)', fontStyle: 'normal' },
            '.cm-completionMatchedText': {
              color: 'var(--tool-sql-text)',
              textDecoration: 'none',
              fontWeight: 700
            },
            '.cm-completionIcon': { color: 'var(--text-muted)' },
            '.cm-tooltip.cm-completionInfo': {
              backgroundColor: 'var(--surface-1)',
              color: 'var(--text-primary)',
              border: '1px solid var(--border-strong, var(--border))'
            },
            '.cm-matchingBracket': {
              background: 'color-mix(in srgb, var(--tool-sql-text) 25%, transparent)',
              outline: 'none'
            }
          })
        ]
      })
    });
  });

  onDestroy(() => view?.destroy());

  $: if (view && value !== view.state.doc.toString()) {
    view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: value } });
  }
  $: if (view && (language || schema)) view.dispatch({ effects: langComp.reconfigure(langExt()) });
  $: if (view) view.dispatch({ effects: roComp.reconfigure(EditorState.readOnly.of(readonly)) });

  export function focus() {
    view?.focus();
  }
</script>

<div class="editor" bind:this={el} data-placeholder={placeholder}></div>

<style>
  .editor {
    height: 100%;
    overflow: hidden;
    background: var(--surface-2);
  }
</style>
