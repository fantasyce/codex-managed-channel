use codex_managed_channel::fd_guard::{FdAction, FdPolicy};
use std::time::Duration;

#[test]
fn applies_warn_idle_recycle_and_hard_limits() {
    let p = FdPolicy {
        warn: 160,
        recycle: 192,
        hard: 240,
        idle_for: Duration::from_secs(300),
    };
    assert_eq!(
        p.action(159, 0, Duration::from_secs(999)),
        FdAction::Healthy
    );
    assert_eq!(p.action(160, 1, Duration::from_secs(999)), FdAction::Warn);
    assert_eq!(p.action(192, 1, Duration::from_secs(999)), FdAction::Warn);
    assert_eq!(p.action(192, 0, Duration::from_secs(299)), FdAction::Warn);
    assert_eq!(
        p.action(192, 0, Duration::from_secs(300)),
        FdAction::Recycle
    );
    assert_eq!(p.action(240, 4, Duration::ZERO), FdAction::HardStop);
}

#[test]
fn rejects_invalid_threshold_order() {
    assert!(FdPolicy::new(192, 160, 240, Duration::ZERO).is_err());
}
