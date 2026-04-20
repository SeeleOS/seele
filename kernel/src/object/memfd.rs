use core::{
    any::Any,
    sync::atomic::{AtomicU64, Ordering},
};

use alloc::{collections::BTreeMap, string::String, sync::Arc, vec::Vec};
use spin::Mutex;

use crate::{
    filesystem::{
        errors::FSError,
        info::{FileLikeInfo, UnixPermission},
        object::FileLikeObject,
        path::Path,
        vfs::{FSResult, WrappedFile},
        vfs_traits::{File, FileLike, FileLikeType, Whence},
    },
    object::misc::ObjectRef,
    systemcall::utils::{SyscallError, SyscallResult},
};

#[derive(Clone, Copy, Debug)]
struct MemFdState {
    seals: u32,
    allow_sealing: bool,
}

#[derive(Default)]
struct MemFdRegistry {
    states: BTreeMap<String, MemFdState>,
}

struct MemFdFile {
    name: String,
    inode: u64,
    offset: usize,
    path: Path,
    data: Vec<u8>,
}

const F_SEAL_SEAL: u32 = 0x0001;
const F_SEAL_WRITE: u32 = 0x0008;
const SUPPORTED_MEMFD_SEALS: u32 = 0x001f;

static NEXT_MEMFD_INODE: AtomicU64 = AtomicU64::new(1);

lazy_static::lazy_static! {
    static ref MEMFD_REGISTRY: Mutex<MemFdRegistry> = Mutex::new(MemFdRegistry::default());
}

impl MemFdFile {
    fn new(name: String, inode: u64, path: Path) -> Self {
        Self {
            name,
            inode,
            offset: 0,
            path,
            data: Vec::new(),
        }
    }

    fn current_seals(&self) -> u32 {
        memfd_get_seals(&self.path).unwrap_or(0)
    }
}

impl File for MemFdFile {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn info(&mut self) -> FSResult<FileLikeInfo> {
        Ok(FileLikeInfo::new(
            self.name.clone(),
            self.data.len(),
            UnixPermission(0o600),
            FileLikeType::File,
        )
        .with_inode(self.inode))
    }

    fn read_at(&mut self, buffer: &mut [u8], offset: u64) -> FSResult<usize> {
        let offset = usize::try_from(offset).map_err(|_| FSError::Other)?;
        if offset >= self.data.len() {
            return Ok(0);
        }

        let len = buffer.len().min(self.data.len() - offset);
        buffer[..len].copy_from_slice(&self.data[offset..offset + len]);
        Ok(len)
    }

    fn read(&mut self, buffer: &mut [u8]) -> FSResult<usize> {
        let read = self.read_at(buffer, self.offset as u64)?;
        self.offset = self.offset.saturating_add(read);
        Ok(read)
    }

    fn write(&mut self, buffer: &[u8]) -> FSResult<usize> {
        if (self.current_seals() & F_SEAL_WRITE) != 0 {
            return Err(FSError::AccessDenied);
        }

        let end = self
            .offset
            .checked_add(buffer.len())
            .ok_or(FSError::Other)?;
        if self.offset > self.data.len() {
            self.data.resize(self.offset, 0);
        }
        if end > self.data.len() {
            self.data.resize(end, 0);
        }

        self.data[self.offset..end].copy_from_slice(buffer);
        self.offset = end;
        Ok(buffer.len())
    }

    fn seek(&mut self, offset: i64, seek_type: Whence) -> FSResult<usize> {
        let base = match seek_type {
            Whence::Start => 0i64,
            Whence::Current => self.offset as i64,
            Whence::End => self.data.len() as i64,
        };
        let next = base.checked_add(offset).ok_or(FSError::Other)?;
        self.offset = next.max(0) as usize;
        Ok(self.offset)
    }
}

fn memfd_key(path: &Path) -> String {
    path.clone().normalize().as_string()
}

pub fn register_memfd(path: &Path, allow_sealing: bool) {
    MEMFD_REGISTRY.lock().states.insert(
        memfd_key(path),
        MemFdState {
            seals: if allow_sealing { 0 } else { F_SEAL_SEAL },
            allow_sealing,
        },
    );
}

pub fn memfd_get_seals(path: &Path) -> Option<u32> {
    MEMFD_REGISTRY
        .lock()
        .states
        .get(&memfd_key(path))
        .map(|state| state.seals)
}

pub fn memfd_add_seals(path: &Path, seals: u32) -> SyscallResult {
    if (seals & !SUPPORTED_MEMFD_SEALS) != 0 {
        return Err(SyscallError::InvalidArguments);
    }

    let mut registry = MEMFD_REGISTRY.lock();
    let state = registry
        .states
        .get_mut(&memfd_key(path))
        .ok_or(SyscallError::InvalidArguments)?;

    if !state.allow_sealing || (state.seals & F_SEAL_SEAL) != 0 {
        return Err(SyscallError::PermissionDenied);
    }

    state.seals |= seals;
    Ok(0)
}

pub fn create_memfd_object(path: Path, name: String, allow_sealing: bool) -> ObjectRef {
    register_memfd(&path, allow_sealing);

    let inode = NEXT_MEMFD_INODE.fetch_add(1, Ordering::Relaxed);
    let file: WrappedFile = Arc::new(Mutex::new(MemFdFile::new(name, inode, path.clone())));
    Arc::new(FileLikeObject::new(FileLike::File(file), path)) as ObjectRef
}
