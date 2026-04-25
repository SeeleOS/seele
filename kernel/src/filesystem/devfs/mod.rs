use alloc::{
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use spin::Mutex;

use crate::{
    filesystem::{
        errors::FSError,
        info::{DirectoryContentInfo, FileLikeInfo, UnixPermission},
        path::{Path, PathPart},
        staticfs::{
            StaticDeviceNode, StaticDirEntry, StaticDirectoryNode, StaticNode, StaticSymlinkNode,
            device::StaticDeviceHandle, directory::StaticDirectoryHandle,
        },
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

pub struct DevFs;

struct DevRootDirectoryHandle;
struct DevPtsDirectoryHandle;

fn root_directory_node() -> &'static StaticDirectoryNode {
    let StaticNode::Directory(node) = &DEV_ROOT_NODE else {
        unreachable!()
    };
    node
}

fn root_directory_file_like() -> FileLike {
    FileLike::Directory(Arc::new(Mutex::new(DevRootDirectoryHandle)))
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

impl Directory for DevRootDirectoryHandle {
    fn as_any(&self) -> &dyn core::any::Any {
        self
    }

    fn info(&self) -> FSResult<FileLikeInfo> {
        StaticDirectoryHandle::new(root_directory_node()).info()
    }

    fn name(&self) -> FSResult<String> {
        Ok("dev".into())
    }

    fn contents(&self) -> FSResult<Vec<DirectoryContentInfo>> {
        StaticDirectoryHandle::new(root_directory_node()).contents()
    }

    fn create(&self, _info: DirectoryContentInfo) -> FSResult<()> {
        Err(FSError::Readonly)
    }

    fn delete(&self, _name: &str) -> FSResult<()> {
        Err(FSError::Readonly)
    }

    fn get(&self, name: &str) -> FSResult<FileLike> {
        if name == "pts" {
            return Ok(pts_directory_file_like());
        }

        StaticDirectoryHandle::new(root_directory_node()).get(name)
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

impl DevFs {
    pub fn new() -> Self {
        Self
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
        let mut current = root_directory_file_like();

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

    fn rename(&self, _old_path: &Path, _new_path: &Path) -> FSResult<()> {
        Err(FSError::Readonly)
    }

    fn link(&self, _old_path: &Path, _new_path: &Path) -> FSResult<()> {
        Err(FSError::Readonly)
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
