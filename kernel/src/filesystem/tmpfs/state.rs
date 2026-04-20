use alloc::{
    collections::{BTreeMap, BTreeSet},
    string::String,
    sync::Arc,
    vec::Vec,
};
use spin::Mutex;

use crate::filesystem::{errors::FSError, path::Path, vfs::FSResult};

const ROOT_INODE: u64 = 0x7000_0000;
pub(crate) const DEFAULT_DIR_MODE: u32 = 0o755;
pub(crate) const DEFAULT_FILE_MODE: u32 = 0o644;
pub(crate) const S_IFMT: u32 = 0o170000;
pub(crate) type TmpfsStateRef = Arc<Mutex<TmpfsState>>;

pub(crate) enum TmpNodeKind {
    Directory {
        children: BTreeSet<String>,
        mode: u32,
    },
    File {
        data: Vec<u8>,
        mode: u32,
    },
    Symlink {
        target: String,
    },
}

pub(crate) struct TmpNode {
    pub(crate) inode: u64,
    pub(crate) kind: TmpNodeKind,
}

pub(crate) struct TmpfsState {
    next_inode: u64,
    nodes: BTreeMap<String, TmpNode>,
}

impl TmpfsState {
    pub(crate) fn new() -> Self {
        let mut nodes = BTreeMap::new();
        nodes.insert(
            "/".into(),
            TmpNode {
                inode: ROOT_INODE,
                kind: TmpNodeKind::Directory {
                    children: BTreeSet::new(),
                    mode: DEFAULT_DIR_MODE,
                },
            },
        );
        Self {
            next_inode: ROOT_INODE + 1,
            nodes,
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
        self.nodes.get(path).ok_or(FSError::NotFound)
    }

    pub(crate) fn node_mut(&mut self, path: &str) -> FSResult<&mut TmpNode> {
        self.nodes.get_mut(path).ok_or(FSError::NotFound)
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
                data: Vec::new(),
                mode: DEFAULT_FILE_MODE,
            },
        )
    }

    pub(crate) fn create_directory(&mut self, parent: &str, name: &str) -> FSResult<()> {
        self.create_node(
            parent,
            name,
            TmpNodeKind::Directory {
                children: BTreeSet::new(),
                mode: DEFAULT_DIR_MODE,
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
        if self.nodes.contains_key(&child) {
            return Err(FSError::AlreadyExists);
        }
        let _ = self.directory_children_mut(&parent)?;
        let inode = self.next_inode;
        self.next_inode += 1;
        self.nodes.insert(child, TmpNode { inode, kind });
        self.directory_children_mut(&parent)?.insert(name.into());
        Ok(())
    }

    pub(crate) fn delete_node(&mut self, parent: &str, name: &str) -> FSResult<()> {
        let parent = Self::normalize(parent);
        let child = Self::child_path(&parent, name);
        let node = self.node(&child)?;
        if let TmpNodeKind::Directory { children, .. } = &node.kind
            && !children.is_empty()
        {
            return Err(FSError::DirectoryNotEmpty);
        }
        self.nodes.remove(&child);
        self.directory_children_mut(&parent)?.remove(name);
        Ok(())
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
            .nodes
            .keys()
            .filter(|path| **path == old_path || path.starts_with(&prefix))
            .cloned()
            .collect::<Vec<_>>();

        let mut moved_nodes = Vec::with_capacity(moved_paths.len());
        for path in moved_paths {
            let suffix: String = path.strip_prefix(&old_path).ok_or(FSError::Other)?.into();
            let node = self.nodes.remove(&path).ok_or(FSError::NotFound)?;
            moved_nodes.push((suffix, node));
        }

        self.directory_children_mut(&old_parent)?.remove(&old_name);
        self.directory_children_mut(&new_parent)?.insert(new_name);

        for (suffix, node) in moved_nodes {
            self.nodes
                .insert(alloc::format!("{new_path}{suffix}"), node);
        }

        Ok(())
    }

    pub(crate) fn update_file_mode(&mut self, path: &str, mode: u32) -> FSResult<()> {
        let node = self.node_mut(path)?;
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
