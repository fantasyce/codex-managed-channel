use codex_managed_channel::protocol::ResumeAttempt;
use codex_managed_channel::takeover::{TakeoverConfig, TakeoverManager, TakeoverOutcome};
use serde_json::{Value, json};
use std::fs;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::thread::{self, JoinHandle};
use std::time::Duration;

#[derive(Clone)]
struct Step {
    method: &'static str,
    reply: Value,
}

struct ScriptServer {
    handle: JoinHandle<()>,
}

impl ScriptServer {
    fn start(path: PathBuf, steps: Vec<Step>) -> Self {
        let handle = thread::spawn(move || {
            let _ = fs::remove_file(&path);
            let listener = UnixListener::bind(&path).unwrap();
            let (mut stream, _) = listener.accept().unwrap();
            websocket_upgrade(&mut stream);
            let mut next = 0;
            while next < steps.len() {
                let request = read_json_frame(&mut stream);
                if request.get("method").and_then(Value::as_str) == Some("initialized") {
                    continue;
                }
                let step = &steps[next];
                assert_eq!(request["method"], step.method);
                let id = request["id"].clone();
                let mut reply = step.reply.clone();
                reply["id"] = id;
                write_json_frame(&mut stream, &reply);
                next += 1;
            }
            let (opcode, payload) = read_frame(&mut stream);
            assert_eq!(opcode, 0x8, "takeover RPC client must close cleanly");
            assert!(payload.is_empty());
        });
        Self { handle }
    }

    fn finish(self) {
        self.handle.join().unwrap();
    }
}

fn initialized() -> Step {
    Step {
        method: "initialize",
        reply: json!({"result":{"userAgent":"test/1.0"}}),
    }
}

fn ok(method: &'static str, result: Value) -> Step {
    Step {
        method,
        reply: json!({"result":result}),
    }
}

fn error(method: &'static str, code: i64, message: &str) -> Step {
    Step {
        method,
        reply: json!({"error":{"code":code,"message":message}}),
    }
}

fn attempt() -> ResumeAttempt {
    ResumeAttempt {
        thread_id: "thread-a".into(),
        params: json!({"threadId":"thread-a","model":"gpt-test"}),
    }
}

fn manager(temp: &tempfile::TempDir) -> TakeoverManager {
    TakeoverManager::new(TakeoverConfig {
        isolated_socket: temp.path().join("isolated.sock"),
        shared_socket: temp.path().join("shared.sock"),
        timeout: Duration::from_secs(2),
    })
}

#[test]
fn idle_shared_thread_retries_transient_writer_release_without_an_internal_resume() {
    let temp = tempfile::tempdir().unwrap();
    let isolated = ScriptServer::start(
        temp.path().join("isolated.sock"),
        vec![
            initialized(),
            ok(
                "thread/read",
                json!({"thread":{"id":"thread-a","status":{"type":"notLoaded"}}}),
            ),
            error(
                "thread/unarchive",
                -32600,
                "thread thread-a already has an active writer",
            ),
            ok(
                "thread/unarchive",
                json!({"thread":{"id":"thread-a","history":"x".repeat(2 * 1024 * 1024)}}),
            ),
        ],
    );
    let shared = ScriptServer::start(
        temp.path().join("shared.sock"),
        vec![
            initialized(),
            ok(
                "thread/read",
                json!({"thread":{"id":"thread-a","status":{"type":"idle"}}}),
            ),
            ok("thread/archive", json!({})),
        ],
    );
    wait_for_socket(&temp.path().join("isolated.sock"));
    wait_for_socket(&temp.path().join("shared.sock"));

    assert_eq!(
        manager(&temp).prepare_resume(&attempt()),
        TakeoverOutcome::TakenOver
    );
    isolated.finish();
    shared.finish();
}

#[test]
fn already_loaded_isolated_thread_never_contacts_shared_server() {
    let temp = tempfile::tempdir().unwrap();
    let isolated = ScriptServer::start(
        temp.path().join("isolated.sock"),
        vec![
            initialized(),
            ok(
                "thread/read",
                json!({"thread":{"id":"thread-a","status":{"type":"idle"}}}),
            ),
        ],
    );
    wait_for_socket(&temp.path().join("isolated.sock"));

    assert_eq!(
        manager(&temp).prepare_resume(&attempt()),
        TakeoverOutcome::AlreadyOwned
    );
    isolated.finish();
    assert!(!temp.path().join("shared.sock").exists());
}

#[test]
fn active_shared_thread_is_never_archived() {
    let temp = tempfile::tempdir().unwrap();
    let isolated = ScriptServer::start(
        temp.path().join("isolated.sock"),
        vec![
            initialized(),
            ok(
                "thread/read",
                json!({"thread":{"id":"thread-a","status":{"type":"notLoaded"}}}),
            ),
        ],
    );
    let shared = ScriptServer::start(
        temp.path().join("shared.sock"),
        vec![
            initialized(),
            ok(
                "thread/read",
                json!({"thread":{"id":"thread-a","status":{"type":"active","activeFlags":[]}}}),
            ),
        ],
    );
    wait_for_socket(&temp.path().join("isolated.sock"));
    wait_for_socket(&temp.path().join("shared.sock"));

    assert_eq!(
        manager(&temp).prepare_resume(&attempt()),
        TakeoverOutcome::RefusedActive
    );
    isolated.finish();
    shared.finish();
}

#[test]
fn a_thread_not_loaded_by_either_server_needs_no_takeover() {
    let temp = tempfile::tempdir().unwrap();
    let isolated = ScriptServer::start(
        temp.path().join("isolated.sock"),
        vec![
            initialized(),
            ok(
                "thread/read",
                json!({"thread":{"id":"thread-a","status":{"type":"notLoaded"}}}),
            ),
        ],
    );
    let shared = ScriptServer::start(
        temp.path().join("shared.sock"),
        vec![
            initialized(),
            ok(
                "thread/read",
                json!({"thread":{"id":"thread-a","status":{"type":"notLoaded"}}}),
            ),
        ],
    );
    wait_for_socket(&temp.path().join("isolated.sock"));
    wait_for_socket(&temp.path().join("shared.sock"));

    assert_eq!(
        manager(&temp).prepare_resume(&attempt()),
        TakeoverOutcome::NoSharedOwner
    );
    isolated.finish();
    shared.finish();
}

#[test]
fn failed_unarchive_after_shared_archive_restores_shared_state() {
    let temp = tempfile::tempdir().unwrap();
    let isolated = ScriptServer::start(
        temp.path().join("isolated.sock"),
        vec![
            initialized(),
            ok(
                "thread/read",
                json!({"thread":{"id":"thread-a","status":{"type":"notLoaded"}}}),
            ),
            error("thread/unarchive", -32603, "load failed"),
        ],
    );
    let shared = ScriptServer::start(
        temp.path().join("shared.sock"),
        vec![
            initialized(),
            ok(
                "thread/read",
                json!({"thread":{"id":"thread-a","status":{"type":"idle"}}}),
            ),
            ok("thread/archive", json!({})),
            ok("thread/unarchive", json!({"thread":{"id":"thread-a"}})),
        ],
    );
    wait_for_socket(&temp.path().join("isolated.sock"));
    wait_for_socket(&temp.path().join("shared.sock"));

    assert_eq!(
        manager(&temp).prepare_resume(&attempt()),
        TakeoverOutcome::FailedRolledBack
    );
    isolated.finish();
    shared.finish();
}

fn wait_for_socket(path: &Path) {
    for _ in 0..100 {
        if path.exists() {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("socket did not appear: {}", path.display());
}

fn websocket_upgrade(stream: &mut UnixStream) {
    let mut request = Vec::new();
    let mut byte = [0u8; 1];
    while !request.ends_with(b"\r\n\r\n") {
        stream.read_exact(&mut byte).unwrap();
        request.push(byte[0]);
    }
    assert!(request.starts_with(b"GET "));
    stream
        .write_all(
            b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n\r\n",
        )
        .unwrap();
}

fn read_json_frame(stream: &mut UnixStream) -> Value {
    let (opcode, payload) = read_frame(stream);
    assert_eq!(opcode, 0x1);
    serde_json::from_slice(&payload).unwrap()
}

fn read_frame(stream: &mut UnixStream) -> (u8, Vec<u8>) {
    let mut head = [0u8; 2];
    stream.read_exact(&mut head).unwrap();
    let opcode = head[0] & 0x0f;
    let masked = head[1] & 0x80 != 0;
    assert!(masked);
    let mut len = (head[1] & 0x7f) as usize;
    if len == 126 {
        let mut bytes = [0u8; 2];
        stream.read_exact(&mut bytes).unwrap();
        len = u16::from_be_bytes(bytes) as usize;
    } else if len == 127 {
        let mut bytes = [0u8; 8];
        stream.read_exact(&mut bytes).unwrap();
        len = u64::from_be_bytes(bytes) as usize;
    }
    let mut mask = [0u8; 4];
    stream.read_exact(&mut mask).unwrap();
    let mut payload = vec![0u8; len];
    stream.read_exact(&mut payload).unwrap();
    for (index, byte) in payload.iter_mut().enumerate() {
        *byte ^= mask[index % 4];
    }
    (opcode, payload)
}

fn write_json_frame(stream: &mut UnixStream, value: &Value) {
    let payload = serde_json::to_vec(value).unwrap();
    let mut frame = vec![0x81];
    if payload.len() <= 125 {
        frame.push(payload.len() as u8);
    } else if payload.len() <= u16::MAX as usize {
        frame.push(126);
        frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    } else {
        frame.push(127);
        frame.extend_from_slice(&(payload.len() as u64).to_be_bytes());
    }
    frame.extend_from_slice(&payload);
    stream.write_all(&frame).unwrap();
}
