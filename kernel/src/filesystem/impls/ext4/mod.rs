use alloc::{string::ToString, sync::Arc};
use spin::mutex::Mutex;

use ext4plus::Ext4;

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

/// Wrapper around the `ext4plus::Ext4` filesystem so it can be used
/// through the kernel's generic `FileSystem` trait.
pub struct EXT4(pub Ext4);

impl EXT4 {
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
                    loop {
                        let next = match &current {
                            FileLike::Directory(dir) => Some(dir.lock().get(name)?),
                            FileLike::Symlink(symlink) => {
                                Some(self.lookup(&symlink.lock().target()?)?)
                            }
                            FileLike::File(_) => return Err(FSError::NotADirectory),
                        };

                        current = next.expect("ext4 lookup next node");
                        if matches!(current, FileLike::Directory(_) | FileLike::File(_)) {
                            break;
                        }
                    }

                    if !is_last {
                        while let FileLike::Symlink(symlink) = &current {
                            let target = symlink.lock().target()?;
                            current = self.lookup(&target)?;
                        }
                    }
                }
            }
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
