use codex_managed_channel::reclaim::{
    ReclaimAction, ReclaimController, ReclaimReason, ReclaimSnapshot,
};
use std::time::Duration;

fn snapshot(client_idle: u64, generation: u64, archive_epoch: u64) -> ReclaimSnapshot {
    ReclaimSnapshot {
        active_turns: 0,
        reliable: true,
        client_activity_generation: generation,
        client_idle: Duration::from_secs(client_idle),
        archive_empty_epoch: archive_epoch,
    }
}

#[test]
fn archive_starts_cancelable_drain_then_reclaims() {
    let mut controller = ReclaimController::new(Duration::from_secs(600), Duration::from_secs(3));
    assert_eq!(
        controller.evaluate(Duration::ZERO, snapshot(0, 7, 1)),
        ReclaimAction::DrainStarted(ReclaimReason::Archive)
    );
    assert_eq!(
        controller.evaluate(Duration::from_secs(2), snapshot(2, 7, 1)),
        ReclaimAction::None
    );
    assert_eq!(
        controller.evaluate(Duration::from_secs(3), snapshot(3, 7, 1)),
        ReclaimAction::Reclaim(ReclaimReason::Archive)
    );
}

#[test]
fn new_client_activity_cancels_drain() {
    let mut controller = ReclaimController::new(Duration::from_secs(600), Duration::from_secs(3));
    controller.evaluate(Duration::ZERO, snapshot(0, 7, 1));
    assert_eq!(
        controller.evaluate(Duration::from_secs(1), snapshot(0, 8, 1)),
        ReclaimAction::DrainCancelled(ReclaimReason::Archive)
    );
}

#[test]
fn ten_minute_idle_drains_and_server_activity_is_irrelevant() {
    let mut controller = ReclaimController::new(Duration::from_secs(600), Duration::from_secs(3));
    assert_eq!(
        controller.evaluate(Duration::ZERO, snapshot(599, 4, 0)),
        ReclaimAction::None
    );
    assert_eq!(
        controller.evaluate(Duration::from_secs(1), snapshot(600, 4, 0)),
        ReclaimAction::DrainStarted(ReclaimReason::Idle)
    );
    assert_eq!(
        controller.evaluate(Duration::from_secs(4), snapshot(603, 4, 0)),
        ReclaimAction::Reclaim(ReclaimReason::Idle)
    );
}

#[test]
fn active_turn_or_unreliable_protocol_never_starts_reclaim() {
    let mut controller = ReclaimController::new(Duration::from_secs(600), Duration::from_secs(3));
    let mut busy = snapshot(900, 1, 1);
    busy.active_turns = 1;
    assert_eq!(
        controller.evaluate(Duration::ZERO, busy),
        ReclaimAction::None
    );
    let mut unreliable = snapshot(900, 1, 2);
    unreliable.reliable = false;
    assert_eq!(
        controller.evaluate(Duration::from_secs(1), unreliable),
        ReclaimAction::None
    );
}

#[test]
fn archive_signal_is_deferred_until_the_active_turn_finishes() {
    let mut controller = ReclaimController::new(Duration::from_secs(600), Duration::from_secs(3));
    let mut busy = snapshot(0, 4, 1);
    busy.active_turns = 1;
    assert_eq!(
        controller.evaluate(Duration::ZERO, busy),
        ReclaimAction::None
    );
    assert_eq!(
        controller.evaluate(Duration::from_secs(1), snapshot(1, 4, 1)),
        ReclaimAction::DrainStarted(ReclaimReason::Archive)
    );
}
