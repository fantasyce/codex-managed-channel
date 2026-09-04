use anyhow::{Result, bail};

const PROXY_MARKER: &str = "exec codex app-server proxy";
const PATH_PROBE_MARKER: &str = "if command -v codex >/dev/null 2>&1; then exit 0; fi";
const VERSION_PROBE_MARKER: &str = "codex --version";
const BOOTSTRAP_MARKER: &str = "CODEX_SSH_SKIP_APP_SERVER_BOOT";
const MAX_COMMAND_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootstrapAction {
    CodexPathProbe,
    CodexVersionProbe,
    AppServerBootstrap,
    AppServerProxy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapRequest {
    pub nonce: [u8; 8],
    pub action: BootstrapAction,
}

pub fn parse_original_command(command: &str) -> Result<BootstrapRequest> {
    if command.is_empty() || command.len() > MAX_COMMAND_BYTES {
        bail!("bootstrap command length is invalid");
    }
    if command.bytes().any(|b| matches!(b, b'\0' | b'\n' | b'\r')) {
        bail!("bootstrap command contains a forbidden control byte");
    }
    if !command.contains("printf") || !command.contains("%b") {
        bail!("bootstrap command has no compatible nonce printer");
    }

    let path_count = command.matches(PATH_PROBE_MARKER).count();
    let path_terminal = has_path_probe_marker(command);
    let version_count = command.matches(VERSION_PROBE_MARKER).count();
    let version_terminal = has_terminal_marker(command, VERSION_PROBE_MARKER);
    let bootstrap_count = command.matches(BOOTSTRAP_MARKER).count();
    let proxy_count = command.matches(PROXY_MARKER).count();
    let proxy_terminal = has_terminal_marker(command, PROXY_MARKER);

    let mut actions = Vec::new();
    if path_terminal {
        actions.push(BootstrapAction::CodexPathProbe);
    }
    if version_terminal {
        actions.push(BootstrapAction::CodexVersionProbe);
    }
    if bootstrap_count > 0 {
        actions.push(BootstrapAction::AppServerBootstrap);
    }
    if proxy_terminal {
        actions.push(BootstrapAction::AppServerProxy);
    }
    if actions.len() != 1 {
        bail!(
            "expected exactly one supported Codex Desktop SSH action \
             (path={path_count}/{path_terminal}, version={version_count}/{version_terminal}, \
             bootstrap={bootstrap_count}, proxy={proxy_count}/{proxy_terminal}, bytes={}, \
             path_tail_hex={})",
            command.len(),
            marker_tail_hex(command, PATH_PROBE_MARKER)
        );
    }

    let candidates = octal_nonce_candidates(command)?;
    if candidates.len() != 1 {
        bail!("expected exactly one eight-byte octal nonce");
    }
    Ok(BootstrapRequest {
        nonce: candidates[0],
        action: actions[0],
    })
}

fn marker_tail_hex(command: &str, marker: &str) -> String {
    let Some(start) = command.find(marker).map(|offset| offset + marker.len()) else {
        return "none".to_owned();
    };
    command.as_bytes()[start..]
        .iter()
        .take(12)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn has_terminal_marker(command: &str, marker: &str) -> bool {
    if command.matches(marker).count() != 1 {
        return false;
    }
    let marker_end = command.find(marker).expect("marker count checked") + marker.len();
    command[marker_end..]
        .bytes()
        .all(|b| b.is_ascii_whitespace() || matches!(b, b'\'' | b'"'))
}

fn has_path_probe_marker(command: &str) -> bool {
    if has_terminal_marker(command, PATH_PROBE_MARKER) {
        return true;
    }
    if command.matches(PATH_PROBE_MARKER).count() != 1 {
        return false;
    }
    let marker_end = command
        .find(PATH_PROBE_MARKER)
        .expect("marker count checked")
        + PATH_PROBE_MARKER.len();
    command[marker_end..].trim_matches(|character: char| {
        character.is_ascii_whitespace() || matches!(character, '\'' | '"')
    }) == "; exit 86"
}

fn octal_nonce_candidates(input: &str) -> Result<Vec<[u8; 8]>> {
    let bytes = input.as_bytes();
    let mut found = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let slash_count = bytes[i..].iter().take_while(|b| **b == b'\\').count();
        if slash_count == 0 || octal_value(bytes, i, slash_count).is_none() {
            i += 1;
            continue;
        }
        let start = i;
        let mut values = Vec::new();
        while let Some(value) = octal_value(bytes, i, slash_count) {
            values.push(value);
            i += slash_count + 3;
        }
        if values.len() == 8 {
            let nonce: [u8; 8] = values.try_into().expect("length checked");
            found.push(nonce);
        }
        if i == start {
            i += 1;
        }
    }
    Ok(found)
}

fn octal_value(bytes: &[u8], offset: usize, slash_count: usize) -> Option<u8> {
    let digits_start = offset.checked_add(slash_count)?;
    let digits = bytes.get(digits_start..digits_start.checked_add(3)?)?;
    if slash_count == 0
        || bytes.get(offset..digits_start)?.iter().any(|b| *b != b'\\')
        || !digits.iter().all(|b| matches!(b, b'0'..=b'7'))
    {
        return None;
    }
    let text = std::str::from_utf8(digits).ok()?;
    u8::from_str_radix(text, 8).ok()
}
