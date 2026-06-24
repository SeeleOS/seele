use alloc::{string::String, vec::Vec};
use ext4plus::{Ext4, inode::Inode};

use crate::filesystem::{
    errors::FSError,
    impls::ext4::{
        LookupCache, OperationLock, chown_inode, inode_times, lookup_cache_insert_raw,
        set_inode_times,
    },
    info::{FileLikeInfo, FileTimes, UnixPermission},
    path::Path,
    vfs::FSResult,
    vfs_traits::{FileLikeType, Symlink},
};
use crate::memory::utils::Mut;

pub struct Ext4Symlink {
    pub fs: Ext4,
    pub inode: Mut<Inode>,
    pub name: String,
    pub parent_inode: u32,
    pub lookup_cache: LookupCache,
    pub operation_lock: OperationLock,
}

impl Ext4Symlink {
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
}

impl Symlink for Ext4Symlink {
    fn info(&self) -> FSResult<FileLikeInfo> {
        let inode = self.refresh_inode()?;
        Ok(FileLikeInfo {
            name: self.name.clone(),
            file_like_type: FileLikeType::Symlink,
            size: inode.metadata().len() as usize,
            inode: inode.index.get().into(),
            uid: inode.uid(),
            gid: inode.gid(),
            nlink: 1,
            rdev: 0,
            times: inode_times(&inode),
            permission: UnixPermission::symlink(),
        })
    }

    fn target(&self) -> FSResult<Path> {
        let fs = &self.fs;
        let target = self
            .inode
            .lock()
            .symlink_target(fs)
            .map_err(FSError::from)?;
        let target = target.to_str().map_err(|_| FSError::Other)?;

        Ok(Path::new(target).normalize())
    }

    fn read_link_target(&self) -> FSResult<String> {
        let target = self
            .inode
            .lock()
            .symlink_target(&self.fs)
            .map_err(FSError::from)?;
        target
            .to_str()
            .map(String::from)
            .map_err(|_| FSError::Other)
    }

    fn chown(&self, uid: u32, gid: u32) -> FSResult<()> {
        let _operation = self.operation_lock.lock();
        let mut inode = self.inode.lock();
        chown_inode(&self.fs, &mut inode, uid, gid)?;
        drop(inode);
        self.update_lookup_cache();
        Ok(())
    }

    fn set_times(&self, times: FileTimes) -> FSResult<()> {
        let _operation = self.operation_lock.lock();
        let mut inode = self.inode.lock();
        set_inode_times(&self.fs, &mut inode, times)?;
        drop(inode);
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
            .map_err(FSError::from)?;
        self.update_lookup_cache();
        Ok(())
    }
}
