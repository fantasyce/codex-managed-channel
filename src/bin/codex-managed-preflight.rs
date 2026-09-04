use anyhow::{Context, Result, bail};
use codex_managed_channel::{
    bootstrap::parse_original_command,
    managed_home::prepare_managed_home,
    marketplace::{
        managed_bundled_marketplace_root, managed_marketplace_root, prepare_marketplace_from_cache,
        validate_marketplace, validate_named_marketplace,
    },
    supervisor::resolve_codex_binary,
};
use serde_json::json;
use std::{env, path::PathBuf, process::Command};

fn main() {
    match real_main() {
        Ok(value) => println!("{value}"),
        Err(error) => {
            println!("{}", json!({"ok": false, "error": format!("{error:#}")}));
            std::process::exit(1);
        }
    }
}

fn real_main() -> Result<serde_json::Value> {
    let command = env::args()
        .nth(1)
        .or_else(|| env::var("SSH_ORIGINAL_COMMAND").ok())
        .context("pass a bootstrap command or set SSH_ORIGINAL_COMMAND")?;
    parse_original_command(&command)?;
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is not set")?;
    let marketplace = managed_marketplace_root(&home);
    let real_codex_home = env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".codex"));
    let plugin_cache = real_codex_home.join("plugins/cache");
    if validate_marketplace(&marketplace).is_err() {
        prepare_marketplace_from_cache(&marketplace, &plugin_cache.join("personal"), "personal")?;
        validate_marketplace(&marketplace)?;
    }
    let bundled_marketplace = managed_bundled_marketplace_root(&home);
    if validate_named_marketplace(&bundled_marketplace, "openai-bundled").is_err() {
        prepare_marketplace_from_cache(
            &bundled_marketplace,
            &plugin_cache.join("openai-bundled"),
            "openai-bundled",
        )?;
        validate_named_marketplace(&bundled_marketplace, "openai-bundled")?;
    }
    let managed_root = env::var_os("CODEX_MANAGED_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".codex-managed"));
    let managed_home = prepare_managed_home(
        &home,
        &real_codex_home,
        &managed_root,
        &marketplace,
        &bundled_marketplace,
    )?;
    let binary = resolve_codex_binary(&home)?;
    let version = Command::new(&binary).arg("--version").output()?;
    let help = Command::new(&binary)
        .args(["app-server", "--help"])
        .output()?;
    let proxy_help = Command::new(&binary)
        .args(["app-server", "proxy", "--help"])
        .output()?;
    if !version.status.success()
        || !help.status.success()
        || !proxy_help.status.success()
        || !String::from_utf8_lossy(&help.stdout).contains("--listen")
        || !String::from_utf8_lossy(&proxy_help.stdout).contains("--sock")
    {
        bail!("installed Codex does not expose isolated app-server plus socket proxy");
    }
    Ok(
        json!({"ok": true, "binary": binary, "version": String::from_utf8_lossy(&version.stdout).trim(), "managedCodexHome": managed_home}),
    )
}
