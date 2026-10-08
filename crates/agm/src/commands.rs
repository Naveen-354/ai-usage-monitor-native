//! What each command does. Handlers write to the streams they are given, so they can be tested without a terminal.

use std::io::{BufRead, Write};

use ai_usage_monitor_core::accounts::{AccountError, AccountManager, AccountResult, LoginMethod, Secret};

use crate::cli::Command;
use crate::render;

/// The streams a command talks to.
pub struct Io<'a> {
    pub out: &'a mut dyn Write,
    pub err: &'a mut dyn Write,
    pub input: &'a mut dyn BufRead,
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn io_err(e: std::io::Error) -> AccountError {
    AccountError::Io(e)
}

/// Runs one command and returns the process exit code (an agent's own exit code for `run`).
pub fn execute(manager: &AccountManager, command: Command, io: &mut Io) -> AccountResult<i32> {
    match command {
        Command::Accounts { agent, check, json } => accounts(manager, agent.as_deref(), check, json, io),
        Command::Login { agent, name, account, device_code, api_key_stdin } => login(manager, &agent, name.as_deref(), account.as_deref(), device_code, api_key_stdin, io),
        Command::Use { agent, account } => use_account(manager, &agent, &account, io),
        Command::Current { json } => current(manager, json, io),
        Command::Usage { agent, all, json } => usage(manager, agent.as_deref(), all, json, io),
        Command::Run { agent, args } => manager.run(&agent, &args),
        Command::Remove { agent, account, purge_usage, yes } => remove(manager, &agent, &account, purge_usage, yes, io),
    }
}

fn accounts(manager: &AccountManager, agent: Option<&str>, check: bool, json: bool, io: &mut Io) -> AccountResult<i32> {
    if check {
        let results = match agent {
            Some(a) => {
                manager.agent(a)?; // unknown agent -> error before anything runs
                manager.store().list(Some(a), false)?.into_iter().map(|acc| (acc.agent_id.clone(), acc.account_id.clone(), manager.check_auth(&acc.agent_id, &acc.account_id))).collect()
            }
            None => manager.check_all(),
        };
        for (a, id, r) in results {
            if let Err(e) = r {
                writeln!(io.err, "agm: could not check {a}/{id}: {e}").map_err(io_err)?;
            }
        }
    }
    let agents = match agent {
        Some(a) => vec![manager.agent(a)?],
        None => manager.agents()?,
    };
    if json {
        writeln!(io.out, "{}", serde_json::to_string_pretty(&render::accounts_json(&agents)).unwrap_or_default()).map_err(io_err)?;
    } else {
        write!(io.out, "{}", render::accounts_text(&agents, now_ms())).map_err(io_err)?;
    }
    Ok(0)
}

fn login(manager: &AccountManager, agent: &str, name: Option<&str>, account: Option<&str>, device_code: bool, api_key_stdin: bool, io: &mut Io) -> AccountResult<i32> {
    let method = if api_key_stdin {
        LoginMethod::ApiKey
    } else if device_code {
        LoginMethod::DeviceCode
    } else {
        LoginMethod::Standard
    };
    let key = if api_key_stdin {
        let mut text = String::new();
        io.input.read_to_string(&mut text).map_err(io_err)?;
        Some(Secret::new(text.trim().to_string()))
    } else {
        None
    };
    let account = match account {
        Some(existing) => {
            writeln!(io.out, "Signing {agent}/{existing} in again - follow the agent's prompts.").map_err(io_err)?;
            manager.relogin(agent, existing, method, key)?
        }
        None => {
            let label = match name {
                Some(n) => n.to_string(),
                None => format!("Account {}", manager.store().list(Some(agent), false)?.len()),
            };
            if method != LoginMethod::ApiKey {
                writeln!(io.out, "Signing in {agent} as '{label}' - follow the agent's prompts.").map_err(io_err)?;
            }
            manager.login(agent, &label, method, key)?
        }
    };
    writeln!(io.out, "{}", render::state_sentence(&account)).map_err(io_err)?;
    writeln!(io.out, "Use it with:  agm use {agent} {}", account.account_id).map_err(io_err)?;
    Ok(0)
}

fn use_account(manager: &AccountManager, agent: &str, account: &str, io: &mut Io) -> AccountResult<i32> {
    let outcome = manager.use_account(agent, account)?;
    writeln!(io.out, "{agent} now uses {} ({}).", outcome.account.account_id, outcome.account.label).map_err(io_err)?;
    if let Some(w) = outcome.warning {
        writeln!(io.err, "agm: warning: {w}").map_err(io_err)?;
    }
    writeln!(io.out, "Start it with:  agm run {agent}").map_err(io_err)?;
    Ok(0)
}

fn current(manager: &AccountManager, json: bool, io: &mut Io) -> AccountResult<i32> {
    let names: Vec<(String, String)> = manager.agents()?.into_iter().map(|a| (a.agent_id, a.name)).collect();
    let current: Vec<(String, String, _)> = manager
        .current()?
        .into_iter()
        .map(|(agent, acc)| {
            let name = names.iter().find(|(id, _)| id == &agent).map(|(_, n)| n.clone()).unwrap_or_else(|| agent.clone());
            (agent, name, acc)
        })
        .collect();
    if json {
        writeln!(io.out, "{}", serde_json::to_string_pretty(&render::current_json(&current)).unwrap_or_default()).map_err(io_err)?;
    } else {
        write!(io.out, "{}", render::current_text(&current)).map_err(io_err)?;
    }
    Ok(0)
}

fn usage(manager: &AccountManager, agent: Option<&str>, all: bool, json: bool, io: &mut Io) -> AccountResult<i32> {
    let agents = match agent {
        Some(a) => vec![manager.agent(a)?],
        None => manager.agents()?,
    };
    let shown: Vec<(String, String, String)> = agents
        .iter()
        .flat_map(|a| {
            a.accounts
                .iter()
                .filter(|acc| all || a.active.as_deref() == Some(&acc.account_id))
                .map(|acc| (a.agent_id.clone(), acc.account_id.clone(), acc.label.clone()))
                .collect::<Vec<_>>()
        })
        .collect();
    let rows = render::usage_rows(&shown, &manager.usage()?, all);
    if json {
        writeln!(io.out, "{}", serde_json::to_string_pretty(&render::usage_json(&rows)).unwrap_or_default()).map_err(io_err)?;
    } else {
        write!(io.out, "{}", render::usage_text(&rows)).map_err(io_err)?;
        writeln!(io.out, "\n  Totals count cached tokens only if \"count cached tokens\" is on in the app's settings. \"-\" = nothing recorded.").map_err(io_err)?;
    }
    Ok(0)
}

fn remove(manager: &AccountManager, agent: &str, account: &str, purge: bool, yes: bool, io: &mut Io) -> AccountResult<i32> {
    let found = manager.store().find(agent, account)?.ok_or_else(|| AccountError::UnknownAccount { agent: agent.into(), account: account.into() })?;
    if found.is_default() {
        return Err(AccountError::CannotRemoveDefault);
    }
    if !yes {
        write!(
            io.out,
            "Remove {} and delete its sign-in folder{}? [y/N] ",
            found.display_ref(),
            if purge { " and its usage history" } else { " (its usage history stays)" }
        )
        .map_err(io_err)?;
        io.out.flush().map_err(io_err)?;
        let mut answer = String::new();
        io.input.read_line(&mut answer).map_err(io_err)?;
        if !matches!(answer.trim().to_lowercase().as_str(), "y" | "yes") {
            writeln!(io.out, "Nothing was removed.").map_err(io_err)?;
            return Ok(1);
        }
    }
    manager.remove(agent, &found.account_id, purge)?;
    writeln!(io.out, "Removed {}.", found.display_ref()).map_err(io_err)?;
    Ok(0)
}
