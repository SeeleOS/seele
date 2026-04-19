use alloc::{format, string::String, sync::Arc, vec::Vec};
use spin::Mutex;

use crate::filesystem::{
    path::{Path, PathPart},
    vfs::FSResult,
    vfs_traits::{FileLike, FileSystem},
};

use super::{
    TMPFS_STATE, TmpNodeKind, TmpfsDirectoryHandle, TmpfsFileHandle, TmpfsState, TmpfsSymlinkHandle,
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
        "run".into()
    } else {
        path.rsplit('/')
            .next()
            .filter(|name| !name.is_empty())
            .unwrap_or("run")
            .into()
    }
}

pub(crate) fn tmpfs_lookup_path(path: &str) -> FSResult<FileLike> {
    let path = TmpfsState::normalize(path);
    let state = TMPFS_STATE.lock();
    let node = state.node(&path)?;
    Ok(match &node.kind {
        TmpNodeKind::Directory { .. } => {
            FileLike::Directory(Arc::new(Mutex::new(TmpfsDirectoryHandle::new(path))))
        }
        TmpNodeKind::File { .. } => {
            FileLike::File(Arc::new(Mutex::new(TmpfsFileHandle::new(path))))
        }
        TmpNodeKind::Symlink { .. } => {
            FileLike::Symlink(Arc::new(Mutex::new(TmpfsSymlinkHandle::new(path))))
        }
    })
}

pub struct TmpFs;

impl TmpFs {
    pub fn new() -> Self {
        Self
    }
}

impl FileSystem for TmpFs {
    fn init(&mut self) -> FSResult<()> {
        Ok(())
    }

    fn lookup(&self, path: &Path) -> FSResult<FileLike> {
        tmpfs_lookup_path(&absolute_tmp_path(path))
    }
}
