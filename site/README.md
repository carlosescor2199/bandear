# BandEar website

The landing page published at **https://carlosescor2199.github.io/bandear/**. Plain HTML, CSS and
JavaScript modules: no build step, no package manager, no external requests except the GitHub
API call that finds the latest release.

| Path | What it is |
|------|------------|
| `index.html`, `style.css` | The page. Every visible string is tagged for translation. |
| `404.html` | Shown by GitHub Pages for any missing path. Self-contained on purpose (works at any depth). |
| `js/i18n.js` | English and Spanish copy and the language toggle. |
| `js/downloads.js` | Finds the latest release and points the download buttons at the visitor's installer. |
| `js/main.js`, `js/scene-*.js`, `js/model-mixer.js`, `js/three-setup.js` | Decorative three.js scenes (only with WebGL 2; the static design stays otherwise). |
| `assets/` | Brand marks, app screenshots, the social card (`og.png`) and the self-hosted fonts. |
| `vendor/three/` | three.js, vendored (see below). |
| `dev/` | Local preview server and the social-card source. Not deployed. |
| `tests/` | Node tests for the pure helpers. Not deployed. |

## Preview locally

```sh
node site/dev/serve.mjs          # http://127.0.0.1:5300/
```

Opening `index.html` from the file system also works, without the 3D scenes and the download
links (modules do not load from `file://`).

Run the tests (Node 22 or newer; pass the glob, a bare folder path fails on Node 24):

```sh
node --test 'site/tests/*.test.mjs'
```

## Languages

`js/i18n.js` holds one dictionary per language. Markup names its strings with attributes:
`data-i18n="key"` (text), `data-i18n-html="key"` (markup with `<em>`, `<a>`…) and
`data-i18n-attr="alt=key;aria-label=key"` (attributes). The first visit follows the browser
(any `es*` language picks Spanish); the toggle remembers the choice in `localStorage`. The tests
fail if a key is missing in either language or if the markup names a key the dictionary lacks.

## Downloads

`js/downloads.js` asks `api.github.com/repos/carlosescor2199/bandear/releases/latest` (cached for
30 minutes; the anonymous limit is 60 requests per hour), matches the installers by file-name
suffix (`_x64-setup.exe`, `_aarch64.dmg`, `_x64.dmg`, `_amd64.AppImage`, `_amd64.deb`,
`_android.apk`) and offers the visitor's system first. Keep those suffixes when renaming release
files. Every link in the markup already opens the latest release page, so an API failure only
loses the direct links.

## Deployment

`.github/workflows/pages.yml` runs the tests and publishes on every push to `main` that touches
`site/**` (or by hand from the Actions tab). It copies `site/` without `dev/`, `tests/` and this
README to a staging folder and uploads that folder as the Pages artifact.

One-time setup: **Settings → Pages → Build and deployment → Source: GitHub Actions**.

## Connecting a custom domain later

1. Add `site/CNAME` containing just the domain, e.g. `bandear.app` (the workflow deploys it with
   the rest of the site).
2. DNS at your registrar:
   - apex domain: four `A` records to `185.199.108.153`, `185.199.109.153`, `185.199.110.153`,
     `185.199.111.153` (optionally `AAAA` to `2606:50c0:8000::153`, `2606:50c0:8001::153`,
     `2606:50c0:8002::153`, `2606:50c0:8003::153`);
   - `www`: a `CNAME` record to `carlosescor2199.github.io`.
3. **Settings → Pages → Custom domain**: enter the domain, wait for the DNS check, then tick
   **Enforce HTTPS** once the certificate is issued.
4. Update the absolute URLs that still name GitHub Pages: the `canonical`, `og:url`, `og:image`
   and `twitter:image` tags in `index.html`, and the root `README.md` link. `404.html` already
   links home to `/` on any host that is not `*.github.io`.

## Vendored three.js

`vendor/three/three.module.min.js` is three.js 0.186.1 (MIT) from the official npm package:
`build/three.module.js` and `three.core.js` bundled and minified into one ES module.
`vendor/three/LICENSE` is its license and `vendor/three/SOURCE.txt` records the exact source and
command. It is vendored so the site makes no third-party requests and needs no build step. To
update it, rebuild the same way from a newer release, update `SOURCE.txt` and check the 3D
scenes.

## Social card

`assets/og.png` (1200×630) and `assets/brand/apple-touch-icon.png` (180×180) are screenshots of
`dev/og-card.html`; the comment at the top of that file explains how to regenerate them.
