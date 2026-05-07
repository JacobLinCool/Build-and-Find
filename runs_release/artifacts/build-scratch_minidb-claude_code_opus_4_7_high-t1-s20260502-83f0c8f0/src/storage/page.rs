//! Page identifier and constants.

/// On-disk page size, fixed by the storage format.
pub const PAGE_SIZE: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PageId(pub u64);

impl PageId {
    pub fn raw(self) -> u64 {
        self.0
    }
}

/// A heap or btree row identifier: page id + slot index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RowId {
    pub page: PageId,
    pub slot: u16,
}

impl RowId {
    pub fn new(page: PageId, slot: u16) -> Self {
        RowId { page, slot }
    }

    pub fn to_u64(self) -> u128 {
        ((self.page.0 as u128) << 16) | (self.slot as u128)
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
    fn rowid_packs() {
        let r = RowId::new(PageId(7), 9);
        assert_eq!(r.to_u64(), (7u128 << 16) | 9);
    }
}
