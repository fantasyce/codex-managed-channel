//! Bounded, single-writer sessions selected only by a fixed server-side identity.
use crate::fd_guard::FdAction;
use crate::protocol::{Direction, ProtocolObserver};
use crate::reclaim::{ReclaimAction, ReclaimController, ReclaimSnapshot};
use crate::supervisor::{self, SupervisorConfig, WorkerProcesses};
use crate::takeover::{TakeoverConfig, TakeoverManager, runtime_is_idle};
use anyhow::{Context, Result, bail};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

fn unsubscribe_idle_threads<W: Write>(
    writer: &mut W,
    observer: &ProtocolObserver,
    next_request_id: &mut u64,
) -> io::Result<usize> {
    let thread_ids = observer.idle_thread_ids();
    for thread_id in &thread_ids {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": format!("codex-managed-unsubscribe-{}", *next_request_id),
            "method": "thread/unsubscribe",
            "params": { "threadId": thread_id },
        });
        *next_request_id = next_request_id.saturating_add(1);
        let payload = serde_json::to_vec(&request)?;
        let frame = masked_websocket_text_frame(&payload)?;
        observer.observe(Direction::ClientToServer, &frame);
        writer.write_all(&frame)?;
    }
    writer.flush()?;
    Ok(thread_ids.len())
}

fn masked_websocket_text_frame(payload: &[u8]) -> io::Result<Vec<u8>> {
    let mut frame = Vec::with_capacity(payload.len() + 8);
    frame.push(0x81);
    match payload.len() {
        length @ 0..=125 => frame.push(0x80 | length as u8),
        length @ 126..=65535 => {
            frame.push(0x80 | 126);
            frame.extend_from_slice(&(length as u16).to_be_bytes());
        }
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "managed unsubscribe request is too large",
            ));
        }
    }
    let mask = [0x43, 0x4d, 0x43, 0x31];
    frame.extend_from_slice(&mask);
    frame.extend(
        payload
            .iter()
            .enumerate()
            .map(|(index, byte)| byte ^ mask[index % 4]),
    );
    Ok(frame)
}

fn wait_for_thread_unload(observer: &ProtocolObserver, timeout: Duration) -> bool {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if observer.idle_thread_ids().is_empty() {
            return true;
        }
        thread::sleep(Duration::from_millis(25));
    }
    observer.idle_thread_ids().is_empty()
}

pub fn validate_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 32
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        bail!("client identity must contain 1..32 ASCII letters, digits, '-' or '_'");
    }
    Ok(())
}

fn private_dir(path: &Path) -> Result<()> {
    match fs::create_dir(path) {
        Ok(()) => fs::set_permissions(path, fs::Permissions::from_mode(0o700))?,
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.into()),
    }
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_dir() || meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o077 != 0 {
        bail!("session directory must be private, owned, and not a symlink");
    }
    Ok(())
}

fn directory(config: &SupervisorConfig, id: &str) -> Result<PathBuf> {
    validate_id(id)?;
    private_dir(&config.managed_root)?;
    let sessions = config.managed_root.join("sessions");
    private_dir(&sessions)?;
    let path = sessions.join(id);
    private_dir(&path)?;
    Ok(path)
}

fn lock(path: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() || meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o077 != 0 {
        bail!("unsafe session lock file");
    }
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        bail!("session is starting, active, or being reaped; retry later");
    }
    Ok(file)
}

fn duration(name: &str, default: u64) -> Result<Duration> {
    let seconds = match std::env::var(name) {
        Ok(value) => value
            .parse::<u64>()
            .with_context(|| format!("invalid {name}"))?,
        Err(_) => default,
    };
    if seconds == 0 || seconds > 7 * 86400 {
        bail!("{name} must be 1..604800 seconds");
    }
    Ok(Duration::from_secs(seconds))
}

pub fn attach(config: SupervisorConfig, id: &str) -> Result<i32> {
    if config.use_existing_daemon {
        bail!("bounded sessions cannot use a shared daemon");
    }
    let path = directory(&config, id)?;
    let startup = lock(&path.join("startup.lock"))?;
    let socket = path.join("control.sock");
    let mut stream = match UnixStream::connect(&socket) {
        Ok(stream) => stream,
        Err(_) => {
            // The owner's lock remains held by its reaper after a crash. Never
            // replace a worker until that independent cleanup has completed.
            drop(lock(&path.join("owner.lock"))?);
            let mut child = Command::new(std::env::current_exe()?);
            child
                .args(["--session-owner", id])
                .env_remove("SSH_ORIGINAL_COMMAND")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            // setsid detaches from sshd's terminal/session as well as its pipes.
            unsafe {
                child.pre_exec(|| {
                    if libc::setsid() < 0 {
                        return Err(io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
            let mut owner = child.spawn()?;
            let start = Instant::now();
            loop {
                if let Ok(stream) = UnixStream::connect(&socket) {
                    break stream;
                }
                if owner.try_wait()?.is_some() {
                    bail!("session owner failed to start; inspect private supervisor log");
                }
                if start.elapsed() > config.socket_start_timeout + Duration::from_secs(5) {
                    bail!("session owner readiness timed out");
                }
                thread::sleep(Duration::from_millis(25));
            }
        }
    };
    stream.set_read_timeout(Some(config.socket_start_timeout + Duration::from_secs(5)))?;
    let mut ack = [0];
    stream.read_exact(&mut ack)?;
    if ack != [1] {
        bail!("this client identity already has an active connection or is draining");
    }
    stream.set_read_timeout(None)?;
    drop(startup);
    let mut input = stream.try_clone()?;
    thread::spawn(move || {
        let _ = io::copy(&mut io::stdin(), &mut input);
        let _ = input.shutdown(Shutdown::Write);
    });
    let mut buffer = [0u8; 16384];
    let mut output = io::stdout();
    loop {
        let n = stream.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        output.write_all(&buffer[..n])?;
        output.flush()?;
    }
    Ok(0)
}

/// Local maintenance only: stop precisely this identity and await its fence.
pub fn stop(config: SupervisorConfig, id: &str) -> Result<i32> {
    let path = directory(&config, id)?;
    let _startup = lock(&path.join("startup.lock"))?;
    let _request = UnixStream::connect(path.join("stop.sock"));
    let deadline =
        Instant::now() + config.term_grace + config.socket_start_timeout + Duration::from_secs(5);
    loop {
        if lock(&path.join("owner.lock")).is_ok() {
            return Ok(0);
        }
        if Instant::now() >= deadline {
            bail!("session cleanup is still in progress; nothing was purged");
        }
        thread::sleep(Duration::from_millis(50));
    }
}

struct Runtime {
    pgid: u32,
    socket_path: PathBuf,
    reaper: Child,
    descendants: supervisor::ProcessRegistry,
    term_grace: Duration,
}

impl Drop for Runtime {
    fn drop(&mut self) {
        // Guardian owns startup and final cleanup, including the owner-crash path.
        self.reaper.stdin.take();
        if self.reaper.wait().is_ok_and(|status| !status.success()) {
            // Independent guardian failure must not leave its still-live worker
            // behind. Revalidate the originally observed process identities.
            self.descendants.signal(self.pgid, libc::SIGTERM);
            thread::sleep(self.term_grace);
            self.descendants.signal(self.pgid, libc::SIGKILL);
        }
        let _ = fs::remove_file(&self.socket_path);
    }
}

pub fn guard_worker(config: SupervisorConfig) -> Result<i32> {
    // stderr holds a duplicate of the owner's flock from BEFORE worker spawn.
    // Thus even death during socket readiness cannot release the ownership fence.
    struct Guard {
        worker: WorkerProcesses,
        grace: Duration,
        descendants: supervisor::ProcessRegistry,
    }
    impl Drop for Guard {
        fn drop(&mut self) {
            self.descendants.signal(self.worker.pgid, libc::SIGTERM);
            if let Some(server) = self.worker.server.as_mut() {
                let _ = supervisor::terminate_group(server, self.worker.pgid, self.grace);
            }
            supervisor::cleanup_remaining_group(self.worker.pgid);
            self.descendants.signal(self.worker.pgid, libc::SIGKILL);
            let _ = self.worker.proxy.wait();
            if let Some(path) = &self.worker.socket_path {
                let _ = fs::remove_file(path);
            }
        }
    }
    let mut guard = Guard {
        worker: supervisor::start_worker_guarded(&config, Some(0))?,
        grace: config.term_grace,
        descendants: supervisor::ProcessRegistry::default(),
    };
    let _ = guard.worker.proxy.kill();
    let _ = guard.worker.proxy.wait();
    writeln!(io::stdout(), "{}", guard.worker.pgid)?;
    io::stdout().flush()?;
    loop {
        guard.descendants.refresh(guard.worker.pgid);
        if guard
            .worker
            .server
            .as_mut()
            .context("missing server")?
            .try_wait()?
            .is_some()
        {
            break;
        }
        let mut fd = libc::pollfd {
            fd: 0,
            events: libc::POLLIN | libc::POLLHUP,
            revents: 0,
        };
        let ready = unsafe { libc::poll(&mut fd, 1, 100) };
        if ready < 0 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            break;
        }
        if ready > 0 {
            break;
        }
    }
    Ok(0)
}

struct Attachment {
    proxy: Child,
    stream: UnixStream,
    input: Option<JoinHandle<io::Result<()>>>,
    output: Option<JoinHandle<io::Result<()>>>,
    done: mpsc::Receiver<()>,
    observer: Arc<ProtocolObserver>,
    gate: Arc<Mutex<()>>,
    writer: Arc<Mutex<ChildStdin>>,
    next_control_id: u64,
}

impl Drop for Attachment {
    fn drop(&mut self) {
        let _ = self.stream.shutdown(Shutdown::Both);
        let _ = self.proxy.kill();
        let _ = self.proxy.wait();
        if let Some(t) = self.input.take() {
            let _ = t.join();
        }
        if let Some(t) = self.output.take() {
            let _ = t.join();
        }
    }
}

fn connect(
    config: &SupervisorConfig,
    runtime: &Runtime,
    mut stream: UnixStream,
) -> Result<Attachment> {
    // macOS accept inherits O_NONBLOCK from the listening socket. The bounded
    // copier owns a blocking stream; otherwise its first read reports EAGAIN.
    stream.set_nonblocking(false)?;
    let socket = &runtime.socket_path;
    let home = config.managed_root.join("codex-home");
    let mut proxy =
        supervisor::spawn_proxy(config, Some(socket), runtime.pgid as i32, Some(&home))?;
    stream.write_all(&[1])?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    let observer = Arc::new(ProtocolObserver::default());
    let gate = Arc::new(Mutex::new(()));
    let mut input = stream.try_clone()?;
    let child_in = Arc::new(Mutex::new(
        proxy.stdin.take().context("proxy stdin missing")?,
    ));
    let child_out = proxy.stdout.take().context("proxy stdout missing")?;
    let output = stream.try_clone()?;
    let (tx, done) = mpsc::channel();
    let input_tx = tx.clone();
    let input_observer = observer.clone();
    let input_gate = gate.clone();
    let input_writer = Arc::clone(&child_in);
    let takeover = config.takeover_enabled.then(|| {
        TakeoverManager::new(TakeoverConfig {
            isolated_socket: socket.clone(),
            shared_socket: config.shared_socket.clone(),
            timeout: config.takeover_timeout,
        })
    });
    let log = config.log_path.clone();
    let output_log = log.clone();
    let pid = runtime.pgid;
    let input = thread::spawn(move || {
        let result = (|| {
            let mut bytes = [0u8; 16384];
            loop {
                let count = input.read(&mut bytes)?;
                if count == 0 {
                    return Ok(());
                }
                let _guard = input_gate.lock().expect("forwarding gate poisoned");
                let mut writer = input_writer.lock().expect("proxy stdin mutex poisoned");
                supervisor::forward_client_chunk(
                    &mut *writer,
                    &input_observer,
                    &bytes[..count],
                    |attempt| {
                        if let Some(manager) = &takeover {
                            supervisor::log_takeover_event(
                                &log,
                                pid,
                                &attempt.thread_id,
                                manager.prepare_resume(attempt),
                            );
                        }
                    },
                )?;
            }
        })();
        supervisor::log_event(
            &log,
            "proxy_input_ended",
            pid,
            result
                .as_ref()
                .err()
                .and_then(|e: &io::Error| e.raw_os_error())
                .map(|e| e as usize),
        );
        let _ = input_tx.send(());
        result
    });
    let output_observer = observer.clone();
    let output = thread::spawn(move || {
        let result = supervisor::copy_observed(
            child_out,
            output,
            &output_observer,
            Direction::ServerToClient,
        );
        supervisor::log_event(
            &output_log,
            "proxy_output_ended",
            pid,
            result
                .as_ref()
                .err()
                .and_then(io::Error::raw_os_error)
                .map(|e| e as usize),
        );
        let _ = tx.send(());
        result
    });
    Ok(Attachment {
        proxy,
        stream,
        input: Some(input),
        output: Some(output),
        done,
        observer,
        gate,
        writer: child_in,
        next_control_id: 0,
    })
}

pub fn own(config: SupervisorConfig, id: &str) -> Result<i32> {
    let max_age = duration("CODEX_MANAGED_SESSION_MAX_SECS", 86400)?;
    let detach_budget = duration("CODEX_MANAGED_DETACH_BUDGET_SECS", 300)?;
    if config.use_existing_daemon || config.eof_grace.is_zero() {
        bail!("invalid bounded-session configuration");
    }
    let path = directory(&config, id)?;
    let owner_lock = lock(&path.join("owner.lock"))?;
    fs::create_dir_all(config.log_path.parent().context("missing log directory")?)?;
    let mut reaper = Command::new(std::env::current_exe()?)
        .arg("--session-worker")
        .env_remove("SSH_ORIGINAL_COMMAND")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(owner_lock.try_clone()?))
        .process_group(0)
        .spawn()?;
    let server_socket = config.run_dir.join(format!("app-{}.sock", reaper.id()));
    let mut ready = reaper.stdout.take().context("guardian readiness missing")?;
    let mut line = Vec::new();
    loop {
        let mut byte = [0];
        if ready.read(&mut byte)? == 0 {
            let _ = reaper.wait();
            bail!("guardian failed during worker startup");
        }
        if byte[0] == b'\n' {
            break;
        }
        if line.len() > 16 {
            bail!("invalid guardian readiness");
        }
        line.push(byte[0]);
    }
    let pid = std::str::from_utf8(&line)?.parse::<u32>()?;
    let mut descendants = supervisor::ProcessRegistry::default();
    descendants.refresh(pid);
    let mut runtime = Runtime {
        pgid: pid,
        socket_path: server_socket,
        reaper,
        descendants,
        term_grace: config.term_grace,
    };
    let socket = path.join("control.sock");
    let _ = fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket)?;
    listener.set_nonblocking(true)?;
    let stop_socket = path.join("stop.sock");
    let _ = fs::remove_file(&stop_socket);
    let stop_listener = UnixListener::bind(&stop_socket)?;
    stop_listener.set_nonblocking(true)?;
    let event = |name, value| supervisor::log_event(&config.log_path, name, pid, value);
    event("worker_started", None);
    let birth = Instant::now();
    let mut detached_at = Some(birth);
    let mut detached_total = Duration::ZERO;
    let mut active: Option<Attachment> = None;
    let mut sample = birth;
    let mut last_activity = birth;
    let mut generation = 0;
    let mut reclaim = ReclaimController::new(config.idle_reclaim, config.drain_grace);
    loop {
        if stop_listener.accept().is_ok() {
            event("session_stopped", None);
            break;
        }
        if runtime.reaper.try_wait()?.is_some() {
            event("runtime_exited", None);
            break;
        }
        if birth.elapsed() >= max_age {
            event("session_max_age", None);
            break;
        }
        if let Some(at) = detached_at
            && (at.elapsed() >= config.eof_grace || detached_total + at.elapsed() >= detach_budget)
        {
            event("session_lease_expired", None);
            break;
        }
        if let Some(connection) = active.as_mut()
            && (connection.done.try_recv().is_ok() || connection.proxy.try_wait()?.is_some())
        {
            // Join both forwarding threads before a new connection can win.
            active.take();
            detached_at = Some(Instant::now());
            event("client_detached", None);
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                if active.is_some() {
                    let _ = stream.write_all(&[0]);
                    event("attach_refused_active", None);
                } else {
                    match connect(&config, &runtime, stream) {
                        Ok(connection) => {
                            if let Some(at) = detached_at.take() {
                                detached_total += at.elapsed();
                            }
                            active = Some(connection);
                            generation = 0;
                            // Cancel a prior drain without renewing birth or FD history.
                            reclaim =
                                ReclaimController::new(config.idle_reclaim, config.drain_grace);
                            event("client_attached", None);
                        }
                        Err(_) => event("attach_failed", None),
                    }
                }
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
            Err(e) => return Err(e.into()),
        }
        if let Some(connection) = active.as_mut() {
            let snapshot = connection.observer.snapshot();
            if snapshot.client_activity_generation != generation {
                generation = snapshot.client_activity_generation;
                last_activity = Instant::now();
            }
            let action = reclaim.evaluate(
                birth.elapsed(),
                ReclaimSnapshot {
                    active_turns: snapshot.active_turns,
                    reliable: snapshot.reliable,
                    client_activity_generation: generation,
                    client_idle: last_activity.elapsed(),
                    archive_empty_epoch: snapshot.archive_empty_epoch,
                },
            );
            if let ReclaimAction::Reclaim(reason) = action
                && let Ok(_gate) = connection.gate.try_lock()
            {
                let before = connection.observer.snapshot();
                let socket = &runtime.socket_path;
                let idle = before.reliable
                    && before.active_turns == 0
                    && runtime_is_idle(socket, Duration::from_secs(2));
                let after = connection.observer.snapshot();
                if idle
                    && after.reliable
                    && after.active_turns == 0
                    && after.client_activity_generation == before.client_activity_generation
                {
                    if matches!(reason, crate::reclaim::ReclaimReason::Idle) {
                        let responses_before = connection.observer.snapshot();
                        let count = {
                            let mut writer = connection
                                .writer
                                .lock()
                                .expect("proxy stdin mutex poisoned");
                            unsubscribe_idle_threads(
                                &mut *writer,
                                &connection.observer,
                                &mut connection.next_control_id,
                            )
                        };
                        match count {
                            Ok(0) => {
                                event("idle_reclaim_noop", None);
                                last_activity = Instant::now();
                                reclaim =
                                    ReclaimController::new(config.idle_reclaim, config.drain_grace);
                            }
                            Ok(count)
                                if wait_for_thread_unload(
                                    &connection.observer,
                                    config.thread_reclaim_wait,
                                ) =>
                            {
                                let responses_after = connection.observer.snapshot();
                                event(
                                    "idle_unsubscribe_accepted",
                                    Some(
                                        responses_after
                                            .unsubscribe_completed
                                            .saturating_sub(responses_before.unsubscribe_completed),
                                    ),
                                );
                                event(
                                    "idle_unsubscribe_rejected",
                                    Some(
                                        responses_after
                                            .unsubscribe_failed
                                            .saturating_sub(responses_before.unsubscribe_failed),
                                    ),
                                );
                                event("idle_unsubscribe_completed", Some(count));
                                last_activity = Instant::now();
                                reclaim =
                                    ReclaimController::new(config.idle_reclaim, config.drain_grace);
                            }
                            Ok(count) => {
                                let responses_after = connection.observer.snapshot();
                                event(
                                    "idle_unsubscribe_accepted",
                                    Some(
                                        responses_after
                                            .unsubscribe_completed
                                            .saturating_sub(responses_before.unsubscribe_completed),
                                    ),
                                );
                                event(
                                    "idle_unsubscribe_rejected",
                                    Some(
                                        responses_after
                                            .unsubscribe_failed
                                            .saturating_sub(responses_before.unsubscribe_failed),
                                    ),
                                );
                                event(
                                    "idle_unsubscribe_remaining",
                                    Some(responses_after.live_threads),
                                );
                                event("idle_unsubscribe_timeout", Some(count));
                                last_activity = Instant::now();
                                reclaim =
                                    ReclaimController::new(config.idle_reclaim, config.drain_grace);
                            }
                            Err(_) => {
                                event("idle_unsubscribe_failed", None);
                                last_activity = Instant::now();
                                reclaim =
                                    ReclaimController::new(config.idle_reclaim, config.drain_grace);
                            }
                        }
                    } else {
                        event("archive_reclaim", None);
                        let _ = connection.stream.shutdown(Shutdown::Both);
                        unsafe {
                            libc::kill(connection.proxy.id() as i32, libc::SIGKILL);
                        }
                        break;
                    }
                }
            }
        }
        if sample.elapsed() >= config.sample_every {
            sample = Instant::now();
            match supervisor::count_process_fds(pid) {
                Ok(count) => {
                    let snapshot = active.as_ref().map(|a| a.observer.snapshot());
                    let busy = snapshot
                        .as_ref()
                        .map_or(1, |s| if s.reliable { s.active_turns } else { 1 });
                    match config
                        .fd_policy
                        .action(count, busy, last_activity.elapsed())
                    {
                        FdAction::HardStop => {
                            event("fd_hard_stop", Some(count));
                            break;
                        }
                        FdAction::Recycle => {
                            if let Some(connection) = active.as_mut()
                                && let Ok(_gate) = connection.gate.try_lock()
                            {
                                let s = connection.observer.snapshot();
                                if s.reliable
                                    && s.active_turns == 0
                                    && runtime_is_idle(&runtime.socket_path, Duration::from_secs(2))
                                    && connection.observer.snapshot().client_activity_generation
                                        == s.client_activity_generation
                                    && connection.observer.snapshot().active_turns == 0
                                {
                                    let requested = {
                                        let mut writer = connection
                                            .writer
                                            .lock()
                                            .expect("proxy stdin mutex poisoned");
                                        unsubscribe_idle_threads(
                                            &mut *writer,
                                            &connection.observer,
                                            &mut connection.next_control_id,
                                        )
                                    };
                                    let recovered = requested.is_ok_and(|requested| {
                                        requested > 0
                                            && wait_for_thread_unload(
                                                &connection.observer,
                                                config.thread_reclaim_wait,
                                            )
                                    }) && supervisor::count_process_fds(pid)
                                        .is_ok_and(|after| {
                                            matches!(
                                                config.fd_policy.action(after, 0, Duration::MAX),
                                                FdAction::Healthy | FdAction::Warn
                                            )
                                        });
                                    if recovered {
                                        event("fd_unsubscribe_completed", Some(count));
                                        last_activity = Instant::now();
                                    } else {
                                        event("fd_idle_recycle", Some(count));
                                        let _ = connection.stream.shutdown(Shutdown::Both);
                                        unsafe {
                                            libc::kill(connection.proxy.id() as i32, libc::SIGKILL);
                                        }
                                        break;
                                    }
                                }
                            }
                        }
                        FdAction::Warn => event("fd_warning", Some(count)),
                        FdAction::Healthy => {}
                    }
                }
                Err(_) => event("fd_sample_failed", None),
            }
        }
        thread::sleep(Duration::from_millis(50));
    }
    drop(active);
    drop(listener);
    drop(stop_listener);
    let _ = fs::remove_file(stop_socket);
    let _ = fs::remove_file(socket);
    drop(runtime);
    event("worker_exited", None);
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::{unsubscribe_idle_threads, wait_for_thread_unload};
    use crate::protocol::{Direction, ProtocolObserver};
    use std::sync::Arc;
    use std::time::Duration;

    fn idle_observer() -> Arc<ProtocolObserver> {
        let observer = Arc::new(ProtocolObserver::default());
        observer.observe(
            Direction::ClientToServer,
            b"{\"id\":1,\"method\":\"thread/resume\",\"params\":{\"threadId\":\"thread-a\"}}\n",
        );
        observer.observe(
            Direction::ServerToClient,
            b"{\"id\":1,\"result\":{\"thread\":{\"id\":\"thread-a\"}}}\n",
        );
        observer
    }

    #[test]
    fn unsubscribe_is_injected_without_counting_as_desktop_activity() {
        let observer = idle_observer();
        let generation = observer.snapshot().client_activity_generation;
        let mut output = Vec::new();

        let count = unsubscribe_idle_threads(&mut output, &observer, &mut 0).unwrap();

        assert_eq!(count, 1);
        assert_eq!(output[0], 0x81);
        assert_ne!(output[1] & 0x80, 0, "client frame must be masked");
        let mut offset = 2;
        let payload_len = match output[1] & 0x7f {
            value @ 0..=125 => value as usize,
            126 => {
                let value = u16::from_be_bytes([output[2], output[3]]) as usize;
                offset += 2;
                value
            }
            value => panic!("unexpected frame length marker {value}"),
        };
        let mask: [u8; 4] = output[offset..offset + 4].try_into().unwrap();
        offset += 4;
        let payload: Vec<u8> = output[offset..offset + payload_len]
            .iter()
            .enumerate()
            .map(|(index, byte)| byte ^ mask[index % 4])
            .collect();
        assert_eq!(
            String::from_utf8(payload).unwrap(),
            "{\"id\":\"codex-managed-unsubscribe-0\",\"jsonrpc\":\"2.0\",\"method\":\"thread/unsubscribe\",\"params\":{\"threadId\":\"thread-a\"}}"
        );
        assert_eq!(observer.snapshot().client_activity_generation, generation);
    }

    #[test]
    fn unload_wait_completes_only_after_thread_closed() {
        let observer = idle_observer();
        let notifier = Arc::clone(&observer);
        let thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            notifier.observe(
                Direction::ServerToClient,
                b"{\"method\":\"thread/closed\",\"params\":{\"threadId\":\"thread-a\"}}\n",
            );
        });

        assert!(wait_for_thread_unload(&observer, Duration::from_secs(1)));
        thread.join().unwrap();
    }

    #[test]
    fn unload_wait_times_out_when_thread_remains_loaded() {
        let observer = idle_observer();
        assert!(!wait_for_thread_unload(
            &observer,
            Duration::from_millis(20)
        ));
    }
}
