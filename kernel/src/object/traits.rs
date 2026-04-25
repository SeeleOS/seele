use alloc::sync::Arc;
use x86_64::VirtAddr;

use crate::{
    filesystem::{info::LinuxStat, vfs_traits::Whence},
    memory::protection::Protection,
    object::{FileFlags, Object, config::ConfigurateRequest, misc::ObjectResult},
};

pub trait Writable: Object {
    /// Write the content of [`buffer`] to [`self`]
    fn write(&self, buffer: &[u8]) -> ObjectResult<usize>;
}

pub trait Readable: Object {
    /// Reads the content of [`self`] and write them to [`buffer`]
    fn read(&self, buffer: &mut [u8]) -> ObjectResult<usize>;

    fn read_with_flags(&self, buffer: &mut [u8], _flags: FileFlags) -> ObjectResult<usize> {
        self.read(buffer)
    }
}

pub trait Configuratable: Object {
    fn configure(&self, request: ConfigurateRequest) -> ObjectResult<isize>;
}

pub trait MemoryMappable: Object {
    fn map(
        self: Arc<Self>,
        offset: u64,
        pages: u64,
        protection: Protection,
    ) -> ObjectResult<VirtAddr>;
}

pub trait Seekable: Object {
    fn seek(self: Arc<Self>, offset: i64, seek_type: Whence) -> ObjectResult<usize>;
}

/// Objects that can synthesize a Linux-style stat result for `fstat`/`stat`.
pub trait Statable: Object {
    fn stat(&self) -> LinuxStat;
}
