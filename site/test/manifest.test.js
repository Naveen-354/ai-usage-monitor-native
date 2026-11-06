// Run with: node --test site/test      (no dependencies)
const test = require('node:test');
const assert = require('node:assert/strict');
const { parseSums, classify, githubDigest, buildManifest, sumsAsset } = require('../api/_manifest');

const REPO = 'Naveen-354/ai-usage-monitor-native';
const BASE = `https://github.com/${REPO}/releases/download/v0.2.0/`;
const H1 = 'a'.repeat(64);
const H2 = 'b'.repeat(64);

const asset = (name, extra = {}) => ({ name, state: 'uploaded', size: 1234, browser_download_url: BASE + name, ...extra });
const release = (assets, extra = {}) => ({
  tag_name: 'v0.2.0', name: 'AI Usage Monitor 0.2.0', published_at: '2026-11-06T10:00:00Z', prerelease: false,
  html_url: `https://github.com/${REPO}/releases/tag/v0.2.0`, body: 'Notes', assets, ...extra,
});

test('checksum files are parsed in both common formats and lower-cased', () => {
  const text = `${H1.toUpperCase()}  setup.exe\n${H2} *portable.zip\r\nnot a checksum line\n`;
  assert.deepEqual(parseSums(text), { 'setup.exe': H1, 'portable.zip': H2 });
  assert.deepEqual(parseSums(''), {});
  assert.deepEqual(parseSums(undefined), {});
});

test('assets are classified by what a person would do with them', () => {
  assert.equal(classify('AI-Usage-Monitor-0.2.0-setup.exe'), 'installer');
  assert.equal(classify('x.msi'), 'installer');
  assert.equal(classify('AI-Usage-Monitor-0.2.0-windows-x64.zip'), 'portable');
  assert.equal(classify('notes.txt'), 'other');
});

test("GitHub's own digest wins over the checksum file, and a bad digest is ignored", () => {
  assert.equal(githubDigest({ digest: `sha256:${H1}` }), H1);
  assert.equal(githubDigest({ digest: 'sha1:abc' }), null);
  assert.equal(githubDigest({}), null);
  const m = buildManifest(release([asset('a.zip', { digest: `sha256:${H1}` })]), REPO, { 'a.zip': H2 });
  assert.equal(m.assets[0].sha256, H1);
  const m2 = buildManifest(release([asset('a.zip')]), REPO, { 'a.zip': H2 });
  assert.equal(m2.assets[0].sha256, H2, 'falls back to the checksum file');
  const m3 = buildManifest(release([asset('a.zip')]), REPO, {});
  assert.equal(m3.assets[0].sha256, null, 'unknown is null, never made up');
});

test('the manifest has the version without the v, the notes and only real, uploaded files', () => {
  const m = buildManifest(release([asset('setup.exe'), asset('SHA256SUMS.txt'), asset('half.zip', { state: 'starter' })]), REPO, {});
  assert.equal(m.available, true);
  assert.equal(m.version, '0.2.0');
  assert.equal(m.tag, 'v0.2.0');
  assert.equal(m.notes, 'Notes');
  assert.deepEqual(m.assets.map((a) => a.name), ['setup.exe'], 'the checksum file and unfinished uploads are not offered');
});

test('a file that is not in this repository\'s releases is never offered, whatever the release document says', () => {
  const evil = asset('evil.exe', { browser_download_url: 'https://evil.example/releases/download/v0.2.0/evil.exe' });
  const other = asset('other.exe', { browser_download_url: 'https://github.com/someone/else/releases/download/v1/other.exe' });
  const m = buildManifest(release([evil, other, asset('good.zip')]), REPO, {});
  assert.deepEqual(m.assets.map((a) => a.name), ['good.zip']);
  const none = buildManifest(release([evil]), REPO, {});
  assert.equal(none.available, false, 'nothing safe to download means nothing is available');
  const page = buildManifest(release([], { html_url: 'https://evil.example/' }), REPO, {});
  assert.equal(page.pageUrl, `https://github.com/${REPO}/releases`, 'a foreign page link is replaced by the releases page');
});

test('very long notes are cut and missing fields do not throw', () => {
  const m = buildManifest({ tag_name: 'v1.0.0', body: 'x'.repeat(50000), assets: undefined }, REPO, undefined);
  assert.equal(m.notes.length, 20000);
  assert.equal(m.available, false);
  assert.equal(m.publishedAt, null);
});

test('the checksum asset is found by name, case-insensitively', () => {
  assert.equal(sumsAsset(release([asset('a.zip'), asset('sha256sums.TXT')])).name, 'sha256sums.TXT');
  assert.equal(sumsAsset(release([asset('a.zip')])), null);
});
