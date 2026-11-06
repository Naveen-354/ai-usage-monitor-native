// Turns a GitHub "latest release" document into the update manifest the website (and, later, the app) reads.
// Pure functions only, so they are tested without a network (see ../test/manifest.test.js).
// A file starting with "_" in api/ is not exposed as a route by Vercel.

const SUMS_NAME = /^SHA256SUMS(\.txt)?$/i;

/** `{ "name": "hex" }` from the text of a SHA256SUMS file ("<hex>  <name>" or "<hex> *<name>" per line). */
function parseSums(text) {
  const sums = {};
  for (const line of String(text || '').split(/\r?\n/)) {
    const m = /^([0-9a-fA-F]{64})\s+\*?(.+?)\s*$/.exec(line);
    if (m) sums[m[2]] = m[1].toLowerCase();
  }
  return sums;
}

function classify(name) {
  const n = name.toLowerCase();
  if (n.endsWith('.exe') && /setup|install/.test(n)) return 'installer';
  if (n.endsWith('.msi')) return 'installer';
  if (n.endsWith('.zip')) return 'portable';
  return 'other';
}

/** The checksum GitHub itself computed for an asset ("sha256:<hex>"), if it reports one. */
function githubDigest(asset) {
  const m = /^sha256:([0-9a-fA-F]{64})$/.exec(asset.digest || '');
  return m ? m[1].toLowerCase() : null;
}

/**
 * @param release the JSON of GET /repos/{repo}/releases/latest
 * @param repo    "owner/name": only files that really live in this repository's releases are ever offered
 * @param sums    parsed SHA256SUMS ({} when the release has none)
 */
function buildManifest(release, repo, sums) {
  const prefix = `https://github.com/${repo}/releases/download/`;
  const assets = (release.assets || [])
    .filter((a) => a.state === 'uploaded' && !SUMS_NAME.test(a.name) && typeof a.browser_download_url === 'string' && a.browser_download_url.startsWith(prefix))
    .map((a) => ({
      name: a.name,
      url: a.browser_download_url,
      size: a.size,
      kind: classify(a.name),
      sha256: githubDigest(a) || (sums || {})[a.name] || null,
    }));
  const tag = String(release.tag_name || '');
  return {
    available: assets.length > 0,
    version: tag.replace(/^v/i, ''),
    tag,
    name: release.name || tag,
    publishedAt: release.published_at || null,
    prerelease: !!release.prerelease,
    pageUrl: typeof release.html_url === 'string' && release.html_url.startsWith(`https://github.com/${repo}/`) ? release.html_url : `https://github.com/${repo}/releases`,
    notes: String(release.body || '').slice(0, 20000),
    assets,
  };
}

/** The asset that carries the checksums, if the release has one. */
function sumsAsset(release) {
  return (release.assets || []).find((a) => SUMS_NAME.test(a.name) && a.state === 'uploaded') || null;
}

module.exports = { parseSums, classify, githubDigest, buildManifest, sumsAsset };
