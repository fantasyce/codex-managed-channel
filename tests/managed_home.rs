use codex_managed_channel::managed_home::prepare_managed_home;
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[test]
fn isolated_server_uses_a_sanitized_managed_config_without_changing_the_real_one() {
    let temp = tempfile::tempdir().unwrap();
    let real_codex_home = temp.path().join(".codex");
    fs::create_dir_all(&real_codex_home).unwrap();
    let real_config = format!(
        "model = \"gpt-test\"\n\n[mcp_servers.node_repl]\ncommand = \"stale-node-repl\"\n\n[mcp_servers.computer-use]\ncommand = \"stale-computer-use\"\nenabled = false\n\n[marketplaces.personal]\nsource_type = \"local\"\nsource = \"{}\"\n\n[projects.\"{}\"]\ntrust_level = \"trusted\"\n\n[projects.\"{}/Documents/projects\"]\ntrust_level = \"trusted\"\n",
        temp.path().display(),
        temp.path().display(),
        temp.path().display()
    );
    fs::write(real_codex_home.join("config.toml"), &real_config).unwrap();
    fs::write(real_codex_home.join("auth.json"), "test-auth").unwrap();
    let installed_sky = real_codex_home.join("computer-use/Codex Computer Use.app");
    fs::create_dir_all(&installed_sky).unwrap();
    let desktop_resources = temp.path().join("Desktop.app/Contents/Resources");
    fs::create_dir_all(desktop_resources.join("cua_node/bin")).unwrap();
    fs::create_dir_all(desktop_resources.join("cua_node/lib/node_modules")).unwrap();
    fs::write(desktop_resources.join("codex"), "test").unwrap();
    fs::write(desktop_resources.join("cua_node/bin/node"), "test").unwrap();
    fs::write(desktop_resources.join("cua_node/bin/node_repl"), "test").unwrap();
    let desktop_resources = fs::canonicalize(desktop_resources).unwrap();
    fs::write(
        real_codex_home.join("logs_2.sqlite"),
        "private-runtime-state",
    )
    .unwrap();

    let marketplace = temp.path().join("marketplace");
    let bundled_marketplace = temp.path().join("bundled-marketplace");
    let plugin = marketplace.join("plugins/test-plugin/.codex-plugin");
    fs::create_dir_all(&plugin).unwrap();
    fs::create_dir_all(marketplace.join(".agents/plugins")).unwrap();
    fs::write(plugin.join("plugin.json"), r#"{"name":"test-plugin"}"#).unwrap();
    fs::write(
        marketplace.join(".agents/plugins/marketplace.json"),
        r#"{"name":"personal","plugins":[{"name":"test-plugin","source":{"source":"local","path":"./plugins/test-plugin"}}]}"#,
    )
    .unwrap();
    let bundled_plugin = bundled_marketplace.join("plugins/unified-computer-use/.codex-plugin");
    fs::create_dir_all(&bundled_plugin).unwrap();
    fs::create_dir_all(bundled_marketplace.join(".agents/plugins")).unwrap();
    fs::write(
        bundled_plugin.join("plugin.json"),
        r#"{"name":"test-plugin"}"#,
    )
    .unwrap();
    fs::write(
        bundled_marketplace.join("plugins/unified-computer-use/.mcp.json"),
        r#"{"mcpServers":{"cua_repl":{"command":"node","args":["scripts/launch.mjs"],"enabled":false}}}"#,
    )
    .unwrap();
    fs::write(
        bundled_marketplace.join(".agents/plugins/marketplace.json"),
        r#"{"name":"openai-bundled","plugins":[{"name":"unified-computer-use","source":{"source":"local","path":"./plugins/unified-computer-use"}}]}"#,
    )
    .unwrap();

    let capture_dir = temp.path().join("capture");
    fs::create_dir_all(&capture_dir).unwrap();
    let fake = temp.path().join("codex");
    fs::write(
        &fake,
        "#!/bin/sh\nprintf '%s' \"${CODEX_HOME:-}\" > \"$CAPTURE_DIR/codex-home\"\npwd > \"$CAPTURE_DIR/cwd\"\ncp \"${CODEX_HOME}/config.toml\" \"$CAPTURE_DIR/config.toml\"\nexit 0\n",
    )
    .unwrap();
    let mut permissions = fs::metadata(&fake).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&fake, permissions).unwrap();

    let _ = Command::new(env!("CARGO_BIN_EXE_codex-managed-entry"))
        .env("HOME", temp.path())
        .env("CODEX_HOME", &real_codex_home)
        .env("CODEX_MANAGED_CODEX_BIN", &fake)
        .env("CODEX_MANAGED_PERSONAL_MARKETPLACE", &marketplace)
        .env("CODEX_MANAGED_BUNDLED_MARKETPLACE", &bundled_marketplace)
        .env("CODEX_MANAGED_DESKTOP_RESOURCES", &desktop_resources)
        .env("CAPTURE_DIR", &capture_dir)
        .env(
            "SSH_ORIGINAL_COMMAND",
            "printf '%b' '\\001\\002\\003\\004\\005\\006\\007\\010'; exec codex app-server proxy",
        )
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    let child_home = fs::read_to_string(capture_dir.join("codex-home")).unwrap();
    let child_cwd = fs::read_to_string(capture_dir.join("cwd")).unwrap();
    let child_config = fs::read_to_string(capture_dir.join("config.toml")).unwrap();
    assert_ne!(child_home, real_codex_home.to_string_lossy());
    assert_ne!(child_cwd.trim(), temp.path().to_string_lossy());
    assert!(child_cwd.trim().ends_with("/.codex-managed/run"));
    assert!(!child_config.contains(&format!("[projects.\"{}\"]", temp.path().display())));
    assert!(child_config.contains(&format!(
        "[projects.\"{}/Documents/projects\"]",
        temp.path().display()
    )));
    assert!(child_config.contains(&format!("source = \"{}\"", marketplace.display())));
    assert!(child_config.contains(&format!("source = \"{}\"", bundled_marketplace.display())));
    assert!(!child_config.contains("[mcp_servers.node_repl]"));
    assert!(!child_config.contains("[mcp_servers.computer-use]"));
    let parsed = child_config.parse::<toml_edit::DocumentMut>().unwrap();
    assert_eq!(
        parsed["mcp_servers"]["cua_repl"]["command"].as_str(),
        desktop_resources.join("cua_node/bin/node").to_str()
    );
    assert_eq!(
        parsed["mcp_servers"]["cua_repl"]["args"][0].as_str(),
        Some(
            bundled_marketplace
                .join("plugins/unified-computer-use/scripts/launch.mjs")
                .to_str()
                .unwrap()
        )
    );
    assert_eq!(
        parsed["mcp_servers"]["cua_repl"]["enabled"].as_bool(),
        Some(true)
    );
    assert_eq!(
        parsed["mcp_servers"]["cua_repl"]["env"]["CUA_REPL_NODE_REPL_PATH"].as_str(),
        desktop_resources.join("cua_node/bin/node_repl").to_str()
    );
    assert_eq!(
        parsed["mcp_servers"]["cua_repl"]["env"]["SKY_CUA_SERVICE_PATH"].as_str(),
        installed_sky.to_str()
    );
    assert_eq!(
        parsed["mcp_servers"]["cua_repl"]["env"]["NODE_REPL_HOST_SERVICES_PIPE_PATH"].as_str(),
        Some("")
    );
    assert_eq!(
        parsed["mcp_servers"]["cua_repl"]["env"]["CODEX_CLI_PATH"].as_str(),
        desktop_resources.join("codex").to_str()
    );
    assert_eq!(
        parsed["mcp_servers"]["cua_repl"]["env"]["CODEX_HOME"].as_str(),
        real_codex_home.to_str()
    );
    assert!(child_config.contains(&format!("sqlite_home = \"{}\"", real_codex_home.display())));
    let linked_auth = std::path::PathBuf::from(&child_home).join("auth.json");
    assert_eq!(fs::read_to_string(linked_auth).unwrap(), "test-auth");
    assert!(
        !std::path::PathBuf::from(&child_home)
            .join("logs_2.sqlite")
            .exists()
    );
    assert_eq!(
        fs::read_to_string(real_codex_home.join("config.toml")).unwrap(),
        real_config
    );
}

struct ManagedHomeFixture {
    _temp: tempfile::TempDir,
    os_home: PathBuf,
    real_codex_home: PathBuf,
    managed_root: PathBuf,
    marketplace: PathBuf,
    bundled_marketplace: PathBuf,
}

impl ManagedHomeFixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let os_home = temp.path().to_path_buf();
        let real_codex_home = os_home.join(".codex");
        fs::create_dir_all(&real_codex_home).unwrap();
        fs::write(
            real_codex_home.join("config.toml"),
            "model = \"gpt-test\"\n",
        )
        .unwrap();
        fs::create_dir_all(real_codex_home.join("computer-use/Codex Computer Use.app")).unwrap();

        let desktop_resources = os_home.join("Applications/ChatGPT.app/Contents/Resources");
        fs::create_dir_all(desktop_resources.join("cua_node/bin")).unwrap();
        fs::create_dir_all(desktop_resources.join("cua_node/lib/node_modules")).unwrap();
        fs::write(desktop_resources.join("codex"), "test").unwrap();
        fs::write(desktop_resources.join("cua_node/bin/node"), "test").unwrap();
        fs::write(desktop_resources.join("cua_node/bin/node_repl"), "test").unwrap();

        let marketplace = os_home.join("marketplace");
        let bundled_marketplace = os_home.join("bundled-marketplace");
        let bundled_plugin = bundled_marketplace.join("plugins/unified-computer-use");
        fs::create_dir_all(&bundled_plugin).unwrap();
        fs::write(
            bundled_plugin.join(".mcp.json"),
            r#"{"mcpServers":{"cua_repl":{"command":"node","args":[],"enabled":false}}}"#,
        )
        .unwrap();

        Self {
            managed_root: os_home.join(".codex-managed"),
            _temp: temp,
            os_home,
            real_codex_home,
            marketplace,
            bundled_marketplace,
        }
    }

    fn prepare(&self) -> anyhow::Result<PathBuf> {
        prepare_managed_home(
            &self.os_home,
            &self.real_codex_home,
            &self.managed_root,
            &self.marketplace,
            &self.bundled_marketplace,
        )
    }
}

#[test]
fn fresh_session_index_is_not_shared_into_managed_home() {
    let fixture = ManagedHomeFixture::new();
    let real_index = fixture.real_codex_home.join("session_index.jsonl");
    fs::write(&real_index, "real-index\n").unwrap();

    let managed_home = fixture.prepare().unwrap();
    let managed_index = managed_home.join("session_index.jsonl");

    assert!(!managed_index.exists());
    assert_eq!(fs::read_to_string(&real_index).unwrap(), "real-index\n");
}

#[test]
fn runtime_replaced_session_index_remains_private_on_next_prepare() {
    let fixture = ManagedHomeFixture::new();
    let real_index = fixture.real_codex_home.join("session_index.jsonl");
    fs::write(&real_index, "real-index\n").unwrap();

    let managed_home = fixture.managed_root.join("codex-home");
    fs::create_dir_all(&managed_home).unwrap();
    let managed_index = managed_home.join("session_index.jsonl");
    symlink(&real_index, &managed_index).unwrap();

    let replacement = managed_home.join(".session_index.jsonl.runtime-replacement");
    fs::write(&replacement, "managed-runtime-index\n").unwrap();
    fs::rename(&replacement, &managed_index).unwrap();
    assert!(
        !fs::symlink_metadata(&managed_index)
            .unwrap()
            .file_type()
            .is_symlink()
    );

    fixture.prepare().unwrap();

    assert_eq!(
        fs::read_to_string(&managed_index).unwrap(),
        "managed-runtime-index\n"
    );
    assert_eq!(fs::read_to_string(&real_index).unwrap(), "real-index\n");
    assert!(
        !fs::symlink_metadata(&managed_index)
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn runtime_replaced_shared_entry_is_still_rejected() {
    let fixture = ManagedHomeFixture::new();
    let real_shared = fixture.real_codex_home.join("shared-entry.json");
    fs::write(&real_shared, "real-shared\n").unwrap();

    let managed_home = fixture.prepare().unwrap();
    let managed_shared = managed_home.join("shared-entry.json");
    let replacement = managed_home.join(".shared-entry.json.runtime-replacement");
    fs::write(&replacement, "unexpected-local-copy\n").unwrap();
    fs::rename(&replacement, &managed_shared).unwrap();

    let error = fixture.prepare().unwrap_err();

    assert!(
        error
            .to_string()
            .contains("managed CODEX_HOME contains an unmanaged entry")
    );
    assert!(error.to_string().contains("shared-entry.json"));
}
