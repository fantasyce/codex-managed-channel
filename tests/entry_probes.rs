use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

const NONCE: &[u8] = &[1, 2, 3, 4, 5, 6, 7, 8];
const PREFIX: &str =
    "printf '%b' '\\001\\002\\003\\004\\005\\006\\007\\010'; PATH=whatever; export PATH; ";

#[test]
fn desktop_path_probe_succeeds_without_opening_a_shell() {
    let temp = tempfile::tempdir().unwrap();
    let fake = fake_codex(&temp);
    let output = entry(
        &temp,
        &fake,
        &format!("{PREFIX}if command -v codex >/dev/null 2>&1; then exit 0; fi"),
    )
    .output()
    .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, NONCE);
}

#[test]
fn desktop_version_probe_runs_only_the_resolved_codex_binary() {
    let temp = tempfile::tempdir().unwrap();
    let fake = fake_codex(&temp);
    let output = entry(&temp, &fake, &format!("{PREFIX}codex --version"))
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, [NONCE, b"codex-cli test\n"].concat());
}

#[test]
fn desktop_bootstrap_probe_is_a_safe_noop_for_the_supervised_worker() {
    let temp = tempfile::tempdir().unwrap();
    let fake = fake_codex(&temp);
    let command = format!(
        "{PREFIX}if [ \"${{CODEX_SSH_SKIP_APP_SERVER_BOOT:-}}\" = \"true\" ]; then exit 0; fi; bundled bootstrap body"
    );
    let output = entry(&temp, &fake, &command).output().unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, NONCE);
}

#[test]
fn preflight_rejects_a_personal_marketplace_without_a_catalog() {
    let temp = tempfile::tempdir().unwrap();
    let fake = temp.path().join("codex");
    fs::write(
        &fake,
        "#!/bin/sh\nif [ \"${1:-}\" = \"--version\" ]; then echo 'codex-cli test'; else echo '--listen --sock'; fi\n",
    )
    .unwrap();
    let mut permissions = fs::metadata(&fake).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&fake, permissions).unwrap();
    let marketplace = temp.path().join("personal-marketplace");
    fs::create_dir_all(&marketplace).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_codex-managed-preflight"))
        .env("HOME", temp.path())
        .env("CODEX_MANAGED_CODEX_BIN", &fake)
        .env("CODEX_MANAGED_PERSONAL_MARKETPLACE", &marketplace)
        .arg(format!("{PREFIX}exec codex app-server proxy"))
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("marketplace"));
}

fn fake_codex(temp: &tempfile::TempDir) -> std::path::PathBuf {
    let fake = temp.path().join("codex");
    fs::write(
        &fake,
        "#!/bin/sh\nif [ \"${1:-}\" = \"--version\" ]; then echo 'codex-cli test'; exit 0; fi\nexit 91\n",
    )
    .unwrap();
    let mut permissions = fs::metadata(&fake).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&fake, permissions).unwrap();
    fake
}

fn entry(temp: &tempfile::TempDir, fake: &std::path::Path, original: &str) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_codex-managed-entry"));
    command
        .env("HOME", temp.path())
        .env("CODEX_MANAGED_CODEX_BIN", fake)
        .env("SSH_ORIGINAL_COMMAND", original);
    command
}
