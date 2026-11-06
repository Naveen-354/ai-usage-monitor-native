# Download site and update feed

A static page plus one function, hosted on Vercel. The installers themselves live on **GitHub Releases**; this site only points at them.

| Path | What it is |
|---|---|
| `index.html`, `style.css`, `app.js` | The download page. Loads nothing from other sites. |
| `api/latest.js` | `GET /api/latest`: the newest release as JSON (`version`, `notes`, `assets[]` with `url`, `size`, `sha256`). The stable address an opt-in "check for updates" in the app can read later. |
| `api/_manifest.js` | The pure logic behind it (checksum parsing, and refusing to offer any file that is not in this repository's releases). |
| `test/` | `npm test` (Node's built-in runner, no dependencies). |
| `vercel.json` | Security headers, including a CSP that only allows this site's own files. |

**No release yet?** `/api/latest` answers `{ "available": false }` and the page says "First release coming soon".

**Checksums:** a release should attach either GitHub's automatic digest (it does this itself) or a `SHA256SUMS.txt` (`<hex>  <file>` per line). A file with neither is shown as "SHA-256 not published".

**Optional setting:** `GITHUB_TOKEN` (read-only, public repos) raises GitHub's rate limit for the feed. Responses are cached for 5 minutes, so it is not needed at low traffic. `GITHUB_REPO` overrides `owner/name`.

**Not here yet:** the in-app update check. Whoever controls this feed controls what installed copies are told to download, so before the app installs anything automatically it must verify a signature, not only a checksum.
