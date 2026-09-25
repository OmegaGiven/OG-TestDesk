import adapter from '@sveltejs/adapter-static';

// Only set for the static "try it in your browser" demo build published to
// docs/try/ on GitHub Pages (a subpath, not the site root) — unset (empty
// string) for the real Tauri app build, which loads from a local file root.
const base = process.env.OGTD_BASE_PATH || '';

/** @type {import('@sveltejs/kit').Config} */
const config = {
  kit: {
    adapter: adapter({
      pages: 'build',
      assets: 'build',
      fallback: 'index.html'
    }),
    paths: { base }
  }
};

export default config;
