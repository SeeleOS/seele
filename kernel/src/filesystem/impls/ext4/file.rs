use alloc::{string::String, vec::Vec};
use core::any::Any;

use ext4plus::{Ext4, file, inode::Inode};

use crate::filesystem::{
    errors::FSError,
    impls::ext4::{LookupCache, OperationLock, chmod_inode, chown_inode, lookup_cache_insert_raw},
    info::{FileLikeInfo, UnixPermission},
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

    fn size(&self) -> Result<usize, FSError> {
        let meta = self.inode.lock().metadata();
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

    fn set_xattr_impl(
        &self,
        name: String,
        value: Vec<u8>,
        create: bool,
        replace: bool,
    ) -> FSResult<()> {
        let mut inode = self.inode.lock();
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
        drop(inode);
        self.update_lookup_cache();
        Ok(())
    }
}

impl File for Ext4File {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn read(&mut self, buffer: &mut [u8]) -> FSResult<usize> {
        let read = file::read_at(&self.fs, &self.inode.lock(), buffer, self.position)
            .map_err(FSError::from)?;
        self.position = self.position.saturating_add(read as u64);
        Ok(read)
    }

    fn write(&mut self, buffer: &[u8]) -> FSResult<usize> {
        let _operation = self.operation_lock.lock();
        let written = file::write_at(&self.fs, &mut self.inode.lock(), buffer, self.position)
            .map_err(FSError::from)?;
        self.position = self.position.saturating_add(written as u64);
        self.update_lookup_cache();
        Ok(written)
    }

    fn read_at(&mut self, buffer: &mut [u8], offset: u64) -> FSResult<usize> {
        file::read_at(&self.fs, &self.inode.lock(), buffer, offset).map_err(Into::into)
    }

    fn info(&mut self) -> FSResult<FileLikeInfo> {
        let size = self.size()?;
        let inode = self.inode.lock();
        Ok(FileLikeInfo::new(
            self.name.clone(),
            size,
            UnixPermission(inode.mode().bits() as u32),
            FileLikeType::File,
        )
        .with_owner(inode.uid(), inode.gid())
        .with_inode(inode.index.get().into()))
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
        file::truncate(&self.fs, &mut self.inode.lock(), length).map_err(FSError::from)?;
        self.position = self.position.min(length);
        self.update_lookup_cache();
        Ok(())
    }

    fn allocate(&mut self, mode: u32, offset: u64, len: u64) -> FSResult<()> {
        let _operation = self.operation_lock.lock();
        if mode != 0 {
            return Err(FSError::Other);
        }

        let end = offset.checked_add(len).ok_or(FSError::Other)?;
        if end > self.inode.lock().size_in_bytes() {
            file::truncate(&self.fs, &mut self.inode.lock(), end).map_err(FSError::from)?;
            self.update_lookup_cache();
        }
        Ok(())
    }

    fn chmod(&self, mode: u32) -> FSResult<()> {
        let _operation = self.operation_lock.lock();
        let mut inode = self.inode.lock();
        chmod_inode(&self.fs, &mut inode, mode)?;
        lookup_cache_insert_raw(&self.lookup_cache, self.parent_inode, &self.name, &inode);
        Ok(())
    }

    fn chown(&self, uid: u32, gid: u32) -> FSResult<()> {
        let _operation = self.operation_lock.lock();
        let mut inode = self.inode.lock();
        chown_inode(&self.fs, &mut inode, uid, gid)?;
        lookup_cache_insert_raw(&self.lookup_cache, self.parent_inode, &self.name, &inode);
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
