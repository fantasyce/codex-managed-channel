use crate::fd_guard::{FdAction, FdPolicy};
use crate::managed_home::prepare_managed_home;
use crate::marketplace::{
    config_override, managed_bundled_marketplace_root, managed_marketplace_root,
    prepare_marketplace_from_cache, validate_marketplace, validate_named_marketplace,
};
use crate::protocol::{Direction, ProtocolObserver, ResumeAttempt};
use crate::reclaim::{ReclaimAction, ReclaimController, ReclaimReason, ReclaimSnapshot};
use crate::takeover::{TakeoverConfig, TakeoverManager, TakeoverOutcome};
use anyhow::{Context, Result, bail};
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct SupervisorConfig {
    pub codex_bin: PathBuf,
    pub eof_grace: Duration,
    pub term_grace: Duration,
    pub sample_every: Duration,
    pub idle_reclaim: Duration,
    pub drain_grace: Duration,
    pub thread_unload_delay: Duration,
    pub thread_reclaim_wait: Duration,
    pub fd_policy: FdPolicy,
    pub log_path: PathBuf,
    pub run_dir: PathBuf,
    pub socket_start_timeout: Duration,
    pub use_existing_daemon: bool,
    pub takeover_enabled: bool,
    pub takeover_timeout: Duration,
    pub shared_socket: PathBuf,
    pub personal_marketplace: PathBuf,
    pub bundled_marketplace: PathBuf,
    pub os_home: PathBuf,
    pub real_codex_home: PathBuf,
    pub managed_root: PathBuf,
}

impl SupervisorConfig {
    pub fn from_env() -> Result<Self> {
        let home = env::var_os("HOME")
            .map(PathBuf::from)
            .context("HOME is not set")?;
        let codex_bin = resolve_codex_binary(&home)?;
        let root = env::var_os("CODEX_MANAGED_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".codex-managed"));
        Ok(Self {
            codex_bin,
            eof_grace: seconds_env("CODEX_MANAGED_EOF_GRACE_SECS", 45),
            term_grace: seconds_env("CODEX_MANAGED_TERM_GRACE_SECS", 5),
            sample_every: seconds_env("CODEX_MANAGED_SAMPLE_SECS", 15),
            idle_reclaim: seconds_env("CODEX_MANAGED_IDLE_SECS", 600),
            drain_grace: seconds_env("CODEX_MANAGED_DRAIN_SECS", 3),
            thread_unload_delay: seconds_env("CODEX_MANAGED_THREAD_UNLOAD_SECS", 2),
            thread_reclaim_wait: seconds_env("CODEX_MANAGED_THREAD_RECLAIM_WAIT_SECS", 10),
            fd_policy: FdPolicy::new(
                usize_env("CODEX_MANAGED_FD_WARN", 160),
                usize_env("CODEX_MANAGED_FD_RECYCLE", 192),
                usize_env("CODEX_MANAGED_FD_HARD", 240),
                seconds_env("CODEX_MANAGED_IDLE_RECYCLE_SECS", 300),
            )?,
            log_path: root.join("log/supervisor.jsonl"),
            run_dir: root.join("run"),
            socket_start_timeout: seconds_env("CODEX_MANAGED_SOCKET_START_SECS", 10),
            use_existing_daemon: bool_env("CODEX_MANAGED_USE_EXISTING_DAEMON"),
            takeover_enabled: enabled_env("CODEX_MANAGED_TAKEOVER", true),
            takeover_timeout: seconds_env("CODEX_MANAGED_TAKEOVER_TIMEOUT_SECS", 5),
            shared_socket: home.join(".codex/app-server-control/app-server-control.sock"),
            personal_marketplace: managed_marketplace_root(&home),
            bundled_marketplace: managed_bundled_marketplace_root(&home),
            os_home: home.clone(),
            real_codex_home: env::var_os("CODEX_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".codex")),
            managed_root: root,
        })
    }
}

pub fn run(config: SupervisorConfig) -> Result<i32> {
    prepare_log(&config.log_path)?;
    let mut worker = start_worker(&config)?;
    let pid = worker.monitored_pid;
    log_event(&config.log_path, "worker_started", pid, None);

    let observer = Arc::new(ProtocolObserver::default());
    let takeover = if config.takeover_enabled && !config.use_existing_daemon {
        worker.socket_path.as_ref().map(|isolated_socket| {
            TakeoverManager::new(TakeoverConfig {
                isolated_socket: isolated_socket.clone(),
                shared_socket: config.shared_socket.clone(),
                timeout: config.takeover_timeout,
            })
        })
    } else {
        None
    };
    let child_stdin = worker
        .proxy
        .stdin
        .take()
        .context("worker stdin unavailable")?;
    let child_stdout = worker
        .proxy
        .stdout
        .take()
        .context("worker stdout unavailable")?;
    let (eof_tx, eof_rx) = mpsc::channel();
    let input_observer = Arc::clone(&observer);
    let input_log_path = config.log_path.clone();
    let _input_thread = thread::spawn(move || {
        let result = copy_client_observed(io::stdin(), child_stdin, &input_observer, |attempt| {
            let Some(manager) = takeover.as_ref() else {
                return;
            };
            let outcome = manager.prepare_resume(attempt);
            log_takeover_event(&input_log_path, pid, &attempt.thread_id, outcome);
        });
        let _ = eof_tx.send(());
        result
    });
    let output_observer = Arc::clone(&observer);
    let output_thread = thread::spawn(move || {
        copy_observed(
            child_stdout,
            io::stdout(),
            &output_observer,
            Direction::ServerToClient,
        )
    });

    let mut eof_at: Option<Instant> = None;
    let supervisor_started = Instant::now();
    let mut reclaim = ReclaimController::new(config.idle_reclaim, config.drain_grace);
    // Give the official worker one sample interval to finish its initial exec
    // and MCP bootstrap before applying limits.
    let mut last_sample = Instant::now();
    let mut protocol_unreliable_logged = false;
    let status = loop {
        if let Some(status) = worker.proxy.try_wait().context("failed to poll worker")? {
            break status;
        }
        if let Some(server) = worker.server.as_mut()
            && let Some(status) = server
                .try_wait()
                .context("failed to poll isolated app-server")?
        {
            break status;
        }
        if eof_at.is_none() && eof_rx.try_recv().is_ok() {
            eof_at = Some(Instant::now());
            log_event(&config.log_path, "client_eof", pid, None);
        }
        if eof_at.is_some_and(|at| at.elapsed() >= config.eof_grace) {
            log_event(&config.log_path, "eof_grace_expired", pid, None);
            break terminate_group(&mut worker.proxy, worker.pgid, config.term_grace)?;
        }
        let protocol = observer.snapshot();
        if !protocol.reliable && !protocol_unreliable_logged {
            protocol_unreliable_logged = true;
            log_event(&config.log_path, "protocol_unreliable", pid, None);
        }
        let reclaim_snapshot = ReclaimSnapshot {
            active_turns: protocol.active_turns,
            reliable: protocol.reliable,
            client_activity_generation: protocol.client_activity_generation,
            client_idle: protocol.client_idle,
            archive_empty_epoch: protocol.archive_empty_epoch,
        };
        match reclaim.evaluate(supervisor_started.elapsed(), reclaim_snapshot) {
            ReclaimAction::None => {}
            ReclaimAction::DrainStarted(reason) => {
                log_event(&config.log_path, drain_started_event(reason), pid, None);
            }
            ReclaimAction::DrainCancelled(reason) => {
                log_event(&config.log_path, drain_cancelled_event(reason), pid, None);
            }
            ReclaimAction::Reclaim(reason) => {
                log_event(&config.log_path, reclaim_event(reason), pid, None);
                break terminate_group(&mut worker.proxy, worker.pgid, config.term_grace)?;
            }
        }
        if last_sample.elapsed() >= config.sample_every {
            last_sample = Instant::now();
            if let Ok(count) = count_process_fds(pid) {
                match config.fd_policy.action(
                    count,
                    observer.active_turns(),
                    observer.idle_elapsed(),
                ) {
                    FdAction::Healthy => {}
                    FdAction::Warn => log_event(&config.log_path, "fd_warning", pid, Some(count)),
                    FdAction::Recycle => {
                        log_event(&config.log_path, "fd_idle_recycle", pid, Some(count));
                        break terminate_group(&mut worker.proxy, worker.pgid, config.term_grace)?;
                    }
                    FdAction::HardStop => {
                        log_event(&config.log_path, "fd_hard_stop", pid, Some(count));
                        break terminate_group(&mut worker.proxy, worker.pgid, config.term_grace)?;
                    }
                }
            }
        }
        thread::sleep(Duration::from_millis(100));
    };

    // Never join the client-input copier here. A hard FD stop must be able to
    // finish while the SSH client still has stdin open. Dropping a JoinHandle
    // detaches only this supervisor thread; process exit closes its descriptors.
    cleanup_remaining_group(worker.pgid);
    let _ = output_thread.join();
    if let Some(server) = worker.server.as_mut() {
        let _ = server.wait();
    }
    if let Some(socket_path) = worker.socket_path.as_ref() {
        let _ = fs::remove_file(socket_path);
    }
    log_event(
        &config.log_path,
        "worker_exited",
        pid,
        status.code().map(|v| v as usize),
    );
    Ok(status.code().unwrap_or(1))
}

pub(crate) struct WorkerProcesses {
    pub(crate) proxy: Child,
    pub(crate) server: Option<Child>,
    pub(crate) pgid: u32,
    pub(crate) monitored_pid: u32,
    pub(crate) socket_path: Option<PathBuf>,
}

pub(crate) fn start_worker(config: &SupervisorConfig) -> Result<WorkerProcesses> {
    start_worker_guarded(config, None)
}

pub(crate) fn start_worker_guarded(
    config: &SupervisorConfig,
    owner_pipe: Option<i32>,
) -> Result<WorkerProcesses> {
    if config.use_existing_daemon {
        let proxy = spawn_proxy(config, None, 0, None)?;
        let pid = proxy.id();
        return Ok(WorkerProcesses {
            proxy,
            server: None,
            pgid: pid,
            monitored_pid: pid,
            socket_path: None,
        });
    }

    fs::create_dir_all(&config.run_dir)?;
    fs::set_permissions(&config.run_dir, fs::Permissions::from_mode(0o700))?;
    let socket_path = config
        .run_dir
        .join(format!("app-{}.sock", std::process::id()));
    let _ = fs::remove_file(&socket_path);
    let listen = format!("unix://{}", socket_path.display());
    let plugin_cache = config.real_codex_home.join("plugins/cache");
    if validate_marketplace(&config.personal_marketplace).is_err() {
        prepare_marketplace_from_cache(
            &config.personal_marketplace,
            &plugin_cache.join("personal"),
            "personal",
        )?;
        validate_marketplace(&config.personal_marketplace)?;
    }
    if validate_named_marketplace(&config.bundled_marketplace, "openai-bundled").is_err() {
        prepare_marketplace_from_cache(
            &config.bundled_marketplace,
            &plugin_cache.join("openai-bundled"),
            "openai-bundled",
        )?;
        validate_named_marketplace(&config.bundled_marketplace, "openai-bundled")?;
    }
    let managed_home = prepare_managed_home(
        &config.os_home,
        &config.real_codex_home,
        &config.managed_root,
        &config.personal_marketplace,
        &config.bundled_marketplace,
    )?;
    let marketplace_override = config_override(&config.personal_marketplace)?;
    let unload_override = format!(
        "thread_unload_delay_secs={}",
        config.thread_unload_delay.as_secs()
    );
    let mut server = Command::new(&config.codex_bin)
        .args([
            "app-server",
            "-c",
            &marketplace_override,
            "-c",
            &unload_override,
            "--listen",
            &listen,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("CODEX_HOME", &managed_home)
        .current_dir(&config.run_dir)
        .process_group(0)
        .spawn()
        .with_context(|| format!("failed to start isolated {}", config.codex_bin.display()))?;
    let pgid = server.id();
    let started = Instant::now();
    loop {
        if let Some(fd) = owner_pipe {
            let mut poll = libc::pollfd {
                fd,
                events: libc::POLLIN | libc::POLLHUP,
                revents: 0,
            };
            if unsafe { libc::poll(&mut poll, 1, 0) } > 0 {
                cleanup_remaining_group(pgid);
                let _ = server.wait();
                bail!("owner disconnected during worker startup");
            }
        }
        if socket_path
            .metadata()
            .is_ok_and(|metadata| metadata.file_type().is_socket())
        {
            break;
        }
        if let Some(status) = server
            .try_wait()
            .context("failed to poll isolated app-server")?
        {
            bail!("isolated app-server exited before socket readiness: {status}");
        }
        if started.elapsed() >= config.socket_start_timeout {
            cleanup_remaining_group(pgid);
            let _ = server.wait();
            bail!("isolated app-server socket readiness timed out");
        }
        thread::sleep(Duration::from_millis(25));
    }

    let proxy = match spawn_proxy(config, Some(&socket_path), pgid as i32, Some(&managed_home)) {
        Ok(proxy) => proxy,
        Err(error) => {
            cleanup_remaining_group(pgid);
            let _ = server.wait();
            let _ = fs::remove_file(&socket_path);
            return Err(error);
        }
    };
    Ok(WorkerProcesses {
        proxy,
        server: Some(server),
        pgid,
        monitored_pid: pgid,
        socket_path: Some(socket_path),
    })
}

pub(crate) fn spawn_proxy(
    config: &SupervisorConfig,
    socket: Option<&std::path::Path>,
    pgid: i32,
    managed_home: Option<&std::path::Path>,
) -> Result<Child> {
    let mut command = Command::new(&config.codex_bin);
    command.args(["app-server", "proxy"]);
    if let Some(socket) = socket {
        command.arg("--sock").arg(socket);
    }
    if let Some(managed_home) = managed_home {
        command.env("CODEX_HOME", managed_home);
        command.current_dir(&config.run_dir);
    }
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(pgid)
        .spawn()
        .with_context(|| format!("failed to start {} proxy", config.codex_bin.display()))
}

fn drain_started_event(reason: ReclaimReason) -> &'static str {
    match reason {
        ReclaimReason::Archive => "archive_drain_started",
        ReclaimReason::Idle => "idle_drain_started",
    }
}

fn drain_cancelled_event(reason: ReclaimReason) -> &'static str {
    match reason {
        ReclaimReason::Archive => "archive_drain_cancelled",
        ReclaimReason::Idle => "idle_drain_cancelled",
    }
}

fn reclaim_event(reason: ReclaimReason) -> &'static str {
    match reason {
        ReclaimReason::Archive => "archive_reclaim",
        ReclaimReason::Idle => "idle_reclaim",
    }
}

pub(crate) fn cleanup_remaining_group(pgid: u32) {
    let groups = process_tree_groups(pgid);
    signal_groups(&groups, libc::SIGTERM);
    thread::sleep(Duration::from_millis(50));
    signal_groups(&groups, libc::SIGKILL);
}

pub(crate) fn copy_observed<R: Read, W: Write>(
    mut reader: R,
    mut writer: W,
    observer: &ProtocolObserver,
    direction: Direction,
) -> io::Result<()> {
    let mut buffer = [0u8; 16 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            writer.flush()?;
            return Ok(());
        }
        let bytes = observer.observe(direction, &buffer[..read]);
        writer.write_all(bytes)?;
        writer.flush()?;
    }
}

fn copy_client_observed<R, W, F>(
    mut reader: R,
    mut writer: W,
    observer: &ProtocolObserver,
    mut prepare: F,
) -> io::Result<()>
where
    R: Read,
    W: Write,
    F: FnMut(&ResumeAttempt),
{
    let mut buffer = [0u8; 16 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            writer.flush()?;
            return Ok(());
        }
        forward_client_chunk(&mut writer, observer, &buffer[..read], |attempt| {
            prepare(attempt)
        })?;
    }
}

pub(crate) fn forward_client_chunk<W, F>(
    writer: &mut W,
    observer: &ProtocolObserver,
    bytes: &[u8],
    mut prepare: F,
) -> io::Result<()>
where
    W: Write,
    F: FnMut(&ResumeAttempt),
{
    let bytes = observer.observe(Direction::ClientToServer, bytes);
    for attempt in observer.take_resume_attempts() {
        prepare(&attempt);
    }
    writer.write_all(bytes)?;
    writer.flush()
}

pub(crate) fn terminate_group(child: &mut Child, pgid: u32, grace: Duration) -> Result<ExitStatus> {
    let groups = process_tree_groups(pgid);
    signal_groups(&groups, libc::SIGTERM);
    let started = Instant::now();
    while started.elapsed() < grace {
        if let Some(status) = child.try_wait()? {
            signal_groups(&groups, libc::SIGKILL);
            return Ok(status);
        }
        thread::sleep(Duration::from_millis(100));
    }
    signal_groups(&groups, libc::SIGKILL);
    child.wait().context("failed to reap worker")
}

pub(crate) fn process_tree_groups(root: u32) -> Vec<i32> {
    let mut groups = HashSet::from([root as i32]);
    let Ok(output) = Command::new("/bin/ps")
        .args(["-axo", "pid=,ppid=,pgid="])
        .output()
    else {
        return groups.into_iter().collect();
    };
    let processes: Vec<(u32, u32, i32)> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            Some((
                fields.next()?.parse().ok()?,
                fields.next()?.parse().ok()?,
                fields.next()?.parse().ok()?,
            ))
        })
        .collect();
    let mut descendants = HashSet::from([root]);
    loop {
        let mut changed = false;
        for &(pid, parent, group) in &processes {
            if descendants.contains(&parent) && descendants.insert(pid) {
                if group > 1 {
                    groups.insert(group);
                }
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    groups.into_iter().collect()
}

/// Remember observed descendants across reparenting. Revalidate both PID and
/// start timestamp before signalling; a historical numeric PID alone is unsafe.
#[derive(Default)]
pub(crate) struct ProcessRegistry {
    known: HashMap<u32, (i32, String)>,
    root_stamp: Option<String>,
}

impl ProcessRegistry {
    pub(crate) fn refresh(&mut self, root: u32) -> bool {
        let Ok(output) = Command::new("/bin/ps")
            .args(["-axo", "pid=,ppid=,pgid=,lstart="])
            .output()
        else {
            return false;
        };
        if !output.status.success() {
            return false;
        }
        let table: Vec<(u32, u32, i32, String)> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| {
                let mut fields = line.split_whitespace();
                Some((
                    fields.next()?.parse().ok()?,
                    fields.next()?.parse().ok()?,
                    fields.next()?.parse().ok()?,
                    fields.collect::<Vec<_>>().join(" "),
                ))
            })
            .collect();
        self.known.retain(|pid, (group, stamp)| {
            table
                .iter()
                .any(|(p, _, g, s)| p == pid && g == group && s == stamp)
        });
        let mut ancestors: HashSet<u32> = self.known.keys().copied().collect();
        let current_stamp = table
            .iter()
            .find(|(p, _, _, _)| *p == root)
            .map(|(_, _, _, s)| s.clone());
        if self.root_stamp.is_none() {
            self.root_stamp = current_stamp.clone();
        }
        let root_matches = current_stamp.is_some() && current_stamp == self.root_stamp;
        if root_matches {
            ancestors.insert(root);
        }
        loop {
            let mut changed = false;
            for (pid, parent, group, stamp) in &table {
                if ((*pid == root && root_matches) || ancestors.contains(parent))
                    && *group > 1
                    && !stamp.is_empty()
                {
                    self.known.insert(*pid, (*group, stamp.clone()));
                    changed |= ancestors.insert(*pid);
                }
            }
            if !changed {
                break;
            }
        }
        true
    }

    pub(crate) fn signal(&mut self, root: u32, signal: i32) {
        if !self.refresh(root) {
            return;
        }
        let groups: HashSet<i32> = self.known.values().map(|(group, _)| *group).collect();
        signal_groups(&groups.into_iter().collect::<Vec<_>>(), signal);
    }
}

pub(crate) fn signal_groups(groups: &[i32], signal: i32) {
    for group in groups {
        unsafe {
            libc::kill(-group, signal);
        }
    }
}

pub fn count_process_fds(pid: u32) -> Result<usize> {
    let output = Command::new("/usr/sbin/lsof")
        .args(["-n", "-P", "-a", "-p", &pid.to_string()])
        .output()
        .context("failed to run lsof")?;
    if !output.status.success() {
        bail!("lsof failed for pid {pid}");
    }
    Ok(output
        .stdout
        .split(|b| *b == b'\n')
        .filter(|line| !line.is_empty())
        .count()
        .saturating_sub(1))
}

pub fn resolve_codex_binary(home: &std::path::Path) -> Result<PathBuf> {
    let candidate = env::var_os("CODEX_MANAGED_CODEX_BIN")
        .map(PathBuf::from)
        .or_else(|| env::var_os("CODEX_INSTALL_DIR").map(|p| PathBuf::from(p).join("bin/codex")))
        .unwrap_or_else(|| home.join(".local/bin/codex"));
    if !candidate.is_file() {
        bail!("official Codex binary not found at {}", candidate.display());
    }
    fs::canonicalize(&candidate).with_context(|| format!("cannot resolve {}", candidate.display()))
}

fn seconds_env(name: &str, default: u64) -> Duration {
    Duration::from_secs(
        env::var(name)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(default),
    )
}

fn usize_env(name: &str, default: usize) -> usize {
    env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn bool_env(name: &str) -> bool {
    env::var(name).is_ok_and(|value| matches!(value.as_str(), "1" | "true" | "yes"))
}

fn enabled_env(name: &str, default: bool) -> bool {
    env::var(name)
        .ok()
        .map(|value| !matches!(value.as_str(), "0" | "false" | "no"))
        .unwrap_or(default)
}

fn prepare_log(path: &std::path::Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn write_record(path: &std::path::Path, record: &serde_json::Value) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;
    let guard = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path.with_extension("lock"))?;
    if unsafe { libc::flock(guard.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return Err(io::Error::last_os_error());
    }
    if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(io::Error::other("log cannot be a symlink"));
    }
    if path.metadata().is_ok_and(|m| m.len() >= 1024 * 1024) {
        fs::rename(path, path.with_extension("jsonl.1"))?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    file.write_all(format!("{record}\n").as_bytes())
}

pub(crate) fn log_event(path: &std::path::Path, event: &str, pid: u32, value: Option<usize>) {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let record = serde_json::json!({"ts": timestamp, "event": event, "pid": pid, "value": value});
    let _ = write_record(path, &record);
}

pub(crate) fn log_takeover_event(
    path: &std::path::Path,
    pid: u32,
    thread_id: &str,
    outcome: TakeoverOutcome,
) {
    let event = match outcome {
        TakeoverOutcome::AlreadyOwned => "takeover_not_needed",
        TakeoverOutcome::TakenOver => "takeover_completed",
        TakeoverOutcome::RefusedActive => "takeover_refused_active",
        TakeoverOutcome::RefusedUnprovenStatus => "takeover_refused_unproven_status",
        TakeoverOutcome::NoSharedOwner => "takeover_not_needed_no_shared_owner",
        TakeoverOutcome::Failed => "takeover_failed",
        TakeoverOutcome::FailedRolledBack => "takeover_failed_rolled_back",
        TakeoverOutcome::FailedRollback => "takeover_failed_rollback",
    };
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let record = serde_json::json!({
        "ts": timestamp,
        "event": event,
        "pid": pid,
        "threadId": thread_id,
    });
    let _ = write_record(path, &record);
}

#[cfg(test)]
mod tests {
    use super::forward_client_chunk;
    use crate::protocol::ProtocolObserver;
    use std::cell::RefCell;
    use std::io::{self, Write};
    use std::rc::Rc;

    #[test]
    fn concurrent_metadata_logs_remain_bounded_and_parseable() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("supervisor.jsonl");
        std::thread::scope(|scope| {
            for _ in 0..4 {
                let path = &path;
                scope.spawn(move || {
                    for _ in 0..400 {
                        super::log_event(path, &"x".repeat(1024), 42, None);
                    }
                });
            }
        });
        assert!(std::fs::metadata(&path).unwrap().len() < 1024 * 1024 + 2048);
        for file in [&path, &path.with_extension("jsonl.1")] {
            for line in std::fs::read_to_string(file).unwrap().lines() {
                serde_json::from_str::<serde_json::Value>(line).unwrap();
            }
        }
    }

    struct EventWriter(Rc<RefCell<Vec<&'static str>>>);

    impl Write for EventWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.borrow_mut().push("write");
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn resume_preparation_happens_before_completed_request_bytes_are_forwarded() {
        let observer = ProtocolObserver::default();
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut writer = EventWriter(Rc::clone(&events));
        let first =
            b"{\"id\":1,\"method\":\"thread/resume\",\"params\":{\"threadId\":\"thread-a\"}";
        let final_bytes = b"}\n";

        forward_client_chunk(&mut writer, &observer, first, |_| {
            events.borrow_mut().push("prepare");
        })
        .unwrap();
        assert_eq!(&*events.borrow(), &["write"]);

        forward_client_chunk(&mut writer, &observer, final_bytes, |_| {
            events.borrow_mut().push("prepare");
        })
        .unwrap();
        assert_eq!(&*events.borrow(), &["write", "prepare", "write"]);
    }
}
