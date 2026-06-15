use super::*;

define_syscall!(Getxattr, |path: CString,
                           name: CString,
                           _value: *mut u8,
                           _size: usize| {
    let path_str = path_from_raw(path)?;
    let _name = xattr_name_from_raw(name)?;
    ensure_path_exists_at(AT_FDCWD, &path_str, false)?;
    Err(SyscallError::NoData)
});

define_syscall!(Lgetxattr, |path: CString,
                            name: CString,
                            _value: *mut u8,
                            _size: usize| {
    let path_str = path_from_raw(path)?;
    let _name = xattr_name_from_raw(name)?;
    ensure_path_exists_at(AT_FDCWD, &path_str, true)?;
    Err(SyscallError::NoData)
});

define_syscall!(Fgetxattr, |object: ObjectRef,
                            name: CString,
                            _value: *mut u8,
                            _size: usize| {
    let _name = xattr_name_from_raw(name)?;
    ensure_object_supports_xattrs(&object)?;
    Err(SyscallError::NoData)
});

define_syscall!(Setxattr, |path: CString,
                           name: CString,
                           _value: *const u8,
                           _size: usize,
                           flags: XattrFlags| {
    let path_str = path_from_raw(path)?;
    let _name = xattr_name_from_raw(name)?;
    validate_xattr_flags(flags)?;
    ensure_path_exists_at(AT_FDCWD, &path_str, false)?;
    Ok(0)
});

define_syscall!(Lsetxattr, |path: CString,
                            name: CString,
                            _value: *const u8,
                            _size: usize,
                            flags: XattrFlags| {
    let path_str = path_from_raw(path)?;
    let _name = xattr_name_from_raw(name)?;
    validate_xattr_flags(flags)?;
    ensure_path_exists_at(AT_FDCWD, &path_str, true)?;
    Ok(0)
});

define_syscall!(Fsetxattr, |object: ObjectRef,
                            name: CString,
                            _value: *const u8,
                            _size: usize,
                            flags: XattrFlags| {
    let _name = xattr_name_from_raw(name)?;
    validate_xattr_flags(flags)?;
    ensure_object_supports_xattrs(&object)?;
    Ok(0)
});

define_syscall!(Listxattr, |path: CString, _list: *mut u8, _size: usize| {
    let path_str = path_from_raw(path)?;
    ensure_path_exists_at(AT_FDCWD, &path_str, false)?;
    Ok(0)
});

define_syscall!(Llistxattr, |path: CString, _list: *mut u8, _size: usize| {
    let path_str = path_from_raw(path)?;
    ensure_path_exists_at(AT_FDCWD, &path_str, true)?;
    Ok(0)
});

define_syscall!(Flistxattr, |object: ObjectRef,
                             _list: *mut u8,
                             _size: usize| {
    ensure_object_supports_xattrs(&object)?;
    Ok(0)
});

define_syscall!(Removexattr, |path: CString, name: CString| {
    let path_str = path_from_raw(path)?;
    let _name = xattr_name_from_raw(name)?;
    ensure_path_exists_at(AT_FDCWD, &path_str, false)?;
    Err(SyscallError::NoData)
});

define_syscall!(Lremovexattr, |path: CString, name: CString| {
    let path_str = path_from_raw(path)?;
    let _name = xattr_name_from_raw(name)?;
    ensure_path_exists_at(AT_FDCWD, &path_str, true)?;
    Err(SyscallError::NoData)
});

define_syscall!(Fremovexattr, |object: ObjectRef, name: CString| {
    let _name = xattr_name_from_raw(name)?;
    ensure_object_supports_xattrs(&object)?;
    Err(SyscallError::NoData)
});
