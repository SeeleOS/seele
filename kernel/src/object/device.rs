use alloc::{collections::btree_map::BTreeMap, string::String, sync::Arc, vec::Vec};
use seele_sys::{SyscallResult, errors::SyscallError};

use crate::{
    misc::fb_object::FramebufferObject,
    object::{Object, misc::ObjectRef},
};

lazy_static::lazy_static! {
    pub static ref DEVICES: BTreeMap<String,ObjectRef> = {
        let mut devices = BTreeMap::new();

        devices.insert("framebuffer".into(), Arc::new(FramebufferObject::default()) as ObjectRef);

        devices
    };
}

pub fn get_device(name: String) -> SyscallResult<ObjectRef> {
    DEVICES
        .get(&name)
        .ok_or(SyscallError::InvalidArguments)
        .cloned()
}
