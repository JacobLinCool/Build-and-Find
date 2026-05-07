//! Transaction manager.
//!
//! minidb uses a single-writer model: at most one transaction is active at
//! a time. The manager allocates monotonically-increasing transaction ids
//! and tracks the current state machine: Idle → Active → (Committed|Aborted).
//!
//! The actual durability protocol — buffering dirty pages, writing WAL
//! records, and flushing to disk — is implemented by the engine layer; this
//! module provides only the boundary and identifier.

use crate::error::{MiniDbError, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TxnState {
    Idle,
    Active,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TxnId(pub u64);

pub struct TxnManager {
    next_id: u64,
    state: TxnState,
    current: Option<TxnId>,
    /// True when the current transaction was started implicitly (auto-commit)
    /// rather than via an explicit BEGIN.
    auto: bool,
}

impl Default for TxnManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TxnManager {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            state: TxnState::Idle,
            current: None,
            auto: false,
        }
    }

    pub fn state(&self) -> TxnState {
        self.state
    }

    pub fn current(&self) -> Option<TxnId> {
        self.current
    }

    pub fn is_active(&self) -> bool {
        self.state == TxnState::Active
    }

    pub fn is_auto(&self) -> bool {
        self.auto
    }

    pub fn begin(&mut self, auto: bool) -> Result<TxnId> {
        if self.state == TxnState::Active {
            return Err(MiniDbError::txn(
                "cannot BEGIN: a transaction is already active",
            ));
        }
        let id = TxnId(self.next_id);
        self.next_id += 1;
        self.state = TxnState::Active;
        self.current = Some(id);
        self.auto = auto;
        Ok(id)
    }

    pub fn finish(&mut self) -> Result<TxnId> {
        let id = self
            .current
            .ok_or_else(|| MiniDbError::txn("no active transaction"))?;
        self.state = TxnState::Idle;
        self.current = None;
        self.auto = false;
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_transition_correctly() {
        let mut m = TxnManager::new();
        assert_eq!(m.state(), TxnState::Idle);
        let _ = m.begin(false).unwrap();
        assert!(m.is_active());
        assert!(m.begin(false).is_err()); // double begin
        m.finish().unwrap();
        assert_eq!(m.state(), TxnState::Idle);
    }

    #[test]
    fn allocates_increasing_ids() {
        let mut m = TxnManager::new();
        let a = m.begin(true).unwrap();
        m.finish().unwrap();
        let b = m.begin(true).unwrap();
        assert!(b.0 > a.0);
    }

    #[test]
    fn finish_without_begin_errors() {
        let mut m = TxnManager::new();
        assert!(m.finish().is_err());
    }
}
