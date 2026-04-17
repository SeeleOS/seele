use crate::filesystem::{
    path::Path,
    staticfs::{StaticDirEntry, StaticDirectoryNode, StaticFs, StaticNode, StaticSymlinkNode},
    vfs::FSResult,
    vfs_traits::{FileLike, FileSystem},
};

static SYS_CLASS_GRAPHICS_FB0_DEVICE_SUBSYSTEM_NODE: StaticNode =
    StaticNode::Symlink(StaticSymlinkNode {
        name: "subsystem",
        inode: 0x2006,
        mode: 0o120777,
        target: "/sys/bus/platform",
    });

static SYS_CLASS_GRAPHICS_FB0_DEVICE_ENTRIES: &[StaticDirEntry] = &[StaticDirEntry {
    name: "subsystem",
    node: &SYS_CLASS_GRAPHICS_FB0_DEVICE_SUBSYSTEM_NODE,
}];

static SYS_CLASS_GRAPHICS_FB0_DEVICE_NODE: StaticNode =
    StaticNode::Directory(StaticDirectoryNode {
        name: "device",
        inode: 0x2005,
        mode: 0o040755,
        entries: SYS_CLASS_GRAPHICS_FB0_DEVICE_ENTRIES,
    });

static SYS_CLASS_GRAPHICS_FB0_ENTRIES: &[StaticDirEntry] = &[StaticDirEntry {
    name: "device",
    node: &SYS_CLASS_GRAPHICS_FB0_DEVICE_NODE,
}];

static SYS_CLASS_GRAPHICS_FB0_NODE: StaticNode = StaticNode::Directory(StaticDirectoryNode {
    name: "fb0",
    inode: 0x2004,
    mode: 0o040755,
    entries: SYS_CLASS_GRAPHICS_FB0_ENTRIES,
});

static SYS_CLASS_GRAPHICS_ENTRIES: &[StaticDirEntry] = &[StaticDirEntry {
    name: "fb0",
    node: &SYS_CLASS_GRAPHICS_FB0_NODE,
}];

static SYS_CLASS_GRAPHICS_NODE: StaticNode = StaticNode::Directory(StaticDirectoryNode {
    name: "graphics",
    inode: 0x2003,
    mode: 0o040755,
    entries: SYS_CLASS_GRAPHICS_ENTRIES,
});

static SYS_CLASS_ENTRIES: &[StaticDirEntry] = &[StaticDirEntry {
    name: "graphics",
    node: &SYS_CLASS_GRAPHICS_NODE,
}];

static SYS_CLASS_NODE: StaticNode = StaticNode::Directory(StaticDirectoryNode {
    name: "class",
    inode: 0x2002,
    mode: 0o040755,
    entries: SYS_CLASS_ENTRIES,
});

static SYS_BUS_PLATFORM_NODE: StaticNode = StaticNode::Directory(StaticDirectoryNode {
    name: "platform",
    inode: 0x2008,
    mode: 0o040755,
    entries: &[],
});

static SYS_BUS_ENTRIES: &[StaticDirEntry] = &[StaticDirEntry {
    name: "platform",
    node: &SYS_BUS_PLATFORM_NODE,
}];

static SYS_BUS_NODE: StaticNode = StaticNode::Directory(StaticDirectoryNode {
    name: "bus",
    inode: 0x2007,
    mode: 0o040755,
    entries: SYS_BUS_ENTRIES,
});

static SYS_ROOT_ENTRIES: &[StaticDirEntry] = &[
    StaticDirEntry {
        name: "class",
        node: &SYS_CLASS_NODE,
    },
    StaticDirEntry {
        name: "bus",
        node: &SYS_BUS_NODE,
    },
];

static SYS_ROOT_NODE: StaticNode = StaticNode::Directory(StaticDirectoryNode {
    name: "sys",
    inode: 0x2000,
    mode: 0o040755,
    entries: SYS_ROOT_ENTRIES,
});

pub struct SysFs {
    inner: StaticFs,
}

impl SysFs {
    pub fn new() -> Self {
        Self {
            inner: StaticFs::new(&SYS_ROOT_NODE),
        }
    }
}

impl FileSystem for SysFs {
    fn init(&mut self) -> FSResult<()> {
        self.inner.init()
    }

    fn lookup(&self, path: &Path) -> FSResult<FileLike> {
        self.inner.lookup(path)
    }
}
