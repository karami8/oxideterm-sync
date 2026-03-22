# OxideTerm marketing site

Static **English** landing page for GitHub Pages or any static host. Content is derived from the main repository README and architecture docs.

## Local preview

From the repository root:

```bash
cd website && python3 -m http.server 8080
```

Open `http://localhost:8080`.

## GitHub Pages (Actions)

1. Repository **Settings → Pages → Build and deployment**.
2. Set **Source** to **GitHub Actions**.
3. Push changes under `website/` on `main`; the workflow **Deploy website** uploads this folder as the site root.

The default URL is `https://<user>.github.io/<repo>/` (or your org equivalent).

## Custom domain

1. In **Pages** settings, enter your domain (e.g. `oxideterm.com`).
2. Add the DNS records GitHub shows (A / CNAME).
3. Optional: add a `CNAME` file in `website/` containing only the hostname (e.g. `oxideterm.com`) so it is deployed with the site.

## Assets

Screenshots and the favicon are loaded from `raw.githubusercontent.com` so the site works without copying large binaries into `website/`. If you rename the GitHub org or repo, update URLs in `index.html`.

## i18n

The desktop app uses full i18n; this landing page is English-only with links to translated READMEs in the footer. Add more pages or a small JS switcher if needed later.
