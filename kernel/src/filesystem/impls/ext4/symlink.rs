use alloc::string::String;
use ext4plus::{Ext4, inode::Inode};

use crate::filesystem::{
    errors::FSError,
    impls::ext4::chown_inode,
    info::{FileLikeInfo, UnixPermission},
    path::Path,
    vfs::FSResult,
    vfs_traits::{FileLikeType, Symlink},
};

pub struct Ext4Symlink {
    pub fs: Ext4,
    pub inode: Inode,
    pub name: String,
}

impl Symlink for Ext4Symlink {
    fn info(&self) -> FSResult<FileLikeInfo> {
        Ok(FileLikeInfo {
            name: self.name.clone(),
            file_like_type: FileLikeType::Symlink,
            size: 0,
            inode: self.inode.index.get().into(),
            uid: self.inode.uid(),
            gid: self.inode.gid(),
            rdev: 0,
            permission: UnixPermission::symlink(),
        })
    }

    fn target(&self) -> FSResult<Path> {
        let fs = &self.fs;
        let target = self.inode.symlink_target(fs).map_err(FSError::from)?;
        let target = target.to_str().map_err(|_| FSError::Other)?;

        Ok(Path::new(target).normalize())
    }

    fn read_link_target(&self) -> FSResult<String> {
        let target = self.inode.symlink_target(&self.fs).map_err(FSError::from)?;
        target
            .to_str()
            .map(String::from)
            .map_err(|_| FSError::Other)
    }

    fn chown(&self, uid: u32, gid: u32) -> FSResult<()> {
        let mut inode = self.inode.clone();
        chown_inode(&self.fs, &mut inode, uid, gid)
    }
}
