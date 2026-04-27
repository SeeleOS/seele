use crate::{
    drm::mode_types::DrmModePropertyEnum,
    memory::{addrspace::AddrSpace, user_safe},
    object::{error::ObjectError, misc::ObjectResult},
    process::misc::with_current_process,
};

pub(super) fn read_user<T: Copy>(ptr: *mut T) -> ObjectResult<T> {
    user_safe::read(ptr.cast_const()).map_err(|_| ObjectError::InvalidArguments)
}

pub(super) fn maybe_write_u32_slice(ptr: u64, capacity: u32, values: &[u32]) -> ObjectResult<()> {
    maybe_write_struct_slice(ptr, capacity, values)
}

pub(super) fn maybe_write_u16_slice(ptr: u64, capacity: u32, values: &[u16]) -> ObjectResult<()> {
    maybe_write_struct_slice(ptr, capacity, values)
}

pub(super) fn maybe_write_u64_slice(ptr: u64, capacity: u32, values: &[u64]) -> ObjectResult<()> {
    maybe_write_struct_slice(ptr, capacity, values)
}

pub(super) fn maybe_write_struct_slice<T: Copy>(
    ptr: u64,
    capacity: u32,
    values: &[T],
) -> ObjectResult<()> {
    if values.is_empty() || ptr == 0 || capacity < values.len() as u32 {
        return Ok(());
    }

    with_current_process(|process| {
        process
            .addrspace
            .write(ptr as *mut T, values)
            .map_err(|_| ObjectError::InvalidArguments)
    })
}

pub(super) fn copy_c_string(ptr: *mut u8, len: usize, value: &str) -> ObjectResult<()> {
    if ptr.is_null() || len == 0 {
        return Ok(());
    }

    let bytes = value.as_bytes();
    let copy_len = bytes.len().min(len.saturating_sub(1));
    with_current_process(|process| {
        write_c_string(&mut process.addrspace, ptr, len, bytes, copy_len)
    })
}

fn write_c_string(
    addrspace: &mut AddrSpace,
    ptr: *mut u8,
    len: usize,
    bytes: &[u8],
    copy_len: usize,
) -> ObjectResult<()> {
    addrspace
        .write(ptr, &bytes[..copy_len])
        .map_err(|_| ObjectError::InvalidArguments)?;
    if len > copy_len {
        addrspace
            .write(unsafe { ptr.add(copy_len) }, &[0u8])
            .map_err(|_| ObjectError::InvalidArguments)?;
    }
    Ok(())
}

pub(super) fn copy_property_name(dst: &mut [u8; 32], value: &str) {
    for (slot, byte) in dst.iter_mut().zip(value.bytes()) {
        *slot = byte;
    }
}

pub(super) fn make_property_enum(value: u64, name: &str) -> DrmModePropertyEnum {
    let mut item = DrmModePropertyEnum {
        value,
        name: [0; 32],
    };
    copy_property_name(&mut item.name, name);
    item
}
