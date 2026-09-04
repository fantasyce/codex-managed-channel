use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{
    Mutex,
    atomic::{AtomicU64, AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
pub enum Direction {
    ClientToServer,
    ServerToClient,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResumeAttempt {
    pub thread_id: String,
    pub params: Value,
}

#[derive(Debug, Clone)]
enum PendingRequest {
    ThreadOpen(Option<String>),
    ThreadRemove { thread_id: String, archive: bool },
    TurnStart,
}

#[derive(Debug, Default)]
struct ProtocolState {
    pending: HashMap<String, PendingRequest>,
    live_threads: HashSet<String>,
    archive_empty_epoch: u64,
}

#[derive(Debug, Clone)]
pub struct ProtocolSnapshot {
    pub active_turns: usize,
    pub reliable: bool,
    pub client_activity_generation: u64,
    pub client_idle: Duration,
    pub live_threads: usize,
    pub archive_empty_epoch: u64,
}

pub struct ProtocolObserver {
    active_turns: AtomicUsize,
    parse_failures: AtomicUsize,
    client_activity_generation: AtomicU64,
    client_wire: Mutex<WireBuffer>,
    server_wire: Mutex<WireBuffer>,
    last_activity: Mutex<Instant>,
    last_client_activity: Mutex<Instant>,
    resume_attempts: Mutex<VecDeque<ResumeAttempt>>,
    state: Mutex<ProtocolState>,
}

impl Default for ProtocolObserver {
    fn default() -> Self {
        let now = Instant::now();
        Self {
            active_turns: AtomicUsize::new(0),
            parse_failures: AtomicUsize::new(0),
            client_activity_generation: AtomicU64::new(0),
            client_wire: Mutex::new(WireBuffer::default()),
            server_wire: Mutex::new(WireBuffer::default()),
            last_activity: Mutex::new(now),
            last_client_activity: Mutex::new(now),
            resume_attempts: Mutex::new(VecDeque::new()),
            state: Mutex::new(ProtocolState::default()),
        }
    }
}

impl ProtocolObserver {
    pub fn observe<'a>(&self, direction: Direction, bytes: &'a [u8]) -> &'a [u8] {
        let now = Instant::now();
        *self.last_activity.lock().expect("activity mutex poisoned") = now;
        let wire = match direction {
            Direction::ClientToServer => &self.client_wire,
            Direction::ServerToClient => &self.server_wire,
        };
        let (messages, failures) = wire
            .lock()
            .expect("protocol buffer mutex poisoned")
            .feed(bytes);
        if failures > 0 {
            self.parse_failures.fetch_add(failures, Ordering::Relaxed);
        }
        for message in messages {
            self.observe_line(direction, &message);
        }
        bytes
    }

    fn observe_line(&self, direction: Direction, line: &[u8]) {
        if line.iter().all(u8::is_ascii_whitespace) {
            return;
        }
        let Ok(value) = serde_json::from_slice::<Value>(line) else {
            self.parse_failures.fetch_add(1, Ordering::Relaxed);
            return;
        };
        match direction {
            Direction::ClientToServer => self.observe_client_message(&value),
            Direction::ServerToClient => self.observe_server_message(&value),
        }
    }

    fn observe_client_message(&self, value: &Value) {
        let Some(method) = value.get("method").and_then(Value::as_str) else {
            return;
        };
        if is_user_activity_method(method) {
            *self
                .last_client_activity
                .lock()
                .expect("client activity mutex poisoned") = Instant::now();
            self.client_activity_generation
                .fetch_add(1, Ordering::Relaxed);
        }
        let request_id = value.get("id").and_then(request_key);
        let thread_id = || {
            value
                .pointer("/params/threadId")
                .and_then(Value::as_str)
                .map(str::to_owned)
        };
        if method == "thread/resume"
            && let (Some(thread_id), Some(params)) = (thread_id(), value.get("params").cloned())
        {
            self.resume_attempts
                .lock()
                .expect("resume attempt mutex poisoned")
                .push_back(ResumeAttempt { thread_id, params });
        }
        let pending = match method {
            "thread/start" => Some(PendingRequest::ThreadOpen(None)),
            "thread/resume" | "thread/unarchive" => Some(PendingRequest::ThreadOpen(thread_id())),
            "thread/archive" => thread_id().map(|thread_id| PendingRequest::ThreadRemove {
                thread_id,
                archive: true,
            }),
            "thread/delete" => thread_id().map(|thread_id| PendingRequest::ThreadRemove {
                thread_id,
                archive: false,
            }),
            "turn/start" => {
                self.active_turns.fetch_add(1, Ordering::Relaxed);
                Some(PendingRequest::TurnStart)
            }
            _ => None,
        };
        if let (Some(id), Some(pending)) = (request_id, pending) {
            self.state
                .lock()
                .expect("protocol state mutex poisoned")
                .pending
                .insert(id, pending);
        }
    }

    fn observe_server_message(&self, value: &Value) {
        if let Some(id) = value.get("id").and_then(request_key) {
            let pending = self
                .state
                .lock()
                .expect("protocol state mutex poisoned")
                .pending
                .remove(&id);
            if let Some(pending) = pending {
                let success = value.get("error").is_none_or(Value::is_null);
                self.finish_request(pending, success, value);
            }
        }
        let Some(method) = value.get("method").and_then(Value::as_str) else {
            return;
        };
        if matches!(method, "turn/completed" | "turn/failed" | "turn/cancelled") {
            decrement(&self.active_turns);
        }
    }

    fn finish_request(&self, pending: PendingRequest, success: bool, response: &Value) {
        match pending {
            PendingRequest::TurnStart if !success => decrement(&self.active_turns),
            PendingRequest::ThreadOpen(fallback) if success => {
                let thread_id = response
                    .pointer("/result/thread/id")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .or(fallback);
                if let Some(thread_id) = thread_id {
                    self.state
                        .lock()
                        .expect("protocol state mutex poisoned")
                        .live_threads
                        .insert(thread_id);
                }
            }
            PendingRequest::ThreadRemove { thread_id, archive } if success => {
                let mut state = self.state.lock().expect("protocol state mutex poisoned");
                let removed = state.live_threads.remove(&thread_id);
                if archive && removed && state.live_threads.is_empty() {
                    state.archive_empty_epoch = state.archive_empty_epoch.saturating_add(1);
                }
            }
            _ => {}
        }
    }

    pub fn active_turns(&self) -> usize {
        self.active_turns.load(Ordering::Relaxed)
    }
    pub fn take_resume_attempts(&self) -> Vec<ResumeAttempt> {
        self.resume_attempts
            .lock()
            .expect("resume attempt mutex poisoned")
            .drain(..)
            .collect()
    }
    pub fn parse_failures(&self) -> usize {
        self.parse_failures.load(Ordering::Relaxed)
    }
    pub fn idle_elapsed(&self) -> Duration {
        self.last_activity
            .lock()
            .expect("activity mutex poisoned")
            .elapsed()
    }
    pub fn snapshot(&self) -> ProtocolSnapshot {
        let state = self.state.lock().expect("protocol state mutex poisoned");
        ProtocolSnapshot {
            active_turns: self.active_turns(),
            reliable: self.parse_failures() == 0,
            client_activity_generation: self.client_activity_generation.load(Ordering::Relaxed),
            client_idle: self
                .last_client_activity
                .lock()
                .expect("client activity mutex poisoned")
                .elapsed(),
            live_threads: state.live_threads.len(),
            archive_empty_epoch: state.archive_empty_epoch,
        }
    }
}

fn is_user_activity_method(method: &str) -> bool {
    matches!(
        method,
        "thread/start"
            | "thread/resume"
            | "thread/fork"
            | "thread/archive"
            | "thread/unarchive"
            | "thread/delete"
            | "thread/compact"
            | "thread/rollback"
            | "thread/name/set"
            | "turn/start"
            | "turn/interrupt"
            | "turn/steer"
            | "review/start"
            | "command/exec"
    )
}

// Desktop can return the complete persisted history in a single WebSocket
// frame. Keep observation bounded, but above the largest supported history
// response exercised by the compatibility suite.
const MAX_WIRE_BUFFER: usize = 64 * 1024 * 1024;

#[derive(Default)]
struct WireBuffer {
    mode: WireMode,
    pending: Vec<u8>,
    fragmented_text: Option<Vec<u8>>,
}

#[derive(Default, PartialEq, Eq)]
enum WireMode {
    #[default]
    Unknown,
    JsonLines,
    WebSocketHandshake,
    WebSocketFrames,
}

impl WireBuffer {
    fn feed(&mut self, bytes: &[u8]) -> (Vec<Vec<u8>>, usize) {
        self.pending.extend_from_slice(bytes);
        if self.mode == WireMode::Unknown {
            let trimmed = self
                .pending
                .iter()
                .position(|byte| !byte.is_ascii_whitespace())
                .unwrap_or(self.pending.len());
            let start = &self.pending[trimmed..];
            if start.starts_with(b"GET ") || start.starts_with(b"HTTP/") {
                self.mode = WireMode::WebSocketHandshake;
            } else if start.starts_with(b"{")
                || start.starts_with(b"[")
                || (start.len() >= 5 && !b"GET ".starts_with(start) && !b"HTTP/".starts_with(start))
            {
                self.mode = WireMode::JsonLines;
            }
        }

        let mut messages = Vec::new();
        let mut failures = 0;
        if self.mode == WireMode::WebSocketHandshake
            && let Some(end) = find_subsequence(&self.pending, b"\r\n\r\n")
        {
            self.pending.drain(..end + 4);
            self.mode = WireMode::WebSocketFrames;
        }
        match self.mode {
            WireMode::JsonLines => {
                while let Some(newline) = self.pending.iter().position(|byte| *byte == b'\n') {
                    let mut line: Vec<u8> = self.pending.drain(..=newline).collect();
                    line.pop();
                    messages.push(line);
                }
            }
            WireMode::WebSocketFrames => {
                failures += self.drain_websocket_frames(&mut messages);
            }
            WireMode::Unknown | WireMode::WebSocketHandshake => {}
        }
        if self.pending.len() > MAX_WIRE_BUFFER {
            self.pending.clear();
            self.fragmented_text = None;
            failures += 1;
        }
        (messages, failures)
    }

    fn drain_websocket_frames(&mut self, messages: &mut Vec<Vec<u8>>) -> usize {
        let mut failures = 0;
        loop {
            if self.pending.len() < 2 {
                break;
            }
            let first = self.pending[0];
            let second = self.pending[1];
            if first & 0x70 != 0 {
                self.pending.clear();
                return failures + 1;
            }
            let final_frame = first & 0x80 != 0;
            let opcode = first & 0x0f;
            let masked = second & 0x80 != 0;
            let mut offset = 2;
            let payload_len = match second & 0x7f {
                value @ 0..=125 => value as usize,
                126 => {
                    if self.pending.len() < 4 {
                        break;
                    }
                    offset = 4;
                    u16::from_be_bytes([self.pending[2], self.pending[3]]) as usize
                }
                127 => {
                    if self.pending.len() < 10 {
                        break;
                    }
                    offset = 10;
                    let value = u64::from_be_bytes(self.pending[2..10].try_into().unwrap());
                    let Ok(value) = usize::try_from(value) else {
                        self.pending.clear();
                        return failures + 1;
                    };
                    value
                }
                _ => unreachable!(),
            };
            if payload_len > MAX_WIRE_BUFFER {
                self.pending.clear();
                return failures + 1;
            }
            let mask = if masked {
                if self.pending.len() < offset + 4 {
                    break;
                }
                let mask: [u8; 4] = self.pending[offset..offset + 4].try_into().unwrap();
                offset += 4;
                Some(mask)
            } else {
                None
            };
            let Some(frame_len) = offset.checked_add(payload_len) else {
                self.pending.clear();
                return failures + 1;
            };
            if self.pending.len() < frame_len {
                break;
            }
            let frame: Vec<u8> = self.pending.drain(..frame_len).collect();
            let mut payload = frame[offset..].to_vec();
            if let Some(mask) = mask {
                for (index, byte) in payload.iter_mut().enumerate() {
                    *byte ^= mask[index % 4];
                }
            }
            match opcode {
                0x1 if final_frame => messages.push(payload),
                0x1 => self.fragmented_text = Some(payload),
                0x0 => {
                    let Some(fragmented) = self.fragmented_text.as_mut() else {
                        failures += 1;
                        continue;
                    };
                    fragmented.extend_from_slice(&payload);
                    if fragmented.len() > MAX_WIRE_BUFFER {
                        self.fragmented_text = None;
                        failures += 1;
                    } else if final_frame {
                        messages.push(self.fragmented_text.take().unwrap());
                    }
                }
                0x2 | 0x8 | 0x9 | 0xa => {}
                _ => failures += 1,
            }
        }
        failures
    }
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn request_key(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(format!("s:{value}")),
        Value::Number(value) => Some(format!("n:{value}")),
        _ => None,
    }
}

fn decrement(counter: &AtomicUsize) {
    let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
        Some(n.saturating_sub(1))
    });
}
