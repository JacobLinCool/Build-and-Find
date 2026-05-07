use crate::error::{MiniDbError, Result};
use crate::storage::page::{PageId, PageManager};
use crate::storage::wal::{Wal, WalRecord};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct TransactionManager {
    wal: Wal,
    wal_path: PathBuf,
    db_path: PathBuf,
    active_txn: Option<ActiveTransaction>,
    auto_commit: bool,
}

struct ActiveTransaction {
    txn_id: u64,
    dirty_pages: HashMap<PageId, [u8; 4096]>,
    snapshot: Vec<u8>,
}

impl TransactionManager {
    pub fn new(db_path: &Path) -> Result<Self> {
        let wal_path = db_path.with_extension("wal");
        let wal = Wal::open(&wal_path)?;
        Ok(Self {
            wal,
            wal_path,
            db_path: db_path.to_path_buf(),
            active_txn: None,
            auto_commit: true,
        })
    }

    pub fn create(db_path: &Path) -> Result<Self> {
        let wal_path = db_path.with_extension("wal");
        let wal = Wal::create(&wal_path)?;
        Ok(Self {
            wal,
            wal_path,
            db_path: db_path.to_path_buf(),
            active_txn: None,
            auto_commit: true,
        })
    }

    pub fn is_in_transaction(&self) -> bool {
        self.active_txn.is_some()
    }

    pub fn begin(&mut self) -> Result<()> {
        if self.active_txn.is_some() {
            return Err(MiniDbError::Transaction(
                "already in a transaction".to_string(),
            ));
        }
        // Snapshot the database file for rollback
        let snapshot = std::fs::read(&self.db_path).unwrap_or_default();
        self.active_txn = Some(ActiveTransaction {
            txn_id: self.wal.offset(),
            dirty_pages: HashMap::new(),
            snapshot,
        });
        self.auto_commit = false;
        log::debug!("BEGIN transaction");
        Ok(())
    }

    pub fn commit(&mut self, pm: &mut PageManager) -> Result<()> {
        let txn = self
            .active_txn
            .take()
            .ok_or_else(|| MiniDbError::Transaction("no active transaction".to_string()))?;

        // Write dirty pages to WAL
        for (pid, page_data) in &txn.dirty_pages {
            self.wal.append(&WalRecord::page_write(pid.0, page_data))?;
        }

        // Write commit record
        self.wal.append(&WalRecord::commit(txn.txn_id))?;
        self.wal.flush_and_sync()?;

        // Apply dirty pages to main file
        for (pid, page_data) in &txn.dirty_pages {
            pm.write_page(*pid, page_data)?;
        }
        pm.flush()?;

        // Truncate WAL after successful commit
        let wal = Wal::create(&self.wal_path)?;
        self.wal = wal;

        self.auto_commit = true;
        log::debug!("COMMIT transaction");
        Ok(())
    }

    pub fn rollback(&mut self) -> Result<()> {
        let txn = self
            .active_txn
            .take()
            .ok_or_else(|| MiniDbError::Transaction("no active transaction".to_string()))?;

        // Restore database file from snapshot
        if !txn.snapshot.is_empty() {
            std::fs::write(&self.db_path, &txn.snapshot)?;
        }

        self.auto_commit = true;
        log::debug!("ROLLBACK transaction");
        Ok(())
    }

    pub fn buffer_write(&mut self, pid: PageId, page: [u8; 4096]) -> Result<()> {
        if let Some(ref mut txn) = self.active_txn {
            txn.dirty_pages.insert(pid, page);
            Ok(())
        } else {
            // Auto-commit mode: write directly
            Err(MiniDbError::Transaction(
                "write outside transaction (auto-commit not yet supported for this path)"
                    .to_string(),
            ))
        }
    }

    pub fn recover(&self, pm: &mut PageManager) -> Result<()> {
        let mut reader = crate::storage::wal::WalReader::open(&self.wal_path)?;
        let records = reader.read_all()?;

        // Find the last commit record
        let mut last_commit_idx = None;
        for (i, record) in records.iter().enumerate().rev() {
            if record.record_type == crate::storage::wal::WalRecordType::Commit {
                last_commit_idx = Some(i);
                break;
            }
        }

        if let Some(commit_idx) = last_commit_idx {
            // Apply all page writes up to and including the last commit
            for record in &records[..=commit_idx] {
                if record.record_type == crate::storage::wal::WalRecordType::PageWrite {
                    if record.data.len() >= 8 {
                        let pid = u64::from_le_bytes(record.data[0..8].try_into().unwrap());
                        let page_data = &record.data[8..];
                        if page_data.len() == 4096 {
                            let mut page = [0u8; 4096];
                            page.copy_from_slice(page_data);
                            pm.write_page(PageId(pid), &page)?;
                        }
                    }
                }
            }
            pm.flush()?;
            log::info!("WAL recovery: applied committed transactions");
        }

        // Truncate WAL after recovery
        let wal = Wal::create(&self.wal_path)?;
        // We can't mutate self here since we only have &self.
        // The caller should handle this.
        drop(wal);

        Ok(())
    }

    pub fn auto_commit_write(
        &mut self,
        pm: &mut PageManager,
        pid: PageId,
        page: [u8; 4096],
    ) -> Result<()> {
        if self.active_txn.is_some() {
            self.buffer_write(pid, page)?;
        } else {
            // Auto-commit: write to WAL, then to page file
            self.wal.append(&WalRecord::page_write(pid.0, &page))?;
            self.wal.append(&WalRecord::commit(0))?;
            self.wal.flush_and_sync()?;
            pm.write_page(pid, &page)?;
            pm.flush()?;
            // Reset WAL
            self.wal = Wal::create(&self.wal_path)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_txn_begin_commit() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let mut pm = PageManager::create(&db_path).unwrap();
        let mut txn = TransactionManager::create(&db_path).unwrap();

        txn.begin().unwrap();
        assert!(txn.is_in_transaction());
        txn.commit(&mut pm).unwrap();
        assert!(!txn.is_in_transaction());
    }

    #[test]
    fn test_txn_begin_rollback() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let mut pm = PageManager::create(&db_path).unwrap();
        let mut txn = TransactionManager::create(&db_path).unwrap();

        txn.begin().unwrap();
        let pid = pm.alloc_page().unwrap();
        let page = [42u8; 4096];
        txn.buffer_write(pid, page).unwrap();
        txn.rollback().unwrap();
        assert!(!txn.is_in_transaction());
    }

    #[test]
    fn test_txn_no_nested() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let _ = PageManager::create(&db_path).unwrap();
        let mut txn = TransactionManager::create(&db_path).unwrap();

        txn.begin().unwrap();
        assert!(txn.begin().is_err());
    }
}
