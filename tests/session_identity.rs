use std::process::Command;

#[test]
fn rejects_client_identity_path_escape_before_bootstrap() {
    let output = Command::new(env!("CARGO_BIN_EXE_codex-managed-entry"))
        .args(["--client-id", "../other"])
        .env("SSH_ORIGINAL_COMMAND", "printf '%b' '\\001\\002\\003\\004\\005\\006\\007\\010'; if command -v codex >/dev/null 2>&1; then exit 0; fi")
        .output().unwrap();
    assert!(!output.status.success());
    assert!(
        output.stdout.is_empty(),
        "invalid identity must be rejected before nonce"
    );
}

#[test]
fn stop_of_absent_client_is_idempotent() {
    let temp = tempfile::tempdir().unwrap();
    let entry = env!("CARGO_BIN_EXE_codex-managed-entry");
    let output = Command::new(entry)
        .args(["--stop-client", "laptop"])
        .env_remove("SSH_ORIGINAL_COMMAND")
        .env("CODEX_MANAGED_CODEX_BIN", entry)
        .env("CODEX_MANAGED_ROOT", temp.path().join("state"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
