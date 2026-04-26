use alloc::{
    collections::BTreeSet,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use spin::Mutex;

use crate::{
    drm::fs::DEV_DRI_NODE,
    filesystem::{
        errors::FSError,
        info::{DirectoryContentInfo, FileLikeInfo, UnixPermission},
        path::{Path, PathPart},
        staticfs::{
            StaticDeviceNode, StaticDirEntry, StaticDirectoryNode, StaticNode, StaticSymlinkNode,
            device::StaticDeviceHandle, directory::StaticDirectoryHandle,
        },
        tmpfs::{TmpNodeKind, TmpfsState, TmpfsStateRef, tmpfs_lookup_path},
        vfs::FSResult,
        vfs_traits::{Directory, DirectoryContentType, FileLike, FileLikeType, FileSystem},
    },
    terminal::pty::{get_pty_slave, list_ptys},
};

static DEV_NULL_NODE: StaticNode = StaticNode::Device(StaticDeviceNode {
    name: "null",
    inode: 0x1001,
    mode: 0o020666,
    device_name: "devnull",
    rdev: Some((1u64 << 8) | 3),
});

static DEV_TTY_NODE: StaticNode = StaticNode::Device(StaticDeviceNode {
    name: "tty",
    inode: 0x1002,
    mode: 0o020666,
    device_name: "tty",
    rdev: Some(5u64 << 8),
});

static DEV_CONSOLE_NODE: StaticNode = StaticNode::Device(StaticDeviceNode {
    name: "console",
    inode: 0x1003,
    mode: 0o020600,
    device_name: "console",
    rdev: Some((5u64 << 8) | 1),
});

static DEV_TTY0_NODE: StaticNode = StaticNode::Device(StaticDeviceNode {
    name: "tty0",
    inode: 0x1004,
    mode: 0o020620,
    device_name: "tty0",
    rdev: Some(4u64 << 8),
});

static DEV_TTY1_NODE: StaticNode = StaticNode::Device(StaticDeviceNode {
    name: "tty1",
    inode: 0x1005,
    mode: 0o020620,
    device_name: "tty1",
    rdev: Some((4u64 << 8) | 1),
});

static DEV_FB0_NODE: StaticNode = StaticNode::Device(StaticDeviceNode {
    name: "fb0",
    inode: 0x1006,
    mode: 0o020666,
    device_name: "framebuffer",
    rdev: Some(29u64 << 8),
});

static DEV_PSAUX_NODE: StaticNode = StaticNode::Device(StaticDeviceNode {
    name: "psaux",
    inode: 0x1007,
    mode: 0o020666,
    device_name: "ps2mouse",
    rdev: Some((10u64 << 8) | 1),
});

static DEV_MOUSE_NODE: StaticNode = StaticNode::Device(StaticDeviceNode {
    name: "mouse",
    inode: 0x1008,
    mode: 0o020666,
    device_name: "ps2mouse",
    rdev: Some((13u64 << 8) | 32),
});

static DEV_INPUT_EVENT0_NODE: StaticNode = StaticNode::Device(StaticDeviceNode {
    name: "event0",
    inode: 0x100A,
    mode: 0o020660,
    device_name: "event-kbd",
    rdev: Some((13u64 << 8) | 64),
});

static DEV_INPUT_EVENT1_NODE: StaticNode = StaticNode::Device(StaticDeviceNode {
    name: "event1",
    inode: 0x100B,
    mode: 0o020660,
    device_name: "event-mouse",
    rdev: Some((13u64 << 8) | 65),
});

static DEV_INPUT_ENTRIES: &[StaticDirEntry] = &[
    StaticDirEntry {
        name: "event0",
        node: &DEV_INPUT_EVENT0_NODE,
    },
    StaticDirEntry {
        name: "event1",
        node: &DEV_INPUT_EVENT1_NODE,
    },
];

static DEV_INPUT_NODE: StaticNode = StaticNode::Directory(StaticDirectoryNode {
    name: "input",
    inode: 0x1009,
    mode: 0o040755,
    entries: DEV_INPUT_ENTRIES,
});

static DEV_PTMX_NODE: StaticNode = StaticNode::Device(StaticDeviceNode {
    name: "ptmx",
    inode: 0x1010,
    mode: 0o020666,
    device_name: "ptmx",
    rdev: Some((5u64 << 8) | 2),
});

static DEV_PTS_NODE: StaticNode = StaticNode::Directory(StaticDirectoryNode {
    name: "pts",
    inode: 0x100c,
    mode: 0o040755,
    entries: &[],
});

static DEV_SHM_NODE: StaticNode = StaticNode::Directory(StaticDirectoryNode {
    name: "shm",
    inode: 0x100d,
    mode: 0o040777,
    entries: &[],
});

static DEV_LOG_NODE: StaticNode = StaticNode::Symlink(StaticSymlinkNode {
    name: "log",
    inode: 0x100e,
    mode: 0o120777,
    target: "/run/systemd/journal/dev-log",
});

static DEV_KMSG_NODE: StaticNode = StaticNode::Device(StaticDeviceNode {
    name: "kmsg",
    inode: 0x100f,
    mode: 0o020600,
    device_name: "kmsg",
    rdev: None,
});

static DEV_ROOT_ENTRIES: &[StaticDirEntry] = &[
    StaticDirEntry {
        name: "null",
        node: &DEV_NULL_NODE,
    },
    StaticDirEntry {
        name: "tty",
        node: &DEV_TTY_NODE,
    },
    StaticDirEntry {
        name: "console",
        node: &DEV_CONSOLE_NODE,
    },
    StaticDirEntry {
        name: "tty0",
        node: &DEV_TTY0_NODE,
    },
    StaticDirEntry {
        name: "tty1",
        node: &DEV_TTY1_NODE,
    },
    StaticDirEntry {
        name: "fb0",
        node: &DEV_FB0_NODE,
    },
    StaticDirEntry {
        name: "psaux",
        node: &DEV_PSAUX_NODE,
    },
    StaticDirEntry {
        name: "mouse",
        node: &DEV_MOUSE_NODE,
    },
    StaticDirEntry {
        name: "input",
        node: &DEV_INPUT_NODE,
    },
    StaticDirEntry {
        name: "dri",
        node: &DEV_DRI_NODE,
    },
    StaticDirEntry {
        name: "ptmx",
        node: &DEV_PTMX_NODE,
    },
    StaticDirEntry {
        name: "pts",
        node: &DEV_PTS_NODE,
    },
    StaticDirEntry {
        name: "shm",
        node: &DEV_SHM_NODE,
    },
    StaticDirEntry {
        name: "log",
        node: &DEV_LOG_NODE,
    },
    StaticDirEntry {
        name: "kmsg",
        node: &DEV_KMSG_NODE,
    },
];

static DEV_ROOT_NODE: StaticNode = StaticNode::Directory(StaticDirectoryNode {
    name: "dev",
    inode: 0x1000,
    mode: 0o040755,
    entries: DEV_ROOT_ENTRIES,
});

pub struct DevFs {
    state: TmpfsStateRef,
}

struct DevDirectoryHandle {
    state: TmpfsStateRef,
    path: String,
    node: &'static StaticDirectoryNode,
}
struct DevPtsDirectoryHandle;

fn root_directory_node() -> &'static StaticDirectoryNode {
    let StaticNode::Directory(node) = &DEV_ROOT_NODE else {
        unreachable!()
    };
    node
}

fn root_directory_file_like(state: TmpfsStateRef) -> FileLike {
    static_directory_file_like(state, "/".into(), root_directory_node())
}

fn static_directory_file_like(
    state: TmpfsStateRef,
    path: String,
    node: &'static StaticDirectoryNode,
) -> FileLike {
    FileLike::Directory(Arc::new(Mutex::new(DevDirectoryHandle {
        state,
        path,
        node,
    })))
}

fn pts_directory_file_like() -> FileLike {
    FileLike::Directory(Arc::new(Mutex::new(DevPtsDirectoryHandle)))
}

fn pts_inode(number: u32) -> u64 {
    0x2000 + u64::from(number)
}

fn pts_file_like(number: u32) -> FSResult<FileLike> {
    let object = get_pty_slave(number).ok_or(FSError::NotFound)?;
    Ok(FileLike::File(Arc::new(Mutex::new(
        StaticDeviceHandle::from_object(
            number.to_string(),
            pts_inode(number),
            0o020620,
            Some((136u64 << 8) | number as u64),
            object,
        ),
    ))))
}

fn overlay_directory_path(path: &Path) -> String {
    let normalized = path.normalize();
    let components = normalized
        .parts
        .iter()
        .filter_map(|part| match part {
            PathPart::Normal(name) => Some(name.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    if components.is_empty() {
        "/".into()
    } else {
        alloc::format!("/{}", components.join("/"))
    }
}

fn static_directory_child(
    node: &'static StaticDirectoryNode,
    name: &str,
) -> Option<&'static StaticNode> {
    node.entries
        .iter()
        .find(|entry| entry.name == name)
        .map(|entry| entry.node)
}

fn static_node_file_like(
    state: TmpfsStateRef,
    path: String,
    node: &'static StaticNode,
) -> FileLike {
    match node {
        StaticNode::Directory(directory) => {
            if path == "/pts" {
                pts_directory_file_like()
            } else {
                static_directory_file_like(state, path, directory)
            }
        }
        StaticNode::File(file) => FileLike::File(Arc::new(Mutex::new(
            crate::filesystem::staticfs::file::StaticFileHandle::new(file),
        ))),
        StaticNode::Symlink(symlink) => FileLike::Symlink(Arc::new(Mutex::new(
            crate::filesystem::staticfs::symlink::StaticSymlinkHandle::new(symlink),
        ))),
        StaticNode::Device(device) => {
            FileLike::File(Arc::new(Mutex::new(StaticDeviceHandle::new(device))))
        }
    }
}

fn dynamic_children(state: &TmpfsStateRef, path: &str) -> FSResult<Vec<DirectoryContentInfo>> {
    let state = state.lock();
    let node = state.node(path)?;
    let children = match &node.kind {
        TmpNodeKind::Directory { children, .. } => children,
        TmpNodeKind::File { .. } | TmpNodeKind::Symlink { .. } => {
            return Err(FSError::NotADirectory);
        }
    };

    let mut entries = Vec::with_capacity(children.len());
    for child in children {
        let child_path = TmpfsState::child_path(path, child);
        let child_node = state.node(&child_path)?;
        let content_type = match child_node.kind {
            TmpNodeKind::Directory { .. } => DirectoryContentType::Directory,
            TmpNodeKind::File { .. } => DirectoryContentType::File,
            TmpNodeKind::Symlink { .. } => DirectoryContentType::Symlink,
        };
        entries.push(DirectoryContentInfo::new(child.clone(), content_type));
    }
    Ok(entries)
}

fn static_root_paths() -> &'static [&'static str] {
    &["/input", "/dri", "/pts", "/shm"]
}

fn seeded_static_directory(path: &str) -> bool {
    path == "/" || static_root_paths().contains(&path)
}

impl Directory for DevDirectoryHandle {
    fn as_any(&self) -> &dyn core::any::Any {
        self
    }

    fn info(&self) -> FSResult<FileLikeInfo> {
        StaticDirectoryHandle::new(self.node).info()
    }

    fn name(&self) -> FSResult<String> {
        Ok(self.node.name.into())
    }

    fn contents(&self) -> FSResult<Vec<DirectoryContentInfo>> {
        let mut seen = BTreeSet::new();
        let mut entries = Vec::new();

        for entry in self.node.entries {
            seen.insert(entry.name.to_string());
            entries.push(DirectoryContentInfo::new(
                entry.name.into(),
                entry.node.content_type(),
            ));
        }

        for entry in dynamic_children(&self.state, &self.path)? {
            if seen.insert(entry.name.clone()) {
                entries.push(entry);
            }
        }

        Ok(entries)
    }

    fn create(&self, info: DirectoryContentInfo) -> FSResult<()> {
        if static_directory_child(self.node, &info.name).is_some() {
            return Err(FSError::AlreadyExists);
        }

        let mut state = self.state.lock();
        match info.content_type {
            DirectoryContentType::File => state.create_file(&self.path, &info.name),
            DirectoryContentType::Directory => state.create_directory(&self.path, &info.name),
            DirectoryContentType::Symlink => Err(FSError::Readonly),
        }
    }

    fn create_symlink(&self, name: &str, target: &str) -> FSResult<()> {
        if static_directory_child(self.node, name).is_some() {
            return Err(FSError::AlreadyExists);
        }

        self.state.lock().create_symlink(&self.path, name, target)
    }

    fn delete(&self, name: &str) -> FSResult<()> {
        if static_directory_child(self.node, name).is_some() {
            return Err(FSError::Readonly);
        }

        self.state.lock().delete_node(&self.path, name)
    }

    fn get(&self, name: &str) -> FSResult<FileLike> {
        if let Some(node) = static_directory_child(self.node, name) {
            let child_path = TmpfsState::child_path(&self.path, name);
            return Ok(static_node_file_like(self.state.clone(), child_path, node));
        }

        let child_path = TmpfsState::child_path(&self.path, name);
        tmpfs_lookup_path(&self.state, &child_path)
    }
}

impl DevFs {
    pub fn new() -> Self {
        let state = Arc::new(Mutex::new(TmpfsState::new()));
        {
            let mut state_guard = state.lock();
            for path in static_root_paths() {
                let name = path.trim_start_matches('/');
                state_guard
                    .create_directory("/", name)
                    .expect("devfs static directory seed should succeed");
            }
        }
        Self { state }
    }
}

impl Default for DevFs {
    fn default() -> Self {
        Self::new()
    }
}

impl FileSystem for DevFs {
    fn init(&mut self) -> FSResult<()> {
        Ok(())
    }

    fn lookup(&self, path: &Path) -> FSResult<FileLike> {
        let normalized = path.normalize();
        let mut current = root_directory_file_like(self.state.clone());

        for component in normalized.parts.iter() {
            match component {
                PathPart::Root | PathPart::CurrentDir => {}
                PathPart::ParentDir => return Err(FSError::NotADirectory),
                PathPart::Normal(name) => {
                    let FileLike::Directory(directory) = current else {
                        return Err(FSError::NotADirectory);
                    };
                    current = directory.lock().get(name)?;
                }
            }
        }

        Ok(current)
    }

    fn rename(&self, old_path: &Path, new_path: &Path) -> FSResult<()> {
        let old_path = overlay_directory_path(old_path);
        let new_path = overlay_directory_path(new_path);
        if seeded_static_directory(&old_path)
            || seeded_static_directory(&new_path)
            || static_path_exists(&old_path)
        {
            return Err(FSError::Readonly);
        }
        if static_path_exists(&new_path) {
            return Err(FSError::Readonly);
        }
        self.state.lock().rename(&old_path, &new_path)
    }

    fn link(&self, old_path: &Path, new_path: &Path) -> FSResult<()> {
        let old_path = overlay_directory_path(old_path);
        let new_path = overlay_directory_path(new_path);
        if static_path_exists(&old_path) || static_path_exists(&new_path) {
            return Err(FSError::Readonly);
        }
        self.state.lock().link(&old_path, &new_path)
    }

    fn name(&self) -> &'static str {
        "devtmpfs"
    }

    fn magic(&self) -> i64 {
        0x0102_1994
    }

    fn mount_source(&self) -> &'static str {
        "devtmpfs"
    }

    fn default_mount_flags(&self, _path: &Path) -> crate::filesystem::vfs_traits::MountFlags {
        crate::filesystem::vfs_traits::MountFlags::MS_NOSUID
            | crate::filesystem::vfs_traits::MountFlags::MS_RELATIME
    }
}

impl Directory for DevPtsDirectoryHandle {
    fn as_any(&self) -> &dyn core::any::Any {
        self
    }

    fn info(&self) -> FSResult<FileLikeInfo> {
        Ok(FileLikeInfo::new(
            "pts".into(),
            0,
            UnixPermission(0o040755),
            FileLikeType::Directory,
        )
        .with_inode(0x100c))
    }

    fn name(&self) -> FSResult<String> {
        Ok("pts".into())
    }

    fn contents(&self) -> FSResult<Vec<DirectoryContentInfo>> {
        Ok(list_ptys()
            .into_iter()
            .map(|number| DirectoryContentInfo::new(number.to_string(), DirectoryContentType::File))
            .collect())
    }

    fn create(&self, _info: DirectoryContentInfo) -> FSResult<()> {
        Err(FSError::Readonly)
    }

    fn delete(&self, _name: &str) -> FSResult<()> {
        Err(FSError::Readonly)
    }

    fn get(&self, name: &str) -> FSResult<FileLike> {
        let number = name.parse::<u32>().map_err(|_| FSError::NotFound)?;
        pts_file_like(number)
    }
}

fn static_path_exists(path: &str) -> bool {
    if path == "/" {
        return true;
    }

    let mut current = root_directory_node();
    let mut parts = path.trim_start_matches('/').split('/').peekable();
    while let Some(name) = parts.next() {
        let Some(node) = static_directory_child(current, name) else {
            return false;
        };
        match node {
            StaticNode::Directory(directory) => {
                if parts.peek().is_none() {
                    return true;
                }
                current = directory;
            }
            StaticNode::File(_) | StaticNode::Device(_) | StaticNode::Symlink(_) => {
                return parts.peek().is_none();
            }
        }
    }
    true
}
