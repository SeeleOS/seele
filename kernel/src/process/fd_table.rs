use crate::memory::utils::Mut;
use alloc::{sync::Arc, vec::Vec};

use crate::process::FdEntry;

pub type FdTable = Vec<Option<FdEntry>>;
pub type FdTableRef = Arc<Mut<FdTable>>;

pub fn new_fd_table() -> FdTableRef {
    Arc::new(Mut::new(Vec::new()))
}

pub fn clone_fd_table(fd_table: &FdTableRef) -> FdTableRef {
    Arc::new(Mut::new(fd_table.lock().clone()))
}
