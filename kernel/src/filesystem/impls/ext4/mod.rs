use crate::memory::utils::Mut;
use alloc::{
    collections::{BTreeMap, VecDeque},
    string::String,
    string::ToString,
    sync::Arc,
};

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
use anyhow::{Context, Result};

pub mod directory;
pub mod error;
pub mod file;
pub mod operator;
pub mod symlink;

const CHMOD_PERMISSION_BITS: u16 = 0o7777;
const FILE_TYPE_BITS: u16 = 0o170000;
const MAX_LOOKUP_CACHE_ENTRIES: usize = 16_384;
pub(crate) type LookupCache = Arc<Mut<LookupCacheState>>;
pub(crate) type OperationLock = Arc<Mut<()>>;

#[derive(Debug, Default)]
pub struct LookupCacheState {
    entries: BTreeMap<u32, BTreeMap<String, Inode>>,
    order: VecDeque<(u32, String)>,
}

impl LookupCacheState {
    fn get(&self, parent_inode: u32, name: &str) -> Option<Inode> {
        self.entries
            .get(&parent_inode)
            .and_then(|children| children.get(name))
            .cloned()
    }

    fn insert(&mut self, parent_inode: u32, name: &str, inode: &Inode) {
        let children = self.entries.entry(parent_inode).or_default();
        let is_new = !children.contains_key(name);
        children.insert(name.into(), inode.clone());
        if is_new {
            self.order.push_back((parent_inode, name.into()));
        }
        self.evict_to_limit();
    }

    fn remove(&mut self, parent_inode: u32, name: &str) {
        let Some(children) = self.entries.get_mut(&parent_inode) else {
            return;
        };
        children.remove(name);
        if children.is_empty() {
            self.entries.remove(&parent_inode);
        }
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
    }

    fn evict_to_limit(&mut self) {
        while self.order.len() > MAX_LOOKUP_CACHE_ENTRIES {
            let Some((parent_inode, name)) = self.order.pop_front() else {
                return;
            };
            self.remove(parent_inode, &name);
        }
    }
}

pub(super) fn lookup_cache_get(
    cache: &LookupCache,
    parent_inode: &Inode,
    name: &str,
) -> Option<Inode> {
    cache.lock().get(parent_inode.index.get(), name)
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
    cache.lock().insert(parent_inode, name, inode);
}

pub(super) fn lookup_cache_remove(cache: &LookupCache, parent_inode: &Inode, name: &str) {
    lookup_cache_remove_raw(cache, parent_inode.index.get(), name);
}

pub(super) fn lookup_cache_remove_raw(cache: &LookupCache, parent_inode: u32, name: &str) {
    cache.lock().remove(parent_inode, name);
}

pub(super) fn lookup_cache_clear(cache: &LookupCache) {
    cache.lock().clear();
}

#[cfg(test)]
pub(super) fn lookup_cache_contains_raw(
    cache: &LookupCache,
    parent_inode: u32,
    name: &str,
) -> bool {
    cache.lock().get(parent_inode, name).is_some()
}

pub(super) fn chmod_inode(fs: &Ext4, inode: &mut Inode, mode: u32) -> FSResult<()> {
    let requested_bits = (mode as u16) & CHMOD_PERMISSION_BITS;
    let requested_mode = InodeMode::from_bits(requested_bits).ok_or(FSError::Other)?;
    let merged_bits = (inode.mode().bits() & FILE_TYPE_BITS) | requested_mode.bits();
    let merged_mode = InodeMode::from_bits(merged_bits).ok_or(FSError::Other)?;
    inode.set_mode(merged_mode).map_err(FSError::from)?;
    inode.write(fs).map_err(FSError::from)?;
    Ok(())
}

pub(super) fn chown_inode(fs: &Ext4, inode: &mut Inode, uid: u32, gid: u32) -> FSResult<()> {
    if uid != u32::MAX {
        inode.set_uid(uid);
    }
    if gid != u32::MAX {
        inode.set_gid(gid);
    }
    inode.write(fs).map_err(FSError::from)?;
    Ok(())
}

/// Wrapper around the `ext4plus::Ext4` filesystem so it can be used
/// through the kernel's generic `FileSystem` trait.
pub struct EXT4 {
    fs: Ext4,
    root_inode: Inode,
    lookup_cache: LookupCache,
    operation_lock: OperationLock,
}

impl EXT4 {
    pub fn new(fs: Ext4) -> Result<Self> {
        let root_inode = fs
            .path_to_inode(Ext4Path::new("/"), FollowSymlinks::All)
            .context("ext4 root inode must exist")?;
        Ok(Self {
            fs,
            root_inode,
            lookup_cache: Arc::new(Mut::new(LookupCacheState::default())),
            operation_lock: Arc::new(Mut::new(())),
        })
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
        Arc::new(Mut::new(Ext4Directory::new(
            "".to_string(),
            "/".to_string(),
            self.fs.clone(),
            self.root_inode.clone(),
            None,
            self.lookup_cache.clone(),
            self.operation_lock.clone(),
        )))
    }
}

impl FileSystem for EXT4 {
    fn as_any(&self) -> &dyn core::any::Any {
        self
    }

    fn init(&mut self) -> FSResult<()> {
        Ok(())
    }

    fn lookup(&self, path: &Path) -> FSResult<FileLike> {
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

    fn rename(&self, old_path: &Path, new_path: &Path) -> FSResult<()> {
        let _operation = self.operation_lock.lock();
        lookup_cache_clear(&self.lookup_cache);
        let old_path = old_path.normalize();
        let new_path = new_path.normalize();
        if old_path == new_path {
            return Ok(());
        }

        let source_inode = self
            .fs
            .path_to_inode(
                Ext4Path::new(&old_path.clone().as_string()),
                FollowSymlinks::ExcludeFinalComponent,
            )
            .map_err(FSError::from)?;
        if source_inode.metadata().is_dir() {
            return Err(FSError::Other);
        }

        let old_parent = old_path.parent().ok_or(FSError::NotFound)?;
        let old_name = old_path.file_name().ok_or(FSError::NotFound)?;
        let new_parent = new_path.parent().ok_or(FSError::NotFound)?;
        let new_name = new_path.file_name().ok_or(FSError::NotFound)?;

        if let Ok(target_inode) = self.fs.path_to_inode(
            Ext4Path::new(&new_path.clone().as_string()),
            FollowSymlinks::ExcludeFinalComponent,
        ) {
            if target_inode.metadata().is_dir() {
                return Err(FSError::DirectoryNotEmpty);
            }
            let new_parent_inode = match self.fs.path_to_inode(
                Ext4Path::new(&new_parent.clone().as_string()),
                FollowSymlinks::All,
            ) {
                Ok(inode) => inode,
                Err(err) => return Err(FSError::from(err)),
            };
            let mut new_parent_dir = match Dir::open_inode(&self.fs, new_parent_inode) {
                Ok(dir) => dir,
                Err(err) => return Err(FSError::from(err)),
            };
            if let Err(err) = new_parent_dir.unlink(
                DirEntryName::try_from(new_name.as_str()).map_err(|_| FSError::Other)?,
                target_inode,
            ) {
                return Err(FSError::from(err));
            }
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
        if let Err(err) = new_parent_dir.link(
            DirEntryName::try_from(new_name.as_str()).map_err(|_| FSError::Other)?,
            &mut source_inode,
        ) {
            return Err(FSError::from(err));
        }

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
        if let Err(err) = old_parent_dir.unlink(
            DirEntryName::try_from(old_name.as_str()).map_err(|_| FSError::Other)?,
            old_inode,
        ) {
            return Err(FSError::from(err));
        }

        lookup_cache_clear(&self.lookup_cache);
        Ok(())
    }

    fn link(&self, old_path: &Path, new_path: &Path) -> FSResult<()> {
        let _operation = self.operation_lock.lock();
        let source_inode = self
            .fs
            .path_to_inode(
                Ext4Path::new(&old_path.clone().as_string()),
                FollowSymlinks::ExcludeFinalComponent,
            )
            .map_err(FSError::from)?;
        let metadata = source_inode.metadata();
        if metadata.is_dir() || metadata.is_symlink() {
            return Err(FSError::Other);
        }

        let new_parent = new_path.parent().ok_or(FSError::NotFound)?;
        let new_name = new_path.file_name().ok_or(FSError::NotFound)?;
        let parent_inode = self
            .fs
            .path_to_inode(
                Ext4Path::new(&new_parent.clone().as_string()),
                FollowSymlinks::All,
            )
            .map_err(FSError::from)?;
        if !parent_inode.metadata().is_dir() {
            return Err(FSError::NotADirectory);
        }
        let mut parent_dir = Dir::open_inode(&self.fs, parent_inode).map_err(FSError::from)?;
        let mut source_inode = source_inode;
        parent_dir
            .link(
                DirEntryName::try_from(new_name.as_str()).map_err(|_| FSError::Other)?,
                &mut source_inode,
            )
            .map_err(FSError::from)?;
        lookup_cache_clear(&self.lookup_cache);
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

#[cfg(test)]
crate::test!(
    ext4_lookup_cache_nested_structure,
    "ext4 lookup cache nested structure preserves lookup semantics",
    ext4_lookup_cache_nested_structure_preserves_lookup_semantics
);

#[cfg(test)]
crate::test!(
    ext4_lookup_cache_limits_entries,
    "ext4 lookup cache evicts old entries at its size limit",
    ext4_lookup_cache_evicts_old_entries_at_its_size_limit
);

#[cfg(test)]
fn ext4_lookup_cache_nested_structure_preserves_lookup_semantics() {
    let cache: LookupCache = Arc::new(Mut::new(LookupCacheState::default()));
    let root = crate::filesystem::vfs::VirtualFS
        .lock()
        .resolve_dir(crate::filesystem::path::Path::new("/"))
        .unwrap();
    let root = root.lock();
    let ext4_root = root.as_any().downcast_ref::<Ext4Directory>().unwrap();
    let inode_a = ext4_root.inode();
    let inode_b = ext4_root.inode();

    lookup_cache_insert_raw(&cache, 7, "alpha", &inode_a);
    lookup_cache_insert_raw(&cache, 7, "beta", &inode_b);

    assert!(lookup_cache_contains_raw(&cache, 7, "alpha"));
    assert!(lookup_cache_contains_raw(&cache, 7, "beta"));

    lookup_cache_remove_raw(&cache, 7, "alpha");
    assert!(!lookup_cache_contains_raw(&cache, 7, "alpha"));
    assert!(lookup_cache_contains_raw(&cache, 7, "beta"));

    lookup_cache_clear(&cache);
    assert!(!lookup_cache_contains_raw(&cache, 7, "beta"));
}

#[cfg(test)]
fn ext4_lookup_cache_evicts_old_entries_at_its_size_limit() {
    let cache: LookupCache = Arc::new(Mut::new(LookupCacheState::default()));
    let root = crate::filesystem::vfs::VirtualFS
        .lock()
        .resolve_dir(crate::filesystem::path::Path::new("/"))
        .unwrap();
    let root = root.lock();
    let ext4_root = root.as_any().downcast_ref::<Ext4Directory>().unwrap();
    let inode = ext4_root.inode();

    for index in 0..=MAX_LOOKUP_CACHE_ENTRIES {
        lookup_cache_insert_raw(&cache, 7, &alloc::format!("entry-{index}"), &inode);
    }

    assert!(!lookup_cache_contains_raw(&cache, 7, "entry-0"));
    assert!(lookup_cache_contains_raw(
        &cache,
        7,
        &alloc::format!("entry-{MAX_LOOKUP_CACHE_ENTRIES}")
    ));
}
