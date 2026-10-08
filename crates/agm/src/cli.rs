//! The command line, as `agm --help` shows it.

use std::ffi::OsString;
use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "agm",
    version,
    about = "Manage several accounts for each AI coding agent: sign in, switch, run, and see usage per account.",
    long_about = "Manage several accounts for each AI coding agent (Codex, Claude Code, Gemini CLI, OpenCode, ...).\n\n\
Every agent has a default account - the sign-in it already uses on this machine - and, where the agent has a documented way to keep a \
second profile apart, any number of accounts of its own. `agm use` picks the account an agent runs with; `agm run` starts the \
agent with it. Nothing the agent keeps is rewritten: each account has its own folder, and switching only changes which folder the \
next launch points at. The desktop app shows and changes the same accounts."
)]
pub struct Cli {
    /// The folder holding the shared database and the accounts' profile folders (default: the desktop app's own).
    #[arg(long, global = true, value_name = "DIR")]
    pub data_dir: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// List agents and their accounts (`agm accounts codex` for one agent).
    Accounts {
        /// Only this agent.
        agent: Option<String>,
        /// Ask each agent whether its accounts are still signed in before listing.
        #[arg(long)]
        check: bool,
        #[arg(long)]
        json: bool,
    },

    /// Add an account by signing in with the agent's own sign-in. With --account, sign an existing account in again.
    Login {
        agent: String,
        /// What to call the new account (default: "Account N").
        #[arg(long, short)]
        name: Option<String>,
        /// Sign this existing account in again (its session expired) instead of adding a new one.
        #[arg(long, value_name = "ID")]
        account: Option<String>,
        /// Sign in with a code typed on another device (for a machine without a browser), where the agent offers it.
        #[arg(long)]
        device_code: bool,
        /// Use an API key read from standard input (never from the command line, where other programs could see it).
        #[arg(long)]
        api_key_stdin: bool,
    },

    /// Choose the account an agent uses from now on. Other agents are not affected.
    Use {
        agent: String,
        /// The account's id or name.
        account: String,
    },

    /// Show the account each agent is using now.
    Current {
        #[arg(long)]
        json: bool,
    },

    /// Show token usage: today, this week, this month, this year and lifetime. Active accounts by default.
    Usage {
        /// Only this agent.
        agent: Option<String>,
        /// Every account, with per-agent and overall totals.
        #[arg(long)]
        all: bool,
        #[arg(long)]
        json: bool,
    },

    /// Start an agent with its active account. Everything after the agent name is passed to the agent.
    Run {
        agent: String,
        #[arg(num_args = 0.., trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },

    /// Remove an account: its sign-in folder and any key kept for it. Its usage history stays unless --purge-usage.
    Remove {
        agent: String,
        account: String,
        /// Also delete the usage recorded for this account.
        #[arg(long)]
        purge_usage: bool,
        /// Do not ask for confirmation.
        #[arg(long, short = 'y')]
        yes: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn the_definition_is_consistent() {
        Cli::command().debug_assert();
    }

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once("agm").chain(args.iter().copied()))
    }

    #[test]
    fn every_command_in_the_brief_exists() {
        for args in [
            &["login", "codex"][..],
            &["accounts"],
            &["accounts", "codex"],
            &["use", "codex", "work"],
            &["current"],
            &["usage"],
            &["run", "codex"],
        ] {
            assert!(parse(args).is_ok(), "{args:?}");
        }
    }

    #[test]
    fn run_passes_everything_after_the_agent_to_the_agent_including_dashed_flags() {
        let Command::Run { agent, args } = parse(&["run", "codex", "exec", "--model", "x", "-v"]).unwrap().command else { panic!() };
        assert_eq!(agent, "codex");
        assert_eq!(args, ["exec", "--model", "x", "-v"].map(OsString::from));
        let Command::Run { args, .. } = parse(&["run", "claude", "--", "--help"]).unwrap().command else { panic!() };
        assert_eq!(args, [OsString::from("--help")]);
    }

    #[test]
    fn the_api_key_is_never_a_command_line_value() {
        assert!(parse(&["login", "claude", "--api-key", "sk-secret"]).is_err(), "there is no such option");
        assert!(parse(&["login", "claude", "--api-key-stdin"]).is_ok());
    }

    #[test]
    fn a_missing_account_is_a_usage_error() {
        assert!(parse(&["use", "codex"]).is_err());
        assert!(parse(&["login"]).is_err());
        assert!(parse(&["remove", "codex"]).is_err());
    }

    #[test]
    fn the_data_folder_can_be_given_before_or_after_the_command() {
        assert_eq!(parse(&["--data-dir", "D:/x", "current"]).unwrap().data_dir, Some(PathBuf::from("D:/x")));
        assert_eq!(parse(&["current", "--data-dir", "D:/x"]).unwrap().data_dir, Some(PathBuf::from("D:/x")));
    }
}
