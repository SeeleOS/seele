use alloc::{collections::btree_map::BTreeMap, string::String, sync::Arc};

use crate::{
    drm::object::DrmCardObject,
    evdev::open_event_device,
    misc::{
        devices::{DevKmsg, DevNull, DevRandom},
        fb_object::FramebufferObject,
        mouse::PS2MouseObject,
    },
    object::{
        fuse_device::FuseDevice,
        misc::ObjectRef,
        tty_device::{get_active_tty, get_default_tty, get_virtual_tty},
    },
    process::manager::get_current_process,
    systemcall::utils::{SyscallError, SyscallResult},
    terminal::pty::open_ptmx,
};

lazy_static::lazy_static! {
    pub static ref DEVICES: BTreeMap<&'static str,ObjectRef> = {
        let mut devices = BTreeMap::new();

        devices.insert("framebuffer", Arc::new(FramebufferObject::default()) as ObjectRef);
        devices.insert("devnull", Arc::new(DevNull) as ObjectRef);
        devices.insert("random", Arc::new(DevRandom) as ObjectRef);
        devices.insert("urandom", Arc::new(DevRandom) as ObjectRef);
        devices.insert("fuse", FuseDevice::new() as ObjectRef);
        devices.insert("kmsg", Arc::new(DevKmsg::default()) as ObjectRef);
        devices.insert("console", get_default_tty() as ObjectRef);
        devices.insert("tty", get_default_tty() as ObjectRef);
        devices.insert("tty0", get_default_tty() as ObjectRef);
        devices.insert("tty1", get_default_tty() as ObjectRef);
        devices.insert("ps2mouse", Arc::new(PS2MouseObject::default()) as ObjectRef);
        devices.insert("drm-card0", Arc::new(DrmCardObject::default()) as ObjectRef);

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

    if name == "tty0" {
        return Ok(get_active_tty());
    }

    if name == "console" {
        return Ok(get_active_tty());
    }

    if name == "tty"
        && let Some(tty) = current_process_tty()
    {
        return Ok(tty);
    }

    if let Some(vt) = name
        .strip_prefix("tty")
        .and_then(|suffix| suffix.parse::<u32>().ok())
        && let Some(tty) = get_virtual_tty(vt)
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
        process.fd_table.lock().first()?.as_ref()?.object.clone()
    };

    let stdin = stdin
        .clone()
        .as_file_like()
        .ok()
        .and_then(|file| file.device_backing_object())
        .unwrap_or(stdin);

    if stdin.clone().as_tty_device().is_ok() || stdin.clone().as_pty_slave().is_ok() {
        Some(stdin)
    } else {
        None
    }
}
