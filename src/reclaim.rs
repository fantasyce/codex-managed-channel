use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub struct ReclaimSnapshot {
    pub active_turns: usize,
    pub reliable: bool,
    pub client_activity_generation: u64,
    pub client_idle: Duration,
    pub archive_empty_epoch: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReclaimReason {
    Archive,
    Idle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReclaimAction {
    None,
    DrainStarted(ReclaimReason),
    DrainCancelled(ReclaimReason),
    Reclaim(ReclaimReason),
}

#[derive(Debug, Clone, Copy)]
struct Drain {
    reason: ReclaimReason,
    started_at: Duration,
    client_activity_generation: u64,
}

pub struct ReclaimController {
    idle_after: Duration,
    drain_for: Duration,
    seen_archive_epoch: u64,
    drain: Option<Drain>,
}

impl ReclaimController {
    pub fn new(idle_after: Duration, drain_for: Duration) -> Self {
        Self {
            idle_after,
            drain_for,
            seen_archive_epoch: 0,
            drain: None,
        }
    }

    pub fn evaluate(&mut self, now: Duration, snapshot: ReclaimSnapshot) -> ReclaimAction {
        if let Some(drain) = self.drain {
            if !snapshot.reliable
                || snapshot.active_turns > 0
                || snapshot.client_activity_generation != drain.client_activity_generation
            {
                self.drain = None;
                return ReclaimAction::DrainCancelled(drain.reason);
            }
            if now.saturating_sub(drain.started_at) >= self.drain_for {
                return ReclaimAction::Reclaim(drain.reason);
            }
            return ReclaimAction::None;
        }
        if !snapshot.reliable || snapshot.active_turns > 0 {
            return ReclaimAction::None;
        }
        let unseen_archive = snapshot.archive_empty_epoch > self.seen_archive_epoch;
        self.seen_archive_epoch = self.seen_archive_epoch.max(snapshot.archive_empty_epoch);
        let reason = if unseen_archive {
            Some(ReclaimReason::Archive)
        } else if snapshot.client_idle >= self.idle_after {
            Some(ReclaimReason::Idle)
        } else {
            None
        };
        let Some(reason) = reason else {
            return ReclaimAction::None;
        };
        self.drain = Some(Drain {
            reason,
            started_at: now,
            client_activity_generation: snapshot.client_activity_generation,
        });
        ReclaimAction::DrainStarted(reason)
    }
}
