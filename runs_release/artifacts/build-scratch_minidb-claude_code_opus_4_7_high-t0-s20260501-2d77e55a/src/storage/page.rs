//! Page primitives: typed page identifier and the system-wide page size.

use serde::{Deserialize, Serialize};
use std::fmt;

/// System-wide page size in bytes. Defined here as a single compile-time constant.
pub const PAGE_SIZE: usize = 4096;

/// Typed page identifier. `0` is reserved as a null/sentinel page id.
#[derive(
    Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Ord, PartialOrd, Serialize, Deserialize,
)]
pub struct PageId(pub u64);

impl PageId {
    pub const NULL: PageId = PageId(0);

    pub fn is_null(self) -> bool {
        self.0 == 0
    }
}

impl fmt::Display for PageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "p{}", self.0)
    }
}

/// Identifier of a row within a heap: (page, slot).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
pub struct RowId {
    pub page: PageId,
    pub slot: u16,
}

impl RowId {
    pub fn new(page: PageId, slot: u16) -> Self {
        Self { page, slot }
    }
}

impl fmt::Display for RowId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.page, self.slot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_size_is_4kib() {
        assert_eq!(PAGE_SIZE, 4096);
    }

    #[test]
    fn null_page_id() {
        assert!(PageId::NULL.is_null());
        assert!(!PageId(1).is_null());
    }
}
