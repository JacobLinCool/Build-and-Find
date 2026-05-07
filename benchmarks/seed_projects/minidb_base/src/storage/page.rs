//! Fixed-size page abstractions used by heap files and the B-tree.

use crate::error::{MiniDbError, Result};
use serde::{Deserialize, Serialize};

/// Fixed compile-time page size in bytes. This matches common OS page sizes
/// and keeps the on-disk format simple.
pub const PAGE_SIZE: usize = 4096;

/// Strongly typed page number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PageId(pub u64);

impl PageId {
    pub const INVALID: PageId = PageId(u64::MAX);

    pub fn raw(self) -> u64 {
        self.0
    }

    pub fn is_valid(self) -> bool {
        self.0 != u64::MAX
    }
}

impl std::fmt::Display for PageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "page#{}", self.0)
    }
}

/// In-memory representation of a page. Contents are an opaque byte array;
/// higher-level code (heap / btree) is responsible for interpretation.
#[derive(Debug, Clone)]
pub struct Page {
    pub id: PageId,
    pub data: Box<[u8; PAGE_SIZE]>,
    pub dirty: bool,
}

impl Page {
    pub fn empty(id: PageId) -> Self {
        Self {
            id,
            data: Box::new([0u8; PAGE_SIZE]),
            dirty: false,
        }
    }

    pub fn from_bytes(id: PageId, bytes: &[u8]) -> Result<Self> {
        if bytes.len() != PAGE_SIZE {
            return Err(MiniDbError::Storage(format!(
                "expected {PAGE_SIZE} bytes, got {}",
                bytes.len()
            )));
        }
        let mut buf = [0u8; PAGE_SIZE];
        buf.copy_from_slice(bytes);
        Ok(Self {
            id,
            data: Box::new(buf),
            dirty: false,
        })
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_size_is_4096() {
        assert_eq!(PAGE_SIZE, 4096);
    }

    #[test]
    fn pageid_invalid_is_max() {
        assert!(!PageId::INVALID.is_valid());
        assert!(PageId(0).is_valid());
    }
}
