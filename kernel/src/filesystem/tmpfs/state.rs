use crate::memory::utils::Mut;
use alloc::{
    collections::{BTreeMap, BTreeSet},
    string::String,
    sync::Arc,
    vec::Vec,
};

use crate::filesystem::{errors::FSError, path::Path, sparse_file::SparseFileData, vfs::FSResult};

const ROOT_INODE: u64 = 0x7000_0000;
pub(crate) const DEFAULT_DIR_MODE: u32 = 0o755;
pub(crate) const DEFAULT_FILE_MODE: u32 = 0o644;
pub(crate) const S_IFMT: u32 = 0o170000;
pub(crate) type TmpfsStateRef = Arc<Mut<TmpfsState>>;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TmpfsQuota {
    pub(crate) block_hardlimit: u64,
    pub(crate) block_softlimit: u64,
    pub(crate) inode_hardlimit: u64,
    pub(crate) inode_softlimit: u64,
    pub(crate) current_space: u64,
    pub(crate) current_inodes: u64,
    pub(crate) block_time: u64,
    pub(crate) inode_time: u64,
    pub(crate) valid: u32,
}

pub(crate) enum TmpNodeKind {
    Directory {
        children: BTreeSet<String>,
        mode: u32,
    },
    File {
        data: SparseFileData,
        mode: u32,
    },
    Symlink {
        target: String,
    },
}

pub(crate) struct TmpNode {
    pub(crate) inode: u64,
    pub(crate) link_count: u64,
    pub(crate) open_count: u64,
    pub(crate) kind: TmpNodeKind,
}

pub(crate) struct TmpfsState {
    next_inode: u64,
    paths: BTreeMap<String, u64>,
    nodes: BTreeMap<u64, TmpNode>,
    user_quotas: BTreeMap<u32, TmpfsQuota>,
    group_quotas: BTreeMap<u32, TmpfsQuota>,
    project_quotas: BTreeMap<u32, TmpfsQuota>,
}

impl TmpfsState {
    pub(crate) fn new() -> Self {
        let mut paths = BTreeMap::new();
        paths.insert("/".into(), ROOT_INODE);
        let mut nodes = BTreeMap::new();
        nodes.insert(
            ROOT_INODE,
            TmpNode {
                inode: ROOT_INODE,
                link_count: 1,
                open_count: 0,
                kind: TmpNodeKind::Directory {
                    children: BTreeSet::new(),
                    mode: DEFAULT_DIR_MODE,
                },
            },
        );
        Self {
            next_inode: ROOT_INODE + 1,
            paths,
            nodes,
            user_quotas: BTreeMap::new(),
            group_quotas: BTreeMap::new(),
            project_quotas: BTreeMap::new(),
        }
    }

    pub(crate) fn normalize(path: &str) -> String {
        if path.is_empty() || path == "/" {
            "/".into()
        } else {
            Path::new(path).normalize().as_string()
        }
    }

    pub(crate) fn child_path(parent: &str, name: &str) -> String {
        if parent == "/" {
            alloc::format!("/{name}")
        } else {
            alloc::format!("{parent}/{name}")
        }
    }

    pub(crate) fn node(&self, path: &str) -> FSResult<&TmpNode> {
        let inode = self.inode_for_path(path)?;
        self.nodes.get(&inode).ok_or(FSError::NotFound)
    }

    pub(crate) fn node_mut(&mut self, path: &str) -> FSResult<&mut TmpNode> {
        let inode = self.inode_for_path(path)?;
        self.nodes.get_mut(&inode).ok_or(FSError::NotFound)
    }

    pub(crate) fn node_by_inode(&self, inode: u64) -> FSResult<&TmpNode> {
        self.nodes.get(&inode).ok_or(FSError::NotFound)
    }

    pub(crate) fn node_by_inode_mut(&mut self, inode: u64) -> FSResult<&mut TmpNode> {
        self.nodes.get_mut(&inode).ok_or(FSError::NotFound)
    }

    fn inode_for_path(&self, path: &str) -> FSResult<u64> {
        self.paths.get(path).copied().ok_or(FSError::NotFound)
    }

    fn directory_children_mut(&mut self, path: &str) -> FSResult<&mut BTreeSet<String>> {
        let node = self.node_mut(path)?;
        match &mut node.kind {
            TmpNodeKind::Directory { children, .. } => Ok(children),
            TmpNodeKind::File { .. } | TmpNodeKind::Symlink { .. } => Err(FSError::NotADirectory),
        }
    }

    pub(crate) fn create_file(&mut self, parent: &str, name: &str) -> FSResult<()> {
        self.create_node(
            parent,
            name,
            TmpNodeKind::File {
                data: SparseFileData::new(),
                mode: DEFAULT_FILE_MODE,
            },
        )
    }

    pub(crate) fn create_directory(&mut self, parent: &str, name: &str, mode: u32) -> FSResult<()> {
        self.create_node(
            parent,
            name,
            TmpNodeKind::Directory {
                children: BTreeSet::new(),
                mode: mode & 0o7777,
            },
        )
    }

    pub(crate) fn create_symlink(
        &mut self,
        parent: &str,
        name: &str,
        target: &str,
    ) -> FSResult<()> {
        self.create_node(
            parent,
            name,
            TmpNodeKind::Symlink {
                target: target.into(),
            },
        )
    }

    fn create_node(&mut self, parent: &str, name: &str, kind: TmpNodeKind) -> FSResult<()> {
        let parent = Self::normalize(parent);
        let child = Self::child_path(&parent, name);
        if self.paths.contains_key(&child) {
            return Err(FSError::AlreadyExists);
        }
        let _ = self.directory_children_mut(&parent)?;
        let inode = self.next_inode;
        self.next_inode += 1;
        self.paths.insert(child, inode);
        self.nodes.insert(
            inode,
            TmpNode {
                inode,
                link_count: 1,
                open_count: 0,
                kind,
            },
        );
        self.directory_children_mut(&parent)?.insert(name.into());
        Ok(())
    }

    pub(crate) fn delete_node(&mut self, parent: &str, name: &str) -> FSResult<()> {
        let parent = Self::normalize(parent);
        let child = Self::child_path(&parent, name);
        let inode = self.inode_for_path(&child)?;
        let node = self.node_by_inode(inode)?;
        if let TmpNodeKind::Directory { children, .. } = &node.kind
            && !children.is_empty()
        {
            return Err(FSError::DirectoryNotEmpty);
        }
        let remove_node = node.link_count == 1;
        if !remove_node {
            let node = self.node_by_inode_mut(inode)?;
            node.link_count = node.link_count.checked_sub(1).ok_or(FSError::Other)?;
        }
        self.paths.remove(&child);
        self.directory_children_mut(&parent)?.remove(name);
        if remove_node {
            let remove_inode = self.node_by_inode(inode)?.open_count == 0;
            if !remove_inode {
                let node = self.node_by_inode_mut(inode)?;
                node.link_count = 0;
            } else {
                self.nodes.remove(&inode);
            }
        }
        Ok(())
    }

    pub(crate) fn retain_inode(&mut self, inode: u64) -> FSResult<()> {
        let node = self.node_by_inode_mut(inode)?;
        node.open_count = node.open_count.checked_add(1).ok_or(FSError::Other)?;
        Ok(())
    }

    pub(crate) fn release_inode(&mut self, inode: u64) -> FSResult<()> {
        let remove_inode = {
            let node = self.node_by_inode_mut(inode)?;
            node.open_count = node.open_count.checked_sub(1).ok_or(FSError::Other)?;
            node.open_count == 0 && node.link_count == 0
        };
        if remove_inode {
            self.nodes.remove(&inode);
        }
        Ok(())
    }

    pub(crate) fn quota(&self, quota_type: u32, id: u32) -> Option<TmpfsQuota> {
        self.quota_map(quota_type)?.get(&id).copied()
    }

    pub(crate) fn set_quota(&mut self, quota_type: u32, id: u32, quota: TmpfsQuota) -> bool {
        let Some(map) = self.quota_map_mut(quota_type) else {
            return false;
        };
        map.insert(id, quota);
        true
    }

    fn quota_map(&self, quota_type: u32) -> Option<&BTreeMap<u32, TmpfsQuota>> {
        match quota_type {
            0 => Some(&self.user_quotas),
            1 => Some(&self.group_quotas),
            2 => Some(&self.project_quotas),
            _ => None,
        }
    }

    fn quota_map_mut(&mut self, quota_type: u32) -> Option<&mut BTreeMap<u32, TmpfsQuota>> {
        match quota_type {
            0 => Some(&mut self.user_quotas),
            1 => Some(&mut self.group_quotas),
            2 => Some(&mut self.project_quotas),
            _ => None,
        }
    }

    fn split_path(path: &str) -> FSResult<(String, String)> {
        let path = Self::normalize(path);
        if path == "/" {
            return Err(FSError::AccessDenied);
        }

        let path = Path::new(&path);
        let parent = path.parent().ok_or(FSError::NotFound)?.as_string();
        let name = path.file_name().ok_or(FSError::NotFound)?;
        Ok((parent, name))
    }

    fn delete_path(&mut self, path: &str) -> FSResult<()> {
        let (parent, name) = Self::split_path(path)?;
        self.delete_node(&parent, &name)
    }

    pub(crate) fn link(&mut self, old_path: &str, new_path: &str) -> FSResult<()> {
        let old_path = Self::normalize(old_path);
        let inode = self.inode_for_path(&old_path)?;
        self.link_inode(inode, new_path)
    }

    pub(crate) fn link_inode(&mut self, inode: u64, new_path: &str) -> FSResult<()> {
        let new_path = Self::normalize(new_path);
        if new_path == "/" {
            return Err(FSError::AccessDenied);
        }
        if self.paths.contains_key(&new_path) {
            return Err(FSError::AlreadyExists);
        }

        let node = self.node_by_inode(inode)?;
        if !matches!(node.kind, TmpNodeKind::File { .. }) {
            return Err(FSError::Other);
        }

        let (new_parent, new_name) = Self::split_path(&new_path)?;
        let _ = self.directory_children_mut(&new_parent)?;

        self.paths.insert(new_path, inode);
        self.directory_children_mut(&new_parent)?.insert(new_name);
        let node = self.node_by_inode_mut(inode)?;
        node.link_count = node.link_count.checked_add(1).ok_or(FSError::Other)?;
        Ok(())
    }

    pub(crate) fn rename(&mut self, old_path: &str, new_path: &str) -> FSResult<()> {
        let old_path = Self::normalize(old_path);
        let new_path = Self::normalize(new_path);
        if old_path == new_path {
            return Ok(());
        }
        if old_path == "/" || new_path == "/" {
            return Err(FSError::AccessDenied);
        }

        let (old_parent, old_name) = Self::split_path(&old_path)?;
        let (new_parent, new_name) = Self::split_path(&new_path)?;
        let source_is_dir = matches!(self.node(&old_path)?.kind, TmpNodeKind::Directory { .. });
        if source_is_dir && new_path.starts_with(&(old_path.clone() + "/")) {
            return Err(FSError::AccessDenied);
        }

        let _ = self.directory_children_mut(&old_parent)?;
        let _ = self.directory_children_mut(&new_parent)?;

        if let Ok(target) = self.node(&new_path) {
            let target_is_dir = matches!(target.kind, TmpNodeKind::Directory { .. });
            if source_is_dir && !target_is_dir {
                return Err(FSError::NotADirectory);
            }
            if !source_is_dir && target_is_dir {
                return Err(FSError::NotAFile);
            }
            self.delete_path(&new_path)?;
        }

        let prefix = alloc::format!("{old_path}/");
        let moved_paths = self
            .paths
            .keys()
            .filter(|path| **path == old_path || path.starts_with(&prefix))
            .cloned()
            .collect::<Vec<_>>();

        let mut moved_nodes = Vec::with_capacity(moved_paths.len());
        for path in moved_paths {
            let suffix: String = path.strip_prefix(&old_path).ok_or(FSError::Other)?.into();
            let inode = self.paths.remove(&path).ok_or(FSError::NotFound)?;
            moved_nodes.push((suffix, inode));
        }

        self.directory_children_mut(&old_parent)?.remove(&old_name);
        self.directory_children_mut(&new_parent)?.insert(new_name);

        for (suffix, inode) in moved_nodes {
            self.paths
                .insert(alloc::format!("{new_path}{suffix}"), inode);
        }

        Ok(())
    }

    pub(crate) fn update_file_mode_by_inode(&mut self, inode: u64, mode: u32) -> FSResult<()> {
        let node = self.node_by_inode_mut(inode)?;
        match &mut node.kind {
            TmpNodeKind::File {
                mode: file_mode, ..
            } => {
                if (mode & S_IFMT) != 0 {
                    *file_mode = mode;
                } else {
                    *file_mode = (*file_mode & S_IFMT) | (mode & 0o7777);
                }
                Ok(())
            }
            TmpNodeKind::Directory { .. } | TmpNodeKind::Symlink { .. } => Err(FSError::NotAFile),
        }
    }
}
