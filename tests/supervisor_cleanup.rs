use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[test]
fn hard_fd_stop_reaps_group_even_while_client_stdin_is_open() {
    let temp = tempfile::tempdir().unwrap();
    let fake = temp.path().join("codex");
    let descendant_pid = temp.path().join("descendant.pid");
    fs::write(
        &fake,
        "#!/bin/sh\npython3 -c 'import os,signal,time; os.setsid(); signal.signal(signal.SIGTERM, signal.SIG_IGN); open(os.environ[\"DESCENDANT_PID_FILE\"], \"w\").write(str(os.getpid())); time.sleep(60)' &\ntrap '' TERM\nwhile :; do sleep 60; done\n",
    )
    .unwrap();
    let mut permissions = fs::metadata(&fake).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&fake, permissions).unwrap();

    let mut entry = Command::new(env!("CARGO_BIN_EXE_codex-managed-entry"))
        .env("HOME", temp.path())
        .env("CODEX_MANAGED_CODEX_BIN", &fake)
        .env("DESCENDANT_PID_FILE", &descendant_pid)
        .env("CODEX_MANAGED_SAMPLE_SECS", "3")
        .env("CODEX_MANAGED_FD_WARN", "1")
        .env("CODEX_MANAGED_FD_RECYCLE", "2")
        .env("CODEX_MANAGED_FD_HARD", "3")
        .env("CODEX_MANAGED_TERM_GRACE_SECS", "1")
        .env("CODEX_MANAGED_USE_EXISTING_DAEMON", "1")
        .env(
            "SSH_ORIGINAL_COMMAND",
            "printf '%b' '\\001\\002\\003\\004\\005\\006\\007\\010'; exec codex app-server proxy",
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let _keep_client_open = entry.stdin.take().unwrap();
    wait_for_path(&descendant_pid, Duration::from_secs(2));
    let deadline = Instant::now() + Duration::from_secs(15);
    let status = loop {
        if let Some(status) = entry.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = entry.kill();
            panic!("supervisor remained stuck after its hard FD stop");
        }
        thread::sleep(Duration::from_millis(100));
    };
    assert!(!status.success());

    let pid: i32 = fs::read_to_string(descendant_pid)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    thread::sleep(Duration::from_millis(200));
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1, "descendant still exists");
}

fn wait_for_path(path: &std::path::Path, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "test worker did not create {}",
            path.display()
        );
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn eof_deadline_reaps_a_worker_that_refuses_to_exit() {
    let temp = tempfile::tempdir().unwrap();
    let fake = temp.path().join("codex");
    fs::write(
        &fake,
        "#!/bin/sh\ntrap '' TERM\nwhile :; do sleep 60; done\n",
    )
    .unwrap();
    let mut permissions = fs::metadata(&fake).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&fake, permissions).unwrap();

    let started = Instant::now();
    let status = Command::new(env!("CARGO_BIN_EXE_codex-managed-entry"))
        .env("HOME", temp.path())
        .env("CODEX_MANAGED_CODEX_BIN", &fake)
        .env("CODEX_MANAGED_EOF_GRACE_SECS", "1")
        .env("CODEX_MANAGED_TERM_GRACE_SECS", "1")
        .env("CODEX_MANAGED_USE_EXISTING_DAEMON", "1")
        .env(
            "SSH_ORIGINAL_COMMAND",
            "printf '%b' '\\001\\002\\003\\004\\005\\006\\007\\010'; exec codex app-server proxy",
        )
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(!status.success());
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn supervisor_starts_the_official_proxy_transport() {
    let temp = tempfile::tempdir().unwrap();
    let fake = temp.path().join("codex");
    let args_file = temp.path().join("args.txt");
    fs::write(
        &fake,
        "#!/bin/sh\nprintf '%s\\n' \"$*\" > \"$ARGS_FILE\"\nexit 0\n",
    )
    .unwrap();
    let mut permissions = fs::metadata(&fake).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&fake, permissions).unwrap();

    let status = managed_entry(&temp, &fake)
        .env("ARGS_FILE", &args_file)
        .stdin(Stdio::null())
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(fs::read_to_string(args_file).unwrap(), "app-server proxy\n");
}

#[test]
fn idle_connection_is_reclaimed_after_cancelable_drain() {
    let temp = tempfile::tempdir().unwrap();
    let fake = make_fake(
        &temp,
        "#!/bin/sh\ntrap '' TERM\nwhile :; do sleep 60; done\n",
    );
    let mut entry = managed_entry(&temp, &fake)
        .env("CODEX_MANAGED_IDLE_SECS", "1")
        .env("CODEX_MANAGED_DRAIN_SECS", "1")
        .spawn()
        .unwrap();
    let mut stdin = entry.stdin.take().unwrap();
    stdin.flush().unwrap();
    wait_for_exit(&mut entry, Duration::from_secs(5));
    let log = fs::read_to_string(temp.path().join(".codex-managed/log/supervisor.jsonl")).unwrap();
    assert!(log.contains("idle_drain_started"));
    assert!(log.contains("idle_reclaim"));
}

#[test]
fn desktop_background_polling_does_not_prevent_idle_reclaim() {
    let temp = tempfile::tempdir().unwrap();
    let fake = make_fake(
        &temp,
        "#!/bin/sh\ntrap '' TERM\nwhile :; do sleep 60; done\n",
    );
    let mut entry = managed_entry(&temp, &fake)
        .env("CODEX_MANAGED_IDLE_SECS", "1")
        .env("CODEX_MANAGED_DRAIN_SECS", "1")
        .spawn()
        .unwrap();
    let mut stdin = entry.stdin.take().unwrap();
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        if entry.try_wait().unwrap().is_some() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Desktop background polling kept the managed entry alive"
        );
        stdin
            .write_all(b"{\"id\":1,\"method\":\"thread/list\",\"params\":{}}\n")
            .unwrap();
        stdin.flush().unwrap();
        thread::sleep(Duration::from_millis(100));
    }
    let log = fs::read_to_string(temp.path().join(".codex-managed/log/supervisor.jsonl")).unwrap();
    assert!(log.contains("idle_drain_started"));
    assert!(log.contains("idle_reclaim"));
}

#[test]
fn archiving_last_thread_reclaims_connection() {
    let temp = tempfile::tempdir().unwrap();
    let fake = make_fake(
        &temp,
        "#!/bin/sh\nIFS= read -r first\nprintf '%s\\n' '{\"id\":1,\"result\":{\"thread\":{\"id\":\"thread-a\"}}}'\nIFS= read -r second\nprintf '%s\\n' '{\"id\":2,\"result\":{}}'\ntrap '' TERM\nwhile :; do sleep 60; done\n",
    );
    let mut entry = managed_entry(&temp, &fake)
        .env("CODEX_MANAGED_IDLE_SECS", "100")
        .env("CODEX_MANAGED_DRAIN_SECS", "1")
        .spawn()
        .unwrap();
    let mut stdin = entry.stdin.take().unwrap();
    stdin
        .write_all(
            b"{\"id\":1,\"method\":\"thread/resume\",\"params\":{\"threadId\":\"thread-a\"}}\n",
        )
        .unwrap();
    stdin
        .write_all(
            b"{\"id\":2,\"method\":\"thread/archive\",\"params\":{\"threadId\":\"thread-a\"}}\n",
        )
        .unwrap();
    stdin.flush().unwrap();
    wait_for_exit(&mut entry, Duration::from_secs(5));
    let log = fs::read_to_string(temp.path().join(".codex-managed/log/supervisor.jsonl")).unwrap();
    assert!(log.contains("archive_drain_started"));
    assert!(log.contains("archive_reclaim"));
}

fn make_fake(temp: &tempfile::TempDir, body: &str) -> std::path::PathBuf {
    let fake = temp.path().join("codex");
    fs::write(&fake, body).unwrap();
    let mut permissions = fs::metadata(&fake).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&fake, permissions).unwrap();
    fake
}

fn managed_entry(temp: &tempfile::TempDir, fake: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_codex-managed-entry"));
    command
        .env("HOME", temp.path())
        .env("CODEX_MANAGED_CODEX_BIN", fake)
        .env("CODEX_MANAGED_TERM_GRACE_SECS", "1")
        .env("CODEX_MANAGED_USE_EXISTING_DAEMON", "1")
        .env("CODEX_MANAGED_SAMPLE_SECS", "60")
        .env("CODEX_MANAGED_FD_WARN", "10000")
        .env("CODEX_MANAGED_FD_RECYCLE", "10001")
        .env("CODEX_MANAGED_FD_HARD", "10002")
        .env(
            "SSH_ORIGINAL_COMMAND",
            "printf '%b' '\\001\\002\\003\\004\\005\\006\\007\\010'; exec codex app-server proxy",
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

fn wait_for_exit(child: &mut std::process::Child, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        if child.try_wait().unwrap().is_some() {
            return;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            panic!("managed entry did not exit before deadline");
        }
        thread::sleep(Duration::from_millis(50));
    }
}
