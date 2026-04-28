use alloc::vec::Vec;

use crate::filesystem::{
    staticfs::{
        StaticDirEntry, StaticDirectoryNode, StaticFileNode, StaticNode, StaticSymlinkNode,
    },
    vfs::FSResult,
};

use super::emit_uevent;

fn mouse_name() -> Vec<u8> {
    b"PS/2 Generic Mouse\n".to_vec()
}

fn mouse_phys() -> Vec<u8> {
    b"isa0060/serio1/input1\n".to_vec()
}

fn mouse_uniq() -> Vec<u8> {
    b"\n".to_vec()
}

fn mouse_properties() -> Vec<u8> {
    b"1\n".to_vec()
}

fn mouse_input_uevent() -> Vec<u8> {
    b"PRODUCT=11/1/2/100\nNAME=\"PS/2 Generic Mouse\"\nPHYS=\"isa0060/serio1/input1\"\nPROP=1\nSUBSYSTEM=input\n"
        .to_vec()
}

fn mouse_input_dir_uevent() -> Vec<u8> {
    b"SUBSYSTEM=input\n".to_vec()
}

fn mouse_serio_uevent() -> Vec<u8> {
    b"DRIVER=psmouse\nMODALIAS=serio:ty06pr00id00ex00\nSUBSYSTEM=serio\n".to_vec()
}

fn mouse_event_dev() -> Vec<u8> {
    b"13:65\n".to_vec()
}

fn mouse_event_uevent() -> Vec<u8> {
    b"MAJOR=13\nMINOR=65\nDEVNAME=input/event1\nSUBSYSTEM=input\nID_INPUT=1\nID_INPUT_MOUSE=1\nID_SEAT=seat0\nWL_SEAT=seat0\nLIBINPUT_DEVICE_GROUP=seele-mouse\n".to_vec()
}

fn mouse_caps_ev() -> Vec<u8> {
    b"7\n".to_vec()
}

fn mouse_caps_key() -> Vec<u8> {
    b"70000 0 0 0\n".to_vec()
}

fn mouse_caps_rel() -> Vec<u8> {
    b"3\n".to_vec()
}

fn mouse_caps_prop() -> Vec<u8> {
    b"1\n".to_vec()
}

fn mouse_caps_abs() -> Vec<u8> {
    b"0\n".to_vec()
}

fn mouse_id_bustype() -> Vec<u8> {
    b"0011\n".to_vec()
}

fn mouse_id_vendor() -> Vec<u8> {
    b"0001\n".to_vec()
}

fn mouse_id_product() -> Vec<u8> {
    b"0002\n".to_vec()
}

fn mouse_id_version() -> Vec<u8> {
    b"0100\n".to_vec()
}

fn mouse_input_uevent_write(buffer: &[u8]) -> FSResult<usize> {
    emit_uevent(
        buffer,
        "/devices/platform/i8042/serio1/input/input1",
        &mouse_input_uevent(),
    )
}

fn mouse_event_uevent_write(buffer: &[u8]) -> FSResult<usize> {
    emit_uevent(
        buffer,
        "/devices/platform/i8042/serio1/input/input1/event1",
        &mouse_event_uevent(),
    )
}

fn mouse_input_dir_uevent_write(buffer: &[u8]) -> FSResult<usize> {
    emit_uevent(
        buffer,
        "/devices/platform/i8042/serio1/input",
        &mouse_input_dir_uevent(),
    )
}

fn mouse_serio_uevent_write(buffer: &[u8]) -> FSResult<usize> {
    emit_uevent(
        buffer,
        "/devices/platform/i8042/serio1",
        &mouse_serio_uevent(),
    )
}

pub(super) static SYS_CLASS_INPUT_EVENT1_NODE: StaticNode =
    StaticNode::Symlink(StaticSymlinkNode {
        name: "event1",
        inode: 0x2011,
        mode: 0o120777,
        target: "/sys/devices/platform/i8042/serio1/input/input1/event1",
    });

pub(super) static SYS_CLASS_INPUT_INPUT1_NODE: StaticNode =
    StaticNode::Symlink(StaticSymlinkNode {
        name: "input1",
        inode: 0x2013,
        mode: 0o120777,
        target: "/sys/devices/platform/i8042/serio1/input/input1",
    });

pub(super) static SYS_DEV_CHAR_13_65_NODE: StaticNode = StaticNode::Symlink(StaticSymlinkNode {
    name: "13:65",
    inode: 0x2021,
    mode: 0o120777,
    target: "/sys/devices/platform/i8042/serio1/input/input1/event1",
});

static SYS_DEVICES_PLATFORM_I8042_SERIO1_SUBSYSTEM_NODE: StaticNode =
    StaticNode::Symlink(StaticSymlinkNode {
        name: "subsystem",
        inode: 0x2032,
        mode: 0o120777,
        target: "/sys/bus/serio",
    });

static SYS_MOUSE_NAME_NODE: StaticNode = StaticNode::File(StaticFileNode {
    name: "name",
    inode: 0x2050,
    mode: 0o100444,
    read: mouse_name,
    write: None,
});

static SYS_MOUSE_PHYS_NODE: StaticNode = StaticNode::File(StaticFileNode {
    name: "phys",
    inode: 0x2051,
    mode: 0o100444,
    read: mouse_phys,
    write: None,
});

static SYS_MOUSE_UNIQ_NODE: StaticNode = StaticNode::File(StaticFileNode {
    name: "uniq",
    inode: 0x2052,
    mode: 0o100444,
    read: mouse_uniq,
    write: None,
});

static SYS_MOUSE_PROPERTIES_NODE: StaticNode = StaticNode::File(StaticFileNode {
    name: "properties",
    inode: 0x2053,
    mode: 0o100444,
    read: mouse_properties,
    write: None,
});

static SYS_MOUSE_UEVENT_NODE: StaticNode = StaticNode::File(StaticFileNode {
    name: "uevent",
    inode: 0x2054,
    mode: 0o100644,
    read: mouse_input_uevent,
    write: Some(mouse_input_uevent_write),
});

static SYS_MOUSE_CAP_EV_NODE: StaticNode = StaticNode::File(StaticFileNode {
    name: "ev",
    inode: 0x2055,
    mode: 0o100444,
    read: mouse_caps_ev,
    write: None,
});

static SYS_MOUSE_CAP_KEY_NODE: StaticNode = StaticNode::File(StaticFileNode {
    name: "key",
    inode: 0x2056,
    mode: 0o100444,
    read: mouse_caps_key,
    write: None,
});

static SYS_MOUSE_CAP_REL_NODE: StaticNode = StaticNode::File(StaticFileNode {
    name: "rel",
    inode: 0x2057,
    mode: 0o100444,
    read: mouse_caps_rel,
    write: None,
});

static SYS_MOUSE_CAP_PROP_NODE: StaticNode = StaticNode::File(StaticFileNode {
    name: "prop",
    inode: 0x2058,
    mode: 0o100444,
    read: mouse_caps_prop,
    write: None,
});

static SYS_MOUSE_CAP_ABS_NODE: StaticNode = StaticNode::File(StaticFileNode {
    name: "abs",
    inode: 0x2059,
    mode: 0o100444,
    read: mouse_caps_abs,
    write: None,
});

static SYS_MOUSE_CAP_ENTRIES: &[StaticDirEntry] = &[
    StaticDirEntry {
        name: "ev",
        node: &SYS_MOUSE_CAP_EV_NODE,
    },
    StaticDirEntry {
        name: "key",
        node: &SYS_MOUSE_CAP_KEY_NODE,
    },
    StaticDirEntry {
        name: "rel",
        node: &SYS_MOUSE_CAP_REL_NODE,
    },
    StaticDirEntry {
        name: "prop",
        node: &SYS_MOUSE_CAP_PROP_NODE,
    },
    StaticDirEntry {
        name: "abs",
        node: &SYS_MOUSE_CAP_ABS_NODE,
    },
];

static SYS_MOUSE_CAP_NODE: StaticNode = StaticNode::Directory(StaticDirectoryNode {
    name: "capabilities",
    inode: 0x205a,
    mode: 0o040755,
    entries: SYS_MOUSE_CAP_ENTRIES,
});

static SYS_MOUSE_ID_BUSTYPE_NODE: StaticNode = StaticNode::File(StaticFileNode {
    name: "bustype",
    inode: 0x205b,
    mode: 0o100444,
    read: mouse_id_bustype,
    write: None,
});

static SYS_MOUSE_ID_VENDOR_NODE: StaticNode = StaticNode::File(StaticFileNode {
    name: "vendor",
    inode: 0x205c,
    mode: 0o100444,
    read: mouse_id_vendor,
    write: None,
});

static SYS_MOUSE_ID_PRODUCT_NODE: StaticNode = StaticNode::File(StaticFileNode {
    name: "product",
    inode: 0x205d,
    mode: 0o100444,
    read: mouse_id_product,
    write: None,
});

static SYS_MOUSE_ID_VERSION_NODE: StaticNode = StaticNode::File(StaticFileNode {
    name: "version",
    inode: 0x205e,
    mode: 0o100444,
    read: mouse_id_version,
    write: None,
});

static SYS_MOUSE_ID_ENTRIES: &[StaticDirEntry] = &[
    StaticDirEntry {
        name: "bustype",
        node: &SYS_MOUSE_ID_BUSTYPE_NODE,
    },
    StaticDirEntry {
        name: "vendor",
        node: &SYS_MOUSE_ID_VENDOR_NODE,
    },
    StaticDirEntry {
        name: "product",
        node: &SYS_MOUSE_ID_PRODUCT_NODE,
    },
    StaticDirEntry {
        name: "version",
        node: &SYS_MOUSE_ID_VERSION_NODE,
    },
];

static SYS_MOUSE_ID_NODE: StaticNode = StaticNode::Directory(StaticDirectoryNode {
    name: "id",
    inode: 0x205f,
    mode: 0o040755,
    entries: SYS_MOUSE_ID_ENTRIES,
});

static SYS_MOUSE_INPUT_SUBSYSTEM_NODE: StaticNode = StaticNode::Symlink(StaticSymlinkNode {
    name: "subsystem",
    inode: 0x2060,
    mode: 0o120777,
    target: "/sys/class/input",
});

static SYS_MOUSE_EVENT_DEV_NODE: StaticNode = StaticNode::File(StaticFileNode {
    name: "dev",
    inode: 0x2061,
    mode: 0o100444,
    read: mouse_event_dev,
    write: None,
});

static SYS_MOUSE_EVENT_UEVENT_NODE: StaticNode = StaticNode::File(StaticFileNode {
    name: "uevent",
    inode: 0x2062,
    mode: 0o100644,
    read: mouse_event_uevent,
    write: Some(mouse_event_uevent_write),
});

static SYS_MOUSE_EVENT_SUBSYSTEM_NODE: StaticNode = StaticNode::Symlink(StaticSymlinkNode {
    name: "subsystem",
    inode: 0x2063,
    mode: 0o120777,
    target: "/sys/class/input",
});

static SYS_MOUSE_EVENT_DEVICE_NODE: StaticNode = StaticNode::Symlink(StaticSymlinkNode {
    name: "device",
    inode: 0x2064,
    mode: 0o120777,
    target: "/sys/devices/platform/i8042/serio1/input/input1",
});

static SYS_MOUSE_EVENT_ENTRIES: &[StaticDirEntry] = &[
    StaticDirEntry {
        name: "dev",
        node: &SYS_MOUSE_EVENT_DEV_NODE,
    },
    StaticDirEntry {
        name: "uevent",
        node: &SYS_MOUSE_EVENT_UEVENT_NODE,
    },
    StaticDirEntry {
        name: "subsystem",
        node: &SYS_MOUSE_EVENT_SUBSYSTEM_NODE,
    },
    StaticDirEntry {
        name: "device",
        node: &SYS_MOUSE_EVENT_DEVICE_NODE,
    },
];

static SYS_MOUSE_EVENT_NODE: StaticNode = StaticNode::Directory(StaticDirectoryNode {
    name: "event1",
    inode: 0x2065,
    mode: 0o040755,
    entries: SYS_MOUSE_EVENT_ENTRIES,
});

static SYS_MOUSE_INPUT_ENTRIES: &[StaticDirEntry] = &[
    StaticDirEntry {
        name: "name",
        node: &SYS_MOUSE_NAME_NODE,
    },
    StaticDirEntry {
        name: "phys",
        node: &SYS_MOUSE_PHYS_NODE,
    },
    StaticDirEntry {
        name: "uniq",
        node: &SYS_MOUSE_UNIQ_NODE,
    },
    StaticDirEntry {
        name: "properties",
        node: &SYS_MOUSE_PROPERTIES_NODE,
    },
    StaticDirEntry {
        name: "uevent",
        node: &SYS_MOUSE_UEVENT_NODE,
    },
    StaticDirEntry {
        name: "capabilities",
        node: &SYS_MOUSE_CAP_NODE,
    },
    StaticDirEntry {
        name: "id",
        node: &SYS_MOUSE_ID_NODE,
    },
    StaticDirEntry {
        name: "subsystem",
        node: &SYS_MOUSE_INPUT_SUBSYSTEM_NODE,
    },
    StaticDirEntry {
        name: "event1",
        node: &SYS_MOUSE_EVENT_NODE,
    },
];

static SYS_MOUSE_INPUT_NODE: StaticNode = StaticNode::Directory(StaticDirectoryNode {
    name: "input1",
    inode: 0x2066,
    mode: 0o040755,
    entries: SYS_MOUSE_INPUT_ENTRIES,
});

static SYS_MOUSE_INPUT_DIR_UEVENT_NODE: StaticNode = StaticNode::File(StaticFileNode {
    name: "uevent",
    inode: 0x2067,
    mode: 0o100644,
    read: mouse_input_dir_uevent,
    write: Some(mouse_input_dir_uevent_write),
});

static SYS_MOUSE_INPUT_DIR_ENTRIES: &[StaticDirEntry] = &[
    StaticDirEntry {
        name: "uevent",
        node: &SYS_MOUSE_INPUT_DIR_UEVENT_NODE,
    },
    StaticDirEntry {
        name: "input1",
        node: &SYS_MOUSE_INPUT_NODE,
    },
];

static SYS_MOUSE_INPUT_DIR_NODE: StaticNode = StaticNode::Directory(StaticDirectoryNode {
    name: "input",
    inode: 0x2068,
    mode: 0o040755,
    entries: SYS_MOUSE_INPUT_DIR_ENTRIES,
});

static SYS_SERIO1_UEVENT_NODE: StaticNode = StaticNode::File(StaticFileNode {
    name: "uevent",
    inode: 0x2069,
    mode: 0o100644,
    read: mouse_serio_uevent,
    write: Some(mouse_serio_uevent_write),
});

static SYS_SERIO1_ENTRIES: &[StaticDirEntry] = &[
    StaticDirEntry {
        name: "subsystem",
        node: &SYS_DEVICES_PLATFORM_I8042_SERIO1_SUBSYSTEM_NODE,
    },
    StaticDirEntry {
        name: "uevent",
        node: &SYS_SERIO1_UEVENT_NODE,
    },
    StaticDirEntry {
        name: "input",
        node: &SYS_MOUSE_INPUT_DIR_NODE,
    },
];

pub(super) static SYS_SERIO1_NODE: StaticNode = StaticNode::Directory(StaticDirectoryNode {
    name: "serio1",
    inode: 0x206a,
    mode: 0o040755,
    entries: SYS_SERIO1_ENTRIES,
});
