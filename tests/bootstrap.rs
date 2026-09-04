use codex_managed_channel::bootstrap::{BootstrapAction, parse_original_command};

const CURRENT: &str = r#"sh -c 'login shell wrapper may change' sh 'printf '\''%b'\'' '\''\362\044\232\001\237\075\154\341'\''; PATH="${CODEX_INSTALL_DIR:-$HOME/.local/bin}:$PATH"; export PATH; if [ -S "${SSH_AUTH_SOCK:-}" ]; then :; fi && exec codex app-server proxy'"#;

#[test]
fn accepts_current_desktop_bootstrap_without_matching_the_whole_wrapper() {
    let parsed = parse_original_command(CURRENT).unwrap();
    assert_eq!(
        parsed.nonce,
        [0o362, 0o044, 0o232, 0o001, 0o237, 0o075, 0o154, 0o341]
    );
    assert_eq!(parsed.action, BootstrapAction::AppServerProxy);
}

#[test]
fn classifies_current_desktop_preflight_commands() {
    let prefix = "sh -c 'login wrapper' sh 'printf '\''%b'\'' '\''\\001\\002\\003\\004\\005\\006\\007\\010'\''; PATH=whatever; export PATH; ";
    let cases = [
        (
            "if command -v codex >/dev/null 2>&1; then exit 0; fi'",
            BootstrapAction::CodexPathProbe,
        ),
        ("codex --version'", BootstrapAction::CodexVersionProbe),
        (
            "if [ \"${CODEX_SSH_SKIP_APP_SERVER_BOOT:-}\" = \"true\" ]; then exit 0; fi; managed bootstrap body'",
            BootstrapAction::AppServerBootstrap,
        ),
    ];
    for (suffix, expected) in cases {
        let parsed = parse_original_command(&format!("{prefix}{suffix}")).unwrap();
        assert_eq!(parsed.action, expected);
        assert_eq!(parsed.nonce, [1, 2, 3, 4, 5, 6, 7, 8]);
    }
}

#[test]
fn rejects_ambiguous_preflight_markers() {
    assert!(
        parse_original_command(
            "printf '%b' '\\001\\002\\003\\004\\005\\006\\007\\010'; CODEX_SSH_SKIP_APP_SERVER_BOOT=true; exec codex app-server proxy"
        )
        .is_err()
    );
}

#[test]
fn action_error_reports_only_the_fixed_marker_tail_as_hex() {
    let error = parse_original_command(
        "printf '%b' '\\001\\002\\003\\004\\005\\006\\007\\010'; if command -v codex >/dev/null 2>&1; then exit 0; fi;id",
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("path_tail_hex=3b6964"));
}

#[test]
fn accepts_desktop_path_probe_missing_codex_sentinel() {
    let parsed = parse_original_command(
        "printf '%b' '\\001\\002\\003\\004\\005\\006\\007\\010'; if command -v codex >/dev/null 2>&1; then exit 0; fi; exit 86'",
    )
    .unwrap();
    assert_eq!(parsed.action, BootstrapAction::CodexPathProbe);
}

#[test]
fn accepts_legacy_direct_payload_shape() {
    let parsed = parse_original_command(
        r#"printf '%b' '\001\002\003\004\005\006\007\010'; exec codex app-server proxy"#,
    )
    .unwrap();
    assert_eq!(parsed.nonce, [1, 2, 3, 4, 5, 6, 7, 8]);
}

#[test]
fn accepts_double_escaped_nonce_from_desktop_login_wrapper() {
    let parsed = parse_original_command(
        r#"sh -c 'login wrapper' sh 'printf %b \\164\\327\\317\\006\\044\\127\\154\\174; if command -v codex >/dev/null 2>&1; then exit 0; fi"#,
    )
    .unwrap();
    assert_eq!(
        parsed.nonce,
        [0o164, 0o327, 0o317, 0o006, 0o044, 0o127, 0o154, 0o174]
    );
    assert_eq!(parsed.action, BootstrapAction::CodexPathProbe);
}

#[test]
fn rejects_ambiguous_or_non_proxy_commands() {
    let error =
        parse_original_command("printf '%b' '\\001\\002\\003\\004\\005\\006\\007\\010'; id")
            .unwrap_err()
            .to_string();
    assert!(error.contains("path=0/false"));
    assert!(error.contains("version=0/false"));
    assert!(error.contains("bootstrap=0"));
    assert!(error.contains("proxy=0/false"));
    assert!(error.contains("bytes="));
    assert!(parse_original_command("printf '%b' '\\001\\002\\003\\004\\005\\006\\007\\010'; exec codex app-server proxy; exec codex app-server proxy").is_err());
}

#[test]
fn rejects_newlines_and_wrong_nonce_lengths() {
    assert!(
        parse_original_command(
            "printf '%b' '\\001\\002\\003\\004\\005\\006\\007'; exec codex app-server proxy"
        )
        .is_err()
    );
    assert!(
        parse_original_command(
            "printf '%b' '\\001\\002\\003\\004\\005\\006\\007\\010';\nexec codex app-server proxy"
        )
        .is_err()
    );
}
