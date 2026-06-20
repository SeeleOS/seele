use crate::memory::utils::Mut;
use core::any::Any;
use core::time::Duration;

use alloc::{
    format,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};

use ext4plus::{
    self, DirEntryName, Ext4, FileType,
    dir::Dir,
    error::Ext4Error,
    inode::{Inode, InodeCreationOptions, InodeFlags, InodeMode},
    path::PathBuf as Ext4PathBuf,
};

use crate::filesystem::{
    errors::FSError,
    impls::ext4::{
        LookupCache, OperationLock, chmod_inode, chown_inode, file::Ext4File, lookup_cache_clear,
        lookup_cache_get, lookup_cache_insert, lookup_cache_insert_raw, lookup_cache_remove,
        symlink::Ext4Symlink,
    },
    info::{DirectoryContentInfo, FileLikeInfo, UnixPermission},
    vfs::FSResult,
    vfs_traits::{Directory, DirectoryContentType, FileLike, FileLikeType},
};

fn map_ext4_error(err: Ext4Error) -> FSError {
    FSError::from(err)
}

#[derive(Clone)]
pub(crate) struct Ext4Lookup {
    pub(crate) name: String,
    pub(crate) inode: Inode,
    pub(crate) file_like_type: FileLikeType,
    pub(crate) parent_inode: u32,
    pub(crate) fs: Ext4,
    pub(crate) lookup_cache: LookupCache,
    pub(crate) operation_lock: OperationLock,
}

impl Ext4Lookup {
    pub(crate) fn info(&self) -> FileLikeInfo {
        file_like_info_from_inode(self.name.clone(), self.file_like_type.clone(), &self.inode)
    }
}

fn file_like_type_from_inode(inode: &Inode) -> FileLikeType {
    let meta = inode.metadata();
    if meta.is_dir() {
        FileLikeType::Directory
    } else if meta.is_symlink() {
        FileLikeType::Symlink
    } else {
        FileLikeType::File
    }
}

fn file_like_info_from_inode(
    name: String,
    file_like_type: FileLikeType,
    inode: &Inode,
) -> FileLikeInfo {
    let permission = if matches!(file_like_type, FileLikeType::Symlink) {
        UnixPermission::symlink()
    } else {
        UnixPermission(inode.mode().bits().into())
    };

    FileLikeInfo::new(
        name,
        inode.metadata().len() as usize,
        permission,
        file_like_type,
    )
    .with_owner(inode.uid(), inode.gid())
    .with_inode(inode.index.get().into())
}

pub(crate) fn lookup_child(
    fs: &Ext4,
    parent_inode: &Inode,
    lookup_cache: &LookupCache,
    operation_lock: &OperationLock,
    name: &str,
) -> FSResult<Ext4Lookup> {
    let parent_id = parent_inode.index.get();

    if let Some(inode) = lookup_cache_get(lookup_cache, parent_inode, name) {
        return Ok(Ext4Lookup {
            name: name.to_string(),
            inode: inode.clone(),
            file_like_type: file_like_type_from_inode(&inode),
            parent_inode: parent_id,
            fs: fs.clone(),
            lookup_cache: lookup_cache.clone(),
            operation_lock: operation_lock.clone(),
        });
    }

    let parent = Dir::open_inode(fs, parent_inode.clone()).map_err(map_ext4_error)?;
    let entry_name = DirEntryName::try_from(name).map_err(|_| FSError::Other)?;
    let inode = parent.get_entry(entry_name).map_err(map_ext4_error)?;
    lookup_cache_insert(lookup_cache, parent_inode, name, &inode);
    Ok(Ext4Lookup {
        name: name.to_string(),
        inode: inode.clone(),
        file_like_type: file_like_type_from_inode(&inode),
        parent_inode: parent_id,
        fs: fs.clone(),
        lookup_cache: lookup_cache.clone(),
        operation_lock: operation_lock.clone(),
    })
}

pub struct Ext4Directory {
    /// Directory name (last path component, empty for root).
    name: String,
    /// Full absolute path within the ext4 filesystem, e.g. `/`, `/usr`.
    path: String,
    fs: Ext4,
    inode: Mut<Inode>,
    parent_inode: Option<u32>,
    lookup_cache: LookupCache,
    operation_lock: OperationLock,
}

impl Ext4Directory {
    pub fn new(
        name: String,
        path: String,
        fs: Ext4,
        inode: Inode,
        parent_inode: Option<u32>,
        lookup_cache: LookupCache,
        operation_lock: OperationLock,
    ) -> Self {
        Self {
            name,
            path,
            fs,
            inode: Mut::new(inode),
            parent_inode,
            lookup_cache,
            operation_lock,
        }
    }

    fn join_child(&self, child: &str) -> String {
        if self.path == "/" {
            format!("/{}", child)
        } else {
            format!("{}/{}", self.path, child)
        }
    }

    pub fn clear_lookup_cache(&self) {
        lookup_cache_clear(&self.lookup_cache);
    }

    fn current_inode(&self) -> Inode {
        self.inode.lock().clone()
    }

    #[cfg(test)]
    pub(crate) fn inode(&self) -> Inode {
        self.current_inode()
    }

    fn update_cached_inode(&self, inode: Inode) {
        *self.inode.lock() = inode;
    }

    fn open_parent_dir(&self) -> FSResult<(Inode, Dir)> {
        let parent_inode = self.current_inode();
        let parent = Dir::open_inode(&self.fs, parent_inode.clone()).map_err(map_ext4_error)?;
        Ok((parent_inode, parent))
    }

    fn file_like_from_lookup(&self, lookup: Ext4Lookup) -> FSResult<FileLike> {
        match lookup.file_like_type {
            FileLikeType::Directory => {
                let path = self.join_child(&lookup.name);
                Ok(FileLike::Directory(Arc::new(Mut::new(Ext4Directory::new(
                    lookup.name,
                    path,
                    lookup.fs,
                    lookup.inode,
                    Some(lookup.parent_inode),
                    lookup.lookup_cache,
                    lookup.operation_lock,
                )))))
            }
            FileLikeType::Symlink => Ok(FileLike::Symlink(Arc::new(Mut::new(Ext4Symlink {
                fs: lookup.fs,
                inode: Mut::new(lookup.inode),
                name: lookup.name,
                parent_inode: lookup.parent_inode,
                lookup_cache: lookup.lookup_cache,
                operation_lock: lookup.operation_lock,
            })))),
            FileLikeType::File => Ok(FileLike::File(Arc::new(Mut::new(Ext4File::new(
                lookup.name,
                lookup.fs,
                lookup.inode,
                lookup.parent_inode,
                lookup.lookup_cache,
                lookup.operation_lock,
            ))))),
        }
    }

    pub(crate) fn lookup_child(&self, name: &str) -> FSResult<Ext4Lookup> {
        lookup_child(
            &self.fs,
            &self.current_inode(),
            &self.lookup_cache,
            &self.operation_lock,
            name,
        )
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
        .with_owner(inode.uid(), inode.gid())
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

            result.push(
                DirectoryContentInfo::new(name, content_type).with_inode(entry.inode.get().into()),
            );
        }

        Ok(result)
    }

    fn create(&self, info: DirectoryContentInfo) -> FSResult<()> {
        let _operation = self.operation_lock.lock();
        let requested_mode = info.permission.map(|permission| permission.0 & 0o7777);
        let (file_type, mode) = match info.content_type {
            DirectoryContentType::File => (
                FileType::Regular,
                InodeMode::from_bits_retain(
                    InodeMode::S_IFREG.bits() | requested_mode.unwrap_or(0o644) as u16,
                ),
            ),
            DirectoryContentType::Directory => (
                FileType::Directory,
                InodeMode::from_bits_retain(
                    InodeMode::S_IFDIR.bits() | requested_mode.unwrap_or(0o755) as u16,
                ),
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
        let (parent_inode, mut parent) = self.open_parent_dir()?;

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

        self.update_cached_inode(parent.inode().clone());
        Ok(())
    }

    fn create_symlink(&self, name: &str, target: &str) -> FSResult<()> {
        let _operation = self.operation_lock.lock();
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
        let _operation = self.operation_lock.lock();
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

            let mut child_inode = inode;
            child_inode.set_links_count(1);
            child_inode.write(&self.fs).map_err(map_ext4_error)?;
            parent
                .unlink(entry_name, child_inode)
                .map_err(map_ext4_error)?;

            let new_links = parent_inode
                .links_count()
                .checked_sub(1)
                .ok_or(FSError::Other)?;
            parent_inode.set_links_count(new_links);
            parent_inode.write(&self.fs).map_err(map_ext4_error)?;
            self.update_cached_inode(parent_inode);
            lookup_cache_clear(&self.lookup_cache);
            return Ok(());
        } else {
            lookup_cache_remove(&self.lookup_cache, &parent_inode, name);
        }

        parent.unlink(entry_name, inode).map_err(map_ext4_error)?;
        Ok(())
    }

    fn get(&self, name: &str) -> FSResult<FileLike> {
        self.file_like_from_lookup(self.lookup_child(name)?)
    }

    fn get_info(&self, name: &str) -> FSResult<FileLikeInfo> {
        Ok(self.lookup_child(name)?.info())
    }

    fn chmod(&self, mode: u32) -> FSResult<()> {
        let _operation = self.operation_lock.lock();
        let mut inode = self.current_inode();
        chmod_inode(&self.fs, &mut inode, mode)?;
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

    fn chown(&self, uid: u32, gid: u32) -> FSResult<()> {
        let _operation = self.operation_lock.lock();
        let mut inode = self.current_inode();
        chown_inode(&self.fs, &mut inode, uid, gid)?;
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

    fn get_xattr(&self, name: &str) -> FSResult<Option<Vec<u8>>> {
        self.current_inode()
            .get_xattr(&self.fs, name)
            .map_err(FSError::from)
    }

    fn set_xattr(&self, name: String, value: Vec<u8>, create: bool, replace: bool) -> FSResult<()> {
        let _operation = self.operation_lock.lock();
        let mut inode = self.current_inode();
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
        self.update_cached_inode(inode);
        Ok(())
    }

    fn list_xattrs(&self) -> FSResult<Vec<String>> {
        self.current_inode()
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
        let mut inode = self.current_inode();
        inode.remove_xattr(&self.fs, name).map_err(FSError::from)?;
        self.update_cached_inode(inode);
        Ok(())
    }
}
