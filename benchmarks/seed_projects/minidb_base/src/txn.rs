//! Transaction manager.
//!
//! minidb supports a single active explicit transaction at a time. Any
//! statement issued outside `BEGIN` / `COMMIT` runs in its own
//! auto-committed transaction. On COMMIT the WAL is fsynced (unless the
//! legacy `fast_mode` flag is on).

use crate::error::{MiniDbError, Result};
use crate::storage::wal::{Wal, WalRecordKind};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Transaction identifier. Monotonically increases for the life of the
/// manager.
pub type TxnId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxnState {
    Idle,
    Active,
    Committed,
    Aborted,
}

/// Global transaction coordinator.
pub struct TxnManager {
    wal: Arc<Wal>,
    next_id: AtomicU64,
    current: std::sync::Mutex<Option<TxnId>>,
    /// Legacy flag. When true, [`Self::commit`] skips the fsync on the
    /// write-ahead log. See `--fast-mode` in the CLI. Kept for backwards
    /// compatibility with older deployments and is **deprecated**.
    pub fast_mode: bool,
}

impl TxnManager {
    pub fn new(wal: Arc<Wal>) -> Self {
        Self {
            wal,
            next_id: AtomicU64::new(1),
            current: std::sync::Mutex::new(None),
            fast_mode: false,
        }
    }

    pub fn with_fast_mode(mut self, on: bool) -> Self {
        self.fast_mode = on;
        self
    }

    pub fn wal(&self) -> &Wal {
        &self.wal
    }

    fn current_lock(&self) -> std::sync::MutexGuard<'_, Option<TxnId>> {
        self.current.lock().expect("txn manager mutex poisoned")
    }

    pub fn current(&self) -> Option<TxnId> {
        *self.current_lock()
    }

    /// Begin a new explicit transaction. Fails if one is already active.
    pub fn begin(&self) -> Result<TxnId> {
        let mut slot = self.current_lock();
        if slot.is_some() {
            return Err(MiniDbError::Transaction(
                "BEGIN: a transaction is already active".into(),
            ));
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        self.wal.append(WalRecordKind::Begin, id, b"")?;
        *slot = Some(id);
        Ok(id)
    }

    /// Commit the active transaction. If `fast_mode` is false the WAL is
    /// fsynced. If `fast_mode` is true the fsync is skipped for speed
    /// (legacy behaviour; documented as deprecated).
    pub fn commit(&self) -> Result<TxnId> {
        let mut slot = self.current_lock();
        let id = slot
            .take()
            .ok_or_else(|| MiniDbError::Transaction("COMMIT without active txn".into()))?;
        self.wal.append(WalRecordKind::Commit, id, b"")?;
        if self.fast_mode {
            // Legacy fast path: skip the durability barrier entirely.
        } else {
            self.wal.fsync()?;
        }
        Ok(id)
    }

    /// Roll back the active transaction.
    pub fn rollback(&self) -> Result<TxnId> {
        let mut slot = self.current_lock();
        let id = slot
            .take()
            .ok_or_else(|| MiniDbError::Transaction("ROLLBACK without active txn".into()))?;
        self.wal.append(WalRecordKind::Rollback, id, b"")?;
        self.wal.fsync()?;
        Ok(id)
    }

    /// Auto-commit helper: if no explicit transaction is active, allocate a
    /// fresh id, run `f`, and commit.
    pub fn autocommit<T>(&self, f: impl FnOnce(TxnId) -> Result<T>) -> Result<T> {
        let explicit = self.current();
        if let Some(id) = explicit {
            return f(id);
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        self.wal.append(WalRecordKind::Begin, id, b"")?;
        let out = f(id)?;
        self.wal.append(WalRecordKind::Commit, id, b"")?;
        if !self.fast_mode {
            self.wal.fsync()?;
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_wal() -> Arc<Wal> {
        let p = std::env::temp_dir().join(format!(
            "minidb_txn_{}_{}.wal",
            std::process::id(),
            rand_like()
        ));
        Arc::new(Wal::open(p).unwrap())
    }

    fn rand_like() -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as u64)
            .unwrap_or(0)
    }

    #[test]
    fn begin_commit_cycle() {
        let mgr = TxnManager::new(tmp_wal());
        let id = mgr.begin().unwrap();
        assert_eq!(mgr.current(), Some(id));
        mgr.commit().unwrap();
        assert!(mgr.current().is_none());
    }

    #[test]
    fn double_begin_errors() {
        let mgr = TxnManager::new(tmp_wal());
        mgr.begin().unwrap();
        assert!(mgr.begin().is_err());
    }

    #[test]
    fn fast_mode_toggle() {
        let mgr = TxnManager::new(tmp_wal()).with_fast_mode(true);
        assert!(mgr.fast_mode);
    }
}
