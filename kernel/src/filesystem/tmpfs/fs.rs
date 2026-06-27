use crate::memory::utils::Mut;
use alloc::{format, string::String, sync::Arc, vec::Vec};

use crate::filesystem::{
    path::{Path, PathPart},
    vfs::FSResult,
    vfs_traits::{DirectoryContentType, FileLike, FileSystem},
};

use super::{
    TmpFsVariant, TmpNodeKind, TmpfsDeviceHandle, TmpfsDirectoryHandle, TmpfsFileHandle,
    TmpfsQuota, TmpfsState, TmpfsStateRef, TmpfsSymlinkHandle,
};

pub(crate) fn relative_components(path: &Path) -> Vec<String> {
    path.normalize()
        .parts
        .iter()
        .filter_map(|part| match part {
            PathPart::Normal(name) => Some(name.clone()),
            _ => None,
        })
        .collect()
}

pub(crate) fn absolute_tmp_path(path: &Path) -> String {
    let parts = relative_components(path);
    if parts.is_empty() {
        "/".into()
    } else {
        format!("/{}", parts.join("/"))
    }
}

pub(crate) fn node_name(path: &str) -> String {
    if path == "/" {
        String::new()
    } else {
        path.rsplit('/')
            .next()
            .filter(|name| !name.is_empty())
            .unwrap_or("run")
            .into()
    }
}

pub(crate) fn tmpfs_lookup_path(state: &TmpfsStateRef, path: &str) -> FSResult<FileLike> {
    let path = TmpfsState::normalize(path);
    let (inode, kind) = {
        let state_guard = state.lock();
        let node = state_guard.node(&path)?;
        let kind = match &node.kind {
            TmpNodeKind::Directory { .. } => DirectoryContentType::Directory,
            TmpNodeKind::File { .. } | TmpNodeKind::Device { .. } => DirectoryContentType::File,
            TmpNodeKind::Symlink { .. } => DirectoryContentType::Symlink,
        };
        (node.inode, kind)
    };

    Ok(match kind {
        DirectoryContentType::Directory => FileLike::Directory(Arc::new(Mut::new(
            TmpfsDirectoryHandle::new(state.clone(), path),
        ))),
        DirectoryContentType::File => {
            state.lock().retain_inode(inode)?;
            let device = {
                let state_guard = state.lock();
                match &state_guard.node_by_inode(inode)?.kind {
                    TmpNodeKind::Device { rdev, .. } => Some(TmpfsDeviceHandle::new(
                        state.clone(),
                        node_name(&path),
                        inode,
                        *rdev,
                    )?),
                    _ => None,
                }
            };
            match device {
                Some(device) => FileLike::File(Arc::new(Mut::new(device))),
                None => FileLike::File(Arc::new(Mut::new(TmpfsFileHandle::new(
                    state.clone(),
                    path,
                    inode,
                )))),
            }
        }
        DirectoryContentType::Symlink => FileLike::Symlink(Arc::new(Mut::new(
            TmpfsSymlinkHandle::new(state.clone(), path),
        ))),
    })
}

pub struct TmpFs {
    state: TmpfsStateRef,
    variant: TmpFsVariant,
}

impl TmpFs {
    pub fn new() -> Self {
        Self::with_variant(TmpFsVariant::TmpFs)
    }

    pub fn ramfs() -> Self {
        Self::with_variant(TmpFsVariant::RamFs)
    }

    fn with_variant(variant: TmpFsVariant) -> Self {
        Self {
            state: Arc::new(Mut::new(TmpfsState::new())),
            variant,
        }
    }

    pub(crate) fn quota(&self, quota_type: u32, id: u32) -> Option<TmpfsQuota> {
        self.state.lock().quota(quota_type, id)
    }

    pub(crate) fn set_quota(&self, quota_type: u32, id: u32, quota: TmpfsQuota) -> bool {
        self.state.lock().set_quota(quota_type, id, quota)
    }
}

impl Default for TmpFs {
    fn default() -> Self {
        Self::new()
    }
}

impl FileSystem for TmpFs {
    fn as_any(&self) -> &dyn core::any::Any {
        self
    }

    fn init(&mut self) -> FSResult<()> {
        Ok(())
    }

    fn lookup(&self, path: &Path) -> FSResult<FileLike> {
        tmpfs_lookup_path(&self.state, &absolute_tmp_path(path))
    }

    fn rename(&self, old_path: &Path, new_path: &Path) -> FSResult<()> {
        self.state
            .lock()
            .rename(&absolute_tmp_path(old_path), &absolute_tmp_path(new_path))
    }

    fn link(&self, old_path: &Path, new_path: &Path) -> FSResult<()> {
        self.state
            .lock()
            .link(&absolute_tmp_path(old_path), &absolute_tmp_path(new_path))
    }

    fn name(&self) -> &'static str {
        self.variant.name()
    }

    fn magic(&self) -> i64 {
        self.variant.magic()
    }

    fn mount_source(&self) -> &'static str {
        self.variant.mount_source()
    }

    fn default_mount_flags(&self, _path: &Path) -> crate::filesystem::vfs_traits::MountFlags {
        self.variant.default_mount_flags()
    }
}
