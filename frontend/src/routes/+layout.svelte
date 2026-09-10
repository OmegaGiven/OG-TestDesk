<script>
  import '../lib/mockTauri.js';
  import '../lib/styles/tokens.css';
  import '../lib/styles/ui.css';
  import { onMount } from 'svelte';
  import { theme, applyTheme } from '../lib/stores.js';

  onMount(() => {
    applyTheme($theme);
  });
</script>

<slot />
