use crate::view::{AppInfo, OverlayDiagnostics, Overview};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum Availability {
    Ok,
    NoDataYet,
    NotInstalled,
    Unavailable { reason: String },
    Error { message: String },
}

impl Availability {
    pub fn as_str(&self) -> &'static str {
        match self {
            Availability::Ok => "ok",
            Availability::NoDataYet => "no-data-yet",
            Availability::NotInstalled => "not-installed",
            Availability::Unavailable { .. } => "unavailable",
            Availability::Error { .. } => "error",
        }
    }
    
    pub fn detail(&self) -> Option<&str> {
        match self {
            Availability::Unavailable { reason } => Some(reason),
            Availability::Error { message } => Some(message),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectorHealth {
    pub agent: String,
    pub availability: Availability,
    pub note: Option<String>,
    pub last_run_utc_ms: Option<i64>,
    pub last_success_utc_ms: Option<i64>,
    pub last_event_utc_ms: Option<i64>,
    pub events_total: u64,
    pub skipped_records: u64,
    pub source_paths: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct DiagnosticsData {
    pub app_info: AppInfo,
    pub overlay: OverlayDiagnostics,
    pub collectors: Vec<CollectorHealth>,
    pub overview: Overview,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticsAction {
    CopyReport(String),
    OpenLogFolder,
    OpenDataFolder,
    RescanAgent(String),
    PauseResume,
}

#[derive(Default)]
pub struct State {}

pub fn show(ui: &mut egui::Ui, _state: &mut State, data: &DiagnosticsData) -> Vec<DiagnosticsAction> {
    let mut actions = Vec::new();

    ui.heading("OVERLAY");
    let topmost_ok = match data.overlay.os_reports_topmost {
        Some(rep) => rep == data.overlay.always_on_top_setting,
        None => true,
    };
    
    egui::Grid::new("overlay_grid").striped(true).show(ui, |ui| {
        ui.label("Visible");
        ui.label(if data.overlay.visible { "yes" } else { "no" });
        ui.end_row();
        
        ui.label("Minimised");
        ui.label(if data.overlay.minimized { "yes" } else { "no" });
        ui.end_row();
        
        ui.label("Always-on-top setting");
        ui.label(if data.overlay.always_on_top_setting { "ON" } else { "OFF" });
        ui.end_row();
        
        ui.label("Frameless (no title bar)");
        let frameless_text = match data.overlay.decorated {
            None => "unknown",
            Some(true) => "NO — has a title bar",
            Some(false) => "yes",
        };
        ui.label(frameless_text);
        ui.end_row();
        
        ui.label("OS reports topmost");
        let os_topmost_text = match data.overlay.os_reports_topmost {
            None => "not readable on this OS",
            Some(true) => "yes",
            Some(false) => "no",
        };
        if data.overlay.os_reports_topmost.is_some() && !topmost_ok {
            ui.colored_label(egui::Color32::RED, os_topmost_text);
        } else {
            ui.label(os_topmost_text);
        }
        ui.end_row();
        
        ui.label("Position");
        if let (Some(x), Some(y)) = (data.overlay.x, data.overlay.y) {
            ui.label(format!("{}, {}", x, y));
        } else {
            ui.label("—");
        }
        ui.end_row();
        
        ui.label("Size (px)");
        if let (Some(w), Some(h)) = (data.overlay.width, data.overlay.height) {
            ui.label(format!("{} × {}", w, h));
        } else {
            ui.label("—");
        }
        ui.end_row();
        
        ui.label("Scale / monitors");
        let scale = data.overlay.scale_factor.map_or("—".to_string(), |s| s.to_string());
        ui.label(format!("{} / {}", scale, data.overlay.monitors));
        ui.end_row();
    });
    ui.small(&data.overlay.platform_note);
    
    ui.add_space(8.0);
    ui.heading("COLLECTORS");
    egui::Grid::new("collectors_grid").striped(true).show(ui, |ui| {
        ui.label("AGENT");
        ui.label("STATE");
        ui.label("DETAIL");
        ui.label("LAST USAGE");
        ui.label("FILES WATCHED");
        ui.label("EVENTS IMPORTED");
        ui.label("LAST SUCCESS");
        ui.label("LAST ERROR");
        ui.label("ACTIONS");
        ui.end_row();
        
        // Sorting: errors first
        let mut sorted = data.collectors.clone();
        sorted.sort_by(|a, b| {
            let a_err = matches!(a.availability, Availability::Error { .. });
            let b_err = matches!(b.availability, Availability::Error { .. });
            b_err.cmp(&a_err).then(a.agent.cmp(&b.agent))
        });
        
        for c in &sorted {
            ui.label(&c.agent);
            ui.label(c.availability.as_str());
            ui.label(c.availability.detail().unwrap_or("—"));
            ui.label(c.last_event_utc_ms.map_or("—".to_string(), format_local_time));
            ui.label(c.source_paths.len().to_string());
            ui.label(c.events_total.to_string());
            ui.label(c.last_success_utc_ms.map_or("—".to_string(), format_local_time));
            
            let last_err = match &c.availability {
                Availability::Error { message } => message.as_str(),
                Availability::Unavailable { reason } => reason.as_str(),
                _ => "—",
            };
            ui.label(last_err);
            
            if ui.button("Rescan").clicked() {
                actions.push(DiagnosticsAction::RescanAgent(c.agent.clone()));
            }
            ui.end_row();
        }
    });
    
    ui.add_space(8.0);
    ui.heading("APPLICATION");
    egui::Grid::new("app_info_grid").striped(true).show(ui, |ui| {
        ui.label("Version");
        ui.label(&data.app_info.version);
        ui.end_row();
        
        ui.label("Platform");
        ui.label(format!("{} / {}", data.app_info.os, data.app_info.arch));
        ui.end_row();
        
        ui.label("Database");
        ui.horizontal(|ui| {
            ui.label(&data.app_info.database_path);
            if ui.button("Open Folder").clicked() {
                actions.push(DiagnosticsAction::OpenDataFolder);
            }
        });
        ui.end_row();
        
        ui.label("Logs");
        ui.horizontal(|ui| {
            ui.label(&data.app_info.log_dir);
            if ui.button("Open Folder").clicked() {
                actions.push(DiagnosticsAction::OpenLogFolder);
            }
        });
        ui.end_row();
        
        ui.label("Database health");
        if data.app_info.database_notice.is_some() {
            ui.label("REBUILT after damage");
        } else {
            ui.label("ok");
        }
        ui.end_row();
    });
    
    if let Some(notice) = &data.app_info.database_notice {
        ui.colored_label(egui::Color32::RED, notice);
    }
    
    if ui.button("Copy Report").clicked() {
        actions.push(DiagnosticsAction::CopyReport(report_text(data)));
    }
    if ui.button(if data.overview.paused { "Resume" } else { "Pause" }).clicked() {
        actions.push(DiagnosticsAction::PauseResume);
    }

    actions
}

pub fn report_text(data: &DiagnosticsData) -> String {
    let mut out = String::new();
    out.push_str(&format!("Version: {}\n", data.app_info.version));
    out.push_str(&format!("Platform: {} / {}\n", data.app_info.os, data.app_info.arch));
    out.push_str(&format!("Database: {}\n", redact_path(&data.app_info.database_path)));
    out.push_str(&format!("Logs: {}\n", redact_path(&data.app_info.log_dir)));
    
    out.push_str("\nCollectors:\n");
    let mut sorted = data.collectors.clone();
    sorted.sort_by(|a, b| {
        let a_err = matches!(a.availability, Availability::Error { .. });
        let b_err = matches!(b.availability, Availability::Error { .. });
        b_err.cmp(&a_err).then(a.agent.cmp(&b.agent))
    });
    
    for c in &sorted {
        out.push_str(&format!("- {}: {}\n", c.agent, c.availability.as_str()));
        if let Some(detail) = c.availability.detail() {
            out.push_str(&format!("  Detail: {}\n", redact_text(detail)));
        }
        for path in &c.source_paths {
            out.push_str(&format!("  Source: {}\n", redact_path(path)));
        }
    }
    out
}

/// Replaces a user's home directory with `~` and any other mention of that user name with `<user>`, anywhere in
/// `text` (a path or a free-text error message). Recognises `<drive>:\Users\<name>`, `/Users/<name>`, `/home/<name>`
/// and admin-share UNC paths (`\\host\c$\Users\<name>`), with either slash style, in any letter case. A path with no
/// home directory is returned unchanged. User names shorter than 3 characters are only replaced when they are a
/// whole path segment (so a user called `al` does not mangle `alpha`).
pub fn redact_text(text: &str) -> String {
    let c: Vec<char> = text.chars().collect();
    let n = c.len();
    let is_sep = |ch: char| ch == '\\' || ch == '/';
    let stops = |ch: char| is_sep(ch) || ch.is_whitespace() || matches!(ch, '"' | '\'' | '<' | '>' | '|' | '*' | '?' | ':' | ',' | ';' | ')');
    let mut out = String::new();
    let mut names: Vec<String> = Vec::new();
    let mut i = 0;
    while i < n {
        if is_sep(c[i]) {
            let keyword = ["users", "home"]
                .iter()
                .find(|k| matches_ci(&c, i + 1, k) && n > i + 1 + k.len() && is_sep(c[i + 1 + k.len()]));
            if let Some(kw) = keyword {
                let name_start = i + 2 + kw.len();
                let mut name_end = name_start;
                while name_end < n && !stops(c[name_end]) {
                    name_end += 1;
                }
                let drive = i >= 2 && c[i - 1] == ':' && c[i - 2].is_ascii_alphabetic();
                let unc_share = i >= 1 && c[i - 1] == '$';
                let anchored = i == 0 || drive || unc_share || c[i - 1].is_whitespace() || matches!(c[i - 1], '"' | '\'' | '(' | '=');
                if name_end > name_start && anchored {
                    names.push(c[name_start..name_end].iter().collect());
                    if drive {
                        out.pop();
                        out.pop();
                    }
                    if unc_share {
                        out.push(c[i]);
                    }
                    out.push('~');
                    i = name_end;
                    continue;
                }
            }
        }
        out.push(c[i]);
        i += 1;
    }
    names.sort();
    names.dedup();
    for name in names {
        out = replace_user_name(&out, &name);
    }
    out
}

/// Path flavour of [`redact_text`] (kept as the name the report code and callers use for file paths).
/// `1791396489000` -> `2026-10-08 00:18:09` in the user's local time zone; "—" for an unrepresentable instant.
pub fn format_local_time(utc_ms: i64) -> String {
    use chrono::TimeZone;
    match chrono::Local.timestamp_millis_opt(utc_ms).single() {
        Some(t) => t.format("%Y-%m-%d %H:%M:%S").to_string(),
        None => "—".to_string(),
    }
}
pub fn redact_path(path: &str) -> String {
    redact_text(path)
}

fn matches_ci(c: &[char], at: usize, word: &str) -> bool {
    let w: Vec<char> = word.chars().collect();
    at + w.len() <= c.len() && c[at..at + w.len()].iter().zip(&w).all(|(a, b)| a.eq_ignore_ascii_case(b))
}

fn replace_user_name(text: &str, name: &str) -> String {
    let c: Vec<char> = text.chars().collect();
    let w: Vec<char> = name.chars().collect();
    let strict = w.len() < 3;
    let sep_or_edge = |idx: Option<usize>| match idx.and_then(|k| c.get(k)) {
        None => true,
        Some(ch) => *ch == '\\' || *ch == '/' || ch.is_whitespace() || matches!(*ch, '"' | '\''),
    };
    let word_edge = |idx: Option<usize>| match idx.and_then(|k| c.get(k)) {
        None => true,
        Some(ch) => !ch.is_alphanumeric(),
    };
    let mut out = String::new();
    let mut i = 0;
    while i < c.len() {
        let hit = i + w.len() <= c.len() && c[i..i + w.len()].iter().zip(&w).all(|(a, b)| a.eq_ignore_ascii_case(b));
        let before = i.checked_sub(1);
        let after = i + w.len();
        let boundary = if strict {
            sep_or_edge(before) && sep_or_edge(Some(after))
        } else {
            word_edge(before) && word_edge(Some(after))
        };
        if hit && boundary {
            out.push_str("<user>");
            i += w.len();
        } else {
            out.push(c[i]);
            i += 1;
        }
    }
    out
}

pub mod mock_builder {
    use super::*;
    pub fn collector_ok(agent: &str) -> CollectorHealth {
        CollectorHealth {
            agent: agent.to_string(),
            availability: Availability::Ok,
            note: None,
            last_run_utc_ms: Some(1000),
            last_success_utc_ms: Some(1000),
            last_event_utc_ms: Some(900),
            events_total: 10,
            skipped_records: 0,
            source_paths: vec!["C:\\Users\\demo\\agent".to_string()],
        }
    }
    pub fn collector_error(agent: &str, err: &str) -> CollectorHealth {
        CollectorHealth {
            agent: agent.to_string(),
            availability: Availability::Error { message: err.to_string() },
            note: None,
            last_run_utc_ms: Some(1000),
            last_success_utc_ms: None,
            last_event_utc_ms: None,
            events_total: 0,
            skipped_records: 0,
            source_paths: vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn times_are_shown_as_local_dates_not_raw_milliseconds() {
        use chrono::TimeZone;
        let ms = chrono::Local.with_ymd_and_hms(2026, 10, 8, 0, 18, 9).unwrap().timestamp_millis();
        assert_eq!(format_local_time(ms), "2026-10-08 00:18:09");
        assert_eq!(format_local_time(i64::MAX), "—");
    }
    #[test]
    fn redacts_windows_home_with_either_slash_style() {
        assert_eq!(redact_path("C:\\Users\\demo\\AppData"), "~\\AppData");
        assert_eq!(redact_path("C:/Users/demo/AppData"), "~/AppData");
        assert_eq!(redact_path("C:\\Users\\demo/AppData\\x"), "~/AppData\\x");
        assert_eq!(redact_path("d:\\users\\Demo\\x"), "~\\x");
    }

    #[test]
    fn redacts_when_the_user_name_is_the_last_segment() {
        assert_eq!(redact_path("C:\\Users\\demo"), "~");
        assert_eq!(redact_path("/home/alice"), "~");
        assert_eq!(redact_path("/Users/bob"), "~");
    }

    #[test]
    fn redacts_unix_and_macos_homes() {
        assert_eq!(redact_path("/home/alice/.config/alice_app"), "~/.config/<user>_app");
        assert_eq!(redact_path("/Users/bob/bob_file"), "~/<user>_file");
    }

    #[test]
    fn redacts_unc_admin_share_paths() {
        assert_eq!(redact_path("\\\\srv\\c$\\Users\\carol\\docs"), "\\\\srv\\c$\\~\\docs");
    }

    #[test]
    fn redacts_every_mention_of_a_repeated_user_name() {
        assert_eq!(redact_path("C:\\Users\\john_doe\\john_doe_file"), "~\\<user>_file");
        assert_eq!(redact_path("C:\\Users\\demo\\demo\\DEMO.txt"), "~\\<user>\\<user>.txt");
    }

    #[test]
    fn short_user_names_only_replace_whole_segments() {
        assert_eq!(redact_path("C:\\Users\\al\\alpha\\al\\x"), "~\\alpha\\<user>\\x");
    }

    #[test]
    fn leaves_paths_without_a_home_directory_alone() {
        for p in ["D:\\work\\project", "/opt/home/x", "/srv/users/data", "", "relative/users/dir", "C:\\Program Files\\App"] {
            assert_eq!(redact_path(p), p, "{p}");
        }
    }

    #[test]
    fn redacts_paths_inside_free_text_error_messages() {
        let msg = "failed to open \"C:\\Users\\demo\\.claude\\x.jsonl\": access denied (user demo)";
        let out = redact_text(msg);
        assert_eq!(out, "failed to open \"~\\.claude\\x.jsonl\": access denied (user <user>)");
        assert!(!out.contains("demo"));
    }

    #[test]
    fn the_copied_report_never_contains_the_user_name() {
        use crate::mock;
        let mut err = mock_builder::collector_error("claude", "cannot read C:\\Users\\demo\\.claude\\projects: denied");
        err.source_paths = vec!["C:\\Users\\demo\\.claude".to_string(), "/home/demo".to_string()];
        let data = DiagnosticsData {
            app_info: AppInfo { database_path: "C:\\Users\\demo\\AppData\\Roaming\\x\\usage.db".into(), ..mock::app_info() },
            overlay: mock::diagnostics(),
            collectors: vec![err, mock_builder::collector_ok("codex")],
            overview: mock::overview(),
        };
        let report = report_text(&data);
        assert!(!report.to_lowercase().contains("demo"), "{report}");
        assert!(report.contains('~'));
    }

    
    #[test]
    fn headless_smoke_test() {
        use crate::mock;
        let mut state = State::default();
        let data = DiagnosticsData {
            app_info: mock::app_info(),
            overlay: mock::diagnostics(),
            collectors: vec![mock_builder::collector_ok("test_agent")],
            overview: mock::overview(),
        };
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let _ = show(ui, &mut state, &data);
            });
        });
    }
}
