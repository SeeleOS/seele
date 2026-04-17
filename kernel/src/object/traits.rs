use alloc::sync::Arc;
use seele_sys::permission::Permissions;
use x86_64::VirtAddr;

use crate::{
    filesystem::{info::LinuxStat, vfs_traits::Whence},
    object::{Object, config::ConfigurateRequest, misc::ObjectResult},
};

pub trait Writable: Object {
    /// Write the content of [`buffer`] to [`self`]
    fn write(&self, buffer: &[u8]) -> ObjectResult<usize>;
}

pub trait Readable: Object {
    /// Reads the content of [`self`] and write them to [`buffer`]
    fn read(&self, buffer: &mut [u8]) -> ObjectResult<usize>;
}

pub trait Configuratable: Object {
    fn configure(&self, request: ConfigurateRequest) -> ObjectResult<isize>;
}

pub trait MemoryMappable: Object {
    fn map(
        self: Arc<Self>,
        offset: u64,
        pages: u64,
        permissions: Permissions,
    ) -> ObjectResult<VirtAddr>;
}

pub trait Seekable: Object {
    fn seek(self: Arc<Self>, offset: i64, seek_type: Whence) -> ObjectResult<usize>;
}

/// Objects that can synthesize a Linux-style stat result for `fstat`/`stat`.
pub trait Statable: Object {
    fn stat(&self) -> LinuxStat;
}
