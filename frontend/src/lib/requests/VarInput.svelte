<script>
  import { createEventDispatcher, tick } from 'svelte';
  // Drop-in replacement for a text <input> that pops a filterable dropdown
  // of {{var}} names (active environment + pm.globals, from the parent)
  // as soon as you type "{{" — so you don't have to remember/retype a
  // variable's exact name. Forwards `input`/`change`/`keydown` like a
  // plain input so existing on:xyz handlers at call sites keep working;
  // `keydown` carries the original KeyboardEvent as `event.detail`.
  export let value = '';
  export let placeholder = '';
  export let cls = '';
  export let vars = [];

  const dispatch = createEventDispatcher();
  let el;
  let open = false;
  let filtered = [];
  let selIdx = 0;
  let triggerStart = -1;

  function checkTrigger() {
    if (!el || !vars.length) {
      open = false;
      return;
    }
    const pos = el.selectionStart ?? value.length;
    const before = value.slice(0, pos);
    const m = before.match(/\{\{([A-Za-z0-9_.]*)$/);
    if (!m) {
      open = false;
      return;
    }
    triggerStart = pos - m[0].length;
    const q = m[1].toLowerCase();
    filtered = vars.filter((v) => v.toLowerCase().includes(q)).slice(0, 20);
    selIdx = 0;
    open = filtered.length > 0;
  }

  async function pick(v) {
    const pos = el.selectionStart ?? value.length;
    let after = value.slice(pos);
    if (after.startsWith('}}')) after = after.slice(2);
    value = value.slice(0, triggerStart) + '{{' + v + '}}' + after;
    open = false;
    await tick();
    const caret = triggerStart + v.length + 4;
    el.focus();
    el.setSelectionRange(caret, caret);
    dispatch('input');
  }

  function onInput() {
    checkTrigger();
    dispatch('input');
  }
  function onKeydown(e) {
    if (open) {
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        selIdx = (selIdx + 1) % filtered.length;
        return;
      }
      if (e.key === 'ArrowUp') {
        e.preventDefault();
        selIdx = (selIdx - 1 + filtered.length) % filtered.length;
        return;
      }
      if (e.key === 'Enter' || e.key === 'Tab') {
        e.preventDefault();
        pick(filtered[selIdx]);
        return;
      }
      if (e.key === 'Escape') {
        open = false;
        return;
      }
    }
    dispatch('keydown', e);
  }
  function onBlur() {
    // Let a dropdown mousedown (which fires before blur's click) land first.
    setTimeout(() => (open = false), 150);
  }
</script>

<div class="var-input-wrap">
  <input
    bind:this={el}
    class={cls}
    {placeholder}
    bind:value
    on:input={onInput}
    on:keydown={onKeydown}
    on:click={checkTrigger}
    on:blur={onBlur}
    on:change
  />
  {#if open}
    <div class="var-dd">
      {#each filtered as v, i (v)}
        <button type="button" class:sel={i === selIdx} on:mousedown|preventDefault={() => pick(v)}>
          {'{{' + v + '}}'}
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .var-input-wrap {
    position: relative;
    flex: 1;
    min-width: 0;
    display: flex;
  }
  .var-input-wrap :global(input) {
    width: 100%;
  }
  .var-dd {
    position: absolute;
    top: 100%;
    left: 0;
    z-index: 50;
    min-width: 180px;
    max-height: 220px;
    overflow-y: auto;
    background: var(--surface-1);
    border: 1px solid var(--border-strong, var(--border));
    border-radius: 6px;
    box-shadow: var(--shadow-pop, 0 4px 16px rgba(0, 0, 0, 0.3));
    padding: 4px;
    margin-top: 2px;
  }
  .var-dd button {
    display: block;
    width: 100%;
    text-align: left;
    background: none;
    border: none;
    font-family: var(--font-mono);
    font-size: 11.5px;
    color: var(--text-primary);
    padding: 5px 8px;
    border-radius: 4px;
    cursor: pointer;
  }
  .var-dd button.sel,
  .var-dd button:hover {
    background: var(--surface-3);
  }
</style>
