use alloc::vec::Vec;

use crate::filesystem::{
    staticfs::{
        StaticDirEntry, StaticDirectoryNode, StaticFileNode, StaticNode, StaticSymlinkNode,
    },
    vfs::FSResult,
};

use super::emit_uevent;

fn keyboard_name() -> Vec<u8> {
    b"AT Translated Set 2 keyboard\n".to_vec()
}

fn keyboard_phys() -> Vec<u8> {
    b"isa0060/serio0/input0\n".to_vec()
}

fn keyboard_uniq() -> Vec<u8> {
    b"\n".to_vec()
}

fn keyboard_properties() -> Vec<u8> {
    b"0\n".to_vec()
}

fn keyboard_input_uevent() -> Vec<u8> {
    b"PRODUCT=11/1/1/100\nNAME=\"AT Translated Set 2 keyboard\"\nPHYS=\"isa0060/serio0/input0\"\nPROP=0\nSUBSYSTEM=input\nID_INPUT=1\nID_INPUT_KEY=1\nID_INPUT_KEYBOARD=1\nID_SEAT=seat0\nWL_SEAT=seat0\nLIBINPUT_DEVICE_GROUP=seele-keyboard\n".to_vec()
}

fn keyboard_input_dir_uevent() -> Vec<u8> {
    b"SUBSYSTEM=input\n".to_vec()
}

fn keyboard_serio_uevent() -> Vec<u8> {
    b"DRIVER=atkbd\nMODALIAS=serio:ty06pr00id00ex00\nSUBSYSTEM=serio\n".to_vec()
}

fn keyboard_event_dev() -> Vec<u8> {
    b"13:64\n".to_vec()
}

fn keyboard_event_uevent() -> Vec<u8> {
    b"MAJOR=13\nMINOR=64\nDEVNAME=input/event0\nSUBSYSTEM=input\nID_INPUT=1\nID_INPUT_KEY=1\nID_INPUT_KEYBOARD=1\nID_SEAT=seat0\nWL_SEAT=seat0\nLIBINPUT_DEVICE_GROUP=seele-keyboard\n".to_vec()
}

fn keyboard_caps_ev() -> Vec<u8> {
    b"3\n".to_vec()
}

fn keyboard_caps_key() -> Vec<u8> {
    b"ffffffff ffffffff ffffffff ffffffff\n".to_vec()
}

fn keyboard_caps_prop() -> Vec<u8> {
    b"0\n".to_vec()
}

fn keyboard_caps_abs() -> Vec<u8> {
    b"0\n".to_vec()
}

fn keyboard_id_bustype() -> Vec<u8> {
    b"0011\n".to_vec()
}

fn keyboard_id_vendor() -> Vec<u8> {
    b"0001\n".to_vec()
}

fn keyboard_id_product() -> Vec<u8> {
    b"0001\n".to_vec()
}

fn keyboard_id_version() -> Vec<u8> {
    b"0100\n".to_vec()
}

fn keyboard_input_uevent_write(buffer: &[u8]) -> FSResult<usize> {
    emit_uevent(
        buffer,
        "/devices/platform/i8042/serio0/input/input0",
        &keyboard_input_uevent(),
    )
}

fn keyboard_event_uevent_write(buffer: &[u8]) -> FSResult<usize> {
    emit_uevent(
        buffer,
        "/devices/platform/i8042/serio0/input/input0/event0",
        &keyboard_event_uevent(),
    )
}

fn keyboard_input_dir_uevent_write(buffer: &[u8]) -> FSResult<usize> {
    emit_uevent(
        buffer,
        "/devices/platform/i8042/serio0/input",
        &keyboard_input_dir_uevent(),
    )
}

fn keyboard_serio_uevent_write(buffer: &[u8]) -> FSResult<usize> {
    emit_uevent(
        buffer,
        "/devices/platform/i8042/serio0",
        &keyboard_serio_uevent(),
    )
}

pub(super) static SYS_CLASS_INPUT_EVENT0_NODE: StaticNode =
    StaticNode::Symlink(StaticSymlinkNode {
        name: "event0",
        inode: 0x2010,
        mode: 0o120777,
        target: "/sys/devices/platform/i8042/serio0/input/input0/event0",
    });

pub(super) static SYS_CLASS_INPUT_INPUT0_NODE: StaticNode =
    StaticNode::Symlink(StaticSymlinkNode {
        name: "input0",
        inode: 0x2012,
        mode: 0o120777,
        target: "/sys/devices/platform/i8042/serio0/input/input0",
    });

pub(super) static SYS_DEV_CHAR_13_64_NODE: StaticNode = StaticNode::Symlink(StaticSymlinkNode {
    name: "13:64",
    inode: 0x2020,
    mode: 0o120777,
    target: "/sys/devices/platform/i8042/serio0/input/input0/event0",
});

static SYS_DEVICES_PLATFORM_I8042_SERIO0_SUBSYSTEM_NODE: StaticNode =
    StaticNode::Symlink(StaticSymlinkNode {
        name: "subsystem",
        inode: 0x2031,
        mode: 0o120777,
        target: "/sys/bus/serio",
    });

static SYS_KEYBOARD_NAME_NODE: StaticNode = StaticNode::File(StaticFileNode {
    name: "name",
    inode: 0x2040,
    mode: 0o100444,
    read: keyboard_name,
    write: None,
});

static SYS_KEYBOARD_PHYS_NODE: StaticNode = StaticNode::File(StaticFileNode {
    name: "phys",
    inode: 0x2041,
    mode: 0o100444,
    read: keyboard_phys,
    write: None,
});

static SYS_KEYBOARD_UNIQ_NODE: StaticNode = StaticNode::File(StaticFileNode {
    name: "uniq",
    inode: 0x2042,
    mode: 0o100444,
    read: keyboard_uniq,
    write: None,
});

static SYS_KEYBOARD_PROPERTIES_NODE: StaticNode = StaticNode::File(StaticFileNode {
    name: "properties",
    inode: 0x2043,
    mode: 0o100444,
    read: keyboard_properties,
    write: None,
});

static SYS_KEYBOARD_UEVENT_NODE: StaticNode = StaticNode::File(StaticFileNode {
    name: "uevent",
    inode: 0x2044,
    mode: 0o100644,
    read: keyboard_input_uevent,
    write: Some(keyboard_input_uevent_write),
});

static SYS_KEYBOARD_CAP_EV_NODE: StaticNode = StaticNode::File(StaticFileNode {
    name: "ev",
    inode: 0x2045,
    mode: 0o100444,
    read: keyboard_caps_ev,
    write: None,
});

static SYS_KEYBOARD_CAP_KEY_NODE: StaticNode = StaticNode::File(StaticFileNode {
    name: "key",
    inode: 0x2046,
    mode: 0o100444,
    read: keyboard_caps_key,
    write: None,
});

static SYS_KEYBOARD_CAP_PROP_NODE: StaticNode = StaticNode::File(StaticFileNode {
    name: "prop",
    inode: 0x2047,
    mode: 0o100444,
    read: keyboard_caps_prop,
    write: None,
});

static SYS_KEYBOARD_CAP_ABS_NODE: StaticNode = StaticNode::File(StaticFileNode {
    name: "abs",
    inode: 0x2048,
    mode: 0o100444,
    read: keyboard_caps_abs,
    write: None,
});

static SYS_KEYBOARD_CAP_ENTRIES: &[StaticDirEntry] = &[
    StaticDirEntry {
        name: "ev",
        node: &SYS_KEYBOARD_CAP_EV_NODE,
    },
    StaticDirEntry {
        name: "key",
        node: &SYS_KEYBOARD_CAP_KEY_NODE,
    },
    StaticDirEntry {
        name: "prop",
        node: &SYS_KEYBOARD_CAP_PROP_NODE,
    },
    StaticDirEntry {
        name: "abs",
        node: &SYS_KEYBOARD_CAP_ABS_NODE,
    },
];

static SYS_KEYBOARD_CAP_NODE: StaticNode = StaticNode::Directory(StaticDirectoryNode {
    name: "capabilities",
    inode: 0x2049,
    mode: 0o040755,
    entries: SYS_KEYBOARD_CAP_ENTRIES,
});

static SYS_KEYBOARD_ID_BUSTYPE_NODE: StaticNode = StaticNode::File(StaticFileNode {
    name: "bustype",
    inode: 0x204a,
    mode: 0o100444,
    read: keyboard_id_bustype,
    write: None,
});

static SYS_KEYBOARD_ID_VENDOR_NODE: StaticNode = StaticNode::File(StaticFileNode {
    name: "vendor",
    inode: 0x204b,
    mode: 0o100444,
    read: keyboard_id_vendor,
    write: None,
});

static SYS_KEYBOARD_ID_PRODUCT_NODE: StaticNode = StaticNode::File(StaticFileNode {
    name: "product",
    inode: 0x204c,
    mode: 0o100444,
    read: keyboard_id_product,
    write: None,
});

static SYS_KEYBOARD_ID_VERSION_NODE: StaticNode = StaticNode::File(StaticFileNode {
    name: "version",
    inode: 0x204d,
    mode: 0o100444,
    read: keyboard_id_version,
    write: None,
});

static SYS_KEYBOARD_ID_ENTRIES: &[StaticDirEntry] = &[
    StaticDirEntry {
        name: "bustype",
        node: &SYS_KEYBOARD_ID_BUSTYPE_NODE,
    },
    StaticDirEntry {
        name: "vendor",
        node: &SYS_KEYBOARD_ID_VENDOR_NODE,
    },
    StaticDirEntry {
        name: "product",
        node: &SYS_KEYBOARD_ID_PRODUCT_NODE,
    },
    StaticDirEntry {
        name: "version",
        node: &SYS_KEYBOARD_ID_VERSION_NODE,
    },
];

static SYS_KEYBOARD_ID_NODE: StaticNode = StaticNode::Directory(StaticDirectoryNode {
    name: "id",
    inode: 0x204e,
    mode: 0o040755,
    entries: SYS_KEYBOARD_ID_ENTRIES,
});

static SYS_KEYBOARD_INPUT_SUBSYSTEM_NODE: StaticNode = StaticNode::Symlink(StaticSymlinkNode {
    name: "subsystem",
    inode: 0x204f,
    mode: 0o120777,
    target: "/sys/class/input",
});

static SYS_KEYBOARD_EVENT_DEV_NODE: StaticNode = StaticNode::File(StaticFileNode {
    name: "dev",
    inode: 0x2050,
    mode: 0o100444,
    read: keyboard_event_dev,
    write: None,
});

static SYS_KEYBOARD_EVENT_UEVENT_NODE: StaticNode = StaticNode::File(StaticFileNode {
    name: "uevent",
    inode: 0x2051,
    mode: 0o100644,
    read: keyboard_event_uevent,
    write: Some(keyboard_event_uevent_write),
});

static SYS_KEYBOARD_EVENT_SUBSYSTEM_NODE: StaticNode = StaticNode::Symlink(StaticSymlinkNode {
    name: "subsystem",
    inode: 0x2052,
    mode: 0o120777,
    target: "/sys/class/input",
});

static SYS_KEYBOARD_EVENT_DEVICE_NODE: StaticNode = StaticNode::Symlink(StaticSymlinkNode {
    name: "device",
    inode: 0x2053,
    mode: 0o120777,
    target: "/sys/devices/platform/i8042/serio0/input/input0",
});

static SYS_KEYBOARD_EVENT_ENTRIES: &[StaticDirEntry] = &[
    StaticDirEntry {
        name: "dev",
        node: &SYS_KEYBOARD_EVENT_DEV_NODE,
    },
    StaticDirEntry {
        name: "uevent",
        node: &SYS_KEYBOARD_EVENT_UEVENT_NODE,
    },
    StaticDirEntry {
        name: "subsystem",
        node: &SYS_KEYBOARD_EVENT_SUBSYSTEM_NODE,
    },
    StaticDirEntry {
        name: "device",
        node: &SYS_KEYBOARD_EVENT_DEVICE_NODE,
    },
];

static SYS_KEYBOARD_EVENT_NODE: StaticNode = StaticNode::Directory(StaticDirectoryNode {
    name: "event0",
    inode: 0x2054,
    mode: 0o040755,
    entries: SYS_KEYBOARD_EVENT_ENTRIES,
});

static SYS_KEYBOARD_INPUT_ENTRIES: &[StaticDirEntry] = &[
    StaticDirEntry {
        name: "name",
        node: &SYS_KEYBOARD_NAME_NODE,
    },
    StaticDirEntry {
        name: "phys",
        node: &SYS_KEYBOARD_PHYS_NODE,
    },
    StaticDirEntry {
        name: "uniq",
        node: &SYS_KEYBOARD_UNIQ_NODE,
    },
    StaticDirEntry {
        name: "properties",
        node: &SYS_KEYBOARD_PROPERTIES_NODE,
    },
    StaticDirEntry {
        name: "uevent",
        node: &SYS_KEYBOARD_UEVENT_NODE,
    },
    StaticDirEntry {
        name: "capabilities",
        node: &SYS_KEYBOARD_CAP_NODE,
    },
    StaticDirEntry {
        name: "id",
        node: &SYS_KEYBOARD_ID_NODE,
    },
    StaticDirEntry {
        name: "subsystem",
        node: &SYS_KEYBOARD_INPUT_SUBSYSTEM_NODE,
    },
    StaticDirEntry {
        name: "event0",
        node: &SYS_KEYBOARD_EVENT_NODE,
    },
];

static SYS_KEYBOARD_INPUT_NODE: StaticNode = StaticNode::Directory(StaticDirectoryNode {
    name: "input0",
    inode: 0x2055,
    mode: 0o040755,
    entries: SYS_KEYBOARD_INPUT_ENTRIES,
});

static SYS_KEYBOARD_INPUT_DIR_UEVENT_NODE: StaticNode = StaticNode::File(StaticFileNode {
    name: "uevent",
    inode: 0x2056,
    mode: 0o100644,
    read: keyboard_input_dir_uevent,
    write: Some(keyboard_input_dir_uevent_write),
});

static SYS_KEYBOARD_INPUT_DIR_ENTRIES: &[StaticDirEntry] = &[
    StaticDirEntry {
        name: "uevent",
        node: &SYS_KEYBOARD_INPUT_DIR_UEVENT_NODE,
    },
    StaticDirEntry {
        name: "input0",
        node: &SYS_KEYBOARD_INPUT_NODE,
    },
];

static SYS_KEYBOARD_INPUT_DIR_NODE: StaticNode = StaticNode::Directory(StaticDirectoryNode {
    name: "input",
    inode: 0x2057,
    mode: 0o040755,
    entries: SYS_KEYBOARD_INPUT_DIR_ENTRIES,
});

static SYS_SERIO0_UEVENT_NODE: StaticNode = StaticNode::File(StaticFileNode {
    name: "uevent",
    inode: 0x2058,
    mode: 0o100644,
    read: keyboard_serio_uevent,
    write: Some(keyboard_serio_uevent_write),
});

static SYS_SERIO0_ENTRIES: &[StaticDirEntry] = &[
    StaticDirEntry {
        name: "subsystem",
        node: &SYS_DEVICES_PLATFORM_I8042_SERIO0_SUBSYSTEM_NODE,
    },
    StaticDirEntry {
        name: "uevent",
        node: &SYS_SERIO0_UEVENT_NODE,
    },
    StaticDirEntry {
        name: "input",
        node: &SYS_KEYBOARD_INPUT_DIR_NODE,
    },
];

pub(super) static SYS_SERIO0_NODE: StaticNode = StaticNode::Directory(StaticDirectoryNode {
    name: "serio0",
    inode: 0x2059,
    mode: 0o040755,
    entries: SYS_SERIO0_ENTRIES,
});
