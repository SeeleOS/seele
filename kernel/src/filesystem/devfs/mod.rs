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
}
