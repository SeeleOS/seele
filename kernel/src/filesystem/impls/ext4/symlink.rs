use alloc::string::String;
use ext4plus::{Ext4, FollowSymlinks, inode::Inode};

use crate::filesystem::{
    errors::FSError,
    info::{FileLikeInfo, UnixPermission},
    path::Path,
    vfs_traits::{FileLikeType, Symlink},
};

pub struct Ext4Symlink {
    pub fs: Ext4,
    pub inode: Inode,
    pub name: String,
    pub parent_path: String,
}

impl Symlink for Ext4Symlink {
    fn info(&self) -> crate::filesystem::vfs::FSResult<crate::filesystem::info::FileLikeInfo> {
        Ok(FileLikeInfo {
            name: self.name.clone(),
            file_like_type: FileLikeType::Symlink,
            size: 0,
            permission: UnixPermission::symlink(),
        })
    }

    fn target(&self) -> crate::filesystem::vfs::FSResult<Path> {
        let fs = &self.fs;
        let target = self.inode.symlink_target(fs).map_err(FSError::from)?;
        let target = target.to_str().map_err(|_| FSError::Other)?;

        let combined = if target.starts_with('/') {
            target.into()
        } else if self.parent_path == "/" {
            alloc::format!("/{}", target)
        } else {
            alloc::format!("{}/{}", self.parent_path, target)
        };

        Ok(Path::new(&combined).as_absolute().as_normal())
    }
}
