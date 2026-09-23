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

  function langExt() {
    if (language === 'sql') return sqlLang(schema ? { schema, upperCaseKeywords: true } : { upperCaseKeywords: true });
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
