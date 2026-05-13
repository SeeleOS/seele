use crate::memory::utils::Mut;
use alloc::sync::Arc;

use crate::filesystem::absolute_path::AbsolutePath;

#[derive(Clone, Debug, Default)]
pub struct FsContext {
    pub root_directory: AbsolutePath,
    pub current_directory: AbsolutePath,
    pub file_mode_creation_mask: u32,
}

pub type FsContextRef = Arc<Mut<FsContext>>;

pub fn new_fs_context() -> FsContextRef {
    Arc::new(Mut::new(FsContext::default()))
}

pub fn clone_fs_context(fs_context: &FsContextRef) -> FsContextRef {
    Arc::new(Mut::new(fs_context.lock().clone()))
}
