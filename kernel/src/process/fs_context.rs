use alloc::sync::Arc;
use spin::Mutex;

use crate::filesystem::absolute_path::AbsolutePath;

#[derive(Clone, Debug, Default)]
pub struct FsContext {
    pub current_directory: AbsolutePath,
    pub file_mode_creation_mask: u32,
}

pub type FsContextRef = Arc<Mutex<FsContext>>;

pub fn new_fs_context() -> FsContextRef {
    Arc::new(Mutex::new(FsContext::default()))
}

pub fn clone_fs_context(fs_context: &FsContextRef) -> FsContextRef {
    Arc::new(Mutex::new(fs_context.lock().clone()))
}
