<script>
  import { onMount, onDestroy, createEventDispatcher } from 'svelte';
  import { EditorView, keymap, lineNumbers, highlightActiveLine } from '@codemirror/view';
  import { EditorState, Compartment } from '@codemirror/state';
  import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands';
  import { sql as sqlLang } from '@codemirror/lang-sql';
  import { json as jsonLang } from '@codemirror/lang-json';
  import { syntaxHighlighting, defaultHighlightStyle, bracketMatching } from '@codemirror/language';

  export let value = '';
  export let language = 'sql'; // 'sql' | 'json' | 'text'
  export let readonly = false;
  export let placeholder = '';

  const dispatch = createEventDispatcher();
  let el;
  let view;
  const langComp = new Compartment();
  const roComp = new Compartment();

  function langExt() {
    if (language === 'sql') return sqlLang();
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
        }
      },
      {
        key: 'Mod-s',
        run: () => {
          dispatch('save');
          return true;
        },
        preventDefault: true
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
          highlightActiveLine(),
          syntaxHighlighting(defaultHighlightStyle, { fallback: true }),
          keymap.of([...defaultKeymap, ...historyKeymap, indentWithTab]),
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
            '&': { height: '100%', fontSize: '12.5px' },
            '.cm-scroller': { fontFamily: 'var(--font-mono)', overflow: 'auto' },
            '&.cm-focused': { outline: 'none' },
            '.cm-gutters': {
              background: 'var(--surface-1)',
              color: 'var(--text-muted)',
              border: 'none'
            },
            '.cm-activeLine': { background: 'color-mix(in srgb, var(--text-secondary) 8%, transparent)' },
            '.cm-activeLineGutter': { background: 'transparent' }
          })
        ]
      })
    });
  });

  onDestroy(() => view?.destroy());

  $: if (view && value !== view.state.doc.toString()) {
    view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: value } });
  }
  $: if (view) view.dispatch({ effects: langComp.reconfigure(langExt()) });
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
