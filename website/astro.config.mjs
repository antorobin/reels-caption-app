// @ts-check
import { defineConfig } from 'astro/config';

// Static output — deploys as plain files to Cloudflare Pages.
// Build command:  npm run build
// Output dir:     dist
export default defineConfig({
  site: 'https://kraftreel.ai',
  output: 'static',
  build: {
    inlineStylesheets: 'auto',
  },
  devToolbar: {
    enabled: false,
  },
});
