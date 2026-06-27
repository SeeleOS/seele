use super::*;

const FILE_ATTR_SIZE_VER0: usize = 24;
const PAGE_SIZE: usize = 4096;

const FS_XFLAG_EXTSIZE: u64 = 0x0000_0800;
const FS_XFLAG_COWEXTSIZE: u64 = 0x0001_0000;
const SUPPORTED_FILE_XFLAGS: u64 = LinuxFileAttributes::FS_IMMUTABLE_FL.bits() as u64
    | LinuxFileAttributes::FS_APPEND_FL.bits() as u64;
const SUPPORTED_FILE_ATTR_FLAGS: i32 =
    AtFlags::SYMLINK_NOFOLLOW.bits() | AtFlags::EMPTY_PATH.bits();

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LinuxFileAttr {
    fa_xflags: u64,
    fa_extsize: u32,
    fa_nextents: u32,
    fa_projid: u32,
    fa_cowextsize: u32,
}

impl LinuxFileAttr {
    fn from_vfs(attributes: LinuxFileAttributes) -> Self {
        Self {
            fa_xflags: attributes.bits() as u64,
            ..Self::default()
        }
    }

    fn vfs_attributes(self) -> Result<LinuxFileAttributes, SyscallError> {
        if self.has_unsupported_fsx_fields() {
            return Err(SyscallError::OperationNotSupported);
        }

        let unsupported_xflags = self.fa_xflags & !SUPPORTED_FILE_XFLAGS;
        if unsupported_xflags != 0 {
            return Err(SyscallError::OperationNotSupported);
        }

        Ok(LinuxFileAttributes::from_bits_retain(self.fa_xflags as u32))
    }

    fn has_unsupported_fsx_fields(self) -> bool {
        (self.fa_xflags & (FS_XFLAG_EXTSIZE | FS_XFLAG_COWEXTSIZE)) != 0
            || self.fa_extsize != 0
            || self.fa_projid != 0
            || self.fa_cowextsize != 0
    }
}

define_syscall!(
    FileGetattr,
    |dirfd: i32, path: CString, attr: *mut LinuxFileAttr, size: usize, at_flags: AtFlags| {
        validate_file_attr_args(path, attr.cast_const(), size, at_flags)?;
        let object = file_attr_object_at(dirfd, path, at_flags)?;
        let linux_attr = LinuxFileAttr::from_vfs(object.linux_file_attributes()?);
        user_safe::write(attr, &linux_attr).map_err(|_| SyscallError::BadAddress)?;
        Ok(0)
    }
);

define_syscall!(
    FileSetattr,
    |dirfd: i32, path: CString, attr: *const LinuxFileAttr, size: usize, at_flags: AtFlags| {
        validate_file_attr_args(path, attr, size, at_flags)?;
        let requested = user_safe::read(attr).map_err(|_| SyscallError::BadAddress)?;
        let object = file_attr_object_at(dirfd, path, at_flags)?;
        object.set_linux_file_attributes(requested.vfs_attributes()?)?;
        Ok(0)
    }
);

fn validate_file_attr_args(
    path: CString,
    attr: *const LinuxFileAttr,
    size: usize,
    at_flags: AtFlags,
) -> Result<(), SyscallError> {
    if path.is_null() || attr.is_null() {
        return Err(SyscallError::BadAddress);
    }
    if size < FILE_ATTR_SIZE_VER0 {
        return Err(SyscallError::InvalidArguments);
    }
    if size > PAGE_SIZE {
        return Err(SyscallError::ArgumentListTooLong);
    }
    if at_flags.bits() & !SUPPORTED_FILE_ATTR_FLAGS != 0 {
        return Err(SyscallError::InvalidArguments);
    }
    Ok(())
}

fn file_attr_object_at(
    dirfd: i32,
    path: CString,
    at_flags: AtFlags,
) -> Result<Arc<FileLikeObject>, SyscallError> {
    let path = path_from_raw(path)?;
    if path.is_empty() && at_flags.contains(AtFlags::EMPTY_PATH) {
        return get_object_current_process(dirfd as u64)?
            .as_file_like()
            .map_err(|_| SyscallError::BadFileDescriptor);
    }

    let resolved = resolve_path_at(dirfd, &path)?;
    let object = if at_flags.contains(AtFlags::SYMLINK_NOFOLLOW) {
        open_path_nofollow(resolved)?
    } else {
        open_path(resolved)?
    };
    Ok(Arc::new(object))
}
