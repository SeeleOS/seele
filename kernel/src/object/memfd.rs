use core::{
    any::Any,
    sync::atomic::{AtomicU64, Ordering},
};

use alloc::{collections::BTreeMap, string::String, sync::Arc, vec::Vec};
use bitflags::bitflags;
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

bitflags! {
    #[derive(Clone, Copy, Debug)]
    struct MemFdSealFlags: u32 {
        const F_SEAL_SEAL = 0x0001;
        const F_SEAL_SHRINK = 0x0002;
        const F_SEAL_GROW = 0x0004;
        const F_SEAL_WRITE = 0x0008;
        const F_SEAL_FUTURE_WRITE = 0x0010;
        const SUPPORTED = Self::F_SEAL_SEAL.bits()
            | Self::F_SEAL_SHRINK.bits()
            | Self::F_SEAL_GROW.bits()
            | Self::F_SEAL_WRITE.bits()
            | Self::F_SEAL_FUTURE_WRITE.bits();
    }
}

#[derive(Clone, Copy, Debug)]
struct MemFdState {
    seals: MemFdSealFlags,
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

    fn current_seals(&self) -> MemFdSealFlags {
        memfd_get_seals(&self.path)
            .map(MemFdSealFlags::from_bits_retain)
            .unwrap_or_else(MemFdSealFlags::empty)
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
        if self.current_seals().contains(MemFdSealFlags::F_SEAL_WRITE) {
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

    fn truncate(&mut self, length: u64) -> FSResult<()> {
        let length = usize::try_from(length).map_err(|_| FSError::Other)?;
        let seals = self.current_seals();
        if length < self.data.len() && seals.contains(MemFdSealFlags::F_SEAL_SHRINK) {
            return Err(FSError::AccessDenied);
        }
        if length > self.data.len() && seals.contains(MemFdSealFlags::F_SEAL_GROW) {
            return Err(FSError::AccessDenied);
        }

        self.data.resize(length, 0);
        Ok(())
    }

    fn allocate(&mut self, mode: u32, offset: u64, len: u64) -> FSResult<()> {
        if mode != 0 {
            return Err(FSError::Other);
        }

        let offset = usize::try_from(offset).map_err(|_| FSError::Other)?;
        let len = usize::try_from(len).map_err(|_| FSError::Other)?;
        let end = offset.checked_add(len).ok_or(FSError::Other)?;
        let seals = self.current_seals();
        if end > self.data.len()
            && (seals.contains(MemFdSealFlags::F_SEAL_GROW)
                || seals.contains(MemFdSealFlags::F_SEAL_WRITE))
        {
            return Err(FSError::AccessDenied);
        }

        if end > self.data.len() {
            self.data.resize(end, 0);
        }
        Ok(())
    }
}

fn memfd_key(path: &Path) -> String {
    path.clone().normalize().as_string()
}

pub fn register_memfd(path: &Path, allow_sealing: bool) {
    MEMFD_REGISTRY.lock().states.insert(
        memfd_key(path),
        MemFdState {
            seals: if allow_sealing {
                MemFdSealFlags::empty()
            } else {
                MemFdSealFlags::F_SEAL_SEAL
            },
            allow_sealing,
        },
    );
}

pub fn memfd_get_seals(path: &Path) -> Option<u32> {
    MEMFD_REGISTRY
        .lock()
        .states
        .get(&memfd_key(path))
        .map(|state| state.seals.bits())
}

pub fn memfd_add_seals(path: &Path, seals: u32) -> SyscallResult {
    let seals = MemFdSealFlags::from_bits(seals).ok_or(SyscallError::InvalidArguments)?;
    if seals.bits() & !MemFdSealFlags::SUPPORTED.bits() != 0 {
        return Err(SyscallError::InvalidArguments);
    }

    let mut registry = MEMFD_REGISTRY.lock();
    let state = registry
        .states
        .get_mut(&memfd_key(path))
        .ok_or(SyscallError::InvalidArguments)?;

    if !state.allow_sealing || state.seals.contains(MemFdSealFlags::F_SEAL_SEAL) {
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
