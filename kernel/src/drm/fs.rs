use alloc::{string::String, vec::Vec};

use crate::filesystem::{
    staticfs::{
        StaticDeviceNode, StaticDirEntry, StaticDirectoryNode, StaticFileNode, StaticNode,
        StaticSymlinkNode,
    },
    sysfs::emit_uevent,
    vfs::FSResult,
};

use super::card::{CARD0_MAJOR, CARD0_MINOR, CARD0_RDEV, RENDERD128_MINOR, RENDERD128_RDEV};

fn card0_dev() -> Vec<u8> {
    format_dev(CARD0_MAJOR, CARD0_MINOR)
}

fn renderd128_dev() -> Vec<u8> {
    format_dev(CARD0_MAJOR, RENDERD128_MINOR)
}

fn card0_uevent() -> Vec<u8> {
    b"MAJOR=226\nMINOR=0\nDEVNAME=dri/card0\nDEVTYPE=drm_minor\nSUBSYSTEM=drm\n".to_vec()
}

fn renderd128_uevent() -> Vec<u8> {
    b"MAJOR=226\nMINOR=128\nDEVNAME=dri/renderD128\nDEVTYPE=drm_minor\nSUBSYSTEM=drm\n".to_vec()
}

fn drm_subsystem_uevent() -> Vec<u8> {
    b"SUBSYSTEM=drm\n".to_vec()
}

fn platform_drm_uevent() -> Vec<u8> {
    b"DRIVER=seele-drm\nMODALIAS=platform:seele-drm\nSUBSYSTEM=platform\n".to_vec()
}

fn card0_uevent_write(buffer: &[u8]) -> FSResult<usize> {
    emit_uevent(
        buffer,
        "/devices/platform/seele-drm/drm/card0",
        &card0_uevent(),
    )
}

fn renderd128_uevent_write(buffer: &[u8]) -> FSResult<usize> {
    emit_uevent(
        buffer,
        "/devices/platform/seele-drm/drm/renderD128",
        &renderd128_uevent(),
    )
}

fn drm_subsystem_uevent_write(buffer: &[u8]) -> FSResult<usize> {
    emit_uevent(buffer, "/class/drm", &drm_subsystem_uevent())
}

fn platform_drm_uevent_write(buffer: &[u8]) -> FSResult<usize> {
    emit_uevent(
        buffer,
        "/devices/platform/seele-drm",
        &platform_drm_uevent(),
    )
}

fn format_dev(major: u64, minor: u64) -> Vec<u8> {
    let mut out = String::new();
    use alloc::fmt::Write;
    let _ = writeln!(&mut out, "{major}:{minor}");
    out.into_bytes()
}

static DEV_DRI_CARD0_NODE: StaticNode = StaticNode::Device(StaticDeviceNode {
    name: "card0",
    inode: 0x1011,
    mode: 0o020660,
    device_name: "drm-card0",
    rdev: Some(CARD0_RDEV),
});

static DEV_DRI_RENDERD128_NODE: StaticNode = StaticNode::Device(StaticDeviceNode {
    name: "renderD128",
    inode: 0x1019,
    mode: 0o020660,
    device_name: "drm-renderD128",
    rdev: Some(RENDERD128_RDEV),
});

static DEV_DRI_ENTRIES: &[StaticDirEntry] = &[
    StaticDirEntry {
        name: "card0",
        node: &DEV_DRI_CARD0_NODE,
    },
    StaticDirEntry {
        name: "renderD128",
        node: &DEV_DRI_RENDERD128_NODE,
    },
];

pub static DEV_DRI_NODE: StaticNode = StaticNode::Directory(StaticDirectoryNode {
    name: "dri",
    inode: 0x1012,
    mode: 0o040755,
    entries: DEV_DRI_ENTRIES,
});

pub static SYS_CLASS_DRM_CARD0_NODE: StaticNode = StaticNode::Symlink(StaticSymlinkNode {
    name: "card0",
    inode: 0x2072,
    mode: 0o120777,
    target: "/sys/devices/platform/seele-drm/drm/card0",
});

pub static SYS_CLASS_DRM_RENDERD128_NODE: StaticNode = StaticNode::Symlink(StaticSymlinkNode {
    name: "renderD128",
    inode: 0x2073,
    mode: 0o120777,
    target: "/sys/devices/platform/seele-drm/drm/renderD128",
});

static SYS_CLASS_DRM_ENTRIES: &[StaticDirEntry] = &[
    StaticDirEntry {
        name: "card0",
        node: &SYS_CLASS_DRM_CARD0_NODE,
    },
    StaticDirEntry {
        name: "renderD128",
        node: &SYS_CLASS_DRM_RENDERD128_NODE,
    },
];

pub static SYS_CLASS_DRM_NODE: StaticNode = StaticNode::Directory(StaticDirectoryNode {
    name: "drm",
    inode: 0x2074,
    mode: 0o040755,
    entries: SYS_CLASS_DRM_ENTRIES,
});

pub static SYS_DEV_CHAR_226_0_NODE: StaticNode = StaticNode::Symlink(StaticSymlinkNode {
    name: "226:0",
    inode: 0x2075,
    mode: 0o120777,
    target: "/sys/devices/platform/seele-drm/drm/card0",
});

pub static SYS_DEV_CHAR_226_128_NODE: StaticNode = StaticNode::Symlink(StaticSymlinkNode {
    name: "226:128",
    inode: 0x2076,
    mode: 0o120777,
    target: "/sys/devices/platform/seele-drm/drm/renderD128",
});

static SYS_DRM_CARD0_DEV_NODE: StaticNode = StaticNode::File(StaticFileNode {
    name: "dev",
    inode: 0x2077,
    mode: 0o100444,
    read: card0_dev,
    write: None,
});

static SYS_DRM_CARD0_UEVENT_NODE: StaticNode = StaticNode::File(StaticFileNode {
    name: "uevent",
    inode: 0x2078,
    mode: 0o100444,
    read: card0_uevent,
    write: Some(card0_uevent_write),
});

static SYS_DRM_CARD0_SUBSYSTEM_NODE: StaticNode = StaticNode::Symlink(StaticSymlinkNode {
    name: "subsystem",
    inode: 0x2079,
    mode: 0o120777,
    target: "/sys/class/drm",
});

static SYS_DRM_CARD0_DEVICE_NODE: StaticNode = StaticNode::Symlink(StaticSymlinkNode {
    name: "device",
    inode: 0x207a,
    mode: 0o120777,
    target: "/sys/devices/platform/seele-drm",
});

static SYS_DRM_CARD0_ENTRIES: &[StaticDirEntry] = &[
    StaticDirEntry {
        name: "dev",
        node: &SYS_DRM_CARD0_DEV_NODE,
    },
    StaticDirEntry {
        name: "uevent",
        node: &SYS_DRM_CARD0_UEVENT_NODE,
    },
    StaticDirEntry {
        name: "subsystem",
        node: &SYS_DRM_CARD0_SUBSYSTEM_NODE,
    },
    StaticDirEntry {
        name: "device",
        node: &SYS_DRM_CARD0_DEVICE_NODE,
    },
];

static SYS_DRM_CARD0_DEVICE_TREE_NODE: StaticNode = StaticNode::Directory(StaticDirectoryNode {
    name: "card0",
    inode: 0x207b,
    mode: 0o040755,
    entries: SYS_DRM_CARD0_ENTRIES,
});

static SYS_DRM_RENDERD128_DEV_NODE: StaticNode = StaticNode::File(StaticFileNode {
    name: "dev",
    inode: 0x207c,
    mode: 0o100444,
    read: renderd128_dev,
    write: None,
});

static SYS_DRM_RENDERD128_UEVENT_NODE: StaticNode = StaticNode::File(StaticFileNode {
    name: "uevent",
    inode: 0x207d,
    mode: 0o100444,
    read: renderd128_uevent,
    write: Some(renderd128_uevent_write),
});

static SYS_DRM_RENDERD128_SUBSYSTEM_NODE: StaticNode = StaticNode::Symlink(StaticSymlinkNode {
    name: "subsystem",
    inode: 0x207e,
    mode: 0o120777,
    target: "/sys/class/drm",
});

static SYS_DRM_RENDERD128_DEVICE_NODE: StaticNode = StaticNode::Symlink(StaticSymlinkNode {
    name: "device",
    inode: 0x207f,
    mode: 0o120777,
    target: "/sys/devices/platform/seele-drm",
});

static SYS_DRM_RENDERD128_ENTRIES: &[StaticDirEntry] = &[
    StaticDirEntry {
        name: "dev",
        node: &SYS_DRM_RENDERD128_DEV_NODE,
    },
    StaticDirEntry {
        name: "uevent",
        node: &SYS_DRM_RENDERD128_UEVENT_NODE,
    },
    StaticDirEntry {
        name: "subsystem",
        node: &SYS_DRM_RENDERD128_SUBSYSTEM_NODE,
    },
    StaticDirEntry {
        name: "device",
        node: &SYS_DRM_RENDERD128_DEVICE_NODE,
    },
];

static SYS_DRM_RENDERD128_DEVICE_TREE_NODE: StaticNode =
    StaticNode::Directory(StaticDirectoryNode {
        name: "renderD128",
        inode: 0x2080,
        mode: 0o040755,
        entries: SYS_DRM_RENDERD128_ENTRIES,
    });

static SYS_DRM_SUBSYSTEM_UEVENT_NODE: StaticNode = StaticNode::File(StaticFileNode {
    name: "uevent",
    inode: 0x2081,
    mode: 0o100444,
    read: drm_subsystem_uevent,
    write: Some(drm_subsystem_uevent_write),
});

static SYS_DRM_DEVICE_TREE_ENTRIES: &[StaticDirEntry] = &[
    StaticDirEntry {
        name: "uevent",
        node: &SYS_DRM_SUBSYSTEM_UEVENT_NODE,
    },
    StaticDirEntry {
        name: "card0",
        node: &SYS_DRM_CARD0_DEVICE_TREE_NODE,
    },
    StaticDirEntry {
        name: "renderD128",
        node: &SYS_DRM_RENDERD128_DEVICE_TREE_NODE,
    },
];

static SYS_DRM_DEVICE_TREE_NODE: StaticNode = StaticNode::Directory(StaticDirectoryNode {
    name: "drm",
    inode: 0x2082,
    mode: 0o040755,
    entries: SYS_DRM_DEVICE_TREE_ENTRIES,
});

static SYS_PLATFORM_DRM_SUBSYSTEM_NODE: StaticNode = StaticNode::Symlink(StaticSymlinkNode {
    name: "subsystem",
    inode: 0x2083,
    mode: 0o120777,
    target: "/sys/bus/platform",
});

static SYS_PLATFORM_DRM_UEVENT_NODE: StaticNode = StaticNode::File(StaticFileNode {
    name: "uevent",
    inode: 0x2084,
    mode: 0o100444,
    read: platform_drm_uevent,
    write: Some(platform_drm_uevent_write),
});

static SYS_PLATFORM_DRM_ENTRIES: &[StaticDirEntry] = &[
    StaticDirEntry {
        name: "subsystem",
        node: &SYS_PLATFORM_DRM_SUBSYSTEM_NODE,
    },
    StaticDirEntry {
        name: "uevent",
        node: &SYS_PLATFORM_DRM_UEVENT_NODE,
    },
    StaticDirEntry {
        name: "drm",
        node: &SYS_DRM_DEVICE_TREE_NODE,
    },
];

pub static SYS_DEVICES_PLATFORM_DRM_NODE: StaticNode = StaticNode::Directory(StaticDirectoryNode {
    name: "seele-drm",
    inode: 0x2085,
    mode: 0o040755,
    entries: SYS_PLATFORM_DRM_ENTRIES,
});
