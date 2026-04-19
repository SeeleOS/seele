use alloc::{string::ToString, sync::Arc};
use spin::mutex::Mutex;

use ext4plus::{Ext4, FollowSymlinks, inode::InodeMode, path::Path as Ext4Path};

use crate::filesystem::{
    errors::FSError,
    impls::ext4::{directory::Ext4Directory, file::Ext4File},
    path::{Path, PathPart},
    vfs::WrappedDirectory,
    vfs_traits::{FileLike, FileSystem},
};

pub mod directory;
pub mod error;
pub mod file;
pub mod operator;
pub mod symlink;

const CHMOD_PERMISSION_BITS: u16 = 0o7777;
const FILE_TYPE_BITS: u16 = 0o170000;

pub(super) fn chmod_path(fs: &Ext4, path: &str, mode: u32) -> crate::filesystem::vfs::FSResult<()> {
    let requested_bits = (mode as u16) & CHMOD_PERMISSION_BITS;
    let requested_mode = InodeMode::from_bits(requested_bits).ok_or(FSError::Other)?;
    let mut inode = fs
        .path_to_inode(Ext4Path::new(path), FollowSymlinks::All)
        .map_err(FSError::from)?;
    let merged_bits = (inode.mode().bits() & FILE_TYPE_BITS) | requested_mode.bits();
    let merged_mode = InodeMode::from_bits(merged_bits).ok_or(FSError::Other)?;
    inode.set_mode(merged_mode).map_err(FSError::from)?;
    inode.write(fs).map_err(FSError::from)?;
    Ok(())
}

/// Wrapper around the `ext4plus::Ext4` filesystem so it can be used
/// through the kernel's generic `FileSystem` trait.
pub struct EXT4(pub Ext4);

impl EXT4 {
    fn follow_intermediate_symlinks(
        &self,
        mut current: FileLike,
    ) -> crate::filesystem::vfs::FSResult<FileLike> {
        const MAX_SYMLINKS: usize = 40;

        for _ in 0..MAX_SYMLINKS {
            let target = match &current {
                FileLike::Symlink(symlink) => symlink.lock().target()?,
                FileLike::Directory(_) | FileLike::File(_) => return Ok(current),
            };
            current = self.lookup(&target)?;
        }

        Err(FSError::TooManySymlinks)
    }

    fn root_dir(&self) -> WrappedDirectory {
        Arc::new(Mutex::new(Ext4Directory::new(
            "".to_string(),
            "/".to_string(),
            self.0.clone(),
        )))
    }
}

impl FileSystem for EXT4 {
    fn init(&mut self) -> crate::filesystem::vfs::FSResult<()> {
        Ok(())
    }

    fn lookup(&self, path: &Path) -> crate::filesystem::vfs::FSResult<FileLike> {
        let normalized = path.normalize();
        let path_string = normalized.clone().as_string();
        let mut current = FileLike::Directory(self.root_dir());
        let components = normalized.parts.clone();

        if components.len() == 1 && matches!(components.first(), Some(PathPart::Root)) {
            return Ok(current);
        }

        for (index, component) in components.iter().enumerate() {
            let is_last = index + 1 == components.len();

            match component {
                PathPart::Root | PathPart::CurrentDir => {}
                PathPart::ParentDir => return Err(FSError::NotADirectory),
                PathPart::Normal(name) => {
                    current = self.follow_intermediate_symlinks(current)?;
                    current = match current {
                        FileLike::Directory(dir) => dir.lock().get(name)?,
                        FileLike::File(_) => return Err(FSError::NotADirectory),
                        FileLike::Symlink(_) => {
                            unreachable!("intermediate symlink was not followed")
                        }
                    };

                    if !is_last {
                        current = self.follow_intermediate_symlinks(current)?;
                    }
                }
            }
        }

        if path_string.ends_with('/') {
            current = self.follow_intermediate_symlinks(current)?;
        }

        if path_string.ends_with('/') && matches!(current, FileLike::File(_)) {
            return Err(FSError::NotADirectory);
        }

        Ok(current)
    }
}

// The underlying ext4plus types are `Send + Sync`, so it is safe to
// share them behind our trait objects.
unsafe impl Sync for EXT4 {}
unsafe impl Sync for Ext4File {}
unsafe impl Send for Ext4File {}
unsafe impl Send for Ext4Directory {}
unsafe impl Sync for Ext4Directory {}
