use super::*;

define_syscall!(Getxattr, |path: CString,
                           name: CString,
                           value: *mut u8,
                           size: usize| {
    let path_str = path_from_raw(path)?;
    let name = xattr_name_from_raw(name)?;
    let object = xattr_path_object_at(AT_FDCWD, &path_str, false)?;
    let xattr_value = object.get_xattr(&name)?.ok_or(SyscallError::NoData)?;
    write_xattr_value(value, size, xattr_value)
});

define_syscall!(Lgetxattr, |path: CString,
                            name: CString,
                            value: *mut u8,
                            size: usize| {
    let path_str = path_from_raw(path)?;
    let name = xattr_name_from_raw(name)?;
    let object = xattr_path_object_at(AT_FDCWD, &path_str, true)?;
    let xattr_value = object.lget_xattr(&name)?.ok_or(SyscallError::NoData)?;
    write_xattr_value(value, size, xattr_value)
});

define_syscall!(Fgetxattr, |object: ObjectRef,
                            name: CString,
                            value: *mut u8,
                            size: usize| {
    let name = xattr_name_from_raw(name)?;
    let object = match xattr_fd_object(&object) {
        Ok(object) => object,
        Err(SyscallError::BadFileDescriptor) => return Err(SyscallError::NoData),
        Err(err) => return Err(err),
    };
    let xattr_value = object.get_xattr(&name)?.ok_or(SyscallError::NoData)?;
    write_xattr_value(value, size, xattr_value)
});

define_syscall!(Setxattr, |path: CString,
                           name: CString,
                           value: *const u8,
                           size: usize,
                           flags: XattrFlags| {
    let path_str = path_from_raw(path)?;
    let name = xattr_name_from_raw(name)?;
    validate_xattr_flags(flags)?;
    let value = xattr_value_from_user(value, size)?;
    let (create, replace) = xattr_flag_modes(flags);
    if name.starts_with("user.") {
        validate_user_xattr_mode(
            file_info_path(resolve_path_at(AT_FDCWD, &path_str)?)?
                .as_linux()
                .st_mode,
            &name,
        )?;
    }
    let object = xattr_path_object_at(AT_FDCWD, &path_str, false)?;
    object
        .set_xattr(name, value, create, replace)
        .map_err(xattr_not_found)?;
    Ok(0)
});

define_syscall!(Lsetxattr, |path: CString,
                            name: CString,
                            value: *const u8,
                            size: usize,
                            flags: XattrFlags| {
    let path_str = path_from_raw(path)?;
    let name = xattr_name_from_raw(name)?;
    validate_xattr_flags(flags)?;
    let value = xattr_value_from_user(value, size)?;
    let (create, replace) = xattr_flag_modes(flags);
    if name.starts_with("user.") {
        validate_user_xattr_mode(
            resolve_path_info_with_final(resolve_path_at(AT_FDCWD, &path_str)?, true)?
                .0
                .as_linux()
                .st_mode,
            &name,
        )?;
    }
    let object = xattr_path_object_at(AT_FDCWD, &path_str, true)?;
    object
        .lset_xattr(name, value, create, replace)
        .map_err(xattr_not_found)?;
    Ok(0)
});

define_syscall!(Fsetxattr, |object: ObjectRef,
                            name: CString,
                            value: *const u8,
                            size: usize,
                            flags: XattrFlags| {
    let name = xattr_name_from_raw(name)?;
    validate_xattr_flags(flags)?;
    let value = xattr_value_from_user(value, size)?;
    let (create, replace) = xattr_flag_modes(flags);
    let object = xattr_fd_object(&object)?;
    validate_user_xattr_target(&object, &name)?;
    object
        .set_xattr(name, value, create, replace)
        .map_err(xattr_not_found)?;
    Ok(0)
});

define_syscall!(Listxattr, |path: CString, list: *mut u8, size: usize| {
    let path_str = path_from_raw(path)?;
    let object = xattr_path_object_at(AT_FDCWD, &path_str, false)?;
    write_xattr_list(list, size, object.list_xattrs()?)
});

define_syscall!(Llistxattr, |path: CString, list: *mut u8, size: usize| {
    let path_str = path_from_raw(path)?;
    let object = xattr_path_object_at(AT_FDCWD, &path_str, true)?;
    write_xattr_list(list, size, object.llist_xattrs()?)
});

define_syscall!(Flistxattr, |object: ObjectRef,
                             list: *mut u8,
                             size: usize| {
    let object = xattr_fd_object(&object)?;
    write_xattr_list(list, size, object.list_xattrs()?)
});

define_syscall!(Removexattr, |path: CString, name: CString| {
    let path_str = path_from_raw(path)?;
    let name = xattr_name_from_raw(name)?;
    xattr_path_object_at(AT_FDCWD, &path_str, false)?
        .remove_xattr(&name)
        .map_err(xattr_not_found)?;
    Ok(0)
});

define_syscall!(Lremovexattr, |path: CString, name: CString| {
    let path_str = path_from_raw(path)?;
    let name = xattr_name_from_raw(name)?;
    xattr_path_object_at(AT_FDCWD, &path_str, true)?
        .lremove_xattr(&name)
        .map_err(xattr_not_found)?;
    Ok(0)
});

define_syscall!(Fremovexattr, |object: ObjectRef, name: CString| {
    let name = xattr_name_from_raw(name)?;
    xattr_fd_object(&object)?
        .remove_xattr(&name)
        .map_err(xattr_not_found)?;
    Ok(0)
});
