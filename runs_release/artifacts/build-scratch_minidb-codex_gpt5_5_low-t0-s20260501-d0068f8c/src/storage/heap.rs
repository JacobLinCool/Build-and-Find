use crate::{
    storage::{RowId, StoredRow},
    types::Row,
};

#[derive(Default)]
pub struct HeapTable {
    pub rows: Vec<StoredRow>,
    pub next: RowId,
}

impl HeapTable {
    pub fn insert(&mut self, values: Row) -> RowId {
        let id = self.next;
        self.next += 1;
        self.rows.push(StoredRow {
            id,
            values,
            deleted: false,
        });
        id
    }
}
