//! Turns the core's accounts and per-account usage into the view model the Accounts page draws. A pure function, so the rules
//! (what is "removed usage", what adds up to what, what the page may offer) are tested without a database or a window.

use ai_usage_monitor_core::accounts::{Account, AgentAccounts, AuthState, LoginMethod, SwitchSupport};
use ai_usage_monitor_core::aggregation::{AccountUsage, SpanTotals};
use ai_usage_monitor_core::model::catalog;
use app_api::view::{AccountView, AccountsOverview, AgentAccountsView, AuthStateView, SpanUsageView, SwitchingView};

fn auth_view(a: AuthState) -> AuthStateView {
    match a {
        AuthState::Valid => AuthStateView::Valid,
        AuthState::Expired => AuthStateView::Expired,
        AuthState::NotLoggedIn => AuthStateView::NotLoggedIn,
        AuthState::Pending => AuthStateView::Pending,
        AuthState::Unknown => AuthStateView::Unknown,
    }
}

fn span_view(t: &SpanTotals) -> SpanUsageView {
    SpanUsageView { day: t.day.total, week: t.week.total, month: t.month.total, year: t.year.total, lifetime: t.lifetime.total }
}

fn sum(items: impl Iterator<Item = SpanUsageView>) -> Option<SpanUsageView> {
    items.fold(None, |acc, u| Some(acc.map_or(u, |a| a.add(&u))))
}

fn account_view(a: &Account, active: Option<&str>, usage: &[AccountUsage]) -> AccountView {
    AccountView {
        account_id: a.account_id.clone(),
        label: a.label.clone(),
        is_default: a.is_default(),
        auth: auth_view(a.auth),
        auth_detail: a.auth_detail.clone(),
        identity: a.identity.clone(),
        active: active == Some(a.account_id.as_str()),
        checked_utc_ms: a.auth_checked_utc_ms,
        last_used_utc_ms: a.last_used_utc_ms,
        usage: usage.iter().find(|u| u.agent_id == a.agent_id && u.account_id == a.account_id).map(|u| span_view(&u.totals)),
    }
}

/// The sign-in methods the desktop page offers. An API key is deliberately not one of them: typing a key into the app would put
/// a secret in UI state, so keys go through `agm login --api-key-stdin` only.
fn offered_methods(methods: &[LoginMethod]) -> Vec<String> {
    methods.iter().filter(|m| **m != LoginMethod::ApiKey).map(|m| m.as_str().to_string()).collect()
}

pub fn overview(agents: &[AgentAccounts], usage: &[AccountUsage]) -> AccountsOverview {
    let views: Vec<AgentAccountsView> = agents
        .iter()
        .map(|a| {
            let accounts: Vec<AccountView> = a.accounts.iter().map(|acc| account_view(acc, a.active.as_deref(), usage)).collect();
            let mine: Vec<&AccountUsage> = usage.iter().filter(|u| u.agent_id == a.agent_id).collect();
            // usage filed under an account that no longer exists: kept in the totals, shown as one line
            let removed_usage = sum(mine.iter().filter(|u| !a.accounts.iter().any(|acc| acc.account_id == u.account_id)).map(|u| span_view(&u.totals)));
            let total = sum(mine.iter().map(|u| span_view(&u.totals)));
            AgentAccountsView {
                agent_id: a.agent_id.clone(),
                name: a.name.clone(),
                color: catalog().iter().find(|m| m.id == a.agent_id).map(|m| m.color.to_string()).unwrap_or_else(|| "#8a8a8a".into()),
                installed: a.installed,
                switching: match &a.switch {
                    SwitchSupport::Supported { mechanism } => SwitchingView { supported: true, mechanism: Some((*mechanism).to_string()), reason: None },
                    SwitchSupport::Unavailable { reason } => SwitchingView { supported: false, mechanism: None, reason: Some((*reason).to_string()) },
                },
                login_methods: if matches!(a.switch, SwitchSupport::Supported { .. }) { offered_methods(&a.login_methods) } else { vec![] },
                active: a.active.clone(),
                accounts,
                removed_usage,
                total,
            }
        })
        .collect();
    let total = sum(views.iter().filter_map(|a| a.total));
    AccountsOverview { agents: views, total }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai_usage_monitor_core::accounts::AccountKind;
    use ai_usage_monitor_core::aggregation::Totals;

    fn account(agent: &str, id: &str, label: &str, auth: AuthState) -> Account {
        Account {
            agent_id: agent.into(),
            account_id: id.into(),
            label: label.into(),
            kind: if id == "default" { AccountKind::Default } else { AccountKind::Managed },
            profile_dir: None,
            identity: Some("someone@example.com".into()),
            auth,
            auth_detail: Some("a short reason".into()),
            auth_checked_utc_ms: Some(5),
            created_utc_ms: 1,
            last_used_utc_ms: None,
            removed_utc_ms: None,
        }
    }

    fn agent(id: &str, switch: SwitchSupport, methods: Vec<LoginMethod>, active: &str, accounts: Vec<Account>) -> AgentAccounts {
        AgentAccounts { agent_id: id.into(), name: id.to_uppercase(), installed: true, binary: None, switch, login_methods: methods, active: Some(active.into()), accounts }
    }

    fn usage(agent: &str, account: &str, day: u64, lifetime: u64) -> AccountUsage {
        let t = |n| Totals { total: n, ..Default::default() };
        AccountUsage { agent_id: agent.into(), account_id: account.into(), totals: SpanTotals { day: t(day), week: t(day), month: t(lifetime), year: t(lifetime), lifetime: t(lifetime) } }
    }

    #[test]
    fn the_hierarchy_comes_through_with_the_active_account_marked() {
        let a = agent(
            "codex",
            SwitchSupport::Supported { mechanism: "CODEX_HOME" },
            vec![LoginMethod::Standard, LoginMethod::DeviceCode, LoginMethod::ApiKey],
            "work",
            vec![account("codex", "default", "Default", AuthState::Unknown), account("codex", "work", "Work", AuthState::Valid)],
        );
        let o = overview(&[a], &[usage("codex", "work", 7, 70)]);
        let v = &o.agents[0];
        assert_eq!(v.active.as_deref(), Some("work"));
        assert_eq!(v.accounts.iter().filter(|x| x.active).map(|x| x.account_id.as_str()).collect::<Vec<_>>(), ["work"]);
        assert_eq!(v.accounts[1].auth, AuthStateView::Valid);
        assert_eq!(v.accounts[1].usage, Some(SpanUsageView { day: 7, week: 7, month: 70, year: 70, lifetime: 70 }));
        assert_eq!(v.accounts[0].usage, None, "never recorded: not a made-up zero");
        assert!(v.accounts[0].is_default && !v.accounts[1].is_default);
        assert_eq!(v.switching, SwitchingView { supported: true, mechanism: Some("CODEX_HOME".into()), reason: None });
    }

    #[test]
    fn an_api_key_is_never_offered_in_the_desktop_page() {
        let a = agent("claude", SwitchSupport::Supported { mechanism: "CLAUDE_CONFIG_DIR" }, vec![LoginMethod::Standard, LoginMethod::ApiKey], "default", vec![account("claude", "default", "Default", AuthState::Valid)]);
        assert_eq!(overview(&[a], &[]).agents[0].login_methods, ["standard"], "keys are typed into `agm login --api-key-stdin`, not into UI state");
    }

    #[test]
    fn a_monitor_only_agent_offers_no_sign_in_and_carries_its_reason() {
        let a = agent("antigravity", SwitchSupport::Unavailable { reason: "no supported profile mechanism" }, vec![], "default", vec![account("antigravity", "default", "Default", AuthState::Unknown)]);
        let v = &overview(&[a], &[usage("antigravity", "default", 1, 2)]).agents[0].clone();
        assert!(!v.switching.supported && v.login_methods.is_empty());
        assert_eq!(v.switching.reason.as_deref(), Some("no supported profile mechanism"));
        assert_eq!(v.accounts[0].usage.unwrap().lifetime, 2, "its usage is still monitored and shown");
    }

    #[test]
    fn usage_of_removed_accounts_stays_in_the_totals_and_shows_as_one_line() {
        let a = agent("codex", SwitchSupport::Supported { mechanism: "CODEX_HOME" }, vec![], "default", vec![account("codex", "default", "Default", AuthState::Unknown)]);
        let o = overview(&[a], &[usage("codex", "default", 10, 100), usage("codex", "gone", 0, 5)]);
        let v = &o.agents[0];
        assert_eq!(v.removed_usage, Some(SpanUsageView { day: 0, week: 0, month: 5, year: 5, lifetime: 5 }));
        assert_eq!(v.total.unwrap().lifetime, 105, "the agent's total still includes what the removed account used");
    }

    #[test]
    fn totals_add_up_from_accounts_to_agents_to_everything_and_an_empty_agent_has_none() {
        let a = agent("codex", SwitchSupport::Supported { mechanism: "CODEX_HOME" }, vec![], "default", vec![account("codex", "default", "Default", AuthState::Unknown), account("codex", "work", "Work", AuthState::Valid)]);
        let b = agent("claude", SwitchSupport::Supported { mechanism: "CLAUDE_CONFIG_DIR" }, vec![], "default", vec![account("claude", "default", "Default", AuthState::Valid)]);
        let c = agent("gemini", SwitchSupport::Supported { mechanism: "GEMINI_CLI_HOME" }, vec![], "default", vec![account("gemini", "default", "Default", AuthState::Unknown)]);
        let o = overview(&[a, b, c], &[usage("codex", "default", 1, 10), usage("codex", "work", 2, 20), usage("claude", "default", 4, 40)]);
        assert_eq!(o.agents[0].total.unwrap().day, 3);
        assert_eq!(o.agents[1].total.unwrap().day, 4);
        assert_eq!(o.agents[2].total, None, "nothing recorded for gemini: no total, not zero");
        assert_eq!(o.total.unwrap(), SpanUsageView { day: 7, week: 7, month: 70, year: 70, lifetime: 70 });
    }

    #[test]
    fn every_agent_gets_its_catalogue_colour() {
        let a = agent("codex", SwitchSupport::Supported { mechanism: "CODEX_HOME" }, vec![], "default", vec![account("codex", "default", "Default", AuthState::Unknown)]);
        assert_eq!(overview(&[a], &[]).agents[0].color, "#00c2a8");
    }
}
