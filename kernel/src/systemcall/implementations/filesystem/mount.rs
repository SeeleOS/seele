use super::*;

define_syscall!(Mount, |source: CString,
                        target: CString,
                        filesystemtype: CString,
                        mountflags: u64,
                        data: CString| {
    let source = string_from_raw_optional(source)?.filter(|value| !value.is_empty());
    let target = path_from_raw(target)?;
    let filesystemtype =
        string_from_raw_optional(filesystemtype)?.filter(|value| !value.is_empty());
    let data = string_from_raw_optional(data)?.filter(|value| !value.is_empty());
    let target_object = open_path(Path::new(&target))?;
    let target_path = target_object.path();
    let target_is_directory = matches!(
        target_object.info()?.file_like_type,
        FileLikeType::Directory
    );
    let operation_flags = MountOperationFlags::from_bits_retain(mountflags);

    if operation_flags.contains(MountOperationFlags::MS_BIND) {
        if operation_flags.contains(MountOperationFlags::MS_REMOUNT) {
            let (remount_flags, remount_mask) = remount_bind_flag_update(mountflags);
            VirtualFS
                .lock()
                .remount_bind(
                    target_path,
                    remount_flags,
                    remount_mask,
                    operation_flags.contains(MountOperationFlags::MS_REC),
                )
                .map_err(SyscallError::from)?;
        } else {
            let source = source.ok_or(SyscallError::BadAddress)?;
            let source_path = resolve_path_at(AT_FDCWD, &source)?;
            VirtualFS
                .lock()
                .bind_mount(
                    source_path,
                    target_path,
                    operation_flags.contains(MountOperationFlags::MS_REC),
                )
                .map_err(SyscallError::from)?;
        }
        return Ok(0);
    }

    if operation_flags.contains(MountOperationFlags::MS_MOVE) {
        return Err(SyscallError::OperationNotSupported);
    }

    if operation_flags.contains(MountOperationFlags::MS_REMOUNT) {
        let (remount_flags, remount_mask) = remount_bind_flag_update(mountflags);
        VirtualFS
            .lock()
            .remount_bind(
                target_path,
                remount_flags,
                remount_mask,
                operation_flags.contains(MountOperationFlags::MS_REC),
            )
            .map_err(SyscallError::from)?;
        return Ok(0);
    }

    if operation_flags.intersects(
        MountOperationFlags::MS_PRIVATE
            | MountOperationFlags::MS_SLAVE
            | MountOperationFlags::MS_SHARED
            | MountOperationFlags::MS_UNBINDABLE,
    ) && filesystemtype.is_none()
    {
        return Ok(0);
    }

    if filesystemtype.is_none() {
        return Err(SyscallError::InvalidArguments);
    }

    if let Some(filesystemtype) = filesystemtype.as_deref()
        && (filesystemtype == "fuse" || filesystemtype.starts_with("fuse."))
    {
        if !target_is_directory {
            return Err(SyscallError::NotADirectory);
        }

        let options = parse_fuse_mount_options(data.as_deref())?;
        let fd = options.fd.ok_or(SyscallError::InvalidArguments)?;
        let fuse_device = get_object_current_process(fd)?
            .as_file_like()
            .ok()
            .and_then(|file| file.device_backing_object())
            .ok_or(SyscallError::InvalidArguments)?
            .as_fuse_device()
            .map_err(|_| SyscallError::InvalidArguments)?;

        VirtualFS
            .lock()
            .mount(target_path, FuseFs::new(fuse_device.connection.clone()))
            .map_err(SyscallError::from)?;
        return Ok(0);
    }

    if filesystemtype
        .as_deref()
        .is_some_and(|filesystemtype| !is_supported_api_mount(filesystemtype))
    {
        return Err(SyscallError::NoSyscall);
    }

    if filesystemtype.as_deref() == Some("tmpfs") {
        if !target_is_directory {
            return Err(SyscallError::NotADirectory);
        }
        resolve_dir_path(target_path.clone())?;
        let root_mode = tmpfs_root_mode_from_mount_data(data.as_deref())?;
        VirtualFS
            .lock()
            .mount(target_path.clone(), TmpFs::new())
            .map_err(SyscallError::from)?;
        if let Some(mode) = root_mode {
            let mount_root = open_path(target_path)?;
            mount_root.chmod(mode)?;
        }
    }
    Ok(0)
});

define_syscall!(Umount2, |target: CString, flags: UmountFlags| {
    let target = path_from_raw(target)?;
    let flags = validate_umount_flags(flags)?;
    let path = resolve_path_at(AT_FDCWD, &target)?.normalize();

    if flags.contains(UmountFlags::NOFOLLOW) {
        let _ = open_path_nofollow(path.clone())?;
    } else {
        let _ = open_path(path.clone())?;
    }

    if path == Path::new("/") {
        return Err(SyscallError::DeviceOrResourceBusy);
    }

    let mount_path = VirtualFS
        .lock()
        .mount_path(path.clone())
        .map_err(SyscallError::from)?;
    if mount_path != path {
        return Err(SyscallError::InvalidArguments);
    }

    if flags.contains(UmountFlags::DETACH) {
        VirtualFS
            .lock()
            .detach_mount(path)
            .map_err(SyscallError::from)?;
    } else {
        VirtualFS.lock().unmount(path).map_err(SyscallError::from)?;
    }
    Ok(0)
});

define_syscall!(Fsopen, |fs_name: CString, flags: FsOpenFlags| {
    let fs_name = path_from_raw(fs_name)?;
    let fd_flags = if flags.contains(FsOpenFlags::FSCONTEXT_CLOEXEC) {
        FdFlags::CLOEXEC
    } else {
        FdFlags::empty()
    };
    let fd = get_current_process()
        .lock()
        .push_object_with_flags(FsContextObject::new(fs_name), fd_flags);
    Ok(fd)
});

define_syscall!(Fsconfig, |fd: i32,
                           cmd: u32,
                           key: CString,
                           value: CString,
                           _aux: i32| {
    let object = get_object_current_process(fd as u64).map_err(SyscallError::from)?;
    let fs_context = object.as_fs_context()?;
    let command = FsConfigCommand::try_from(cmd).map_err(|_| SyscallError::InvalidArguments)?;
    let key = string_from_raw_optional(key)?;
    let value = string_from_raw_optional(value)?;
    fs_context.configure(command, key.as_deref(), value.as_deref())?;
    Ok(0)
});

define_syscall!(Fsmount, |fd: i32,
                          flags: FsMountFlags,
                          _mount_attrs: u32| {
    let object = get_object_current_process(fd as u64).map_err(SyscallError::from)?;
    let fs_context = object.as_fs_context()?;
    let mount_path = next_api_mount_path()?;
    let mounted_fs = fs_context.created_fs()?;
    VirtualFS
        .lock()
        .mount_ref(mount_path.clone(), mounted_fs)
        .map_err(SyscallError::from)?;
    if let Some(mode) = fs_context.root_mode()? {
        let mount_root = open_path(mount_path.clone())?;
        mount_root.chmod(mode)?;
    }

    let mount_root: ObjectRef = Arc::new(open_path(mount_path)?);
    let fd_flags = if flags.contains(FsMountFlags::FSMOUNT_CLOEXEC) {
        FdFlags::CLOEXEC
    } else {
        FdFlags::empty()
    };
    Ok(get_current_process()
        .lock()
        .push_object_with_flags(mount_root, fd_flags))
});

define_syscall!(
    MoveMount,
    |from_dirfd: i32,
     from_path: CString,
     to_dirfd: i32,
     to_path: CString,
     flags: MoveMountFlags| {
        let source_path = if from_path.is_null() {
            if !flags.contains(MoveMountFlags::MOVE_MOUNT_F_EMPTY_PATH) {
                return Err(SyscallError::BadAddress);
            }
            let object =
                get_object_current_process(from_dirfd as u64).map_err(SyscallError::from)?;
            object.as_file_like()?.path().normalize()
        } else {
            let from_path = path_from_raw(from_path)?;
            if from_path.is_empty() {
                if !flags.contains(MoveMountFlags::MOVE_MOUNT_F_EMPTY_PATH) {
                    return Err(SyscallError::InvalidArguments);
                }
                let object =
                    get_object_current_process(from_dirfd as u64).map_err(SyscallError::from)?;
                object.as_file_like()?.path().normalize()
            } else {
                resolve_path_at(from_dirfd, &from_path)?.normalize()
            }
        };

        let (mount_path, mount_fs, mount_source_path, mount_flags) =
            VirtualFS.lock().mount_metadata(source_path.clone())?;
        if mount_path != source_path {
            return Err(SyscallError::InvalidArguments);
        }

        let target_path = if to_path.is_null() {
            if !flags.contains(MoveMountFlags::MOVE_MOUNT_T_EMPTY_PATH) {
                return Err(SyscallError::BadAddress);
            }
            let object = get_object_current_process(to_dirfd as u64).map_err(SyscallError::from)?;
            object.as_file_like()?.path().normalize()
        } else {
            let to_path = path_from_raw(to_path)?;
            if to_path.is_empty() {
                if !flags.contains(MoveMountFlags::MOVE_MOUNT_T_EMPTY_PATH) {
                    return Err(SyscallError::InvalidArguments);
                }
                let object =
                    get_object_current_process(to_dirfd as u64).map_err(SyscallError::from)?;
                object.as_file_like()?.path().normalize()
            } else {
                resolve_path_at(to_dirfd, &to_path)?.normalize()
            }
        };

        let _ = open_path(target_path.clone())?;

        VirtualFS
            .lock()
            .attach_mount(target_path, mount_fs, mount_source_path, mount_flags)
            .map_err(SyscallError::from)?;
        VirtualFS
            .lock()
            .unmount(source_path.clone())
            .map_err(SyscallError::from)?;
        if is_api_mount_path(&source_path) {
            let _ = VirtualFS.lock().delete_file(source_path);
        }

        let _ = flags.contains(MoveMountFlags::MOVE_MOUNT_BENEATH);
        Ok(0)
    }
);

define_syscall!(OpenTree, |dirfd: i32,
                           path: CString,
                           flags: OpenTreeFlags| {
    let object = if path.is_null() {
        if !flags.contains(OpenTreeFlags::AT_EMPTY_PATH) {
            return Err(SyscallError::BadAddress);
        }
        get_object_current_process(dirfd as u64).map_err(SyscallError::from)?
    } else {
        let path = path_from_raw(path)?;
        if path.is_empty() && flags.contains(OpenTreeFlags::AT_EMPTY_PATH) {
            get_object_current_process(dirfd as u64).map_err(SyscallError::from)?
        } else {
            let path = resolve_path_at(dirfd, &path)?;
            let file = if flags.contains(OpenTreeFlags::AT_SYMLINK_NOFOLLOW) {
                open_path_nofollow(path)?
            } else {
                open_path(path)?
            };
            Arc::new(file)
        }
    };

    let _ = object.clone().as_file_like()?;

    let fd_flags = if flags.contains(OpenTreeFlags::OPEN_TREE_CLOEXEC) {
        FdFlags::CLOEXEC
    } else {
        FdFlags::empty()
    };
    let process = get_current_process();
    let fd = process.lock().push_object_with_flags(object, fd_flags);
    Ok(fd)
});

define_syscall!(MountSetattr, |dirfd: i32,
                               path: CString,
                               flags: AtFlags,
                               attr: *const LinuxMountAttr,
                               size: usize| {
    let allowed_flags =
        (AtFlags::SYMLINK_NOFOLLOW | AtFlags::EMPTY_PATH | AtFlags::RECURSIVE).bits();
    if flags.bits() & !allowed_flags != 0 {
        return Err(SyscallError::InvalidArguments);
    }
    if size < core::mem::size_of::<LinuxMountAttr>() {
        return Err(SyscallError::InvalidArguments);
    }
    if attr.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let attr = user_safe::read(attr)?;
    let target_path = mount_setattr_target_path(dirfd, path, flags)?;
    let (remount_flags, remount_mask) = mount_attr_flag_update(&attr)?;

    VirtualFS
        .lock()
        .remount_bind(
            target_path,
            remount_flags,
            remount_mask,
            flags.contains(AtFlags::RECURSIVE),
        )
        .map_err(SyscallError::from)?;

    Ok(0)
});

define_syscall!(
    NameToHandleAt,
    |dirfd: i32, path: CString, handle: *mut LinuxFileHandle, mount_id: *mut i32, flags: i32| {
        let raw_flags = flags;
        let flags = AtFlags::from_bits_truncate(flags);
        let allowed_flags = AtFlags::EMPTY_PATH | AtFlags::SYMLINK_NOFOLLOW | AtFlags::NO_AUTOMOUNT;
        if raw_flags != (flags & allowed_flags).bits() {
            return Err(SyscallError::InvalidArguments);
        }
        if handle.is_null() || mount_id.is_null() {
            return Err(SyscallError::BadAddress);
        }

        let path_str = if path.is_null() {
            if flags.contains(AtFlags::EMPTY_PATH) {
                String::new()
            } else {
                return Err(SyscallError::BadAddress);
            }
        } else {
            path_from_raw(path)?
        };
        let stat = stat_at(dirfd, &path_str, flags)?;
        let kernel_mount_id = stat_mount_id_at(dirfd, &path_str, flags)?;
        let mount_id_out =
            i32::try_from(kernel_mount_id).map_err(|_| SyscallError::InvalidArguments)?;
        let required_bytes = u32::try_from(size_of::<SeeleFileHandle>())
            .map_err(|_| SyscallError::InvalidArguments)?;
        let mut file_handle = user_safe::read(handle)?;
        let caller_bytes = file_handle.handle_bytes;
        file_handle.handle_type = SEELE_FILE_HANDLE_TYPE_INODE;
        file_handle.handle_bytes = required_bytes;
        user_safe::write(handle, &file_handle)?;
        user_safe::write(mount_id, &mount_id_out)?;

        if required_bytes > caller_bytes {
            return Err(SyscallError::ValueTooLarge);
        }

        let encoded = SeeleFileHandle { inode: stat.st_ino };
        let handle_bytes = encoded.inode.to_ne_bytes();
        let handle_ptr = unsafe { handle.cast::<u8>().add(size_of::<LinuxFileHandle>()) };
        user_safe::write_buffer(handle_ptr, &handle_bytes)?;
        Ok(0)
    }
);

#[cfg(test)]
mod tests {
    use crate::{
        filesystem::{path::Path, vfs::VirtualFS},
        object::misc::get_object_current_process,
        process::FdFlags,
        systemcall::{
            implementations::{
                Fsconfig, Fsmount, Fsopen, Mount, MountSetattr, MoveMount, OpenTree, Umount2,
            },
            test::{assert_fd_flags, close_test_fd, expect_fd, write_user_cstr},
            test_helpers::{
                SyscallArgs, allocate_user_test_page, expect_errno, expect_ok, write_user_value,
            },
            utils::SyscallError,
        },
    };

    crate::test!(
        mount_api_syscalls,
        "mount and new mount api syscalls follow linux rules",
        mount_api_syscalls_follow_linux_rules
    );

    fn mount_api_syscalls_follow_linux_rules() {
        const AT_FDCWD: u64 = (-100i32) as u64;
        const AT_RECURSIVE: u64 = 0x8000;
        const OPEN_TREE_CLOEXEC: u64 = 0x0008_0000;
        const MOVE_MOUNT_F_EMPTY_PATH: u64 = 0x0000_0004;
        const FSCONFIG_SET_STRING: u64 = 1;
        const FSCONFIG_CMD_CREATE: u64 = 6;
        const MS_BIND: u64 = 4096;

        #[repr(C)]
        #[derive(Clone, Copy, Default)]
        struct TestLinuxMountAttr {
            attr_set: u64,
            attr_clr: u64,
            propagation: u64,
            userns_fd: u64,
        }

        let page = allocate_user_test_page();
        VirtualFS
            .lock()
            .create_dir(Path::new("/tmp/syscall-mount-test"))
            .unwrap();
        VirtualFS
            .lock()
            .create_dir(Path::new("/tmp/syscall-mount-test/src"))
            .unwrap();
        VirtualFS
            .lock()
            .create_dir(Path::new("/tmp/syscall-mount-test/dst"))
            .unwrap();
        VirtualFS
            .lock()
            .create_dir(Path::new("/tmp/syscall-mount-test/newdst"))
            .unwrap();
        write_user_cstr(page, b"/tmp/syscall-mount-test/src\0");
        write_user_cstr(page + 128, b"/tmp/syscall-mount-test/dst\0");
        write_user_cstr(page + 256, b"/tmp/syscall-mount-test/newdst\0");
        write_user_cstr(page + 384, b"tmpfs\0");
        write_user_cstr(page + 448, b"mode=700\0");
        write_user_cstr(page + 512, b"mode\0");
        write_user_cstr(page + 576, b"755\0");
        write_user_cstr(page + 704, b"\0");

        expect_ok(
            SyscallArgs::new([0, page + 128, page + 384, 0, page + 448, 0]).call::<Mount>(),
            0,
        );
        let mounted_root = {
            let mut vfs = VirtualFS.lock();
            vfs.open(Path::new("/tmp/syscall-mount-test/dst")).unwrap()
        };
        let mounted_stat = mounted_root.stat();
        assert_eq!(mounted_stat.st_mode & 0o777, 0o700);

        expect_ok(
            SyscallArgs::new([page, page + 128, 0, MS_BIND, 0, 0]).call::<Mount>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([page + 128, 0, 0, 0, 0, 0]).call::<Umount2>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([page + 128, 16, 0, 0, 0, 0]).call::<Umount2>(),
            SyscallError::InvalidArguments,
        );

        let fsfd = expect_fd(SyscallArgs::new([page + 384, 1, 0, 0, 0, 0]).call::<Fsopen>());
        assert_fd_flags(fsfd, FdFlags::CLOEXEC);
        expect_ok(
            SyscallArgs::new([
                fsfd as u64,
                FSCONFIG_SET_STRING,
                page + 512,
                page + 576,
                0,
                0,
            ])
            .call::<Fsconfig>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([fsfd as u64, FSCONFIG_CMD_CREATE, 0, 0, 0, 0]).call::<Fsconfig>(),
            0,
        );
        let mount_fd = expect_fd(SyscallArgs::new([fsfd as u64, 1, 0, 0, 0, 0]).call::<Fsmount>());
        assert_fd_flags(mount_fd, FdFlags::CLOEXEC);
        let mount_root_stat = get_object_current_process(mount_fd as u64)
            .unwrap()
            .as_statable()
            .unwrap()
            .stat();
        assert_eq!(mount_root_stat.st_mode & 0o777, 0o755);

        let tree_fd = expect_fd(
            SyscallArgs::new([AT_FDCWD, page + 128, OPEN_TREE_CLOEXEC, 0, 0, 0]).call::<OpenTree>(),
        );
        assert_fd_flags(tree_fd, FdFlags::CLOEXEC);
        expect_ok(
            SyscallArgs::new([
                mount_fd as u64,
                page + 704,
                AT_FDCWD,
                page + 256,
                MOVE_MOUNT_F_EMPTY_PATH,
                0,
            ])
            .call::<MoveMount>(),
            0,
        );
        let moved_root = {
            let mut vfs = VirtualFS.lock();
            vfs.open(Path::new("/tmp/syscall-mount-test/newdst"))
                .unwrap()
        };
        let moved_stat = moved_root.stat();
        assert_eq!(moved_stat.st_mode & 0o777, 0o755);

        write_user_value(
            page + 768,
            &TestLinuxMountAttr {
                attr_set: 1,
                attr_clr: 0,
                propagation: 0,
                userns_fd: 0,
            },
        );
        expect_ok(
            SyscallArgs::new([
                AT_FDCWD,
                page + 256,
                AT_RECURSIVE,
                page + 768,
                core::mem::size_of::<TestLinuxMountAttr>() as u64,
                0,
            ])
            .call::<MountSetattr>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([AT_FDCWD, 0, 0, page + 768, 1, 0]).call::<MountSetattr>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([
                AT_FDCWD,
                0,
                0,
                0,
                core::mem::size_of::<TestLinuxMountAttr>() as u64,
                0,
            ])
            .call::<MountSetattr>(),
            SyscallError::BadAddress,
        );
        expect_errno(
            SyscallArgs::new([AT_FDCWD, page + 128, 2, 0, 0, 0]).call::<OpenTree>(),
            SyscallError::InvalidArguments,
        );

        close_test_fd(tree_fd);
        close_test_fd(mount_fd);
        close_test_fd(fsfd);
        let _ = SyscallArgs::new([page + 256, 0, 0, 0, 0, 0]).call::<Umount2>();
        let _ = VirtualFS
            .lock()
            .delete_file(Path::new("/tmp/syscall-mount-test/newdst"));
        let _ = VirtualFS
            .lock()
            .delete_file(Path::new("/tmp/syscall-mount-test/dst"));
        let _ = VirtualFS
            .lock()
            .delete_file(Path::new("/tmp/syscall-mount-test/src"));
        let _ = VirtualFS
            .lock()
            .delete_file(Path::new("/tmp/syscall-mount-test"));
    }
}
