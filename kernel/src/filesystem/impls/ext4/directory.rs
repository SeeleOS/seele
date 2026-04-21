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
    inode::{Inode, InodeCreationOptions, InodeFlags, InodeMode},
    path::{Path, PathBuf as Ext4PathBuf},
};

use crate::filesystem::{
    errors::FSError,
    impls::ext4::{
        LookupCache, chmod_path, file::Ext4File, lookup_cache_clear, lookup_cache_get,
        lookup_cache_insert, lookup_cache_insert_raw, lookup_cache_remove, symlink::Ext4Symlink,
    },
    info::{DirectoryContentInfo, FileLikeInfo, UnixPermission},
    vfs::FSResult,
    vfs_traits::{Directory, DirectoryContentType, FileLike, FileLikeType},
};
use crate::misc::systemd_perf::{self, PerfBucket};

fn map_ext4_error(err: Ext4Error) -> FSError {
    FSError::from(err)
}

pub struct Ext4Directory {
    /// Directory name (last path component, empty for root).
    name: String,
    /// Full absolute path within the ext4 filesystem, e.g. `/`, `/usr`.
    path: String,
    fs: Ext4,
    inode: Mutex<Inode>,
    parent_inode: Option<u32>,
    lookup_cache: LookupCache,
}

impl Ext4Directory {
    pub fn new(
        name: String,
        path: String,
        fs: Ext4,
        inode: Inode,
        parent_inode: Option<u32>,
        lookup_cache: LookupCache,
    ) -> Self {
        Self {
            name,
            path,
            fs,
            inode: Mutex::new(inode),
            parent_inode,
            lookup_cache,
        }
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

    pub fn inode(&self) -> Inode {
        self.current_inode()
    }

    pub fn clear_lookup_cache(&self) {
        lookup_cache_clear(&self.lookup_cache);
    }

    fn current_inode(&self) -> Inode {
        self.inode.lock().clone()
    }

    fn update_cached_inode(&self, inode: Inode) {
        *self.inode.lock() = inode;
    }

    fn open_parent_dir(&self) -> FSResult<(Inode, Dir)> {
        let parent_inode = self.current_inode();
        let parent = Dir::open_inode(&self.fs, parent_inode.clone()).map_err(map_ext4_error)?;
        Ok((parent_inode, parent))
    }

    fn file_like_from_inode(
        &self,
        name: String,
        path: String,
        parent_inode: u32,
        inode: Inode,
    ) -> FSResult<FileLike> {
        let meta = inode.metadata();

        if meta.is_dir() {
            Ok(FileLike::Directory(Arc::new(Mutex::new(
                Ext4Directory::new(
                    name,
                    path,
                    self.fs.clone(),
                    inode,
                    Some(parent_inode),
                    self.lookup_cache.clone(),
                ),
            ))))
        } else if meta.is_symlink() {
            Ok(FileLike::Symlink(Arc::new(Mutex::new(Ext4Symlink {
                fs: self.fs.clone(),
                inode,
                name,
                parent_path: self.path.clone(),
            }))))
        } else {
            let inner_file = Ext4InnerFile::open_inode(&self.fs, inode).map_err(map_ext4_error)?;
            Ok(FileLike::File(Arc::new(Mutex::new(Ext4File::new(
                name,
                path,
                self.fs.clone(),
                inner_file,
                parent_inode,
                self.lookup_cache.clone(),
            )))))
        }
    }
}

impl Directory for Ext4Directory {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn info(&self) -> FSResult<FileLikeInfo> {
        let inode = self.current_inode();
        Ok(FileLikeInfo::new(
            self.name.clone(),
            0,
            UnixPermission(inode.mode().bits().into()),
            FileLikeType::Directory,
        )
        .with_inode(inode.index.get().into()))
    }

    fn name(&self) -> FSResult<String> {
        Ok(self.name.clone())
    }

    fn contents(&self) -> FSResult<Vec<DirectoryContentInfo>> {
        let mut result = Vec::new();

        let iter = match self.fs.read_dir(self.path.as_str()) {
            Ok(iter) => iter,
            Err(err) => return Err(map_ext4_error(err)),
        };

        for entry_res in iter {
            let entry = match entry_res {
                Ok(entry) => entry,
                Err(err) => return Err(map_ext4_error(err)),
            };
            let name = entry
                .file_name()
                .as_str()
                .unwrap_or("<non-utf8>")
                .to_string();

            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(err) => return Err(map_ext4_error(err)),
            };
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

    fn create(&self, info: DirectoryContentInfo) -> FSResult<()> {
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
        let (mut parent_inode, mut parent) = self.open_parent_dir()?;

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
        lookup_cache_insert(&self.lookup_cache, &parent_inode, &info.name, &new_inode);

        if matches!(info.content_type, DirectoryContentType::Directory) {
            let new_links = parent_inode
                .links_count()
                .checked_add(1)
                .ok_or(FSError::Other)?;
            parent_inode.set_links_count(new_links);
            parent_inode.write(&self.fs).map_err(map_ext4_error)?;
            self.update_cached_inode(parent_inode);
        }
        Ok(())
    }

    fn create_symlink(&self, name: &str, target: &str) -> FSResult<()> {
        let (parent_inode, mut parent) = self.open_parent_dir()?;
        let entry_name = DirEntryName::try_from(name).map_err(|_| FSError::PathTooLong)?;
        let target = Ext4PathBuf::try_from(target.to_string()).map_err(|_| FSError::PathTooLong)?;

        match self.fs.symlink(
            &mut parent,
            entry_name,
            target,
            0,
            0,
            Duration::from_millis(0),
        ) {
            Ok(inode) => lookup_cache_insert(&self.lookup_cache, &parent_inode, name, &inode),
            Err(err) => return Err(map_ext4_error(err)),
        }
        Ok(())
    }

    fn delete(&self, name: &str) -> FSResult<()> {
        let (mut parent_inode, mut parent) = self.open_parent_dir()?;

        let entry_name = DirEntryName::try_from(name).map_err(|_| FSError::Other)?;
        let inode = parent.get_entry(entry_name).map_err(map_ext4_error)?;

        if inode.metadata().is_dir() {
            let path = self.join_child(name);
            let iter = self.fs.read_dir(path.as_str()).map_err(map_ext4_error)?;
            for entry in iter {
                let entry = entry.map_err(map_ext4_error)?;
                let entry_name = entry
                    .file_name()
                    .as_str()
                    .map_err(|_| FSError::Other)?
                    .to_string();

                if entry_name != "." && entry_name != ".." {
                    return Err(FSError::DirectoryNotEmpty);
                }
            }

            let new_links = parent_inode
                .links_count()
                .checked_sub(1)
                .ok_or(FSError::Other)?;
            parent_inode.set_links_count(new_links);
            parent_inode.write(&self.fs).map_err(map_ext4_error)?;
            self.update_cached_inode(parent_inode);
            lookup_cache_clear(&self.lookup_cache);
        } else {
            lookup_cache_remove(&self.lookup_cache, &parent_inode, name);
        }

        parent.unlink(entry_name, inode).map_err(map_ext4_error)?;
        Ok(())
    }

    fn get(&self, name: &str) -> FSResult<FileLike> {
        systemd_perf::profile_current_process(PerfBucket::Ext4DirGet, || {
            let path = self.join_child(name);
            let (parent_inode, parent) = self.open_parent_dir()?;
            let parent_id = parent_inode.index.get();

            if let Some(inode) = lookup_cache_get(&self.lookup_cache, &parent_inode, name) {
                return self.file_like_from_inode(name.to_string(), path, parent_id, inode);
            }

            let entry_name = DirEntryName::try_from(name).map_err(|_| FSError::Other)?;
            let inode = parent.get_entry(entry_name).map_err(map_ext4_error)?;
            lookup_cache_insert(&self.lookup_cache, &parent_inode, name, &inode);
            self.file_like_from_inode(name.to_string(), path, parent_id, inode)
        })
    }

    fn chmod(&self, mode: u32) -> FSResult<()> {
        chmod_path(&self.fs, &self.path, mode)?;
        let inode = self
            .fs
            .path_to_inode(Path::new(&self.path), FollowSymlinks::All)
            .map_err(map_ext4_error)?;
        self.update_cached_inode(inode);
        if let Some(parent_inode) = self.parent_inode {
            lookup_cache_insert_raw(
                &self.lookup_cache,
                parent_inode,
                &self.name,
                &self.current_inode(),
            );
        }
        Ok(())
    }
}
