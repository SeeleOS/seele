use super::*;

use alloc::sync::Arc;

pub(super) fn xattr_name_from_raw(name: CString) -> Result<String, SyscallError> {
    let name = path_from_raw(name)?;
    if name.is_empty() || name.as_bytes().contains(&0) {
        return Err(SyscallError::InvalidArguments);
    }
    Ok(name)
}

pub(super) fn xattr_path_object_at(
    dirfd: i32,
    path_str: &str,
    nofollow: bool,
) -> Result<Arc<FileLikeObject>, SyscallError> {
    let path = resolve_path_at(dirfd, path_str)?;
    Ok(Arc::new(if nofollow {
        open_path_nofollow(path)?
    } else {
        open_path(path)?
    }))
}

pub(super) fn xattr_fd_object(object: &ObjectRef) -> Result<Arc<FileLikeObject>, SyscallError> {
    object.clone().as_file_like()
}

pub(super) fn validate_xattr_flags(flags: XattrFlags) -> Result<(), SyscallError> {
    if flags.bits() != flags.bits() & (XattrFlags::CREATE | XattrFlags::REPLACE).bits() {
        return Err(SyscallError::InvalidArguments);
    }
    if flags.contains(XattrFlags::CREATE) && flags.contains(XattrFlags::REPLACE) {
        return Err(SyscallError::InvalidArguments);
    }
    Ok(())
}

pub(super) fn xattr_flag_modes(flags: XattrFlags) -> (bool, bool) {
    (
        flags.contains(XattrFlags::CREATE),
        flags.contains(XattrFlags::REPLACE),
    )
}

pub(super) fn xattr_value_from_user(
    value: *const u8,
    size: usize,
) -> Result<Vec<u8>, SyscallError> {
    user_safe::read_buffer(value, size)
}

pub(super) fn write_xattr_value(
    value_ptr: *mut u8,
    size: usize,
    value: Vec<u8>,
) -> Result<usize, SyscallError> {
    if size == 0 {
        return Ok(value.len());
    }
    if size < value.len() {
        return Err(SyscallError::RangeError);
    }
    user_safe::write_buffer(value_ptr, &value)?;
    Ok(value.len())
}

pub(super) fn write_xattr_list(
    list_ptr: *mut u8,
    size: usize,
    names: Vec<String>,
) -> Result<usize, SyscallError> {
    let mut bytes = Vec::new();
    for name in names {
        bytes.extend_from_slice(name.as_bytes());
        bytes.push(0);
    }

    if size == 0 {
        return Ok(bytes.len());
    }
    if size < bytes.len() {
        return Err(SyscallError::RangeError);
    }
    user_safe::write_buffer(list_ptr, &bytes)?;
    Ok(bytes.len())
}

pub(super) fn xattr_not_found(err: FSError) -> SyscallError {
    match err {
        FSError::NotFound => SyscallError::NoData,
        err => err.into(),
    }
}
