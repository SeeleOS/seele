use alloc::{collections::btree_map::BTreeMap, string::String, sync::Arc, vec::Vec};
use seele_sys::{SyscallResult, errors::SyscallError};

use crate::{
    misc::{devices::DevNull, fb_object::FramebufferObject, mouse::PS2MouseObject},
    object::{Object, misc::ObjectRef},
};

lazy_static::lazy_static! {
    pub static ref DEVICES: BTreeMap<&'static str,ObjectRef> = {
        let mut devices = BTreeMap::new();

        devices.insert("framebuffer", Arc::new(FramebufferObject::default()) as ObjectRef);
        devices.insert("devnull", Arc::new(DevNull) as ObjectRef);
        devices.insert("ps2mouse", Arc::new(PS2MouseObject::default()) as ObjectRef);

        devices
    };
}

pub fn get_device(name: String) -> SyscallResult<ObjectRef> {
    DEVICES
        .get(name.as_str())
        .ok_or(SyscallError::InvalidArguments)
        .cloned()
}
