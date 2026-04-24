use alloc::{collections::btree_map::BTreeMap, string::String, string::ToString, sync::Arc};
use spin::mutex::Mutex;

use ext4plus::{
    DirEntryName, Ext4, FollowSymlinks,
    dir::Dir,
    inode::{Inode, InodeMode},
    path::Path as Ext4Path,
};

use crate::filesystem::{
    errors::FSError,
    impls::ext4::{directory::Ext4Directory, file::Ext4File},
    path::{Path, PathPart},
    vfs::{FSResult, WrappedDirectory},
    vfs_traits::{FileLike, FileSystem},
};
use crate::misc::systemd_perf::{self, PerfBucket};

pub mod directory;
pub mod error;
pub mod file;
pub mod operator;
pub mod symlink;

const CHMOD_PERMISSION_BITS: u16 = 0o7777;
const FILE_TYPE_BITS: u16 = 0o170000;
pub(super) type LookupCache = Arc<Mutex<BTreeMap<(u32, String), Inode>>>;

pub(super) fn lookup_cache_get(
    cache: &LookupCache,
    parent_inode: &Inode,
    name: &str,
) -> Option<Inode> {
    cache
        .lock()
        .get(&(parent_inode.index.get(), name.to_string()))
        .cloned()
}

pub(super) fn lookup_cache_insert(
    cache: &LookupCache,
    parent_inode: &Inode,
    name: &str,
    inode: &Inode,
) {
    lookup_cache_insert_raw(cache, parent_inode.index.get(), name, inode);
}

pub(super) fn lookup_cache_insert_raw(
    cache: &LookupCache,
    parent_inode: u32,
    name: &str,
    inode: &Inode,
) {
    cache
        .lock()
        .insert((parent_inode, name.to_string()), inode.clone());
}

pub(super) fn lookup_cache_remove(cache: &LookupCache, parent_inode: &Inode, name: &str) {
    lookup_cache_remove_raw(cache, parent_inode.index.get(), name);
}

pub(super) fn lookup_cache_remove_raw(cache: &LookupCache, parent_inode: u32, name: &str) {
    cache.lock().remove(&(parent_inode, name.to_string()));
}

pub(super) fn lookup_cache_clear(cache: &LookupCache) {
    cache.lock().clear();
}

pub(super) fn chmod_path(fs: &Ext4, path: &str, mode: u32) -> FSResult<()> {
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
pub struct EXT4 {
    fs: Ext4,
    root_inode: Inode,
    lookup_cache: LookupCache,
}

impl EXT4 {
    pub fn new(fs: Ext4) -> Self {
        let root_inode = fs
            .path_to_inode(Ext4Path::new("/"), FollowSymlinks::All)
            .expect("ext4 root inode must exist");
        Self {
            fs,
            root_inode,
            lookup_cache: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    fn follow_intermediate_symlinks(&self, mut current: FileLike) -> FSResult<FileLike> {
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
            self.fs.clone(),
            self.root_inode.clone(),
            None,
            self.lookup_cache.clone(),
        )))
    }
}

impl FileSystem for EXT4 {
    fn init(&mut self) -> FSResult<()> {
        Ok(())
    }

    fn lookup(&self, path: &Path) -> FSResult<FileLike> {
        systemd_perf::profile_current_process(PerfBucket::Ext4Lookup, || {
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
        })
    }

    fn rename(&self, old_path: &Path, new_path: &Path) -> FSResult<()> {
        lookup_cache_clear(&self.lookup_cache);
        let old_path = old_path.normalize();
        let new_path = new_path.normalize();
        if old_path == new_path {
            return Ok(());
        }

        let source = self.lookup(&old_path)?;
        if matches!(source, FileLike::Directory(_)) {
            return Err(FSError::Other);
        }
        let source_inode = self
            .fs
            .path_to_inode(
                Ext4Path::new(&old_path.clone().as_string()),
                FollowSymlinks::ExcludeFinalComponent,
            )
            .map_err(FSError::from)?;

        let old_parent = old_path.parent().ok_or(FSError::NotFound)?;
        let old_name = old_path.file_name().ok_or(FSError::NotFound)?;
        let new_parent = new_path.parent().ok_or(FSError::NotFound)?;
        let new_name = new_path.file_name().ok_or(FSError::NotFound)?;

        if let Ok(target) = self.lookup(&new_path) {
            if matches!(target, FileLike::Directory(_)) {
                return Err(FSError::DirectoryNotEmpty);
            }
            let new_parent_inode = self
                .fs
                .path_to_inode(
                    Ext4Path::new(&new_parent.clone().as_string()),
                    FollowSymlinks::All,
                )
                .map_err(FSError::from)?;
            let mut new_parent_dir =
                Dir::open_inode(&self.fs, new_parent_inode).map_err(FSError::from)?;
            let target_inode = self
                .fs
                .path_to_inode(
                    Ext4Path::new(&new_path.clone().as_string()),
                    FollowSymlinks::ExcludeFinalComponent,
                )
                .map_err(FSError::from)?;
            new_parent_dir
                .unlink(
                    DirEntryName::try_from(new_name.as_str()).map_err(|_| FSError::Other)?,
                    target_inode,
                )
                .map_err(FSError::from)?;
        }

        let new_parent_inode = self
            .fs
            .path_to_inode(
                Ext4Path::new(&new_parent.clone().as_string()),
                FollowSymlinks::All,
            )
            .map_err(FSError::from)?;
        let mut new_parent_dir =
            Dir::open_inode(&self.fs, new_parent_inode).map_err(FSError::from)?;
        let mut source_inode = source_inode;
        new_parent_dir
            .link(
                DirEntryName::try_from(new_name.as_str()).map_err(|_| FSError::Other)?,
                &mut source_inode,
            )
            .map_err(FSError::from)?;

        let old_parent_inode = self
            .fs
            .path_to_inode(
                Ext4Path::new(&old_parent.clone().as_string()),
                FollowSymlinks::All,
            )
            .map_err(FSError::from)?;
        let mut old_parent_dir =
            Dir::open_inode(&self.fs, old_parent_inode).map_err(FSError::from)?;
        let old_inode = self
            .fs
            .path_to_inode(
                Ext4Path::new(&old_path.clone().as_string()),
                FollowSymlinks::ExcludeFinalComponent,
            )
            .map_err(FSError::from)?;
        old_parent_dir
            .unlink(
                DirEntryName::try_from(old_name.as_str()).map_err(|_| FSError::Other)?,
                old_inode,
            )
            .map_err(FSError::from)?;

        Ok(())
    }

    fn link(&self, old_path: &Path, new_path: &Path) -> FSResult<()> {
        let source = self.lookup(old_path)?;
        let source_inode = match source {
            FileLike::File(file) => {
                let file = file.lock();
                let ext4_file = file
                    .as_any()
                    .downcast_ref::<Ext4File>()
                    .ok_or(FSError::Other)?;
                ext4_file.inode()
            }
            FileLike::Symlink(_) | FileLike::Directory(_) => return Err(FSError::Other),
        };

        let new_parent = new_path.parent().ok_or(FSError::NotFound)?;
        let new_name = new_path.file_name().ok_or(FSError::NotFound)?;
        let parent = self.lookup(&new_parent)?;
        let parent = match parent {
            FileLike::Directory(parent) => parent,
            FileLike::File(_) | FileLike::Symlink(_) => return Err(FSError::NotADirectory),
        };
        let parent = parent.lock();
        let ext4_parent = parent
            .as_any()
            .downcast_ref::<Ext4Directory>()
            .ok_or(FSError::Other)?;

        let parent_inode = ext4_parent.inode();
        let mut parent_dir =
            Dir::open_inode(ext4_parent.fs(), parent_inode).map_err(FSError::from)?;
        let mut source_inode = source_inode;
        parent_dir
            .link(
                DirEntryName::try_from(new_name.as_str()).map_err(|_| FSError::Other)?,
                &mut source_inode,
            )
            .map_err(FSError::from)?;
        ext4_parent.clear_lookup_cache();
        Ok(())
    }

    fn name(&self) -> &'static str {
        "ext4"
    }

    fn magic(&self) -> i64 {
        0xEF53
    }

    fn mount_source(&self) -> &'static str {
        "rootfs"
    }

    fn default_mount_flags(&self, _path: &Path) -> crate::filesystem::vfs_traits::MountFlags {
        crate::filesystem::vfs_traits::MountFlags::MS_RELATIME
    }
}

// The underlying ext4plus types are `Send + Sync`, so it is safe to
// share them behind our trait objects.
unsafe impl Sync for EXT4 {}
unsafe impl Sync for Ext4File {}
unsafe impl Send for Ext4File {}
unsafe impl Send for Ext4Directory {}
unsafe impl Sync for Ext4Directory {}
