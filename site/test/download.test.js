const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const script = fs.readFileSync(path.join(__dirname, "../app.js"), "utf8");
const releases =
  "https://github.com/Naveen-354/ai-usage-monitor-native/releases";
const prefix = releases + "/download/v1.0.0/";
class Element {
  constructor() {
    this.children = [];
    this.hidden = true;
    this.textContent = "";
  }
  appendChild(child) {
    this.children.push(child);
    return child;
  }
  replaceChildren(...children) {
    this.children = children;
  }
  setAttribute(name, value) {
    this[name] = value;
  }
}
async function render(manifest, options = {}) {
  const ids = Object.fromEntries(
    [
      "release-line",
      "files-version",
      "file-list",
      "files",
      "notes",
      "notes-box",
    ].map((id) => [id, new Element()]),
  );
  const buttons = [new Element(), new Element()];
  const heat = new Element();
  const document = {
    querySelectorAll: () => buttons,
    querySelector: () => heat,
    getElementById: (id) => ids[id],
    createElement: () => new Element(),
    createTextNode: (text) => ({ textContent: text }),
  };
  vm.runInNewContext(script, {
    document,
    AbortSignal,
    fetch: async () => {
      if (options.reject) throw new Error("Network failure");
      return { ok: options.ok !== false, json: async () => manifest };
    },
  });
  await new Promise((resolve) => setImmediate(resolve));
  return { ids, buttons, heat };
}
const asset = (name, extra = {}) => ({
  name,
  url: prefix + name,
  size: 1048576,
  ...extra,
});
test("both buttons download installer exe ahead of zip and standalone exe", async () => {
  const result = await render({
    available: true,
    version: "1.0.0",
    assets: [
      asset("portable.zip"),
      asset("monitor.exe"),
      asset("monitor-setup.exe"),
    ],
  });
  for (const button of result.buttons) {
    assert.equal(button.href, prefix + "monitor-setup.exe");
    assert.match(button.children[0].textContent, /Download \.exe/);
  }
  assert.equal(result.ids.files.hidden, false);
  assert.equal(result.ids["file-list"].children.length, 3);
});
test("standalone exe is downloadable even when manifest classifies it as other", async () => {
  const result = await render({
    version: "1.0.0",
    assets: [asset("monitor.exe", { kind: "other" })],
  });
  assert.equal(result.buttons[0].href, prefix + "monitor.exe");
});
test("no release keeps an actionable releases link and an honest message", async () => {
  const result = await render({ available: false, reason: "no-release" });
  assert.equal(result.buttons[0].href, releases);
  assert.match(result.ids["release-line"].textContent, /first release/);
  assert.equal(result.ids.files.hidden, true);
});
test("zip-only releases never masquerade as exe downloads", async () => {
  const result = await render({
    version: "1.0.0",
    assets: [asset("portable.zip")],
  });
  assert.equal(result.buttons[0].href, releases);
  assert.match(result.ids["release-line"].textContent, /No Windows \.exe/);
  assert.equal(
    result.ids["file-list"].children[0].children[0].href,
    prefix + "portable.zip",
  );
});
test("foreign URLs are rejected and release notes are inserted as text", async () => {
  const notes = '<script>alert("untrusted")</script>';
  const result = await render({
    assets: [
      asset("evil.exe", { url: "https://evil.example/evil.exe" }),
      asset("safe.exe"),
    ],
    notes,
  });
  assert.equal(result.buttons[0].href, prefix + "safe.exe");
  assert.equal(result.ids["file-list"].children.length, 1);
  assert.equal(result.ids.notes.textContent, notes);
  assert.equal(result.ids["notes-box"].hidden, false);
});
test("HTTP and network failures both retain a working GitHub fallback", async () => {
  for (const options of [{ ok: false }, { reject: true }]) {
    const result = await render({}, options);
    assert.equal(result.buttons[0].href, releases);
    assert.match(
      result.ids["release-line"].textContent,
      /could not be checked/,
    );
  }
});
