// GET /api/latest  ->  the newest published release as { available, version, notes, assets: [{ name, url, size, kind, sha256 }] }.
// This address is the stable "update feed": the website reads it today, and an opt-in "check for updates" in the app can read
// it later, so the files themselves can move without breaking installed copies. Binaries stay on GitHub Releases.

const { parseSums, buildManifest, sumsAsset } = require('./_manifest');

const REPO = process.env.GITHUB_REPO || 'Naveen-354/ai-usage-monitor-native';

function headers() {
  const h = { Accept: 'application/vnd.github+json', 'User-Agent': 'ai-usage-monitor-site', 'X-GitHub-Api-Version': '2022-11-28' };
  if (process.env.GITHUB_TOKEN) h.Authorization = `Bearer ${process.env.GITHUB_TOKEN}`; // optional: only raises the rate limit
  return h;
}

module.exports = async function handler(req, res) {
  res.setHeader('Content-Type', 'application/json; charset=utf-8');
  res.setHeader('Cache-Control', 'public, s-maxage=300, stale-while-revalidate=600');
  res.setHeader('Access-Control-Allow-Origin', '*');
  try {
    const r = await fetch(`https://api.github.com/repos/${REPO}/releases/latest`, { headers: headers() });
    if (r.status === 404) {
      // No release has been published yet (or the repository is private): say so plainly, it is not an error.
      return res.status(200).json({ available: false, reason: 'no-release' });
    }
    if (!r.ok) return res.status(502).json({ available: false, reason: 'github-unavailable', status: r.status });
    const release = await r.json();

    let sums = {};
    const asset = sumsAsset(release);
    if (asset && asset.browser_download_url.startsWith(`https://github.com/${REPO}/releases/download/`)) {
      const s = await fetch(asset.browser_download_url, { headers: { 'User-Agent': 'ai-usage-monitor-site' } });
      if (s.ok) sums = parseSums(await s.text());
    }
    return res.status(200).json(buildManifest(release, REPO, sums));
  } catch (e) {
    return res.status(502).json({ available: false, reason: 'feed-error' });
  }
};
