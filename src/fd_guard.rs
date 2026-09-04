use anyhow::{Result, bail};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FdAction {
    Healthy,
    Warn,
    Recycle,
    HardStop,
}

#[derive(Debug, Clone, Copy)]
pub struct FdPolicy {
    pub warn: usize,
    pub recycle: usize,
    pub hard: usize,
    pub idle_for: Duration,
}

impl FdPolicy {
    pub fn new(warn: usize, recycle: usize, hard: usize, idle_for: Duration) -> Result<Self> {
        if warn == 0 || warn >= recycle || recycle >= hard {
            bail!("FD thresholds must satisfy 0 < warn < recycle < hard");
        }
        Ok(Self {
            warn,
            recycle,
            hard,
            idle_for,
        })
    }

    pub fn action(&self, count: usize, active_turns: usize, idle: Duration) -> FdAction {
        if count >= self.hard {
            FdAction::HardStop
        } else if count >= self.recycle && active_turns == 0 && idle >= self.idle_for {
            FdAction::Recycle
        } else if count >= self.warn {
            FdAction::Warn
        } else {
            FdAction::Healthy
        }
    }
}
