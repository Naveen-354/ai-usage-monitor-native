// Reads the update feed (/api/latest) and fills in the download button, the file list and the release notes.
// Everything from the feed is put into the page as text (never as HTML).
(function () {
  'use strict';

  // The sample heat map in the picture: a repeatable pattern, not data.
  var heat = document.querySelector('.heat');
  if (heat) {
    for (var i = 0; i < 18 * 5; i++) {
      var cell = document.createElement('i');
      // a fixed scatter of five levels (0 = empty .. 4 = busiest), the same on every visit
      cell.style.setProperty('--l', String((Math.imul(i + 7, 2654435761) >>> 28) % 5));
      heat.appendChild(cell);
    }
  }

  var button = document.getElementById('download');
  var line = document.getElementById('release-line');

  function setButton(text, href, enabled) {
    button.textContent = text;
    button.setAttribute('href', href);
    button.classList.toggle('disabled', !enabled);
    button.setAttribute('aria-disabled', enabled ? 'false' : 'true');
  }

  function size(bytes) {
    return bytes >= 1048576 ? (bytes / 1048576).toFixed(1) + ' MB' : Math.max(1, Math.round(bytes / 1024)) + ' KB';
  }

  function show(m) {
    if (!m || !m.available) {
      setButton('First release coming soon', '#install', false);
      line.textContent = 'No version has been published yet. Watch the GitHub repository to hear when it is.';
      return;
    }
    // The main button: the installer if there is one, else the portable zip, else the first file.
    var main = m.assets.find(function (a) { return a.kind === 'installer'; }) || m.assets.find(function (a) { return a.kind === 'portable'; }) || m.assets[0];
    setButton('Download ' + m.version + ' for Windows', main.url, true);
    var when = m.publishedAt ? new Date(m.publishedAt).toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' }) : '';
    line.textContent = main.name + ' · ' + size(main.size) + (when ? ' · released ' + when : '') + (m.prerelease ? ' · pre-release' : '');

    var list = document.getElementById('file-list');
    document.getElementById('files-version').textContent = m.version;
    m.assets.forEach(function (a) {
      var li = document.createElement('li');
      var link = document.createElement('a');
      link.href = a.url;
      link.rel = 'noopener';
      link.textContent = a.name + ' (' + size(a.size) + ')';
      var sum = document.createElement('span');
      sum.className = 'sum';
      sum.textContent = a.sha256 ? 'SHA-256 ' + a.sha256 : 'SHA-256 not published for this file';
      li.appendChild(link);
      li.appendChild(sum);
      list.appendChild(li);
    });
    document.getElementById('files').hidden = false;

    if (m.notes && m.notes.trim()) {
      document.getElementById('notes').textContent = m.notes.trim();
      document.getElementById('notes-box').hidden = false;
    }
  }

  fetch('/api/latest', { headers: { Accept: 'application/json' } })
    .then(function (r) { return r.json(); })
    .then(show)
    .catch(function () {
      setButton('See all releases', 'https://github.com/Naveen-354/ai-usage-monitor-native/releases', true);
      line.textContent = 'The version check is unavailable right now.';
    });
})();
