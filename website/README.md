# KraftReel.ai — marketing website

Single-page product site for **KraftReel.ai**, built with [Astro](https://astro.build).
Static output, zero runtime — deploys to **Cloudflare Pages** as plain files.

## Develop

```bash
cd website
npm install
npm run dev        # http://localhost:4321
```

## Build

```bash
npm run build      # -> website/dist
npm run preview    # serve the built site locally
```

## Deploy to Cloudflare Pages

**Dashboard → Workers & Pages → Create → Pages → Connect to Git**

| Setting | Value |
| --- | --- |
| Production branch | `develop` (or your default) |
| Framework preset | `Astro` |
| Build command | `npm run build` |
| Build output directory | `dist` |
| Root directory | `website` |
| Node version | set `NODE_VERSION = 20` in **Settings → Environment variables** |

`public/_headers` is copied into `dist/` and applied automatically by Cloudflare Pages
(security headers + long-cache for hashed `/_astro/*` assets).

### Or deploy with Wrangler (no Git connection)

```bash
npm run build
npx wrangler pages deploy dist --project-name kraftreel-site
```

## Editing content

Almost everything lives in [`src/config/site.js`](src/config/site.js):

- **Download links** — `downloads[]`. The Windows entry points at
  `https://github.com/antorobin/reels-caption-app/releases/latest/download/KraftReel_<version>_x64-setup.exe`.
  Update `site.version` and the asset filenames when the release workflow publishes
  a new signed installer, or replace `url` with any direct link (R2, CDN, etc.).
- **Feature cards** — `features[]`
- **Workflow steps** — `workflow[]`
- **Audiences** — `audiences[]`

When macOS / Linux builds ship, change their `status` from `'soon'` to
`'available'` and add a `url`.

## Structure

```
website/
├─ astro.config.mjs        static config, site URL
├─ public/                 favicon, og image, _headers, robots.txt
└─ src/
   ├─ config/site.js       ← all copy, links, versions
   ├─ layouts/Base.astro   <head>, fonts, meta/OG tags
   ├─ styles/global.css    design tokens + primitives
   ├─ components/          Nav, Hero, Features, Workflow, Downloads, CreatorSection, Footer
   └─ pages/index.astro    the one page
```
