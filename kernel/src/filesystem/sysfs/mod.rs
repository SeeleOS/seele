mod keyboard;
mod mouse;

use alloc::vec::Vec;

use crate::filesystem::{
    path::Path,
    staticfs::{
        StaticDirEntry, StaticDirectoryNode, StaticFileNode, StaticFs, StaticNode,
        StaticSymlinkNode,
    },
    vfs::FSResult,
    vfs_traits::{FileLike, FileSystem},
};

use self::{
    keyboard::{
        SYS_CLASS_INPUT_EVENT0_NODE, SYS_CLASS_INPUT_INPUT0_NODE, SYS_DEV_CHAR_13_64_NODE,
        SYS_SERIO0_NODE,
    },
    mouse::{
        SYS_CLASS_INPUT_EVENT1_NODE, SYS_CLASS_INPUT_INPUT1_NODE, SYS_DEV_CHAR_13_65_NODE,
        SYS_SERIO1_NODE,
    },
};

fn devices_uevent() -> Vec<u8> {
    b"SUBSYSTEM=devices\n".to_vec()
}

fn platform_uevent() -> Vec<u8> {
    b"SUBSYSTEM=platform\n".to_vec()
}

fn i8042_uevent() -> Vec<u8> {
    b"DRIVER=i8042\nMODALIAS=platform:i8042\nSUBSYSTEM=platform\n".to_vec()
}

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

static SYS_CLASS_INPUT_ENTRIES: &[StaticDirEntry] = &[
    StaticDirEntry {
        name: "event0",
        node: &SYS_CLASS_INPUT_EVENT0_NODE,
    },
    StaticDirEntry {
        name: "event1",
        node: &SYS_CLASS_INPUT_EVENT1_NODE,
    },
    StaticDirEntry {
        name: "input0",
        node: &SYS_CLASS_INPUT_INPUT0_NODE,
    },
    StaticDirEntry {
        name: "input1",
        node: &SYS_CLASS_INPUT_INPUT1_NODE,
    },
];

static SYS_CLASS_INPUT_NODE: StaticNode = StaticNode::Directory(StaticDirectoryNode {
    name: "input",
    inode: 0x200f,
    mode: 0o040755,
    entries: SYS_CLASS_INPUT_ENTRIES,
});

static SYS_CLASS_ENTRIES: &[StaticDirEntry] = &[
    StaticDirEntry {
        name: "graphics",
        node: &SYS_CLASS_GRAPHICS_NODE,
    },
    StaticDirEntry {
        name: "input",
        node: &SYS_CLASS_INPUT_NODE,
    },
];

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

static SYS_BUS_SERIO_NODE: StaticNode = StaticNode::Directory(StaticDirectoryNode {
    name: "serio",
    inode: 0x2009,
    mode: 0o040755,
    entries: &[],
});

static SYS_BUS_ENTRIES: &[StaticDirEntry] = &[
    StaticDirEntry {
        name: "platform",
        node: &SYS_BUS_PLATFORM_NODE,
    },
    StaticDirEntry {
        name: "serio",
        node: &SYS_BUS_SERIO_NODE,
    },
];

static SYS_BUS_NODE: StaticNode = StaticNode::Directory(StaticDirectoryNode {
    name: "bus",
    inode: 0x2007,
    mode: 0o040755,
    entries: SYS_BUS_ENTRIES,
});

static SYS_DEV_CHAR_ENTRIES: &[StaticDirEntry] = &[
    StaticDirEntry {
        name: "13:64",
        node: &SYS_DEV_CHAR_13_64_NODE,
    },
    StaticDirEntry {
        name: "13:65",
        node: &SYS_DEV_CHAR_13_65_NODE,
    },
];

static SYS_DEV_CHAR_NODE: StaticNode = StaticNode::Directory(StaticDirectoryNode {
    name: "char",
    inode: 0x201f,
    mode: 0o040755,
    entries: SYS_DEV_CHAR_ENTRIES,
});

static SYS_DEV_ENTRIES: &[StaticDirEntry] = &[StaticDirEntry {
    name: "char",
    node: &SYS_DEV_CHAR_NODE,
}];

static SYS_DEV_NODE: StaticNode = StaticNode::Directory(StaticDirectoryNode {
    name: "dev",
    inode: 0x201e,
    mode: 0o040755,
    entries: SYS_DEV_ENTRIES,
});

static SYS_DEVICES_PLATFORM_I8042_SUBSYSTEM_NODE: StaticNode =
    StaticNode::Symlink(StaticSymlinkNode {
        name: "subsystem",
        inode: 0x2030,
        mode: 0o120777,
        target: "/sys/bus/platform",
    });

static SYS_DEVICES_PLATFORM_I8042_UEVENT_NODE: StaticNode = StaticNode::File(StaticFileNode {
    name: "uevent",
    inode: 0x2064,
    mode: 0o100444,
    read: i8042_uevent,
});

static SYS_DEVICES_PLATFORM_I8042_ENTRIES: &[StaticDirEntry] = &[
    StaticDirEntry {
        name: "subsystem",
        node: &SYS_DEVICES_PLATFORM_I8042_SUBSYSTEM_NODE,
    },
    StaticDirEntry {
        name: "uevent",
        node: &SYS_DEVICES_PLATFORM_I8042_UEVENT_NODE,
    },
    StaticDirEntry {
        name: "serio0",
        node: &SYS_SERIO0_NODE,
    },
    StaticDirEntry {
        name: "serio1",
        node: &SYS_SERIO1_NODE,
    },
];

static SYS_DEVICES_PLATFORM_I8042_NODE: StaticNode = StaticNode::Directory(StaticDirectoryNode {
    name: "i8042",
    inode: 0x2061,
    mode: 0o040755,
    entries: SYS_DEVICES_PLATFORM_I8042_ENTRIES,
});

static SYS_DEVICES_PLATFORM_UEVENT_NODE: StaticNode = StaticNode::File(StaticFileNode {
    name: "uevent",
    inode: 0x2065,
    mode: 0o100444,
    read: platform_uevent,
});

static SYS_DEVICES_PLATFORM_ENTRIES: &[StaticDirEntry] = &[
    StaticDirEntry {
        name: "uevent",
        node: &SYS_DEVICES_PLATFORM_UEVENT_NODE,
    },
    StaticDirEntry {
        name: "i8042",
        node: &SYS_DEVICES_PLATFORM_I8042_NODE,
    },
];

static SYS_DEVICES_PLATFORM_NODE: StaticNode = StaticNode::Directory(StaticDirectoryNode {
    name: "platform",
    inode: 0x2062,
    mode: 0o040755,
    entries: SYS_DEVICES_PLATFORM_ENTRIES,
});

static SYS_DEVICES_UEVENT_NODE: StaticNode = StaticNode::File(StaticFileNode {
    name: "uevent",
    inode: 0x2066,
    mode: 0o100444,
    read: devices_uevent,
});

static SYS_DEVICES_ENTRIES: &[StaticDirEntry] = &[
    StaticDirEntry {
        name: "uevent",
        node: &SYS_DEVICES_UEVENT_NODE,
    },
    StaticDirEntry {
        name: "platform",
        node: &SYS_DEVICES_PLATFORM_NODE,
    },
];

static SYS_DEVICES_NODE: StaticNode = StaticNode::Directory(StaticDirectoryNode {
    name: "devices",
    inode: 0x2063,
    mode: 0o040755,
    entries: SYS_DEVICES_ENTRIES,
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
    StaticDirEntry {
        name: "dev",
        node: &SYS_DEV_NODE,
    },
    StaticDirEntry {
        name: "devices",
        node: &SYS_DEVICES_NODE,
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
