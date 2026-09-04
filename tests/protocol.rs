use codex_managed_channel::protocol::{Direction, ProtocolObserver};

#[test]
fn observes_turn_lifecycle_and_tolerates_unknown_fields() {
    let observer = ProtocolObserver::default();
    observer.observe(
        Direction::ClientToServer,
        b"{\"id\":1,\"method\":\"turn/start\",\"params\":{},\"future\":true}\n",
    );
    assert_eq!(observer.active_turns(), 1);
    observer.observe(
        Direction::ServerToClient,
        b"{\"method\":\"future/event\",\"params\":{\"x\":1}}\n",
    );
    assert_eq!(observer.active_turns(), 1);
    observer.observe(
        Direction::ServerToClient,
        b"{\"method\":\"turn/completed\",\"params\":{},\"extra\":42}\n",
    );
    assert_eq!(observer.active_turns(), 0);
}

#[test]
fn passthrough_never_changes_bytes() {
    let observer = ProtocolObserver::default();
    let bytes = b"{\"id\":7,\"method\":\"initialize\"}\nraw trailing bytes\0";
    assert_eq!(observer.observe(Direction::ClientToServer, bytes), bytes);
}

#[test]
fn complete_resume_queues_original_params_once_without_changing_jsonl_bytes() {
    let observer = ProtocolObserver::default();
    let first = b"{\"id\":7,\"method\":\"thread/resume\",\"params\":{\"threadId\":\"thread-a\",\"model\":\"gpt-test\"}";
    let second = b"}\n";

    assert_eq!(observer.observe(Direction::ClientToServer, first), first);
    assert!(observer.take_resume_attempts().is_empty());
    assert_eq!(observer.observe(Direction::ClientToServer, second), second);

    let attempts = observer.take_resume_attempts();
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].thread_id, "thread-a");
    assert_eq!(attempts[0].params["model"], "gpt-test");
    assert!(observer.take_resume_attempts().is_empty());
}

#[test]
fn websocket_resume_queues_only_after_the_complete_frame() {
    let observer = ProtocolObserver::default();
    observer.observe(
        Direction::ClientToServer,
        b"GET / HTTP/1.1\r\nUpgrade: websocket\r\n\r\n",
    );
    let frame = websocket_text(
        br#"{"id":"resume-1","method":"thread/resume","params":{"threadId":"thread-ws","excludeTurns":true}}"#,
        true,
    );
    let split = frame.len() - 3;

    assert_eq!(
        observer.observe(Direction::ClientToServer, &frame[..split]),
        &frame[..split]
    );
    assert!(observer.take_resume_attempts().is_empty());
    assert_eq!(
        observer.observe(Direction::ClientToServer, &frame[split..]),
        &frame[split..]
    );

    let attempts = observer.take_resume_attempts();
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].thread_id, "thread-ws");
    assert_eq!(attempts[0].params["excludeTurns"], true);
}

#[test]
fn malformed_input_does_not_panic_or_change_state() {
    let observer = ProtocolObserver::default();
    observer.observe(Direction::ClientToServer, b"not-json\n");
    assert_eq!(observer.active_turns(), 0);
    assert_eq!(observer.parse_failures(), 1);
    assert!(!observer.snapshot().reliable);
}

#[test]
fn successful_archive_of_last_known_thread_advances_epoch() {
    let observer = ProtocolObserver::default();
    observer.observe(
        Direction::ClientToServer,
        b"{\"id\":1,\"method\":\"thread/resume\",\"params\":{\"threadId\":\"thread-a\"}}\n",
    );
    observer.observe(
        Direction::ServerToClient,
        b"{\"id\":1,\"result\":{\"thread\":{\"id\":\"thread-a\"}}}\n",
    );
    assert_eq!(observer.snapshot().live_threads, 1);

    observer.observe(
        Direction::ClientToServer,
        b"{\"id\":2,\"method\":\"thread/archive\",\"params\":{\"threadId\":\"thread-a\"}}\n",
    );
    observer.observe(Direction::ServerToClient, b"{\"id\":2,\"result\":{}}\n");
    let snapshot = observer.snapshot();
    assert_eq!(snapshot.live_threads, 0);
    assert_eq!(snapshot.archive_empty_epoch, 1);
}

#[test]
fn archive_does_not_trigger_while_another_thread_is_live() {
    let observer = ProtocolObserver::default();
    for (id, thread) in [(1, "thread-a"), (2, "thread-b")] {
        observer.observe(Direction::ClientToServer, format!("{{\"id\":{id},\"method\":\"thread/resume\",\"params\":{{\"threadId\":\"{thread}\"}}}}\n").as_bytes());
        observer.observe(
            Direction::ServerToClient,
            format!("{{\"id\":{id},\"result\":{{\"thread\":{{\"id\":\"{thread}\"}}}}}}\n")
                .as_bytes(),
        );
    }
    observer.observe(
        Direction::ClientToServer,
        b"{\"id\":3,\"method\":\"thread/archive\",\"params\":{\"threadId\":\"thread-a\"}}\n",
    );
    observer.observe(Direction::ServerToClient, b"{\"id\":3,\"result\":{}}\n");
    let snapshot = observer.snapshot();
    assert_eq!(snapshot.live_threads, 1);
    assert_eq!(snapshot.archive_empty_epoch, 0);
}

#[test]
fn failed_archive_keeps_thread_live_and_failed_turn_start_is_not_active() {
    let observer = ProtocolObserver::default();
    observer.observe(
        Direction::ClientToServer,
        b"{\"id\":1,\"method\":\"thread/resume\",\"params\":{\"threadId\":\"thread-a\"}}\n",
    );
    observer.observe(
        Direction::ServerToClient,
        b"{\"id\":1,\"result\":{\"thread\":{\"id\":\"thread-a\"}}}\n",
    );
    observer.observe(
        Direction::ClientToServer,
        b"{\"id\":2,\"method\":\"thread/archive\",\"params\":{\"threadId\":\"thread-a\"}}\n",
    );
    observer.observe(
        Direction::ServerToClient,
        b"{\"id\":2,\"error\":{\"code\":-1,\"message\":\"no\"}}\n",
    );
    observer.observe(
        Direction::ClientToServer,
        b"{\"id\":3,\"method\":\"turn/start\",\"params\":{}}\n",
    );
    observer.observe(
        Direction::ServerToClient,
        b"{\"id\":3,\"error\":{\"code\":-1,\"message\":\"no\"}}\n",
    );
    assert_eq!(observer.snapshot().live_threads, 1);
    assert_eq!(observer.snapshot().archive_empty_epoch, 0);
    assert_eq!(observer.active_turns(), 0);
}

#[test]
fn desktop_background_requests_do_not_advance_user_activity_generation() {
    let observer = ProtocolObserver::default();
    let initial = observer.snapshot().client_activity_generation;
    observer.observe(
        Direction::ServerToClient,
        b"{\"method\":\"future/event\"}\n",
    );
    assert_eq!(observer.snapshot().client_activity_generation, initial);
    observer.observe(Direction::ClientToServer, b"{\"method\":\"initialized\"}\n");
    observer.observe(
        Direction::ClientToServer,
        b"{\"id\":1,\"method\":\"account/rateLimits/read\",\"params\":{}}\n",
    );
    observer.observe(
        Direction::ClientToServer,
        b"{\"id\":2,\"method\":\"thread/list\",\"params\":{}}\n",
    );
    assert_eq!(observer.snapshot().client_activity_generation, initial);
}

#[test]
fn user_intent_requests_advance_client_activity_generation() {
    let observer = ProtocolObserver::default();
    let initial = observer.snapshot().client_activity_generation;
    observer.observe(
        Direction::ClientToServer,
        b"{\"id\":1,\"method\":\"thread/resume\",\"params\":{\"threadId\":\"thread-a\"}}\n",
    );
    assert!(observer.snapshot().client_activity_generation > initial);
}

#[test]
fn observes_lifecycle_inside_websocket_text_frames() {
    let observer = ProtocolObserver::default();
    observer.observe(
        Direction::ClientToServer,
        b"GET / HTTP/1.1\r\nUpgrade: websocket\r\n\r\n",
    );
    observer.observe(
        Direction::ServerToClient,
        b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n\r\n",
    );
    observer.observe(
        Direction::ClientToServer,
        &websocket_text(
            br#"{"id":1,"method":"thread/resume","params":{"threadId":"thread-a"}}"#,
            true,
        ),
    );
    observer.observe(
        Direction::ServerToClient,
        &websocket_text(br#"{"id":1,"result":{"thread":{"id":"thread-a"}}}"#, false),
    );
    observer.observe(
        Direction::ClientToServer,
        &websocket_text(
            br#"{"id":2,"method":"thread/archive","params":{"threadId":"thread-a"}}"#,
            true,
        ),
    );
    observer.observe(
        Direction::ServerToClient,
        &websocket_text(br#"{"id":2,"result":{}}"#, false),
    );
    let snapshot = observer.snapshot();
    assert!(snapshot.reliable);
    assert_eq!(snapshot.live_threads, 0);
    assert_eq!(snapshot.archive_empty_epoch, 1);
}

#[test]
fn multi_megabyte_resume_response_keeps_lifecycle_observation_reliable() {
    let observer = ProtocolObserver::default();
    observer.observe(
        Direction::ClientToServer,
        b"GET / HTTP/1.1\r\nUpgrade: websocket\r\n\r\n",
    );
    observer.observe(
        Direction::ServerToClient,
        b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n\r\n",
    );
    observer.observe(
        Direction::ClientToServer,
        &websocket_text(
            br#"{"id":1,"method":"thread/resume","params":{"threadId":"large-thread"}}"#,
            true,
        ),
    );
    let padding = "x".repeat(2 * 1024 * 1024);
    let response = format!(
        "{{\"id\":1,\"result\":{{\"thread\":{{\"id\":\"large-thread\",\"turns\":[{{\"payload\":\"{padding}\"}}]}}}}}}"
    );
    let frame = websocket_text(response.as_bytes(), false);
    for chunk in frame.chunks(16 * 1024) {
        observer.observe(Direction::ServerToClient, chunk);
    }

    let snapshot = observer.snapshot();
    assert!(snapshot.reliable);
    assert_eq!(snapshot.live_threads, 1);
}

fn websocket_text(payload: &[u8], masked: bool) -> Vec<u8> {
    let mask = [0x11, 0x22, 0x33, 0x44];
    let mut frame = vec![0x81];
    if payload.len() <= 125 {
        frame.push(payload.len() as u8 | if masked { 0x80 } else { 0 });
    } else if payload.len() <= u16::MAX as usize {
        frame.push(126 | if masked { 0x80 } else { 0 });
        frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    } else {
        frame.push(127 | if masked { 0x80 } else { 0 });
        frame.extend_from_slice(&(payload.len() as u64).to_be_bytes());
    }
    if masked {
        frame.extend_from_slice(&mask);
        frame.extend(
            payload
                .iter()
                .enumerate()
                .map(|(index, byte)| byte ^ mask[index % 4]),
        );
    } else {
        frame.extend_from_slice(payload);
    }
    frame
}
