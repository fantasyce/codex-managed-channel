use crate::protocol::ResumeAttempt;
use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

// Thread restore responses contain history and can be much larger than a normal
// control message. Keep this bounded, but above the largest history observed in
// compatibility acceptance (about 26 MiB).
const MAX_MESSAGE_BYTES: usize = 64 * 1024 * 1024;
const WEBSOCKET_KEY: &str = "dGhlIHNhbXBsZSBub25jZQ==";
const WEBSOCKET_ACCEPT: &str = "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=";

/// Read-only and fail closed. A partial inventory or unknown status is not idle.
pub(crate) fn runtime_is_idle(path: &Path, timeout: Duration) -> bool {
    let check = || -> Result<bool> {
        let mut rpc = RpcClient::connect(path, timeout, "codex-managed-idle-check")?;
        let mut cursor = Value::Null;
        let mut seen = std::collections::HashSet::new();
        for _ in 0..128 {
            let page = rpc
                .request("thread/loaded/list", json!({"cursor":cursor,"limit":100}))
                .map_err(rpc_failure_to_anyhow)?;
            let ids = page
                .get("data")
                .and_then(Value::as_array)
                .context("missing inventory")?;
            for id in ids {
                let id = id.as_str().context("invalid loaded thread id")?;
                if !seen.insert(id.to_owned()) || seen.len() > 4096 {
                    return Ok(false);
                }
                let result = rpc
                    .request("thread/read", json!({"threadId":id,"includeTurns":false}))
                    .map_err(rpc_failure_to_anyhow)?;
                if thread_status(&result) != Some("idle") {
                    return Ok(false);
                }
            }
            cursor = page.get("nextCursor").cloned().context("missing cursor")?;
            if cursor.is_null() {
                return Ok(true);
            }
        }
        Ok(false)
    };
    check().unwrap_or(false)
}

#[derive(Debug, Clone)]
pub struct TakeoverConfig {
    pub isolated_socket: PathBuf,
    pub shared_socket: PathBuf,
    pub timeout: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TakeoverOutcome {
    AlreadyOwned,
    TakenOver,
    RefusedActive,
    RefusedUnprovenStatus,
    NoSharedOwner,
    Failed,
    FailedRolledBack,
    FailedRollback,
}

pub struct TakeoverManager {
    config: TakeoverConfig,
}

impl TakeoverManager {
    pub fn new(config: TakeoverConfig) -> Self {
        Self { config }
    }

    pub fn prepare_resume(&self, attempt: &ResumeAttempt) -> TakeoverOutcome {
        self.prepare_resume_inner(attempt)
            .unwrap_or(TakeoverOutcome::Failed)
    }

    fn prepare_resume_inner(&self, attempt: &ResumeAttempt) -> Result<TakeoverOutcome> {
        let mut isolated = RpcClient::connect(
            &self.config.isolated_socket,
            self.config.timeout,
            "codex-managed-takeover-isolated",
        )?;
        let isolated_read = isolated
            .request(
                "thread/read",
                json!({"threadId": attempt.thread_id, "includeTurns": false}),
            )
            .map_err(rpc_failure_to_anyhow)?;
        match thread_status(&isolated_read) {
            Some("idle" | "active") => return Ok(TakeoverOutcome::AlreadyOwned),
            Some("notLoaded") => {}
            _ => return Ok(TakeoverOutcome::RefusedUnprovenStatus),
        }

        let mut shared = RpcClient::connect(
            &self.config.shared_socket,
            self.config.timeout,
            "codex-managed-takeover-shared",
        )?;
        let read = match shared.request(
            "thread/read",
            json!({"threadId": attempt.thread_id, "includeTurns": false}),
        ) {
            Ok(read) => read,
            Err(_) => return Ok(TakeoverOutcome::RefusedUnprovenStatus),
        };
        match thread_status(&read) {
            Some("idle") => {}
            Some("active") => return Ok(TakeoverOutcome::RefusedActive),
            Some("notLoaded") => return Ok(TakeoverOutcome::NoSharedOwner),
            _ => return Ok(TakeoverOutcome::RefusedUnprovenStatus),
        }

        if shared
            .request("thread/archive", json!({"threadId": attempt.thread_id}))
            .is_err()
        {
            return Ok(TakeoverOutcome::Failed);
        }

        let deadline = Instant::now() + self.config.timeout;
        loop {
            match isolated.request("thread/unarchive", json!({"threadId": attempt.thread_id})) {
                Ok(_) => return Ok(TakeoverOutcome::TakenOver),
                Err(error) if is_active_writer(&error) && Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(50));
                }
                Err(_) => break,
            }
        }

        let rollback = shared.request("thread/unarchive", json!({"threadId": attempt.thread_id}));
        Ok(if rollback.is_ok() {
            TakeoverOutcome::FailedRolledBack
        } else {
            TakeoverOutcome::FailedRollback
        })
    }
}

fn is_active_writer(error: &RpcFailure) -> bool {
    matches!(
        error,
        RpcFailure::Remote { code: -32600, message }
            if message.contains("already has an active writer")
    )
}

fn thread_status(response: &Value) -> Option<&str> {
    response
        .pointer("/thread/status/type")
        .and_then(Value::as_str)
}

#[derive(Debug)]
enum RpcFailure {
    Remote { code: i64, message: String },
    Transport(anyhow::Error),
}

struct RpcClient {
    stream: UnixStream,
    next_id: u64,
    deadline: Instant,
}

impl RpcClient {
    fn read_exact(&mut self, mut bytes: &mut [u8]) -> Result<()> {
        while !bytes.is_empty() {
            let left = self
                .deadline
                .checked_duration_since(Instant::now())
                .context("control deadline expired")?;
            self.stream.set_read_timeout(Some(left))?;
            match self.stream.read(bytes) {
                Ok(0) => bail!("control socket closed"),
                Ok(n) => {
                    bytes = &mut bytes[n..];
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }
    fn connect(path: &Path, timeout: Duration, name: &str) -> Result<Self> {
        let stream = UnixStream::connect(path)
            .with_context(|| format!("failed to connect to {}", path.display()))?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;
        let mut client = Self {
            stream,
            next_id: 1,
            deadline: Instant::now() + timeout,
        };
        client.upgrade()?;
        client
            .request(
                "initialize",
                json!({
                    "clientInfo": {"name": name, "title": "Codex Managed Takeover", "version": env!("CARGO_PKG_VERSION")}
                }),
            )
            .map_err(rpc_failure_to_anyhow)?;
        client.send_json(&json!({"method":"initialized","params":{}}))?;
        Ok(client)
    }

    fn upgrade(&mut self) -> Result<()> {
        write!(
            self.stream,
            "GET / HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {WEBSOCKET_KEY}\r\nSec-WebSocket-Version: 13\r\n\r\n"
        )?;
        self.stream.flush()?;
        let mut response = Vec::new();
        let mut byte = [0u8; 1];
        while !response.ends_with(b"\r\n\r\n") {
            if response.len() >= 16 * 1024 {
                bail!("websocket upgrade response exceeded limit");
            }
            self.read_exact(&mut byte)?;
            response.push(byte[0]);
        }
        let response = String::from_utf8(response).context("upgrade response was not UTF-8")?;
        let lower = response.to_ascii_lowercase();
        if !lower.starts_with("http/1.1 101 ")
            || !lower.contains(&format!(
                "sec-websocket-accept: {}",
                WEBSOCKET_ACCEPT.to_ascii_lowercase()
            ))
        {
            bail!("control socket rejected websocket upgrade");
        }
        Ok(())
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, RpcFailure> {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.send_json(&json!({"id":id,"method":method,"params":params}))
            .map_err(RpcFailure::Transport)?;
        loop {
            let message = self.read_json().map_err(RpcFailure::Transport)?;
            if message.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(error) = message.get("error") {
                return Err(RpcFailure::Remote {
                    code: error.get("code").and_then(Value::as_i64).unwrap_or(0),
                    message: error
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown JSON-RPC error")
                        .to_owned(),
                });
            }
            return message
                .get("result")
                .cloned()
                .ok_or_else(|| RpcFailure::Transport(anyhow!("response omitted result")));
        }
    }

    fn send_json(&mut self, value: &Value) -> Result<()> {
        let payload = serde_json::to_vec(value)?;
        if payload.len() > MAX_MESSAGE_BYTES {
            bail!("control request exceeded message limit");
        }
        let mut frame = vec![0x81];
        if payload.len() <= 125 {
            frame.push(0x80 | payload.len() as u8);
        } else if payload.len() <= u16::MAX as usize {
            frame.push(0x80 | 126);
            frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        } else {
            frame.push(0x80 | 127);
            frame.extend_from_slice(&(payload.len() as u64).to_be_bytes());
        }
        let mut mask = [0u8; 4];
        File::open("/dev/urandom")?.read_exact(&mut mask)?;
        frame.extend_from_slice(&mask);
        frame.extend(
            payload
                .iter()
                .enumerate()
                .map(|(index, byte)| byte ^ mask[index % 4]),
        );
        self.stream.write_all(&frame)?;
        self.stream.flush()?;
        Ok(())
    }

    fn read_json(&mut self) -> Result<Value> {
        let mut fragments = Vec::new();
        loop {
            let (final_frame, opcode, payload) = self.read_frame()?;
            match opcode {
                0x1 => fragments = payload,
                0x0 if !fragments.is_empty() => fragments.extend_from_slice(&payload),
                0x8 => bail!("control socket closed websocket"),
                0x9 => {
                    self.send_control_frame(0xA, &payload)?;
                    continue;
                }
                0xA => continue,
                _ => continue,
            }
            if fragments.len() > MAX_MESSAGE_BYTES {
                bail!("fragmented control response exceeded limit");
            }
            if final_frame {
                return serde_json::from_slice(&fragments)
                    .context("control socket returned invalid JSON");
            }
        }
    }

    fn read_frame(&mut self) -> Result<(bool, u8, Vec<u8>)> {
        let remaining = self
            .deadline
            .checked_duration_since(Instant::now())
            .context("control deadline expired")?;
        self.stream.set_read_timeout(Some(remaining))?;
        self.stream.set_write_timeout(Some(remaining))?;
        let mut head = [0u8; 2];
        self.read_exact(&mut head)?;
        if head[0] & 0x70 != 0 {
            bail!("control socket sent websocket frame with reserved bits");
        }
        let mut length = (head[1] & 0x7f) as usize;
        if length == 126 {
            let mut bytes = [0u8; 2];
            self.read_exact(&mut bytes)?;
            length = u16::from_be_bytes(bytes) as usize;
        } else if length == 127 {
            let mut bytes = [0u8; 8];
            self.read_exact(&mut bytes)?;
            let value = u64::from_be_bytes(bytes);
            if value > MAX_MESSAGE_BYTES as u64 {
                bail!("control response exceeded message limit");
            }
            length = value as usize;
        }
        if length > MAX_MESSAGE_BYTES {
            bail!("control response exceeded message limit");
        }
        let masked = head[1] & 0x80 != 0;
        let mut mask = [0u8; 4];
        if masked {
            self.read_exact(&mut mask)?;
        }
        let mut payload = vec![0u8; length];
        self.read_exact(&mut payload)?;
        if masked {
            for (index, byte) in payload.iter_mut().enumerate() {
                *byte ^= mask[index % 4];
            }
        }
        Ok((head[0] & 0x80 != 0, head[0] & 0x0f, payload))
    }

    fn send_control_frame(&mut self, opcode: u8, payload: &[u8]) -> Result<()> {
        if payload.len() > 125 {
            bail!("oversized websocket control frame");
        }
        let mask = [0x13, 0x57, 0x9b, 0xdf];
        let mut frame = vec![0x80 | opcode, 0x80 | payload.len() as u8];
        frame.extend_from_slice(&mask);
        frame.extend(
            payload
                .iter()
                .enumerate()
                .map(|(index, byte)| byte ^ mask[index % 4]),
        );
        self.stream.write_all(&frame)?;
        self.stream.flush()?;
        Ok(())
    }
}

impl Drop for RpcClient {
    fn drop(&mut self) {
        let _ = self.send_control_frame(0x8, &[]);
        let _ = self.stream.shutdown(std::net::Shutdown::Write);
    }
}

fn rpc_failure_to_anyhow(error: RpcFailure) -> anyhow::Error {
    match error {
        RpcFailure::Remote { code, message } => anyhow!("JSON-RPC {code}: {message}"),
        RpcFailure::Transport(error) => error,
    }
}
