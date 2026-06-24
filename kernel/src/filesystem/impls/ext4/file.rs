use alloc::{string::String, vec::Vec};
use core::any::Any;

use ext4plus::{Ext4, file, inode::Inode};

use crate::filesystem::{
    errors::FSError,
    impls::ext4::{
        LookupCache, OperationLock, chmod_inode, chown_inode, duration_from_parts, inode_times,
        lookup_cache_insert_raw, set_inode_times,
    },
    info::{FileLikeInfo, FileTimes, UnixPermission},
    vfs::FSResult,
    vfs_traits::{File, FileLikeType, Whence},
};
use crate::memory::utils::Mut;

pub struct Ext4File {
    name: String,
    fs: Ext4,
    inode: Mut<Inode>,
    position: u64,
    parent_inode: u32,
    lookup_cache: LookupCache,
    operation_lock: OperationLock,
}

impl Ext4File {
    pub fn new(
        name: String,
        fs: Ext4,
        inode: Inode,
        parent_inode: u32,
        lookup_cache: LookupCache,
        operation_lock: OperationLock,
    ) -> Self {
        Self {
            name,
            fs,
            inode: Mut::new(inode),
            position: 0,
            parent_inode,
            lookup_cache,
            operation_lock,
        }
    }

    fn size_from_inode(inode: &Inode) -> Result<usize, FSError> {
        let meta = inode.metadata();
        Ok(usize::try_from(meta.len()).unwrap())
    }

    pub fn inode(&self) -> Inode {
        self.inode.lock().clone()
    }

    fn update_lookup_cache(&self) {
        lookup_cache_insert_raw(
            &self.lookup_cache,
            self.parent_inode,
            &self.name,
            &self.inode.lock(),
        );
    }

    fn refresh_inode(&self) -> FSResult<Inode> {
        let inode_index = self.inode.lock().index;
        Inode::read(&self.fs, inode_index).map_err(FSError::from)
    }

    fn replace_cached_inode(&self, inode: Inode) {
        *self.inode.lock() = inode;
    }

    fn set_xattr_impl(
        &self,
        name: String,
        value: Vec<u8>,
        create: bool,
        replace: bool,
    ) -> FSResult<()> {
        let mut inode = self.refresh_inode()?;
        let exists = inode
            .get_xattr(&self.fs, &name)
            .map_err(FSError::from)?
            .is_some();
        if create && exists {
            return Err(FSError::AlreadyExists);
        }
        if replace && !exists {
            return Err(FSError::NotFound);
        }
        inode
            .set_xattr(&self.fs, name.as_bytes(), value.as_slice())
            .map_err(FSError::from)?;
        self.replace_cached_inode(inode);
        self.update_lookup_cache();
        Ok(())
    }
}

impl File for Ext4File {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn read(&mut self, buffer: &mut [u8]) -> FSResult<usize> {
        let _operation = self.operation_lock.lock();
        let mut inode = self.refresh_inode()?;
        let read = file::read_at(&self.fs, &inode, buffer, self.position).map_err(FSError::from)?;
        let now = FileTimes::now();
        inode.set_atime(duration_from_parts(now.atime_sec, now.atime_nsec)?);
        inode.write(&self.fs).map_err(FSError::from)?;
        self.replace_cached_inode(inode);
        self.update_lookup_cache();
        self.position = self.position.saturating_add(read as u64);
        Ok(read)
    }

    fn write(&mut self, buffer: &[u8]) -> FSResult<usize> {
        let _operation = self.operation_lock.lock();
        let mut inode = self.refresh_inode()?;
        let written =
            file::write_at(&self.fs, &mut inode, buffer, self.position).map_err(FSError::from)?;
        if written != 0 {
            let now = FileTimes::now();
            let duration = duration_from_parts(now.mtime_sec, now.mtime_nsec)?;
            inode.set_mtime(duration);
            inode.set_ctime(duration);
            inode.write(&self.fs).map_err(FSError::from)?;
        }
        self.position = self.position.saturating_add(written as u64);
        self.replace_cached_inode(inode);
        self.update_lookup_cache();
        Ok(written)
    }

    fn read_at(&mut self, buffer: &mut [u8], offset: u64) -> FSResult<usize> {
        let _operation = self.operation_lock.lock();
        let mut inode = self.refresh_inode()?;
        let read = file::read_at(&self.fs, &inode, buffer, offset).map_err(FSError::from)?;
        let now = FileTimes::now();
        inode.set_atime(duration_from_parts(now.atime_sec, now.atime_nsec)?);
        inode.write(&self.fs).map_err(FSError::from)?;
        self.replace_cached_inode(inode);
        self.update_lookup_cache();
        Ok(read)
    }

    fn info(&mut self) -> FSResult<FileLikeInfo> {
        let inode = self.refresh_inode()?;
        let size = Self::size_from_inode(&inode)?;
        Ok(FileLikeInfo::new(
            self.name.clone(),
            size,
            UnixPermission(inode.mode().bits() as u32),
            FileLikeType::File,
        )
        .with_owner(inode.uid(), inode.gid())
        .with_inode(inode.index.get().into())
        .with_times(inode_times(&inode)))
    }

    fn seek(&mut self, offset: i64, seek_type: Whence) -> FSResult<usize> {
        let len = self.inode.lock().size_in_bytes() as i64;
        let pos = match seek_type {
            Whence::Start => offset,
            Whence::Current => self.position as i64 + offset,
            Whence::End => len + offset,
            Whence::Data => {
                if offset < 0 || offset >= len {
                    return Err(FSError::Other);
                }
                offset
            }
            Whence::Hole => {
                if offset < 0 || offset > len {
                    return Err(FSError::Other);
                }
                len
            }
        };

        if pos < 0 {
            return Err(FSError::InvalidArguments);
        }
        self.position = pos as u64;

        Ok(self.position as usize)
    }

    fn truncate(&mut self, length: u64) -> FSResult<()> {
        let _operation = self.operation_lock.lock();
        let mut inode = self.refresh_inode()?;
        let old_length = inode.size_in_bytes();
        const ZERO_TAIL_GRANULE: u64 = 4096;
        if length < old_length && !length.is_multiple_of(ZERO_TAIL_GRANULE) {
            let zeroes = [0u8; ZERO_TAIL_GRANULE as usize];
            let tail_end = old_length.min(length.next_multiple_of(ZERO_TAIL_GRANULE));
            let mut offset = length;
            while offset < tail_end {
                let chunk_len = usize::try_from((tail_end - offset).min(zeroes.len() as u64))
                    .map_err(|_| FSError::Other)?;
                file::write_at(&self.fs, &mut inode, &zeroes[..chunk_len], offset)
                    .map_err(FSError::from)?;
                offset += chunk_len as u64;
            }
        }
        file::truncate(&self.fs, &mut inode, length).map_err(FSError::from)?;
        self.position = self.position.min(length);
        self.replace_cached_inode(inode);
        self.update_lookup_cache();
        Ok(())
    }

    fn allocate(&mut self, mode: u32, offset: u64, len: u64) -> FSResult<()> {
        let _operation = self.operation_lock.lock();
        if mode != 0 {
            return Err(FSError::Other);
        }

        let end = offset.checked_add(len).ok_or(FSError::Other)?;
        let mut inode = self.refresh_inode()?;
        if end > inode.size_in_bytes() {
            file::truncate(&self.fs, &mut inode, end).map_err(FSError::from)?;
            self.replace_cached_inode(inode);
            self.update_lookup_cache();
        }
        Ok(())
    }

    fn chmod(&self, mode: u32) -> FSResult<()> {
        let _operation = self.operation_lock.lock();
        let mut inode = self.refresh_inode()?;
        chmod_inode(&self.fs, &mut inode, mode)?;
        lookup_cache_insert_raw(&self.lookup_cache, self.parent_inode, &self.name, &inode);
        self.replace_cached_inode(inode);
        Ok(())
    }

    fn chown(&self, uid: u32, gid: u32) -> FSResult<()> {
        let _operation = self.operation_lock.lock();
        let mut inode = self.refresh_inode()?;
        chown_inode(&self.fs, &mut inode, uid, gid)?;
        lookup_cache_insert_raw(&self.lookup_cache, self.parent_inode, &self.name, &inode);
        self.replace_cached_inode(inode);
        Ok(())
    }

    fn set_times(&self, times: FileTimes) -> FSResult<()> {
        let _operation = self.operation_lock.lock();
        let mut inode = self.refresh_inode()?;
        set_inode_times(&self.fs, &mut inode, times)?;
        lookup_cache_insert_raw(&self.lookup_cache, self.parent_inode, &self.name, &inode);
        self.replace_cached_inode(inode);
        crate::filesystem::impls::ext4::lookup_cache_clear(&self.lookup_cache);
        Ok(())
    }

    fn get_xattr(&self, name: &str) -> FSResult<Option<Vec<u8>>> {
        self.inode
            .lock()
            .get_xattr(&self.fs, name)
            .map_err(FSError::from)
    }

    fn set_xattr(&self, name: String, value: Vec<u8>, create: bool, replace: bool) -> FSResult<()> {
        let _operation = self.operation_lock.lock();
        self.set_xattr_impl(name, value, create, replace)
    }

    fn list_xattrs(&self) -> FSResult<Vec<String>> {
        self.inode
            .lock()
            .list_xattrs(&self.fs)
            .map(|names| {
                names
                    .into_iter()
                    .map(String::from_utf8)
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| FSError::Other)
            })
            .map_err(FSError::from)?
    }

    fn remove_xattr(&self, name: &str) -> FSResult<()> {
        let _operation = self.operation_lock.lock();
        self.inode
            .lock()
            .remove_xattr(&self.fs, name)
            .map_err(FSError::from)
    }
}
