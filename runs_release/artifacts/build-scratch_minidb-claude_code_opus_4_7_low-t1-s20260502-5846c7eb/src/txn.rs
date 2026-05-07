//! Transaction manager.
//!
//! The model is a single-writer "no-steal, force-at-commit" scheme:
//! - During a transaction, changes are applied to the in-memory pager and
//!   indexes; before-images of touched pages and the pre-transaction catalog
//!   and index snapshots are kept so that ROLLBACK can restore them.
//! - At COMMIT, all logical operations performed during the txn are appended
//!   to the WAL with a final Commit record, then the WAL is fsynced, then
//!   the pager is flushed (force), and finally the WAL is truncated.

use crate::catalog::Catalog;
use crate::error::Result;
use crate::storage::btree::BTreeIndex;
use crate::storage::page::PageId;
use crate::storage::wal::WalRecord;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxnState {
    None,
    Active,
}

#[derive(Debug)]
pub struct Transaction {
    pub id: u64,
    pub explicit: bool,
    pub state: TxnState,
    pub records: Vec<WalRecord>,
    pub before_pages: HashMap<PageId, Vec<u8>>,
    pub before_catalog: Option<Catalog>,
    pub before_indexes: HashMap<String, BTreeIndex>,
    pub before_next_page: u64,
}

pub struct TransactionManager {
    next_id: u64,
    current: Option<Transaction>,
}

impl Default for TransactionManager {
    fn default() -> Self {
        TransactionManager {
            next_id: 1,
            current: None,
        }
    }
}

impl TransactionManager {
    pub fn new() -> Self {
        TransactionManager::default()
    }

    pub fn in_transaction(&self) -> bool {
        self.current.is_some()
    }

    pub fn current(&self) -> Option<&Transaction> {
        self.current.as_ref()
    }

    pub fn current_mut(&mut self) -> Option<&mut Transaction> {
        self.current.as_mut()
    }

    pub fn explicit(&self) -> bool {
        self.current.as_ref().map_or(false, |t| t.explicit)
    }

    pub fn begin(
        &mut self,
        explicit: bool,
        catalog: &Catalog,
        indexes: &std::collections::HashMap<String, BTreeIndex>,
        next_page: u64,
    ) -> Result<u64> {
        if self.current.is_some() {
            return Err(crate::error::MiniDbError::Runtime(
                "a transaction is already active".into(),
            ));
        }
        let id = self.next_id;
        self.next_id += 1;
        self.current = Some(Transaction {
            id,
            explicit,
            state: TxnState::Active,
            records: vec![WalRecord::Begin { txn_id: id }],
            before_pages: HashMap::new(),
            before_catalog: Some(catalog.clone()),
            before_indexes: indexes.clone(),
            before_next_page: next_page,
        });
        Ok(id)
    }

    pub fn record(&mut self, rec: WalRecord) {
        if let Some(t) = self.current.as_mut() {
            t.records.push(rec);
        }
    }

    pub fn snapshot_page_if_needed(&mut self, page_id: PageId, page_data: &[u8]) {
        if let Some(t) = self.current.as_mut() {
            t.before_pages
                .entry(page_id)
                .or_insert_with(|| page_data.to_vec());
        }
    }

    pub fn snapshot_index_if_needed(&mut self, name: &str, idx: &BTreeIndex) {
        if let Some(t) = self.current.as_mut() {
            t.before_indexes
                .entry(name.to_string())
                .or_insert_with(|| idx.clone());
        }
    }

    /// Take ownership of the active transaction (used by commit/rollback).
    pub fn take(&mut self) -> Option<Transaction> {
        self.current.take()
    }
}
