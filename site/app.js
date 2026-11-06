// Release data is displayed as text; downloads must belong to this repository.
(function () {
  "use strict";
  var releases =
    "https://github.com/Naveen-354/ai-usage-monitor-native/releases";
  var downloadPrefix = releases + "/download/";
  var buttons = document.querySelectorAll("[data-download]");
  var line = document.getElementById("release-line");
  var heat = document.querySelector(".heat");
  if (heat) {
    for (var i = 0; i < 65; i++) {
      var cell = document.createElement("i");
      cell.className = "level-" + ((Math.imul(i + 7, 2654435761) >>> 28) % 5);
      heat.appendChild(cell);
    }
  }

  function setButtons(label, url) {
    buttons.forEach(function (button) {
      button.replaceChildren(document.createTextNode(label + " "));
      var arrow = document.createElement("span");
      arrow.setAttribute("aria-hidden", "true");
      arrow.textContent = "↓";
      button.appendChild(arrow);
      button.href = url;
    });
  }
  function size(bytes) {
    if (!Number.isFinite(bytes) || bytes < 0) return "Size not published";
    return bytes >= 1048576
      ? (bytes / 1048576).toFixed(1) + " MB"
      : Math.max(1, Math.round(bytes / 1024)) + " KB";
  }
  function show(manifest) {
    var assets =
      manifest && Array.isArray(manifest.assets)
        ? manifest.assets.filter(function (asset) {
            return (
              asset &&
              typeof asset.name === "string" &&
              typeof asset.url === "string" &&
              asset.url.startsWith(downloadPrefix)
            );
          })
        : [];
    var executables = assets.filter(function (asset) {
      return /\.exe$/i.test(asset.name);
    });
    // Prefer the installer, then the standalone exe. Never substitute an unrelated release attachment.
    var exe =
      executables.find(function (asset) {
        return /setup|install/i.test(asset.name);
      }) || executables[0];
    if (exe) {
      setButtons(
        "Download .exe" + (manifest.version ? " · v" + manifest.version : ""),
        exe.url,
      );
      line.textContent =
        exe.name + " · " + size(exe.size) + " · Hosted on GitHub Releases";
    } else {
      setButtons("View GitHub releases", releases);
      line.textContent = assets.length
        ? "No Windows .exe is attached to the latest release yet. Browse the available files below."
        : "The first release is on its way. The Windows .exe will appear here when published on GitHub.";
    }
    if (!assets.length) return;
    document.getElementById("files-version").textContent =
      manifest.version || "";
    var list = document.getElementById("file-list");
    list.replaceChildren();
    assets.forEach(function (asset) {
      var item = document.createElement("li");
      var link = document.createElement("a");
      link.href = asset.url;
      link.rel = "noopener noreferrer";
      link.textContent = asset.name + " (" + size(asset.size) + ")";
      var checksum = document.createElement("span");
      checksum.className = "sum";
      checksum.textContent = /^[a-f0-9]{64}$/i.test(asset.sha256 || "")
        ? "SHA-256 " + asset.sha256
        : "SHA-256 not published for this file";
      item.appendChild(link);
      item.appendChild(checksum);
      list.appendChild(item);
    });
    document.getElementById("files").hidden = false;
    if (typeof manifest.notes === "string" && manifest.notes.trim()) {
      document.getElementById("notes").textContent = manifest.notes;
      document.getElementById("notes-box").hidden = false;
    }
  }
  fetch("/api/latest", {
    headers: { Accept: "application/json" },
    signal: AbortSignal.timeout(15000),
  })
    .then(function (response) {
      if (!response.ok) throw new Error("Release feed unavailable");
      return response.json();
    })
    .then(show)
    .catch(function () {
      setButtons("View GitHub releases", releases);
      line.textContent =
        "The latest version could not be checked. Get the Windows .exe directly from GitHub Releases.";
    });
})();
