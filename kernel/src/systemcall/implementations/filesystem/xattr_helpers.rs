use super::*;

pub(super) fn xattr_name_from_raw(name: CString) -> Result<String, SyscallError> {
    path_from_raw(name)
}

pub(super) fn ensure_path_exists_at(
    dirfd: i32,
    path_str: &str,
    nofollow: bool,
) -> Result<(), SyscallError> {
    let path = resolve_path_at(dirfd, path_str)?;
    let _ = if nofollow {
        open_path_nofollow(path)?
    } else {
        open_path(path)?
    };
    Ok(())
}

pub(super) fn ensure_object_supports_xattrs(object: &ObjectRef) -> Result<(), SyscallError> {
    let _ = object.clone().as_file_like()?;
    Ok(())
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
