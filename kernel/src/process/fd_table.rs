use alloc::{sync::Arc, vec::Vec};
use spin::Mutex;

use crate::process::FdEntry;

pub type FdTable = Vec<Option<FdEntry>>;
pub type FdTableRef = Arc<Mutex<FdTable>>;

pub fn new_fd_table() -> FdTableRef {
    Arc::new(Mutex::new(Vec::new()))
}

pub fn clone_fd_table(fd_table: &FdTableRef) -> FdTableRef {
    Arc::new(Mutex::new(fd_table.lock().clone()))
}
