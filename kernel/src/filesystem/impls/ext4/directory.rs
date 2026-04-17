use core::any::Any;
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
    impls::ext4::{file::Ext4File, symlink::Ext4Symlink},
    info::DirectoryContentInfo,
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

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn fs(&self) -> &Ext4 {
        &self.fs
    }

    pub fn chmod(&self, mode: InodeMode) -> crate::filesystem::vfs::FSResult<()> {
        let mut inode = self
            .fs
            .path_to_inode(Path::new(&self.path), FollowSymlinks::All)
            .map_err(map_ext4_error)?;
        inode.set_mode(mode).map_err(map_ext4_error)?;
        inode.write(&self.fs).map_err(map_ext4_error)?;
        Ok(())
    }
}

impl Directory for Ext4Directory {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn info(&self) -> crate::filesystem::vfs::FSResult<crate::filesystem::info::FileLikeInfo> {
        let inode = self
            .fs
            .path_to_inode(Path::new(&self.path), FollowSymlinks::All)
            .map_err(map_ext4_error)?;
        Ok(crate::filesystem::info::FileLikeInfo::new(
            self.name.clone(),
            0,
            crate::filesystem::info::UnixPermission(inode.mode().bits().into()),
            crate::filesystem::vfs_traits::FileLikeType::Directory,
        )
        .with_inode(inode.index.get().into()))
    }

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
        let (file_type, mode) = match info.content_type {
            DirectoryContentType::File => (
                FileType::Regular,
                InodeMode::S_IFREG
                    | InodeMode::S_IRUSR
                    | InodeMode::S_IWUSR
                    | InodeMode::S_IRGRP
                    | InodeMode::S_IROTH,
            ),
            DirectoryContentType::Directory => (
                FileType::Directory,
                InodeMode::S_IFDIR
                    | InodeMode::S_IRUSR
                    | InodeMode::S_IWUSR
                    | InodeMode::S_IXUSR
                    | InodeMode::S_IRGRP
                    | InodeMode::S_IXGRP
                    | InodeMode::S_IROTH
                    | InodeMode::S_IXOTH,
            ),
            _ => unimplemented!(),
        };

        let mut new_inode = self.fs.create_inode(InodeCreationOptions {
            file_type,
            uid: 0,
            gid: 0,
            flags: InodeFlags::empty(),
            time: Duration::from_millis(0),
            mode,
        })?;

        // Parent inode of the new inode. In this case, the parent inode is [`self`]
        let mut parent_inode = self
            .fs
            .path_to_inode(Path::new(&self.path), FollowSymlinks::All)
            .map_err(map_ext4_error)?;
        let mut parent = Dir::open_inode(&self.fs, parent_inode.clone()).map_err(map_ext4_error)?;

        if matches!(info.content_type, DirectoryContentType::Directory) {
            // A freshly-created ext4 directory needs an initialized first block
            // containing "." and ".." before new children can be linked into it.
            new_inode.set_links_count(1);
            new_inode.write(&self.fs).map_err(map_ext4_error)?;
            let child_dir = Dir::init(self.fs.clone(), new_inode, parent_inode.index)
                .map_err(map_ext4_error)?;
            new_inode = child_dir.inode().clone();
        }

        parent
            .link(
                DirEntryName::try_from(info.name.as_str()).unwrap(),
                &mut new_inode,
            )
            .map_err(map_ext4_error)?;

        if matches!(info.content_type, DirectoryContentType::Directory) {
            let new_links = parent_inode
                .links_count()
                .checked_add(1)
                .ok_or(FSError::Other)?;
            parent_inode.set_links_count(new_links);
            parent_inode.write(&self.fs).map_err(map_ext4_error)?;
        }

        Ok(())
    }

    fn delete(&self, name: &str) -> crate::filesystem::vfs::FSResult<()> {
        let mut parent_inode = self
            .fs
            .path_to_inode(Path::new(&self.path), FollowSymlinks::All)
            .map_err(map_ext4_error)?;
        let mut parent = Dir::open_inode(&self.fs, parent_inode.clone()).map_err(map_ext4_error)?;

        let entry_name = DirEntryName::try_from(name).map_err(|_| FSError::Other)?;
        let inode = parent.get_entry(entry_name).map_err(map_ext4_error)?;

        if inode.metadata().is_dir() {
            let path = self.join_child(name);
            let mut iter = self.fs.read_dir(path.as_str()).map_err(map_ext4_error)?;
            while let Some(entry) = iter.next() {
                let entry = entry.map_err(map_ext4_error)?;
                let entry_name = entry
                    .file_name()
                    .as_str()
                    .map_err(|_| FSError::Other)?
                    .to_string();

                if entry_name != "." && entry_name != ".." {
                    return Err(FSError::Other);
                }
            }

            let new_links = parent_inode
                .links_count()
                .checked_sub(1)
                .ok_or(FSError::Other)?;
            parent_inode.set_links_count(new_links);
            parent_inode.write(&self.fs).map_err(map_ext4_error)?;
        }

        parent.unlink(entry_name, inode).map_err(map_ext4_error)?;
        Ok(())
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
        } else if meta.is_symlink() {
            Ok(FileLike::Symlink(Arc::new(Mutex::new(Ext4Symlink {
                fs: self.fs.clone(),
                inode,
                name: name.into(),
                parent_path: self.path.clone(),
            }))))
        } else {
            let inner_file = Ext4InnerFile::open_inode(&self.fs, inode).map_err(map_ext4_error)?;
            Ok(FileLike::File(Arc::new(Mutex::new(Ext4File::new(
                name.to_string(),
                inner_file,
            )))))
        }
    }
}
