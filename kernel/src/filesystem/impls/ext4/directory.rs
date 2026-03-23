use core::time::Duration;

use alloc::{
    format,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use spin::mutex::Mutex;

use ext4plus::{
    self, DirEntryName, Ext4, FileType, FollowSymlinks,
    dir::Dir,
    error::Ext4Error,
    file::File as Ext4InnerFile,
    inode::{InodeCreationOptions, InodeFlags, InodeMode},
    path::Path,
};

use crate::filesystem::{
    errors::FSError,
    impls::ext4::file::Ext4File,
    info::DirectoryContentInfo,
    path,
    vfs_traits::{Directory, DirectoryContentType, FileLike},
};

fn map_ext4_error(err: Ext4Error) -> FSError {
    FSError::from(err)
}

pub struct Ext4Directory {
    /// Directory name (last path component, empty for root).
    name: String,
    /// Full absolute path within the ext4 filesystem, e.g. `/`, `/usr`.
    path: String,
    fs: Ext4,
}

impl Ext4Directory {
    pub fn new(name: String, path: String, fs: Ext4) -> Self {
        Self { name, path, fs }
    }

    fn join_child(&self, child: &str) -> String {
        if self.path == "/" {
            format!("/{}", child)
        } else {
            format!("{}/{}", self.path, child)
        }
    }
}

impl Directory for Ext4Directory {
    fn name(&self) -> crate::filesystem::vfs::FSResult<String> {
        Ok(self.name.clone())
    }

    fn contents(&self) -> crate::filesystem::vfs::FSResult<Vec<DirectoryContentInfo>> {
        let mut result = Vec::new();

        let iter = self
            .fs
            .read_dir(self.path.as_str())
            .map_err(map_ext4_error)?;

        for entry_res in iter {
            let entry = entry_res.map_err(map_ext4_error)?;
            let name = entry
                .file_name()
                .as_str()
                .unwrap_or("<non-utf8>")
                .to_string();

            let file_type = entry.file_type().map_err(map_ext4_error)?;
            let content_type = if file_type.is_dir() {
                DirectoryContentType::Directory
            } else if file_type.is_symlink() {
                DirectoryContentType::Symlink
            } else {
                DirectoryContentType::File
            };

            result.push(DirectoryContentInfo { name, content_type });
        }

        Ok(result)
    }

    fn create(&self, info: DirectoryContentInfo) -> crate::filesystem::vfs::FSResult<()> {
        let mut new_inode = self.fs.create_inode(InodeCreationOptions {
            file_type: match info.content_type {
                DirectoryContentType::File => FileType::Regular,
                DirectoryContentType::Directory => FileType::Directory,
                _ => unimplemented!(),
            },
            uid: 0,
            gid: 0,
            flags: InodeFlags::empty(),
            time: Duration::from_millis(0),
            mode: InodeMode::S_IFREG
                | InodeMode::S_IRUSR
                | InodeMode::S_IWUSR
                | InodeMode::S_IRGRP
                | InodeMode::S_IROTH,
        })?;

        // Parent inode of the new inode. In this case, the parent inode is [`self`]
        let parent_inode = self
            .fs
            .path_to_inode(Path::new(&self.path), FollowSymlinks::All)
            .map_err(map_ext4_error)?;
        let parent = Dir::open_inode(&self.fs, parent_inode).map_err(map_ext4_error)?;

        parent
            .link(
                DirEntryName::try_from(info.name.as_str()).unwrap(),
                &mut new_inode,
            )
            .map_err(Into::into)
    }

    fn delete(&self, _name: &str) -> crate::filesystem::vfs::FSResult<()> {
        // The initrd-backed filesystem is effectively read-only right now.
        Err(FSError::Other)
    }

    fn get(&self, name: &str) -> crate::filesystem::vfs::FSResult<FileLike> {
        let path = self.join_child(name);

        // Use `path_to_inode` so we can decide whether this is a file or directory.
        let inode = self
            .fs
            .path_to_inode(ext4plus::path::Path::new(&path), FollowSymlinks::All)
            .map_err(map_ext4_error)?;

        let meta = inode.metadata();

        if meta.is_dir() {
            Ok(FileLike::Directory(Arc::new(Mutex::new(
                Ext4Directory::new(name.to_string(), path, self.fs.clone()),
            ))))
        } else {
            let inner_file = Ext4InnerFile::open_inode(&self.fs, inode).map_err(map_ext4_error)?;
            Ok(FileLike::File(Arc::new(Mutex::new(Ext4File::new(
                name.to_string(),
                inner_file,
            )))))
        }
    }
}
