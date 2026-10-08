// Release builds are a GUI app: no console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod accounts_view;
mod app;
mod backend;
mod bridge;
mod demo;
mod flame;
mod layout;
mod placement;
mod shell;

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use ai_usage_monitor_core::api::{default_data_dir, Config};
use native_platform::platform::tray::{TrayAction, TrayState};
use native_ui::expanded::Page;

#[derive(Debug, Default, PartialEq)]
struct Args {
    data_dir: Option<PathBuf>,
    no_monitor: bool,
    open: Option<Page>,
    /// Fixture data instead of the real backend (for screenshots and design comparison).
    demo: bool,
    /// `--set key=value` overrides of the demo's settings (value is JSON, or a bare word taken as a string).
    sets: Vec<(String, serde_json::Value)>,
    /// Hides the overlay after 4 s and shows it after 7 s through the tray's own event path, to prove it can come back.
    selftest_shell: bool,
}

fn parse_page(s: &str) -> Option<Page> {
    match s.to_ascii_lowercase().as_str() {
        "statistics" => Some(Page::Statistics),
        "accounts" => Some(Page::Accounts),
        "settings" => Some(Page::Settings),
        "diagnostics" => Some(Page::Diagnostics),
        "privacy" => Some(Page::Privacy),
        _ => None,
    }
}

fn parse_args(mut it: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut a = Args::default();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--data-dir" => a.data_dir = Some(it.next().ok_or("--data-dir needs a folder")?.into()),
            "--no-monitor" => a.no_monitor = true,
            "--demo" => a.demo = true,
            "--set" => {
                let kv = it.next().ok_or("--set needs key=value")?;
                let (k, v) = kv.split_once('=').ok_or_else(|| format!("--set expects key=value, got '{kv}'"))?;
                let value = serde_json::from_str(v).unwrap_or_else(|_| serde_json::Value::String(v.to_string()));
                a.sets.push((k.to_string(), value));
            }
            "--selftest-shell" => a.selftest_shell = true,
            // Accepted and ignored: an earlier start-up entry passed it, and an unknown flag would stop the app at login.
            "--minimized" => {}
            "--open" => {
                let v = it.next().ok_or("--open needs a page: statistics, accounts, settings, diagnostics or privacy")?;
                a.open = Some(parse_page(&v).ok_or_else(|| format!("unknown page '{v}'"))?);
            }
            other => return Err(format!("unknown argument '{other}'")),
        }
    }
    if !a.sets.is_empty() && !a.demo {
        return Err("--set only works together with --demo".into());
    }
    Ok(a)
}

fn data_dir(args: &Args) -> PathBuf {
args.data_dir.clone().unwrap_or_else(default_data_dir)
}

fn fail(msg: &str, code: i32) -> ! {
    eprintln!("ai-usage-monitor-native: {msg}");
    std::process::exit(code);
}

fn main() -> eframe::Result {
    let args = parse_args(std::env::args().skip(1)).unwrap_or_else(|e| fail(&e, 2));

    // One copy per user: two collectors writing the same database would be a bug, not a feature.
    // (A demo copy touches no database, so it may run next to the real one.)
    let _single_instance = (!args.demo).then(|| native_platform::platform::single_instance::acquire().unwrap_or_else(|_| fail("another copy is already running (look for the tray icon)", 3)));

    // The backend threads need to wake the UI; the egui context only exists once the window does.
    let ctx_cell: Arc<OnceLock<egui::Context>> = Arc::default();
    let repaint = {
        let cell = ctx_cell.clone();
        Arc::new(move || {
            if let Some(ctx) = cell.get() {
                ctx.request_repaint();
            }
        })
    };

    let backend: Arc<dyn backend::AppBackend> = if args.demo {
        Arc::new(demo::DemoBackend::new(&args.sets).unwrap_or_else(|e| fail(&e, 2)))
    } else {
        let mut config = Config::in_dir(data_dir(&args));
        config.monitoring_on = !args.no_monitor;
        backend::CoreBackend::start(config, repaint).unwrap_or_else(|e| fail(&format!("could not start: {e}"), 1))
    };
    let settings = app_api::api::Backend::settings(&*backend).unwrap_or_else(|e| fail(&format!("could not read settings: {e}"), 1));

    let size = if settings.compact_mode {
        settings.compact_size.map(|(w, h)| (w as f32, h as f32)).unwrap_or_else(|| layout::default_compact_size(settings.token_text_size))
    } else {
        settings.expanded_size.map(|(w, h)| (w as f32, h as f32)).unwrap_or((480.0, 680.0))
    };
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("AI Usage")
        .with_inner_size([size.0, size.1])
        .with_decorations(false)
        .with_transparent(true)
        .with_taskbar(settings.show_overlay_in_taskbar)
        .with_resizable(false);
    if settings.always_on_top {
        viewport = viewport.with_always_on_top();
    }
    if let (Some(x), Some(y)) = (settings.overlay_x, settings.overlay_y) {
        viewport = viewport.with_position([x as f32, y as f32]);
    }

    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native(
        "AI Usage",
        options,
        Box::new(move |cc| {
            let ctx = cc.egui_ctx.clone();
            native_ui::web::install_fonts(&ctx);
            // The old UI treated a press that moved more than 4 px as a window drag, otherwise a click.
            ctx.options_mut(|o| o.input_options.max_click_dist = 4.0);
            let _ = ctx_cell.set(ctx.clone());
            let wake: Arc<dyn Fn() + Send + Sync> = {
                let ctx = ctx.clone();
                Arc::new(move || ctx.request_repaint())
            };
            // Tray icon and hotkeys must be created on this (the window's) thread.
            let tray_state = TrayState { paused: settings.paused, always_on_top: settings.always_on_top, overlay_visible: true, unknown_data: true, total_tokens: 0 };
            let shared: Arc<shell::OverlayShared> = Arc::default();
            let shell = shell::Shell::start(&tray_state, [&settings.shortcut_toggle, &settings.shortcut_expand, &settings.shortcut_focus], shared.clone(), wake.clone());
            if args.selftest_shell {
                let tx = shell.tx.clone();
                let shared = shared.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(4));
                    let _ = tx.send(shell::ShellEvent::Tray(TrayAction::HideOverlay));
                    wake();
                    std::thread::sleep(std::time::Duration::from_secs(3));
                    shared.restore_if_hidden(); // exactly what the tray menu's thread does
                    let _ = tx.send(shell::ShellEvent::Tray(TrayAction::ShowOverlay));
                    wake();
                });
            }
            Ok(Box::new(app::App::new(backend, Some(shell), shared, size, args.open)))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai_usage_monitor_core::api::DATA_DIR_NAME;

    fn args(list: &[&str]) -> Result<Args, String> {
        parse_args(list.iter().map(|s| s.to_string()))
    }

    #[test]
    fn no_arguments_means_defaults() {
        assert_eq!(args(&[]).unwrap(), Args::default());
    }

    #[test]
    fn flags_and_pages_are_parsed() {
        let a = args(&["--data-dir", "D:\\x", "--no-monitor", "--open", "Diagnostics"]).unwrap();
        assert_eq!(a.data_dir, Some(PathBuf::from("D:\\x")));
        assert!(a.no_monitor);
        assert_eq!(a.open, Some(Page::Diagnostics));
    }

    #[test]
    fn demo_overrides_are_parsed_as_json_or_plain_words() {
        let a = args(&["--demo", "--set", "compactMode=false", "--set", "theme=light", "--set", "overlayX=300", "--set", r#"period="week""#]).unwrap();
        assert!(a.demo);
        assert_eq!(a.sets[0], ("compactMode".into(), serde_json::Value::Bool(false)));
        assert_eq!(a.sets[1], ("theme".into(), serde_json::Value::String("light".into())));
        assert_eq!(a.sets[2], ("overlayX".into(), serde_json::Value::from(300)));
        assert_eq!(a.sets[3], ("period".into(), serde_json::Value::String("week".into())));
    }

    #[test]
    fn the_app_starts_with_exactly_the_arguments_its_start_up_entry_passes() {
        // Regression: the entry used to pass `--minimized`, which this parser rejected, so the app exited at every login.
        let launch: Vec<&str> = native_platform::platform::autostart::LAUNCH_ARGS.to_vec();
        assert_eq!(args(&launch).unwrap(), Args::default());
        assert_eq!(args(&["--minimized"]).unwrap(), Args::default(), "an entry written by an older build still starts the app");
    }

    #[test]
    fn set_without_demo_is_refused_instead_of_silently_ignored() {
        assert!(args(&["--set", "theme=light"]).is_err());
        assert!(args(&["--demo", "--set", "novalue"]).is_err());
        assert!(args(&["--demo", "--set"]).is_err());
    }

    #[test]
    fn bad_input_is_reported_not_ignored() {
        assert!(args(&["--open"]).is_err());
        assert!(args(&["--open", "nowhere"]).is_err());
        assert!(args(&["--data-dir"]).is_err());
        assert!(args(&["--frobnicate"]).is_err());
    }

    #[test]
    fn the_default_data_folder_is_never_the_earlier_builds_folder() {
        let d = data_dir(&Args::default());
        assert!(d.ends_with(DATA_DIR_NAME));
        assert!(!d.ends_with("dev.aiusage.monitor"));
    }

    #[test]
    fn an_explicit_data_dir_wins() {
        let a = Args { data_dir: Some(PathBuf::from("Z:\\mine")), ..Default::default() };
        assert_eq!(data_dir(&a), PathBuf::from("Z:\\mine"));
    }
}
