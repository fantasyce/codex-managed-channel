use anyhow::{Context, Result};
use codex_managed_channel::{
    bootstrap::{BootstrapAction, parse_original_command},
    supervisor::{SupervisorConfig, run},
};
use std::env;
use std::io::{self, Write};
use std::process::Command;

fn main() {
    let code = match real_main() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("codex-managed-entry: {error:#}");
            64
        }
    };
    std::process::exit(code);
}

fn real_main() -> Result<i32> {
    let original = env::var("SSH_ORIGINAL_COMMAND").context("SSH_ORIGINAL_COMMAND is missing")?;
    let request = parse_original_command(&original)?;
    io::stdout().write_all(&request.nonce)?;
    io::stdout().flush()?;
    let config = SupervisorConfig::from_env()?;
    match request.action {
        BootstrapAction::CodexPathProbe | BootstrapAction::AppServerBootstrap => Ok(0),
        BootstrapAction::CodexVersionProbe => {
            let status = Command::new(&config.codex_bin)
                .arg("--version")
                .status()
                .context("failed to run the resolved Codex version probe")?;
            Ok(status.code().unwrap_or(1))
        }
        BootstrapAction::AppServerProxy => run(config),
    }
}
