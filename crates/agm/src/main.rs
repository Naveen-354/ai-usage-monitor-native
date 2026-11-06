//! `agm` - the command-line side of the multi-account manager. The desktop app and this tool read and change the same
//! accounts (they share one database), so what one does the other sees at once.

mod cli;
mod commands;
mod render;

use std::io::{stderr, stdin, stdout};

use ai_usage_monitor_core::accounts::AccountManager;
use ai_usage_monitor_core::api::default_data_dir;
use clap::Parser;

fn main() {
    let cli = cli::Cli::parse();
    let data_dir = cli.data_dir.clone().unwrap_or_else(default_data_dir);
    let manager = match AccountManager::open(&data_dir) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("agm: could not open the accounts in {}: {e}", data_dir.display());
            std::process::exit(1);
        }
    };
    let (mut out, mut err, mut input) = (stdout().lock(), stderr().lock(), stdin().lock());
    let mut io = commands::Io { out: &mut out, err: &mut err, input: &mut input };
    match commands::execute(&manager, cli.command, &mut io) {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("agm: {e}");
            std::process::exit(1);
        }
    }
}
