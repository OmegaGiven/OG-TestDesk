// Named colour themes. Each is a set of CSS-variable overrides applied on
// top of tokens.css, per light/dark mode. "default" clears all overrides.
// Only the variables that differ from the base need listing.

export const FONT_SANS = [
  ['system', 'System UI', "-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif"],
  ['inter', 'Inter', "'Inter', -apple-system, BlinkMacSystemFont, sans-serif"],
  ['helvetica', 'Helvetica', "'Helvetica Neue', Helvetica, Arial, sans-serif"],
  ['ibm', 'IBM Plex Sans', "'IBM Plex Sans', system-ui, sans-serif"],
  ['literata', 'Serif', "'Iowan Old Style', 'Palatino Linotype', 'Times New Roman', Georgia, serif"]
];

export const FONT_MONO = [
  ['system', 'System Mono', "ui-monospace, 'SF Mono', Menlo, Consolas, monospace"],
  ['jetbrains', 'JetBrains Mono', "'JetBrains Mono', ui-monospace, monospace"],
  ['fira', 'Fira Code', "'Fira Code', ui-monospace, monospace"],
  ['ibm', 'IBM Plex Mono', "'IBM Plex Mono', ui-monospace, monospace"],
  ['courier', 'Courier', "'Courier New', Courier, monospace"]
];

export function fontStack(list, key) {
  return (list.find((f) => f[0] === key) || list[0])[2];
}

export const COLOR_THEMES = {
  default: { label: 'Default', light: {}, dark: {} },

  nord: {
    label: 'Nord',
    light: {
      '--surface-0': '#e5e9f0',
      '--surface-1': '#eceff4',
      '--surface-2': '#ffffff',
      '--surface-3': '#dde3ee',
      '--text-primary': '#2e3440',
      '--text-secondary': '#4c566a',
      '--text-muted': '#7b869c',
      '--border': 'rgba(46,52,64,0.12)',
      '--border-strong': 'rgba(46,52,64,0.22)',
      '--tool-sql-tint': '#dbe6f2',
      '--tool-sql-text': '#5e81ac',
      '--tool-requests-tint': '#dfeae1',
      '--tool-requests-text': '#4f7a52',
      '--tool-inspector-tint': '#e6e0ee',
      '--tool-inspector-text': '#7a6a9e',
      '--ok': '#a3be8c',
      '--warn': '#ebcb8b',
      '--danger': '#bf616a',
      '--j-key': '#8f6fb3',
      '--j-string': '#4f894c',
      '--j-number': '#b48a3f',
      '--j-bool': '#b0526a'
    },
    dark: {
      '--surface-0': '#2e3440',
      '--surface-1': '#323a48',
      '--surface-2': '#3b4252',
      '--surface-3': '#434c5e',
      '--text-primary': '#eceff4',
      '--text-secondary': '#c5cddd',
      '--text-muted': '#8b95a8',
      '--border': 'rgba(236,239,244,0.10)',
      '--border-strong': 'rgba(236,239,244,0.20)',
      '--tool-sql-tint': '#4c6a8f4d',
      '--tool-sql-text': '#88c0d0',
      '--tool-requests-tint': '#5b7a5c4d',
      '--tool-requests-text': '#a3be8c',
      '--tool-inspector-tint': '#6f5f994d',
      '--tool-inspector-text': '#b48ead',
      '--ok': '#a3be8c',
      '--warn': '#ebcb8b',
      '--danger': '#bf616a',
      '--j-key': '#b48ead',
      '--j-string': '#a3be8c',
      '--j-number': '#d08770',
      '--j-bool': '#bf616a'
    }
  },

  sepia: {
    label: 'Sepia',
    light: {
      '--surface-0': '#efe6d5',
      '--surface-1': '#f6efe1',
      '--surface-2': '#fffaf0',
      '--surface-3': '#e7dcc4',
      '--text-primary': '#3b2f22',
      '--text-secondary': '#6b5a44',
      '--text-muted': '#9a8873',
      '--border': 'rgba(59,47,34,0.14)',
      '--border-strong': 'rgba(59,47,34,0.26)',
      '--tool-sql-tint': '#e3e0d0',
      '--tool-sql-text': '#4a6a6b',
      '--tool-requests-tint': '#e6e2c8',
      '--tool-requests-text': '#5f6b34',
      '--tool-inspector-tint': '#ebdfd0',
      '--tool-inspector-text': '#7a5a4a',
      '--j-key': '#8a5a3c',
      '--j-string': '#5f6b34',
      '--j-number': '#a5683a',
      '--j-bool': '#9a4b4b'
    },
    dark: {
      '--surface-0': '#241d16',
      '--surface-1': '#2b231b',
      '--surface-2': '#332a20',
      '--surface-3': '#3d3226',
      '--text-primary': '#efe6d5',
      '--text-secondary': '#c9bca4',
      '--text-muted': '#938672',
      '--border': 'rgba(239,230,213,0.10)',
      '--border-strong': 'rgba(239,230,213,0.20)',
      '--tool-sql-text': '#8fb7b0',
      '--tool-requests-text': '#b0c47a',
      '--tool-inspector-text': '#d0a58f',
      '--j-key': '#d0a58f',
      '--j-string': '#b0c47a',
      '--j-number': '#e0a458',
      '--j-bool': '#e08f8f'
    }
  },

  contrast: {
    label: 'High contrast',
    light: {
      '--surface-0': '#ffffff',
      '--surface-1': '#ffffff',
      '--surface-2': '#ffffff',
      '--surface-3': '#e8e8e8',
      '--text-primary': '#000000',
      '--text-secondary': '#1c1c1c',
      '--text-muted': '#444444',
      '--border': 'rgba(0,0,0,0.4)',
      '--border-strong': 'rgba(0,0,0,0.7)',
      '--tool-sql-text': '#0033aa',
      '--tool-requests-text': '#006600',
      '--tool-inspector-text': '#5500aa'
    },
    dark: {
      '--surface-0': '#000000',
      '--surface-1': '#000000',
      '--surface-2': '#0a0a0a',
      '--surface-3': '#1e1e1e',
      '--text-primary': '#ffffff',
      '--text-secondary': '#e8e8e8',
      '--text-muted': '#b0b0b0',
      '--border': 'rgba(255,255,255,0.4)',
      '--border-strong': 'rgba(255,255,255,0.75)',
      '--tool-sql-text': '#7db4ff',
      '--tool-requests-text': '#7dff9e',
      '--tool-inspector-text': '#c79dff'
    }
  },

  ocean: {
    label: 'Ocean',
    light: {
      '--surface-0': '#e4edf1',
      '--surface-1': '#eef4f7',
      '--surface-2': '#ffffff',
      '--surface-3': '#d7e4ea',
      '--text-primary': '#14303a',
      '--text-secondary': '#3d5a66',
      '--text-muted': '#6f8b95',
      '--border': 'rgba(20,48,58,0.12)',
      '--border-strong': 'rgba(20,48,58,0.22)',
      '--tool-sql-text': '#1f6f8b',
      '--tool-requests-text': '#2f7d6a',
      '--tool-inspector-text': '#5b6bad'
    },
    dark: {
      '--surface-0': '#0d1b26',
      '--surface-1': '#12242f',
      '--surface-2': '#182f3c',
      '--surface-3': '#23404e',
      '--text-primary': '#e2eef2',
      '--text-secondary': '#a9c3cc',
      '--text-muted': '#6f8b95',
      '--border': 'rgba(226,238,242,0.10)',
      '--border-strong': 'rgba(226,238,242,0.20)',
      '--tool-sql-text': '#5fb8d4',
      '--tool-requests-text': '#5fc9a8',
      '--tool-inspector-text': '#9aa8e6'
    }
  }
};

export function applyColorTheme(name, mode) {
  const root = document.documentElement;
  const preset = COLOR_THEMES[name] || COLOR_THEMES.default;
  const all = new Set([
    ...Object.keys(COLOR_THEMES.nord.light),
    ...Object.keys(COLOR_THEMES.nord.dark),
    ...Object.keys(COLOR_THEMES.sepia.light),
    ...Object.keys(COLOR_THEMES.ocean.dark),
    ...Object.keys(COLOR_THEMES.contrast.dark)
  ]);
  for (const v of all) root.style.removeProperty(v);
  const vars = mode === 'dark' ? preset.dark : preset.light;
  for (const [k, val] of Object.entries(vars)) root.style.setProperty(k, val);
}
