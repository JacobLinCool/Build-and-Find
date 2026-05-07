use crate::error::Result;
use crate::storage::PAGE_SIZE;
use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

const MAGIC: &[u8] = b"MINIDB01";

pub fn ensure_database_file(path: &Path) -> Result<()> {
    if !path.exists() {
        initialize_database_file(path)?;
    }
    Ok(())
}

pub fn initialize_database_file(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .read(true)
        .open(path)?;
    let mut page = vec![0u8; PAGE_SIZE];
    page[..MAGIC.len()].copy_from_slice(MAGIC);
    file.write_all(&page)?;
    file.sync_all()?;
    Ok(())
}

pub fn read_page(path: &Path, page_id: crate::storage::PageId) -> Result<Vec<u8>> {
    let mut file = OpenOptions::new().read(true).open(path)?;
    file.seek(SeekFrom::Start(page_id.0 * PAGE_SIZE as u64))?;
    let mut page = vec![0u8; PAGE_SIZE];
    file.read_exact(&mut page)?;
    Ok(page)
}

pub fn write_page(path: &Path, page_id: crate::storage::PageId, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new().write(true).read(true).open(path)?;
    file.seek(SeekFrom::Start(page_id.0 * PAGE_SIZE as u64))?;
    let mut page = vec![0u8; PAGE_SIZE];
    let len = bytes.len().min(PAGE_SIZE);
    page[..len].copy_from_slice(&bytes[..len]);
    file.write_all(&page)?;
    file.sync_data()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initializes_one_page_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db");
        initialize_database_file(&path).unwrap();
        assert_eq!(std::fs::metadata(path).unwrap().len(), PAGE_SIZE as u64);
    }
}
