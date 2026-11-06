//! Turning accounts and usage into text (and JSON). Pure functions, so they can be tested without a terminal.

use ai_usage_monitor_core::accounts::{Account, AgentAccounts, AuthState, SwitchSupport};
use ai_usage_monitor_core::aggregation::{AccountUsage, SpanTotals, Totals};
use serde_json::{json, Value};

/// `1.23M`, `45.6K`, `789`: three significant digits, K/M/B/T - the same shorthand the desktop app uses.
pub fn compact(n: u64) -> String {
    const UNITS: [(&str, f64); 4] = [("T", 1e12), ("B", 1e9), ("M", 1e6), ("K", 1e3)];
    let v = n as f64;
    for (suffix, size) in UNITS {
        if v >= size {
            let scaled = v / size;
            let text = if scaled >= 100.0 {
                format!("{scaled:.0}")
            } else if scaled >= 10.0 {
                format!("{scaled:.1}")
            } else {
                format!("{scaled:.2}")
            };
            let text = if text.contains('.') { text.trim_end_matches('0').trim_end_matches('.').to_string() } else { text };
            return format!("{text}{suffix}");
        }
    }
    n.to_string()
}

/// "5m ago", "2h ago", "3d ago".
pub fn ago(now_ms: i64, then_ms: i64) -> String {
    let secs = ((now_ms - then_ms) / 1000).max(0);
    match secs {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{}m ago", secs / 60),
        3600..=86_399 => format!("{}h ago", secs / 3600),
        _ => format!("{}d ago", secs / 86_400),
    }
}

/// Left-aligned columns padded to their widest cell.
pub fn table(header: &[&str], rows: &[Vec<String>]) -> String {
    let mut widths: Vec<usize> = header.iter().map(|h| h.chars().count()).collect();
    for r in rows {
        for (i, c) in r.iter().enumerate() {
            widths[i] = widths[i].max(c.chars().count());
        }
    }
    let line = |cells: Vec<&str>| {
        let padded: Vec<String> = cells.iter().enumerate().map(|(i, c)| format!("{c:<w$}", w = widths[i])).collect();
        format!("  {}\n", padded.join("  ").trim_end())
    };
    let mut out = line(header.to_vec());
    for r in rows {
        out.push_str(&line(r.iter().map(String::as_str).collect()));
    }
    out
}

fn switching_text(s: &SwitchSupport) -> String {
    match s {
        SwitchSupport::Supported { mechanism } => format!("accounts kept apart with {mechanism}"),
        SwitchSupport::Unavailable { .. } => "monitor only - switching unavailable".to_string(),
    }
}

fn auth_text(a: &Account) -> String {
    a.auth.label().to_string()
}

pub fn accounts_text(agents: &[AgentAccounts], now_ms: i64) -> String {
    let mut out = String::new();
    for a in agents {
        out.push_str(&format!("{} ({}) - {} - {}\n", a.name, a.agent_id, if a.installed { "installed" } else { "not installed" }, switching_text(&a.switch)));
        if let SwitchSupport::Unavailable { reason } = &a.switch {
            out.push_str(&format!("  why: {reason}\n"));
        }
        let rows: Vec<Vec<String>> = a
            .accounts
            .iter()
            .map(|acc| {
                vec![
                    if a.active.as_deref() == Some(&acc.account_id) { "*".into() } else { "".into() },
                    acc.account_id.clone(),
                    acc.label.clone(),
                    auth_text(acc),
                    acc.identity.clone().unwrap_or_else(|| "-".into()),
                    acc.auth_checked_utc_ms.map(|t| ago(now_ms, t)).unwrap_or_else(|| "never".into()),
                ]
            })
            .collect();
        out.push_str(&table(&["", "ID", "NAME", "SIGN-IN", "IDENTITY", "CHECKED"], &rows));
        out.push('\n');
    }
    out
}

pub fn accounts_json(agents: &[AgentAccounts]) -> Value {
    json!({
        "agents": agents.iter().map(|a| {
            json!({
                "agent": a.agent_id,
                "name": a.name,
                "installed": a.installed,
                "switching": match &a.switch {
                    SwitchSupport::Supported { mechanism } => json!({"supported": true, "mechanism": mechanism}),
                    SwitchSupport::Unavailable { reason } => json!({"supported": false, "reason": reason}),
                },
                "active": a.active,
                "accounts": a.accounts.iter().map(|acc| json!({
                    "id": acc.account_id,
                    "name": acc.label,
                    "kind": acc.kind.as_str(),
                    "active": a.active.as_deref() == Some(&acc.account_id),
                    "signIn": acc.auth.as_str(),
                    "identity": acc.identity,
                    "checkedUtcMs": acc.auth_checked_utc_ms,
                    "lastUsedUtcMs": acc.last_used_utc_ms,
                })).collect::<Vec<_>>(),
            })
        }).collect::<Vec<_>>()
    })
}

pub fn current_text(current: &[(String, String, Account)]) -> String {
    let rows: Vec<Vec<String>> = current
        .iter()
        .map(|(agent, name, acc)| vec![agent.clone(), name.clone(), acc.account_id.clone(), acc.label.clone(), auth_text(acc)])
        .collect();
    table(&["AGENT", "NAME", "ACCOUNT", "LABEL", "SIGN-IN"], &rows)
}

pub fn current_json(current: &[(String, String, Account)]) -> Value {
    json!({
        "current": current.iter().map(|(agent, _, acc)| json!({
            "agent": agent, "account": acc.account_id, "name": acc.label, "kind": acc.kind.as_str(), "signIn": acc.auth.as_str(),
        })).collect::<Vec<_>>()
    })
}

/// One line of the usage table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageRow {
    pub agent: String,
    pub account: String,
    /// `None` = nothing has ever been recorded for it (so we say "-", never a made-up 0).
    pub totals: Option<SpanTotals>,
}

/// The rows for the given accounts (agent, account id, label), looked up in the usage report. With `all`, usage that belongs
/// to accounts which no longer exist is shown as `(removed)`, so the rows still add up to the agent's total.
pub fn usage_rows(shown: &[(String, String, String)], usage: &[AccountUsage], all: bool) -> Vec<UsageRow> {
    let mut rows: Vec<UsageRow> = shown
        .iter()
        .map(|(agent, id, label)| UsageRow {
            agent: agent.clone(),
            account: format!("{id} ({label})"),
            totals: usage.iter().find(|u| &u.agent_id == agent && &u.account_id == id).map(|u| u.totals),
        })
        .collect();
    if all {
        let mut agents: Vec<&String> = shown.iter().map(|(a, _, _)| a).collect();
        agents.dedup();
        for agent in agents {
            let gone: Vec<&AccountUsage> =
                usage.iter().filter(|u| &u.agent_id == agent && !shown.iter().any(|(a, i, _)| a == agent && i == &u.account_id)).collect();
            if !gone.is_empty() {
                rows.push(UsageRow { agent: agent.clone(), account: "(removed accounts)".into(), totals: Some(sum_spans(gone.iter().map(|u| u.totals))) });
            }
        }
    }
    rows
}

pub fn sum_spans(items: impl Iterator<Item = SpanTotals>) -> SpanTotals {
    let add = |a: Totals, b: Totals| Totals {
        input: a.input.saturating_add(b.input),
        output: a.output.saturating_add(b.output),
        cache_read: a.cache_read.saturating_add(b.cache_read),
        cache_write: a.cache_write.saturating_add(b.cache_write),
        reasoning: a.reasoning.saturating_add(b.reasoning),
        estimated: a.estimated.saturating_add(b.estimated),
        events: a.events.saturating_add(b.events),
        total: a.total.saturating_add(b.total),
    };
    items.fold(SpanTotals::default(), |s, t| SpanTotals {
        day: add(s.day, t.day),
        week: add(s.week, t.week),
        month: add(s.month, t.month),
        year: add(s.year, t.year),
        lifetime: add(s.lifetime, t.lifetime),
    })
}

fn cells(t: &SpanTotals) -> Vec<String> {
    [t.day, t.week, t.month, t.year, t.lifetime].iter().map(|x| compact(x.total)).collect()
}

pub fn usage_text(rows: &[UsageRow]) -> String {
    let mut table_rows: Vec<Vec<String>> = Vec::new();
    let mut by_agent: Vec<(&str, Vec<&UsageRow>)> = Vec::new();
    for r in rows {
        match by_agent.last_mut() {
            Some((a, v)) if *a == r.agent => v.push(r),
            _ => by_agent.push((&r.agent, vec![r])),
        }
    }
    let mut grand: Vec<SpanTotals> = Vec::new();
    for (agent, rs) in &by_agent {
        for r in rs {
            let mut line = vec![agent.to_string(), r.account.clone()];
            match &r.totals {
                Some(t) => line.extend(cells(t)),
                None => line.extend(std::iter::repeat_n("-".to_string(), 5)),
            }
            table_rows.push(line);
        }
        let known: Vec<SpanTotals> = rs.iter().filter_map(|r| r.totals).collect();
        if rs.len() > 1 && !known.is_empty() {
            let mut line = vec![agent.to_string(), "ALL ACCOUNTS".to_string()];
            line.extend(cells(&sum_spans(known.iter().copied())));
            table_rows.push(line);
        }
        grand.extend(known);
    }
    if by_agent.len() > 1 && !grand.is_empty() {
        let mut line = vec!["ALL AGENTS".to_string(), "".to_string()];
        line.extend(cells(&sum_spans(grand.iter().copied())));
        table_rows.push(line);
    }
    table(&["AGENT", "ACCOUNT", "TODAY", "WEEK", "MONTH", "YEAR", "LIFETIME"], &table_rows)
}

pub fn usage_json(rows: &[UsageRow]) -> Value {
    let span = |t: &Totals| json!({"total": t.total, "input": t.input, "output": t.output, "cached": t.cache_read + t.cache_write, "requests": t.events});
    json!({
        "usage": rows.iter().map(|r| json!({
            "agent": r.agent,
            "account": r.account,
            "spans": r.totals.as_ref().map(|t| json!({"today": span(&t.day), "week": span(&t.week), "month": span(&t.month), "year": span(&t.year), "lifetime": span(&t.lifetime)})),
        })).collect::<Vec<_>>()
    })
}

/// Says what a freshly checked account's state means in a sentence (shown after `login`).
pub fn state_sentence(a: &Account) -> String {
    match a.auth {
        AuthState::Valid => format!("{} is signed in{}.", a.display_ref(), a.identity.as_ref().map(|i| format!(" as {i}")).unwrap_or_default()),
        other => format!("{} is {} ({}).", a.display_ref(), other.label().to_lowercase(), a.auth_detail.clone().unwrap_or_else(|| "no detail".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai_usage_monitor_core::accounts::AccountKind;

    fn account(agent: &str, id: &str, label: &str, auth: AuthState) -> Account {
        Account {
            agent_id: agent.into(),
            account_id: id.into(),
            label: label.into(),
            kind: if id == "default" { AccountKind::Default } else { AccountKind::Managed },
            profile_dir: None,
            identity: None,
            auth,
            auth_detail: None,
            auth_checked_utc_ms: None,
            created_utc_ms: 0,
            last_used_utc_ms: None,
            removed_utc_ms: None,
        }
    }

    fn totals(n: u64) -> Totals {
        Totals { total: n, input: n, ..Default::default() }
    }

    fn spans(day: u64, lifetime: u64) -> SpanTotals {
        SpanTotals { day: totals(day), week: totals(day), month: totals(lifetime), year: totals(lifetime), lifetime: totals(lifetime) }
    }

    #[test]
    fn numbers_are_shortened_like_the_desktop_app() {
        assert_eq!(compact(0), "0");
        assert_eq!(compact(999), "999");
        assert_eq!(compact(1_000), "1K");
        assert_eq!(compact(1_234), "1.23K");
        assert_eq!(compact(45_600), "45.6K");
        assert_eq!(compact(256_000_000), "256M");
        assert_eq!(compact(1_500_000_000), "1.5B");
        assert_eq!(compact(2_000_000_000_000), "2T");
    }

    #[test]
    fn time_is_said_the_way_people_say_it() {
        assert_eq!(ago(10_000, 9_000), "just now");
        assert_eq!(ago(10 * 60_000, 0), "10m ago");
        assert_eq!(ago(5 * 3_600_000, 0), "5h ago");
        assert_eq!(ago(3 * 86_400_000, 0), "3d ago");
        assert_eq!(ago(0, 5_000), "just now", "a clock that went backwards is not a negative age");
    }

    #[test]
    fn the_table_aligns_columns_and_trims_trailing_space() {
        let t = table(&["A", "LONGER"], &[vec!["x".into(), "y".into()], vec!["long cell".into(), "z".into()]]);
        let lines: Vec<&str> = t.lines().collect();
        assert_eq!(lines[0], "  A          LONGER");
        assert_eq!(lines[1], "  x          y");
        assert!(lines.iter().all(|l| !l.ends_with(' ')));
    }

    fn agent(id: &str, switch: SwitchSupport, accounts: Vec<Account>, active: &str) -> AgentAccounts {
        AgentAccounts { agent_id: id.into(), name: id.to_uppercase(), installed: true, binary: None, switch, login_methods: vec![], active: Some(active.into()), accounts }
    }

    #[test]
    fn the_hierarchy_agent_accounts_active_is_visible_in_the_listing() {
        let a = agent(
            "codex",
            SwitchSupport::Supported { mechanism: "CODEX_HOME" },
            vec![account("codex", "default", "Default", AuthState::Unknown), account("codex", "work", "Work", AuthState::Valid)],
            "work",
        );
        let text = accounts_text(&[a], 0);
        assert!(text.contains("CODEX (codex) - installed - accounts kept apart with CODEX_HOME"), "{text}");
        let work = text.lines().find(|l| l.contains("work")).unwrap();
        assert!(work.trim_start().starts_with('*'), "the active account is marked: {work}");
        let default = text.lines().find(|l| l.contains("default")).unwrap();
        assert!(!default.trim_start().starts_with('*'));
        assert!(text.contains("SIGNED IN") && text.contains("NOT CHECKED"));
    }

    #[test]
    fn a_monitor_only_agent_says_so_and_why() {
        let a = agent(
            "antigravity",
            SwitchSupport::Unavailable { reason: "one sign-in in the OS credential store" },
            vec![account("antigravity", "default", "Default", AuthState::Unknown)],
            "default",
        );
        let text = accounts_text(&[a], 0);
        assert!(text.contains("monitor only - switching unavailable"), "{text}");
        assert!(text.contains("why: one sign-in in the OS credential store"), "{text}");
    }

    #[test]
    fn the_json_listing_is_complete_and_never_has_secrets_to_leak() {
        let mut acc = account("codex", "work", "Work", AuthState::Expired);
        acc.identity = Some("me@example.com".into());
        let v = accounts_json(&[agent("codex", SwitchSupport::Supported { mechanism: "CODEX_HOME" }, vec![acc], "work")]);
        let a = &v["agents"][0];
        assert_eq!(a["switching"]["mechanism"], "CODEX_HOME");
        assert_eq!(a["accounts"][0]["signIn"], "expired");
        assert_eq!(a["accounts"][0]["active"], true);
        assert_eq!(a["accounts"][0]["identity"], "me@example.com");
        let keys: Vec<&String> = a["accounts"][0].as_object().unwrap().keys().collect();
        assert!(keys.iter().all(|k| !k.to_lowercase().contains("token") && !k.to_lowercase().contains("key") && !k.to_lowercase().contains("secret")), "{keys:?}");
    }

    #[test]
    fn usage_shows_a_dash_for_an_account_that_has_never_been_recorded_not_a_zero() {
        let shown = vec![("codex".to_string(), "default".to_string(), "Default".to_string()), ("claude".to_string(), "default".to_string(), "Default".to_string())];
        let usage = vec![AccountUsage { agent_id: "codex".into(), account_id: "default".into(), totals: spans(1_500, 2_000_000) }];
        let rows = usage_rows(&shown, &usage, false);
        let text = usage_text(&rows);
        let codex = text.lines().find(|l| l.contains("codex")).unwrap();
        assert!(codex.contains("1.5K") && codex.contains("2M"), "{codex}");
        let claude = text.lines().find(|l| l.contains("claude")).unwrap();
        assert!(claude.matches('-').count() >= 5 && !claude.contains(" 0"), "{claude}");
    }

    #[test]
    fn with_all_the_accounts_add_up_per_agent_and_overall_and_removed_ones_are_not_lost() {
        let shown = vec![
            ("codex".to_string(), "default".to_string(), "Default".to_string()),
            ("codex".to_string(), "work".to_string(), "Work".to_string()),
            ("claude".to_string(), "default".to_string(), "Default".to_string()),
        ];
        let usage = vec![
            AccountUsage { agent_id: "codex".into(), account_id: "default".into(), totals: spans(100, 1_000) },
            AccountUsage { agent_id: "codex".into(), account_id: "work".into(), totals: spans(10, 100) },
            AccountUsage { agent_id: "codex".into(), account_id: "old".into(), totals: spans(0, 5) },
            AccountUsage { agent_id: "claude".into(), account_id: "default".into(), totals: spans(1, 1) },
        ];
        let text = usage_text(&usage_rows(&shown, &usage, true));
        assert!(text.contains("(removed accounts)"), "{text}");
        let all_codex = text.lines().find(|l| l.contains("codex") && l.contains("ALL ACCOUNTS")).unwrap();
        assert!(all_codex.contains("110") && all_codex.contains("1.1K"), "100+10 today, 1000+100+5 lifetime: {all_codex}");
        let grand = text.lines().find(|l| l.contains("ALL AGENTS")).unwrap();
        assert!(grand.contains("111") && grand.contains("1.11K"), "{grand}");
        let without_all = usage_text(&usage_rows(&shown, &usage, false));
        assert!(!without_all.contains("(removed accounts)"));
    }

    #[test]
    fn the_sentence_after_signing_in_names_the_account_and_the_person() {
        let mut a = account("claude", "work", "Work", AuthState::Valid);
        a.identity = Some("me@example.com".into());
        assert_eq!(state_sentence(&a), "claude/work is signed in as me@example.com.");
        let b = account("claude", "work", "Work", AuthState::Expired);
        assert!(state_sentence(&b).contains("expired"));
    }
}
