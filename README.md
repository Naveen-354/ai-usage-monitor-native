# AI Usage Monitor

A local-first floating overlay showing how many tokens your local AI coding agents (Claude Code, Codex, Gemini CLI,
Antigravity, OpenCode, Ollama, Aider) have used, built as a **native Rust app (egui) with no WebView**. Nothing is uploaded and
there is no telemetry: [PRIVACY.md](PRIVACY.md) says exactly what is read, [docs/COLLECTORS.md](docs/COLLECTORS.md) the file formats.

*Origin:* this is a rewrite of an earlier Tauri / WebView2 version, which is archived in its own repository
([`ai-usage-monitor-webview2`](https://github.com/Naveen-354/ai-usage-monitor-webview2)). Nothing here depends on it.

## Measured on the development machine (Windows 11, release build, ~75 s after launch)

| | |
|---|---|
| processes | 1 |
| private memory | ~44 MB |
| working set | ~78 MB |
| CPU, quiet | ~0.7 % of a core |
| executable | 7.9 MB |

CPU while the flame animates live was ~3.5 % (spike) to ~11 % (debug builds) and depends on activity. Not measured over hours (no leak test).

## Animations

Settings -> Display -> **Animation style** swaps the flame for one of sixteen pixel-art scenes (hamster wheel, rubber duck, slot machine,
piggy bank, typing cat, token muncher, token spitter, golden egg chicken, turbo snail, sneezing panda, multitasking octopus, bubble gum pop,
starfield warp, and three that draw your real numbers: a **heartbeat monitor** that follows the recent token rate, a **water tank** whose level is
how full the day is, and an **agent equalizer** with one bar per enabled agent, in that agent's colour, bouncing while it works); the Settings page
shows a live preview of the one you pick. **Animation quality** sets how
sharp and smooth the scenes are. Every scene has the flame's three states: *live* (an agent is working: it moves), *idle* (a still pose,
no timer), *off* (paused or no data: a still, greyed pose); and a token **surge** makes a bigger moment (a jump in tokens, scaled by size).

| quality | canvas | frame rate | memory held | draw time per frame (release, one core) |
|---|---|---|---|---|
| Low | 80 x 60 | 8 fps | 23 KB | 7-35 us |
| Medium | 160 x 120 | 12 fps | 94 KB | 160-275 us |
| High | 240 x 180 | 20 fps | 211 KB | 370-575 us (about 1 % of a core at most) |

The canvas is one byte per pixel (an index into one shared 35-colour palette); the memory column is that plus the RGBA buffer the
texture upload needs. A test (`crates/fire-engine/tests/test_alloc.rs`) fails if a scene allocates while running, and switching style or
quality drops the old animation and its buffers. The classic flame ignores the quality setting. Measure on your own machine with
`cargo run --release -p fire-engine --example bench`; look at one scene with
`cargo run -p fire-engine --example sheet -- hamster high out.ppm`.

## Build and install (Windows 10/11)

There is no installer yet; the release build is a single self-contained `.exe` (fonts and SQLite are compiled in; no WebView2, Node or
other runtime to install). It needs the Visual C++ runtime (`VCRUNTIME140.dll`), which most PCs already have.

**Build** - needs [Rust](https://rustup.rs) (stable, `x86_64-pc-windows-msvc`) and the Visual Studio *Build Tools* with the
"Desktop development with C++" workload (the linker, and the C compiler for the bundled SQLite). Close a running copy first,
otherwise Windows will not let the build replace the `.exe`:

```powershell
git clone https://github.com/Naveen-354/ai-usage-monitor-native.git
cd ai-usage-monitor-native
cargo build --release -p ai-usage-monitor-native      # a few minutes the first time
# result: target\release\ai-usage-monitor-native.exe  (about 8 MB)
```

**Install** - copy the exe somewhere permanent and add a Start-menu shortcut:

```powershell
$dir = "$env:LOCALAPPDATA\Programs\AI Usage Monitor"
New-Item -ItemType Directory -Force $dir | Out-Null
Copy-Item .\target\release\ai-usage-monitor-native.exe $dir
$lnk = "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\AI Usage Monitor.lnk"
$s = (New-Object -ComObject WScript.Shell).CreateShortcut($lnk)
$s.TargetPath = "$dir\ai-usage-monitor-native.exe"; $s.WorkingDirectory = $dir; $s.Save()
# optional - start at login: put the same shortcut in the Startup folder
Copy-Item $lnk "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\Startup\"
```

**Uninstall** - quit from the tray menu, delete the install folder and the shortcut(s). Your data stays in
`%APPDATA%\dev.aiusage.monitor.native` until you delete it.

## Run

```bash
cargo run --release -p ai-usage-monitor-native
cargo run --release -p ai-usage-monitor-native -- --open diagnostics     # also open a page: statistics|settings|diagnostics|privacy
cargo run --release -p ai-usage-monitor-native -- --data-dir D:\scratch  # use another data folder
cargo run --release -p ai-usage-monitor-native -- --no-monitor           # open the database without starting the collectors
cargo run --release -p ai-usage-monitor-native -- --demo                 # fixture data, nothing read or written (screenshots, design checks)
cargo run --release -p ai-usage-monitor-native -- --demo --set theme=light --set overlayOpacity=0.6   # override settings in demo mode
```

* **Data folder:** `%APPDATA%\dev.aiusage.monitor.native` - deliberately *not* `dev.aiusage.monitor` (the earlier build's folder), so the two
  builds never write to one SQLite file. A first start re-imports your agents' history (about a minute).
* One copy per user (a second launch exits with code 3).
* Click the overlay to expand; drag to move (position is remembered); click the label to cycle DAY/WEEK/MONTH/YEAR;
  hover for PIN / EXP / MIN; the tray icon's menu and the three global shortcuts show/hide/expand it.

## Design

The look comes from the app's original web design: the compact and expanded overlay, the Statistics / Settings / Diagnostics / Privacy pages and
the header tabs. `crates/ui/src/web/` is the small kit that makes egui behave like the CSS it was designed in:

* the same fonts (JetBrains Mono 400/700/800, Archivo Black; bundled in `crates/ui/assets/fonts`, SIL OFL) laid out glyph by glyph with the
  fonts' exact advances, because egui rounds advances to whole pixels and a browser does not;
* the same tokens, 2 px borders, hard shadows, Gaussian text halos, letter-spacing in `em`, the stepped "ping" ring, the number's
  `clamp()` sizing, CSS margin collapsing, auto-fit grids and table column stretching;
* every size was measured from a browser rendering of the original design rather than guessed; widths such as the tabs
  (115.2 / 97.38 / 121.13 / 89.45 px), the big number (136.88 px) and the table columns are pinned by unit tests.

Two quirks of the original were kept on purpose: the per-agent usage bar never shows its fill, so here it is the same plain 4 px line
(`FILL_VISIBLE` in `web/all_agents.rs`), and the Settings page has no rows for shortcuts or start-with-system (both are still wired in the shell).

Hooks for design checks (set before launch): `AI_USAGE_MONITOR_MAIN_SIZE=WxH` sizes the main window, `AI_USAGE_MONITOR_MAIN_SCROLL=<px>` scrolls it.

## Layout

| crate | what it is |
|---|---|
| `core` | the backend: collectors (Claude Code, Codex, Gemini CLI, Antigravity, OpenCode, Ollama, Aider), SQLite, aggregation, monitoring. `api.rs` is the facade the UI calls. |
| `app-api` | the shared view types, the `Backend` trait, a mock backend, and the UI state store. |
| `fire-engine` | the flame simulation (no GUI dependency, deterministic, golden-tested). |
| `ui` (`native-ui`) | egui modules: theme, expanded overlay, statistics, settings, diagnostics, motion logic, and the `web` kit that draws every screen. |
| `platform` (`native-platform`) | tray icon, global-hotkey, autostart, single-instance, Win32 window helpers. |
| `app` | the program: compact overlay, window handling, wiring of everything above. |

## Verified (by running it, not by reading it)

* `cargo test --workspace`: 524 tests, 0 failures, 6 ignored (core; need real agent data) - run with the app closed;
  `cargo clippy --workspace --all-targets -- -D warnings`: clean.
* Real data end to end: the overlay, expanded view, Settings, Diagnostics and Privacy pages show the real counts; per-collector
  numbers match independent checks made against the agents' own files (e.g. Gemini 1,812 events).
* Local-time periods (the facade originally used UTC; a regression test now pins "today starts at local midnight").
* Transparent, frameless, always-on-top window (Win32 `WS_EX_TOPMOST` read back); position and compact/expanded mode persist.
* The Statistics page opens with an activity heat map in the style of GitHub's contribution graph: one square per local day for the past
  year, a column per week (the "week starts on" setting decides the first row), month and weekday names, a Less/More legend and a hover
  box with the day's total. Levels are quartiles of the days that had usage, so one huge day does not wash out the rest. It is built from
  the same 15-minute rollup and the same rules as the totals (enabled agents only, cached tokens only if "count cached tokens" is on).
  Checked on fixture data and on a real import of the agents' history; when the days cannot be read it says TOKEN DATA UNAVAILABLE, never zeros.
* **OpenCode 2.x works.** On this machine OpenCode is now 2.2.5, which replaced the tables the collector read (`message`, `session`) with
  `session_message` and `session_v2`; Diagnostics showed it as UNAVAILABLE ("opencode.db has no `message` table: the OpenCode schema has
  changed"). The collector now reads both layouts (role in the `type` column, model in `data.model.id`, project from `session_v2.directory`);
  on the real database it finds the one message and its numbers equal an independent count
  (`cargo test -p ai_usage_monitor_core opencode::tests::real -- --ignored --nocapture`).
* **RELOAD AGENTS** on the Diagnostics page (not in the original design): looks for every supported agent on this computer again. A running
  program keeps the `PATH` it started with, so a CLI installed since then is invisible to it; the button re-reads the machine's and the
  user's `PATH` from the registry first, then wakes every collector to detect and collect now.
* The expanded overlay has a **5-HOUR WINDOW** box for Claude Code: the time left (`RESETS IN 2H 47M`), a bar of the time elapsed with a
  tick per hour, the tokens and requests inside the window, and when it opened and closes (local time). It is an **estimate**, and the box
  says so: Claude Code does not store how much of the limit is used (it logs a `five_hour` record only when a request is *rejected*), and the
  limit is not a token count. What the local data supports is the window's shape: it opens with the first request after the previous window
  ended and closes 5 hours later (`core/aggregation/window.rs`). Checked against the provider's own number: on the one rejection logged on
  the development machine the model's reset time was 4.8 minutes after the provider's `resetsAt`
  (`cargo test -p ai_usage_monitor_core real_data_window -- --ignored --nocapture`). Between windows it says NO WINDOW OPEN, and when the
  window cannot be read it says UNAVAILABLE, never "no window". Both themes were checked on the demo (`--demo`).
* Expanding and collapsing keeps the overlay on screen: it grows and shrinks around the screen corner it is nearest to (a bottom-right
  overlay opens up and to the left) and is clamped into the work area of *its own* monitor. Measured on a real window in all four
  corners, and when the app restarts while expanded; each collapses back to exactly where it started.
* Hide -> tray -> show round trip works (this found and fixed a real bug: a hidden window runs no frame loop, so it is now
  restored through Windows directly). Single-instance guard works.
* A damaged database is set aside and rebuilt, and the notice reaches Diagnostics.

## Not done / not verified

* **Timeline charts:** the Statistics page is the activity heat map plus the per-agent breakdown. The core has no timeline/by-model/by-project
  query, and the first-generation `statistics` module (charts) is not shown anywhere. Also not implemented: JSON/CSV export, clear history, custom data location.
* **5-hour window:** Claude Code only, and only this computer's usage: claude.ai and other devices count toward the same limit but are
  invisible here, and the real "percent used" is not on disk, so there is no percentage. It is shown in the expanded overlay only (not the
  compact one or the Statistics page). Not done for the other agents: Antigravity has a quota with a remaining fraction and a reset time, but
  it is fetched from Google's servers while the CLI runs and is not stored in the files this app reads; Codex logs a real `rate_limits`
  percentage in its rollouts (the collector ignores it today), but there is no Codex data on the development machine to build or check that against.
* **Light theme:** built from the same tokens, but only the dark theme was compared against the browser rendering.
* The tray menu was exercised through its event path, not by right-clicking it; the global hotkeys' key mapping is tested but no key was pressed;
  start-with-system is wired but was never toggled on a real machine (it writes a start-up entry), and the Settings page has no row for it
  (as in the original design), so use the Startup-folder shortcut above. Its entry used to pass an argument the app rejected, which would have
  stopped the app at every login; fixed and pinned by a test, but still not tried on a real login.
* No installer, no CI, no macOS/Linux build (the platform code is written to be portable but has never been compiled for them).
* No long-running leak test; memory/CPU above are 30-40 s samples.
* OpenCode's values are barely validated: it reads the 1.x layout (`message`, from fixtures only) and the 2.x layout (`session_message`,
  from the real 2.2.5 database, where exactly one message exists, and it matches an independent count). OpenCode's own 2.x session totals
  run a little above its stored messages (calls such as titles are not stored as messages), so they will not match exactly.
* **Reload agents** (Diagnostics page): the new `PATH` is read from the registry and the monitor detects every agent again, tested with a
  fake agent that appears after start (a plain rescan does not find it, the reload does) and by clicking the button headlessly. Not yet
  tried by installing a real agent while the app runs, and not built for macOS/Linux (there it re-detects but cannot see a new `PATH`).
* Known leftovers to clean up: first-generation egui pages that the design port superseded are still compiled and tested but no longer shown
  (`expanded::show`, `settings::page`, `diagnostics::show`, the `statistics` charts), as are the unused `motion::drag` and
  `platform::hotkeys` parsers (the validating parser in `ui::settings::shortcut` is the one in use). `expanded::Page` is still the tab enum.
* The `native_platform` single-instance test uses the real per-user lock, so it fails while the app itself is running.

## History of how this was built

Most of the UI modules were written by Antigravity agents working in isolated scratch folders (not part of this repository), then merged here and verified.
Several agent reports were wrong (claimed "clean" when it was not, notes describing code that did not exist, a redactor that leaked user names);
each was checked by running it, and the fixes are in the tests.
