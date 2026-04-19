use alloc::{format, string::String, sync::Arc, vec::Vec};
use spin::Mutex;

use crate::filesystem::{
    path::{Path, PathPart},
    vfs::FSResult,
    vfs_traits::{FileLike, FileSystem},
};

use super::{
    TmpNodeKind, TmpfsDirectoryHandle, TmpfsFileHandle, TmpfsState, TmpfsStateRef,
    TmpfsSymlinkHandle,
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
    let state_guard = state.lock();
    let node = state_guard.node(&path)?;
    Ok(match &node.kind {
        TmpNodeKind::Directory { .. } => FileLike::Directory(Arc::new(Mutex::new(
            TmpfsDirectoryHandle::new(state.clone(), path),
        ))),
        TmpNodeKind::File { .. } => FileLike::File(Arc::new(Mutex::new(TmpfsFileHandle::new(
            state.clone(),
            path,
        )))),
        TmpNodeKind::Symlink { .. } => FileLike::Symlink(Arc::new(Mutex::new(
            TmpfsSymlinkHandle::new(state.clone(), path),
        ))),
    })
}

pub struct TmpFs {
    state: TmpfsStateRef,
}

impl TmpFs {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(TmpfsState::new())),
        }
    }
}

impl FileSystem for TmpFs {
    fn init(&mut self) -> FSResult<()> {
        Ok(())
    }

    fn lookup(&self, path: &Path) -> FSResult<FileLike> {
        tmpfs_lookup_path(&self.state, &absolute_tmp_path(path))
    }

    fn name(&self) -> &'static str {
        "tmpfs"
    }

    fn magic(&self) -> i64 {
        0x0102_1994
    }

    fn mount_source(&self) -> &'static str {
        "tmpfs"
    }

    fn mount_options(&self, _path: &Path) -> &'static str {
        "rw,nosuid,nodev,relatime"
    }
}
