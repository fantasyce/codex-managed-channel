use codex_managed_channel::protocol::{Direction, ProtocolObserver};

// A terminal notification from a different turn must not make the parent idle.
#[test]
fn unrelated_child_completion_does_not_clear_parent() {
    let observer = ProtocolObserver::default();
    observer.observe(
        Direction::ClientToServer,
        b"{\"id\":1,\"method\":\"turn/start\",\"params\":{\"threadId\":\"parent\"}}\n",
    );
    observer.observe(
        Direction::ServerToClient,
        b"{\"id\":1,\"result\":{\"turn\":{\"id\":\"parent-turn\",\"status\":\"inProgress\"}}}\n",
    );
    observer.observe(Direction::ServerToClient, b"{\"method\":\"turn/completed\",\"params\":{\"threadId\":\"child\",\"turn\":{\"id\":\"child-turn\",\"status\":\"completed\"}}}\n");
    assert_eq!(
        observer.active_turns(),
        1,
        "unrelated completion must not authorize idle reclaim"
    );
}

// Resuming a running thread must protect it even without a local turn/start.
#[test]
fn resumed_in_progress_turn_is_active() {
    let observer = ProtocolObserver::default();
    observer.observe(
        Direction::ClientToServer,
        b"{\"id\":2,\"method\":\"thread/resume\",\"params\":{\"threadId\":\"parent\"}}\n",
    );
    observer.observe(Direction::ServerToClient, b"{\"id\":2,\"result\":{\"thread\":{\"id\":\"parent\",\"status\":{\"type\":\"active\",\"activeFlags\":[]},\"turns\":[{\"id\":\"parent-turn\",\"status\":\"inProgress\"}]}}}\n");
    assert_eq!(
        observer.active_turns(),
        1,
        "resume must restore runtime activity accounting"
    );
}

#[test]
fn server_started_turn_is_active_without_client_start() {
    let observer = ProtocolObserver::default();
    observer.observe(Direction::ServerToClient, b"{\"method\":\"turn/started\",\"params\":{\"threadId\":\"child\",\"turn\":{\"id\":\"child-turn\",\"status\":\"inProgress\"}}}\n");
    assert_eq!(
        observer.active_turns(),
        1,
        "server-started work must not be reclaimed as idle"
    );
}

#[test]
fn unknown_terminal_cannot_authorize_idle_reclaim() {
    let observer = ProtocolObserver::default();
    observer.observe(
        Direction::ServerToClient,
        b"{\"method\":\"turn/completed\",\"params\":{}}\n",
    );
    assert!(!observer.snapshot().reliable);
}

#[test]
fn completion_before_start_response_is_not_resurrected() {
    let observer = ProtocolObserver::default();
    observer.observe(
        Direction::ClientToServer,
        b"{\"id\":1,\"method\":\"turn/start\",\"params\":{\"threadId\":\"t\"}}\n",
    );
    observer.observe(Direction::ServerToClient, b"{\"method\":\"turn/completed\",\"params\":{\"threadId\":\"t\",\"turn\":{\"id\":\"a\",\"status\":\"completed\"}}}\n");
    observer.observe(
        Direction::ServerToClient,
        b"{\"id\":1,\"result\":{\"turn\":{\"id\":\"a\",\"status\":\"inProgress\"}}}\n",
    );
    assert_eq!(observer.active_turns(), 0);
}

#[test]
fn duplicate_completion_cannot_clear_another_turn() {
    let observer = ProtocolObserver::default();
    for id in ["a", "b"] {
        observer.observe(Direction::ServerToClient, format!("{{\"method\":\"turn/started\",\"params\":{{\"threadId\":\"t\",\"turn\":{{\"id\":\"{id}\",\"status\":\"inProgress\"}}}}}}\n").as_bytes());
    }
    for _ in 0..2 {
        observer.observe(Direction::ServerToClient, b"{\"method\":\"turn/completed\",\"params\":{\"threadId\":\"t\",\"turn\":{\"id\":\"a\",\"status\":\"completed\"}}}\n");
    }
    assert_eq!(observer.active_turns(), 1);
}

#[test]
fn invalid_turn_start_without_request_id_fails_closed() {
    let observer = ProtocolObserver::default();
    observer.observe(
        Direction::ClientToServer,
        b"{\"method\":\"turn/start\",\"params\":{\"threadId\":\"t\"}}\n",
    );
    assert!(!observer.snapshot().reliable);
}

#[test]
fn pending_command_is_work_even_without_a_thread() {
    let observer = ProtocolObserver::default();
    observer.observe(
        Direction::ClientToServer,
        b"{\"id\":3,\"method\":\"command/exec\",\"params\":{\"command\":[\"sleep\",\"1000\"]}}\n",
    );
    assert_eq!(observer.active_turns(), 1);
    observer.observe(
        Direction::ServerToClient,
        b"{\"id\":3,\"result\":{\"exitCode\":0}}\n",
    );
    assert_eq!(observer.active_turns(), 0);
}
