use alloc::{collections::btree_map::BTreeMap, string::String, sync::Arc};

use crate::{
    evdev::open_event_device,
    misc::{
        devices::{DevKmsg, DevNull},
        fb_object::FramebufferObject,
        mouse::PS2MouseObject,
    },
    object::{
        misc::ObjectRef,
        tty_device::{get_console_tty, get_default_tty},
    },
    systemcall::utils::{SyscallError, SyscallResult},
    terminal::pty::open_ptmx,
};

lazy_static::lazy_static! {
    pub static ref DEVICES: BTreeMap<&'static str,ObjectRef> = {
        let mut devices = BTreeMap::new();

        devices.insert("framebuffer", Arc::new(FramebufferObject) as ObjectRef);
        devices.insert("devnull", Arc::new(DevNull) as ObjectRef);
        devices.insert("kmsg", Arc::new(DevKmsg::default()) as ObjectRef);
        devices.insert("console", get_console_tty() as ObjectRef);
        devices.insert("tty", get_default_tty() as ObjectRef);
        devices.insert("tty0", get_default_tty() as ObjectRef);
        devices.insert("tty1", get_default_tty() as ObjectRef);
        devices.insert("ps2mouse", Arc::new(PS2MouseObject::default()) as ObjectRef);

        devices
    };
}

pub fn get_device(name: String) -> SyscallResult<ObjectRef> {
    get_device_ref(name.as_str())
}

pub fn get_device_ref(name: &str) -> SyscallResult<ObjectRef> {
    if name == "ptmx" {
        return Ok(open_ptmx());
    }

    if let Some(device) = open_event_device(name) {
        return Ok(device);
    }

    DEVICES
        .get(name)
        .ok_or(SyscallError::InvalidArguments)
        .cloned()
}
