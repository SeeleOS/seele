use crate::filesystem::{
    path::Path,
    staticfs::{StaticDeviceNode, StaticDirEntry, StaticDirectoryNode, StaticFs, StaticNode},
    vfs::FSResult,
    vfs_traits::{FileLike, FileSystem},
};

static DEV_NULL_NODE: StaticNode = StaticNode::Device(StaticDeviceNode {
    name: "null",
    inode: 0x1001,
    mode: 0o020666,
    device_name: "devnull",
});

static DEV_TTY_NODE: StaticNode = StaticNode::Device(StaticDeviceNode {
    name: "tty",
    inode: 0x1002,
    mode: 0o020666,
    device_name: "tty",
});

static DEV_CONSOLE_NODE: StaticNode = StaticNode::Device(StaticDeviceNode {
    name: "console",
    inode: 0x1003,
    mode: 0o020600,
    device_name: "tty",
});

static DEV_TTY0_NODE: StaticNode = StaticNode::Device(StaticDeviceNode {
    name: "tty0",
    inode: 0x1004,
    mode: 0o020666,
    device_name: "tty",
});

static DEV_TTY1_NODE: StaticNode = StaticNode::Device(StaticDeviceNode {
    name: "tty1",
    inode: 0x1005,
    mode: 0o020666,
    device_name: "tty",
});

static DEV_FB0_NODE: StaticNode = StaticNode::Device(StaticDeviceNode {
    name: "fb0",
    inode: 0x1006,
    mode: 0o020666,
    device_name: "framebuffer",
});

static DEV_PSAUX_NODE: StaticNode = StaticNode::Device(StaticDeviceNode {
    name: "psaux",
    inode: 0x1007,
    mode: 0o020666,
    device_name: "ps2mouse",
});

static DEV_MOUSE_NODE: StaticNode = StaticNode::Device(StaticDeviceNode {
    name: "mouse",
    inode: 0x1008,
    mode: 0o020666,
    device_name: "ps2mouse",
});

static DEV_INPUT_EVENT0_NODE: StaticNode = StaticNode::Device(StaticDeviceNode {
    name: "event0",
    inode: 0x100A,
    mode: 0o020660,
    device_name: "event-kbd",
});

static DEV_INPUT_EVENT1_NODE: StaticNode = StaticNode::Device(StaticDeviceNode {
    name: "event1",
    inode: 0x100B,
    mode: 0o020660,
    device_name: "event-mouse",
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

static DEV_LOG_NODE: StaticNode = StaticNode::Symlink(
    crate::filesystem::staticfs::StaticSymlinkNode {
        name: "log",
        inode: 0x100e,
        mode: 0o120777,
        target: "/run/systemd/journal/dev-log",
    },
);

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
];

static DEV_ROOT_NODE: StaticNode = StaticNode::Directory(StaticDirectoryNode {
    name: "dev",
    inode: 0x1000,
    mode: 0o040755,
    entries: DEV_ROOT_ENTRIES,
});

pub struct DevFs {
    inner: StaticFs,
}

impl DevFs {
    pub fn new() -> Self {
        Self {
            inner: StaticFs::new(&DEV_ROOT_NODE),
        }
    }
}

impl FileSystem for DevFs {
    fn init(&mut self) -> FSResult<()> {
        self.inner.init()
    }

    fn lookup(&self, path: &Path) -> FSResult<FileLike> {
        self.inner.lookup(path)
    }

    fn rename(&self, _old_path: &Path, _new_path: &Path) -> FSResult<()> {
        Err(crate::filesystem::errors::FSError::Readonly)
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

    fn mount_options(&self, _path: &Path) -> &'static str {
        "rw,nosuid,relatime"
    }
}
