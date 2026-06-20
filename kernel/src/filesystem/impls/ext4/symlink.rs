use alloc::{string::String, vec::Vec};
use ext4plus::{Ext4, inode::Inode};

use crate::filesystem::{
    errors::FSError,
    impls::ext4::{OperationLock, chown_inode},
    info::{FileLikeInfo, UnixPermission},
    path::Path,
    vfs::FSResult,
    vfs_traits::{FileLikeType, Symlink},
};
use crate::memory::utils::Mut;

pub struct Ext4Symlink {
    pub fs: Ext4,
    pub inode: Mut<Inode>,
    pub name: String,
    pub operation_lock: OperationLock,
}

impl Symlink for Ext4Symlink {
    fn info(&self) -> FSResult<FileLikeInfo> {
        let inode = self.inode.lock();
        Ok(FileLikeInfo {
            name: self.name.clone(),
            file_like_type: FileLikeType::Symlink,
            size: 0,
            inode: inode.index.get().into(),
            uid: inode.uid(),
            gid: inode.gid(),
            rdev: 0,
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
        chown_inode(&self.fs, &mut inode, uid, gid)
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
            .map_err(FSError::from)
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
