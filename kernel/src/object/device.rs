use alloc::{collections::btree_map::BTreeMap, string::String, vec::Vec};
use seele_sys::{SyscallResult, errors::SyscallError};

use crate::object::misc::ObjectRef;

lazy_static::lazy_static! {
    pub static ref DEVICES: BTreeMap<String,ObjectRef> = {
        let devices = BTreeMap::new();

        devices
    };
}

pub fn get_device(name: String) -> SyscallResult<ObjectRef> {
    DEVICES
        .iter()
        .find(|(obj_name, _)| **obj_name == name)
        .ok_or(SyscallError::InvalidArguments)
        .map(|(_, obj)| obj)
        .cloned()
}
