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
    process::manager::get_current_process,
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

    if name == "tty"
        && let Some(tty) = current_process_tty()
    {
        return Ok(tty);
    }

    if let Some(device) = open_event_device(name) {
        return Ok(device);
    }

    DEVICES
        .get(name)
        .ok_or(SyscallError::InvalidArguments)
        .cloned()
}

fn current_process_tty() -> Option<ObjectRef> {
    let stdin = {
        let process = get_current_process();
        let process = process.lock();
        process.fd_table.first()?.as_ref()?.object.clone()
    };

    if stdin.clone().as_tty_device().is_ok() || stdin.clone().as_pty_slave().is_ok() {
        Some(stdin)
    } else {
        None
    }
}
