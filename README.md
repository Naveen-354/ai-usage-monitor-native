# AI Usage Monitor - native build (egui)

The same product as the earlier Tauri / WebView2 app (a floating overlay showing how many tokens your local AI coding
agents used), rebuilt as a **native Rust app with no WebView**. It reuses the Rust backend (collectors, SQLite, aggregation)
unchanged in behaviour; only the UI and the OS shell are new. The earlier app now lives in its own repository,
[`ai-usage-monitor-webview2`](https://github.com/Naveen-354/ai-usage-monitor-webview2); this repository no longer depends on it.
Privacy statement and exactly what is read: [PRIVACY.md](PRIVACY.md). Collector file formats: [docs/COLLECTORS.md](docs/COLLECTORS.md).

## Measured on the development machine (Windows 11, release build, ~75 s after launch)

| | Tauri app (debug build) | This app |
|---|---|---|
| processes | 8 | **1** |
| private memory | ~178 MB | **~44 MB** |
| working set | ~196 MB | **~78 MB** |
| CPU, quiet | ~0.9 % of a core | ~0.7 % of a core |
| executable | 22 MB (debug) | 7.9 MB |

The Tauri row is a debug build and was measured while the machine was quiet; its release numbers were never taken.
CPU while the flame animates live was ~3.5 % (spike) to ~11 % (debug builds) and depends on activity. Not measured over hours (no leak test).

## Run

```bash
cd native-app
cargo run --release -p ai-usage-monitor-native
cargo run --release -p ai-usage-monitor-native -- --open diagnostics     # also open a page: statistics|settings|diagnostics|privacy
cargo run --release -p ai-usage-monitor-native -- --data-dir D:\scratch  # use another data folder
cargo run --release -p ai-usage-monitor-native -- --no-monitor           # open the database without starting the collectors
cargo run --release -p ai-usage-monitor-native -- --demo                 # fixture data, nothing read or written (screenshots, design checks)
cargo run --release -p ai-usage-monitor-native -- --demo --set theme=light --set overlayOpacity=0.6   # override settings in demo mode
```

* **Data folder:** `%APPDATA%\dev.aiusage.monitor.native` - deliberately *not* the Tauri app's `dev.aiusage.monitor`, so the two apps
  never write to one SQLite file. A first start re-imports your agents' history (about a minute) into this separate database.
* One copy per user (a second launch exits with code 3).
* Click the overlay to expand; drag to move (position is remembered); click the label to cycle DAY/WEEK/MONTH/YEAR;
  hover for PIN / EXP / MIN; the tray icon's menu and the three global shortcuts show/hide/expand it.

## Design

The look is a port of the WebView2 app's web UI (`src/styles/*.css`, `src/components/*` in the `ai-usage-monitor-webview2` repository), not a reinterpretation: the compact and expanded
overlay, the Statistics / Settings / Diagnostics / Privacy pages and the header tabs. `crates/ui/src/web/` is the small kit that makes egui
behave like those CSS rules:

* the same fonts (JetBrains Mono 400/700/800, Archivo Black; bundled in `crates/ui/assets/fonts`, SIL OFL) laid out glyph by glyph with the
  fonts' exact advances, because egui rounds advances to whole pixels and a browser does not;
* the same tokens, 2 px borders, hard shadows, Gaussian text halos, letter-spacing in `em`, the stepped "ping" ring, the number's
  `clamp()` sizing, CSS margin collapsing, auto-fit grids and table column stretching;
* measured against a browser rendering of the old UI (`reference.html?view=compact|expanded|settings|diagnostics|privacy|statistics` in the
  `ai-usage-monitor-webview2` repository, dev only, not part of its production build): widths such as the tabs (115.2 / 97.38 / 121.13 / 89.45 px), the big number (136.88 px) and the table columns
  are pinned by unit tests.

Two things were copied on purpose even though they look like bugs: the old per-agent usage bar never shows its fill (a CSS flex quirk), so here it
is the same plain 4 px line (`FILL_VISIBLE` in `web/all_agents.rs`), and the old Settings page has no rows for shortcuts or start-with-system, so
this one does not either (both are still wired in the shell). One sentence on the Privacy page is deliberately not a literal copy: the old "its content-security policy forbids outside
connections" is now "has no way to reach outside connections", because there is no WebView and so no CSP to cite.

Hooks for design checks (set before launch): `AI_USAGE_MONITOR_MAIN_SIZE=WxH` sizes the main window, `AI_USAGE_MONITOR_MAIN_SCROLL=<px>` scrolls it.

## Layout

| crate | what it is |
|---|---|
| `core` | the backend, with no Tauri: collectors (Claude Code, Codex, Gemini CLI, Antigravity, OpenCode, Ollama, Aider), SQLite, aggregation, monitoring. `api.rs` is the facade the UI calls. |
| `app-api` | the shared view types, the `Backend` trait, a mock backend, and the UI state store. |
| `fire-engine` | the flame simulation (no GUI dependency, deterministic, golden-tested). |
| `ui` (`native-ui`) | egui modules: theme, expanded overlay, statistics, settings, diagnostics, privacy, motion logic. |
| `platform` (`native-platform`) | tray icon, global-hotkey, autostart, single-instance, Win32 window helpers. |
| `app` | the program: compact overlay, window handling, wiring of everything above. |

## Verified (by running it, not by reading it)

* `cargo test --workspace`: 420 tests, 0 failures, 4 ignored (core; need real agent data) - run with the app closed;
  `cargo clippy --workspace --all-targets -- -D warnings`: clean.
* Real data end to end: the overlay, expanded view, Settings, Diagnostics and Privacy pages show the real counts; per-collector
  numbers match the independent checks done on the Tauri app (e.g. Gemini 1,812 events).
* Local-time periods (the facade originally used UTC; a regression test now pins "today starts at local midnight").
* Transparent, frameless, always-on-top window (Win32 `WS_EX_TOPMOST` read back); position and compact/expanded mode persist.
* Hide -> tray -> show round trip works (this found and fixed a real bug: a hidden window runs no frame loop, so it is now
  restored through Windows directly). Single-instance guard works.
* A damaged database is set aside and rebuilt, and the notice reaches Diagnostics.

## Not done / not verified

* **History charts:** the Tauri app's Statistics page is the per-agent breakdown (no charts), and so is this one. The core has no history
  query, and the first-generation `statistics` module (charts) is not shown anywhere. Also not implemented (same as the Tauri app):
  JSON/CSV export, clear history, custom data location.
* **Light theme:** built from the same tokens, but only the dark theme was compared against the browser rendering.
* The tray menu was exercised through its event path, not by right-clicking it; the global hotkeys' key mapping is tested but no key was pressed;
  start-with-system is wired but was never toggled on a real machine (it writes a start-up entry).
* No installer, no CI, no macOS/Linux build (the platform code is written to be portable but has never been compiled for them).
* No long-running leak test; memory/CPU above are 30-40 s samples.
* OpenCode's values are unvalidated (no local data to compare against), as in the Tauri app.
* Known leftovers to clean up: the first-generation egui pages that the design port superseded are still compiled and tested but no longer shown
  (`expanded::show`, `settings::page`, `diagnostics::show`, `privacy`, the `statistics` charts), as are the unused `motion::drag` and
  `platform::hotkeys` parsers (the validating parser in `ui::settings::shortcut` is the one in use). `expanded::Page` is still the tab enum.
* The `native_platform` single-instance test uses the real per-user lock, so it fails while the app itself is running.

## History of how this was built

Most of the UI modules were written by Antigravity agents working in isolated scratch folders (not part of this repository), then merged here and verified.
Several agent reports were wrong (claimed "clean" when it was not, notes describing code that did not exist, a redactor that leaked user names);
each was checked by running it, and the fixes are in the tests.
