use super::*;

define_syscall!(Mount, |source: CString,
                        target: CString,
                        filesystemtype: CString,
                        mountflags: u64,
                        data: CString| {
    require_cap_sys_admin()?;
    let source = string_from_raw_optional(source)?.filter(|value| !value.is_empty());
    let target = path_from_raw(target)?;
    let target_is_proc_fd = is_proc_fd_path(&target);
    let target = resolve_path_at(AT_FDCWD, &target)?;
    let filesystemtype =
        string_from_raw_optional(filesystemtype)?.filter(|value| !value.is_empty());
    let data = string_from_raw_optional(data)?.filter(|value| !value.is_empty());
    let target_object = open_path(target.clone())?;
    let target_path = target_object.path();
    let target_is_directory = matches!(
        target_object.info()?.file_like_type,
        FileLikeType::Directory
    );
    let operation_flags = MountOperationFlags::from_bits_retain(mountflags);
    let requested_mount_flags = mount_flags_from_mount_bits(mountflags);
    let propagation = mount_propagation_from_mount_flags(operation_flags);

    if operation_flags.contains(MountOperationFlags::MS_BIND) {
        if operation_flags.contains(MountOperationFlags::MS_REMOUNT) {
            ensure_mount_root(&target_path)?;
            let (remount_flags, remount_mask) = remount_bind_flag_update(mountflags);
            VirtualFS
                .lock()
                .remount_bind_in_current_namespace(
                    target_path,
                    remount_flags,
                    remount_mask,
                    operation_flags.contains(MountOperationFlags::MS_REC),
                )
                .map_err(SyscallError::from)?;
        } else {
            let source = source.ok_or(SyscallError::BadAddress)?;
            let source_path = resolve_path_at(AT_FDCWD, &source)?;
            if target_is_proc_fd {
                return Err(SyscallError::FileNotFound);
            }
            if is_proc_fd_path(&source_path.clone().normalize().as_string()) {
                return Err(SyscallError::FileNotFound);
            }
            VirtualFS
                .lock()
                .bind_mount(
                    source_path.clone(),
                    target_path.clone(),
                    operation_flags.contains(MountOperationFlags::MS_REC),
                )
                .map_err(SyscallError::from)?;
            publish_mount_to_shared_namespace(target_path)?;
        }
        return Ok(0);
    }

    if operation_flags.contains(MountOperationFlags::MS_MOVE) {
        let source = source.ok_or(SyscallError::InvalidArguments)?;
        let source_path = resolve_path_at(AT_FDCWD, &source)?.normalize();
        let (mount_path, mount_fs, mount_source_path, mount_flags) =
            VirtualFS.lock().mount_metadata(source_path.clone())?;
        if mount_path != source_path {
            return Err(SyscallError::InvalidArguments);
        }
        VirtualFS
            .lock()
            .attach_mount(target_path, mount_fs, mount_source_path, mount_flags)
            .map_err(SyscallError::from)?;
        VirtualFS
            .lock()
            .unmount(source_path)
            .map_err(SyscallError::from)?;
        return Ok(0);
    }

    if operation_flags.contains(MountOperationFlags::MS_REMOUNT) {
        ensure_mount_root(&target_path)?;
        if VirtualFS.lock().is_mount_busy(target_path.clone())? {
            return Err(SyscallError::DeviceOrResourceBusy);
        }
        let (remount_flags, remount_mask) = remount_bind_flag_update(mountflags);
        VirtualFS
            .lock()
            .remount_bind_in_current_namespace(
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
    ) {
        if filesystemtype.as_deref().is_some_and(|fs| fs != "none") {
            return Err(SyscallError::InvalidArguments);
        }
        let propagation = propagation.ok_or(SyscallError::InvalidArguments)?;
        VirtualFS
            .lock()
            .set_mount_propagation(
                target_path,
                propagation,
                operation_flags.contains(MountOperationFlags::MS_REC),
            )
            .map_err(SyscallError::from)?;
        if propagation == crate::filesystem::vfs::MountPropagationUpdate::Shared {
            let process = get_current_process();
            let mut process = process.lock();
            if process.user_namespace_uid_map.is_none() {
                process.mount_namespace_shared_with_parent = true;
            }
        }
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
            .mount(
                target_path.clone(),
                FuseFs::new(fuse_device.connection.clone()),
            )
            .map_err(SyscallError::from)?;
        publish_mount_to_shared_namespace(target_path)?;
        return Ok(0);
    }

    let filesystemtype = filesystemtype
        .as_deref()
        .ok_or(SyscallError::InvalidArguments)?;
    if !is_supported_api_mount(filesystemtype) {
        return Err(SyscallError::NoDevice);
    }

    if !target_is_directory {
        return Err(SyscallError::NotADirectory);
    }
    resolve_dir_path(target_path.clone())?;
    if VirtualFS.lock().contains_mount_at(target_path.clone()) {
        return Err(SyscallError::DeviceOrResourceBusy);
    }

    if filesystemtype == "tmpfs" {
        let root_mode = tmpfs_root_mode_from_mount_data(data.as_deref())?;
        VirtualFS
            .lock()
            .mount(target_path.clone(), TmpFs::new())
            .map_err(SyscallError::from)?;
        apply_initial_mount_flags(target_path.clone(), requested_mount_flags)?;
        if let Some(mode) = root_mode {
            let mount_root = open_path(target_path.clone())?;
            mount_root.chmod(mode)?;
        }
        publish_mount_to_shared_namespace(target_path)?;
        return Ok(0);
    }

    if matches!(filesystemtype, "ext2" | "ext3" | "ext4") {
        let source = source.ok_or(SyscallError::InvalidArguments)?;
        let source_path = resolve_path_at(AT_FDCWD, &source)?;
        if let Ok((source_info, _)) = resolve_path_info_with_final(source_path.clone(), false)
            && is_char_device_mode(source_info.permission.0)
        {
            return Err(SyscallError::BlockDeviceRequired);
        }
        let source_object = open_path(source_path)?;
        if is_char_device_mode(source_object.info()?.permission.0) {
            return Err(SyscallError::BlockDeviceRequired);
        }
        let block_device = source_object
            .device_backing_object()
            .ok_or(SyscallError::NoDevice)?
            .as_block_device()?;
        let device = block_device.backing_device();
        let reader = Ext4BlockOperator::new(device.clone());
        let writer = Ext4BlockOperator::new(device.clone());
        let ext4 = Ext4Inner::load_with_writer(Box::new(reader), Some(Box::new(writer)))
            .map_err(|_| SyscallError::IOError)?;
        let ext4 = EXT4::new_with_device(ext4, device).map_err(|_| SyscallError::IOError)?;
        VirtualFS
            .lock()
            .mount(target_path.clone(), ext4)
            .map_err(SyscallError::from)?;
        apply_initial_mount_flags(target_path.clone(), requested_mount_flags)?;
        publish_mount_to_shared_namespace(target_path)?;
        return Ok(0);
    }

    let fs = create_api_filesystem(filesystemtype)?;
    VirtualFS
        .lock()
        .mount_ref(target_path.clone(), fs)
        .map_err(SyscallError::from)?;
    apply_initial_mount_flags(target_path.clone(), requested_mount_flags)?;
    publish_mount_to_shared_namespace(target_path)?;
    Ok(0)
});

define_syscall!(Umount2, |target: CString, flags: UmountFlags| {
    require_cap_sys_admin()?;
    let target = path_from_raw(target)?;
    let flags = validate_umount_flags(flags)?;
    let path = resolve_path_at(AT_FDCWD, &target)?.normalize();

    if path == Path::new("/") {
        return Err(SyscallError::DeviceOrResourceBusy);
    }

    if !flags.contains(UmountFlags::EXPIRE) {
        if flags.contains(UmountFlags::NOFOLLOW) {
            let _ = open_path_nofollow(path.clone())?;
        } else {
            let _ = resolve_path_info_with_final(path.clone(), false)?;
            let _ = open_path(path.clone())?;
        }
    }

    let mount_path = VirtualFS
        .lock()
        .mount_path(path.clone())
        .map_err(SyscallError::from)?;
    if mount_path != path {
        return Err(SyscallError::InvalidArguments);
    }
    if flags.contains(UmountFlags::EXPIRE) {
        if flags.intersects(UmountFlags::FORCE | UmountFlags::DETACH) {
            return Err(SyscallError::InvalidArguments);
        }
        if !VirtualFS.lock().begin_expire_mount(path.clone())? {
            return Err(SyscallError::TryAgain);
        }
    }
    if VirtualFS.lock().is_mount_busy(path.clone())? {
        return Err(SyscallError::DeviceOrResourceBusy);
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
    if !is_supported_fs_context_type(&fs_name) {
        return Err(SyscallError::NoDevice);
    }
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

define_syscall!(Fsmount, |fd: i32, flags: FsMountFlags, mount_attrs: u32| {
    const FSMOUNT_SUPPORTED_ATTRS: u32 = (MOUNT_ATTR_RDONLY
        | MOUNT_ATTR_NOSUID
        | MOUNT_ATTR_NODEV
        | MOUNT_ATTR_NOEXEC
        | MOUNT_ATTR_NOATIME
        | MOUNT_ATTR_STRICTATIME
        | MOUNT_ATTR_NODIRATIME) as u32;
    if mount_attrs & !FSMOUNT_SUPPORTED_ATTRS != 0 {
        return Err(SyscallError::InvalidArguments);
    }
    let attr = LinuxMountAttr {
        attr_set: u64::from(mount_attrs),
        attr_clr: 0,
        propagation: 0,
        userns_fd: 0,
    };
    let (mount_flags, mount_mask, _) = mount_attr_flag_update(&attr)?;
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
    VirtualFS
        .lock()
        .remount_bind(mount_path.clone(), mount_flags, mount_mask, false)
        .map_err(SyscallError::from)?;

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
                let object =
                    get_object_current_process(from_dirfd as u64).map_err(SyscallError::from)?;
                let file_like = object.as_file_like()?;
                let base = file_like.path().normalize();
                let resolved = if from_path.starts_with('/') {
                    resolve_path_at(from_dirfd, &from_path)?
                } else {
                    let candidate = base.join_path(&Path::new(&from_path));
                    let _ = open_path(candidate.clone())?;
                    candidate
                };
                resolved.normalize()
            }
        };

        let (mount_path, mount_fs, mount_source_path, mount_flags, mount_propagation) = VirtualFS
            .lock()
            .mount_metadata_with_propagation(source_path.clone())?;
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

        if flags.contains(MoveMountFlags::MOVE_MOUNT_BENEATH) {
            VirtualFS
                .lock()
                .stack_mount_beneath(source_path.clone(), target_path)
                .map_err(SyscallError::from)?;
        } else {
            let attached_path = target_path.clone();
            VirtualFS
                .lock()
                .attach_mount_with_propagation(
                    target_path,
                    mount_fs,
                    mount_source_path,
                    mount_flags,
                    mount_propagation,
                )
                .map_err(SyscallError::from)?;
            VirtualFS
                .lock()
                .unmount(source_path.clone())
                .map_err(SyscallError::from)?;
            publish_mount_to_shared_namespace(attached_path)?;
        }
        if is_api_mount_path(&source_path) {
            let _ = VirtualFS.lock().delete_file(source_path);
        }

        Ok(0)
    }
);

define_syscall!(Fspick, |dirfd: i32, path: CString, flags: FsPickFlags| {
    let object_path = if path.is_null() {
        if !flags.contains(FsPickFlags::FSPICK_EMPTY_PATH) {
            return Err(SyscallError::BadAddress);
        }
        get_object_current_process(dirfd as u64)
            .map_err(SyscallError::from)?
            .as_file_like()?
            .path()
            .normalize()
    } else {
        let path = path_from_raw(path)?;
        if path.is_empty() {
            if !flags.contains(FsPickFlags::FSPICK_EMPTY_PATH) {
                return Err(SyscallError::InvalidArguments);
            }
            get_object_current_process(dirfd as u64)
                .map_err(SyscallError::from)?
                .as_file_like()?
                .path()
                .normalize()
        } else {
            let resolved = resolve_path_at(dirfd, &path)?.normalize();
            if flags.contains(FsPickFlags::FSPICK_SYMLINK_NOFOLLOW) {
                open_path_nofollow(resolved.clone())?;
            } else {
                open_path(resolved.clone())?;
            }
            resolved
        }
    };

    let (mount_path, fs, _, _) = VirtualFS
        .lock()
        .mount_metadata(object_path)
        .map_err(SyscallError::from)?;
    let fs_context = FsContextObject::new_picked("picked".into(), fs, mount_path);
    let fd_flags = if flags.contains(FsPickFlags::FSPICK_CLOEXEC) {
        FdFlags::CLOEXEC
    } else {
        FdFlags::empty()
    };
    Ok(get_current_process()
        .lock()
        .push_object_with_flags(fs_context, fd_flags))
});

define_syscall!(OpenTree, |dirfd: i32,
                           path: CString,
                           flags: OpenTreeFlags| {
    let object_path = if path.is_null() {
        if !flags.contains(OpenTreeFlags::AT_EMPTY_PATH) {
            return Err(SyscallError::BadAddress);
        }
        get_object_current_process(dirfd as u64)
            .map_err(SyscallError::from)?
            .as_file_like()?
            .path()
            .normalize()
    } else {
        let path = path_from_raw(path)?;
        if path.is_empty() && flags.contains(OpenTreeFlags::AT_EMPTY_PATH) {
            get_object_current_process(dirfd as u64)
                .map_err(SyscallError::from)?
                .as_file_like()?
                .path()
                .normalize()
        } else {
            let path = resolve_path_at(dirfd, &path)?;
            if flags.contains(OpenTreeFlags::AT_SYMLINK_NOFOLLOW) {
                open_path_nofollow(path.clone())?;
            } else {
                open_path(path.clone())?;
            };
            path.normalize()
        }
    };

    let object: ObjectRef = if flags.contains(OpenTreeFlags::OPEN_TREE_CLONE) {
        let (fs, source_path, mount_flags, propagation) = VirtualFS
            .lock()
            .mount_metadata_for_path_with_propagation(object_path.clone())?;
        let detached_path = next_api_mount_path()?;
        VirtualFS
            .lock()
            .attach_mount_with_propagation(
                detached_path.clone(),
                fs,
                source_path,
                mount_flags,
                propagation,
            )
            .map_err(SyscallError::from)?;
        Arc::new(open_path(detached_path)?)
    } else {
        let object: ObjectRef = Arc::new(open_path(object_path)?);
        object.clone().set_flags(FileFlags::PATH)?;
        object
    };

    let fd_flags = if flags.contains(OpenTreeFlags::OPEN_TREE_CLOEXEC) {
        FdFlags::CLOEXEC
    } else {
        FdFlags::empty()
    };
    let process = get_current_process();
    let fd = process.lock().push_object_with_flags(object, fd_flags);
    Ok(fd)
});

define_syscall!(OpenTreeAttr, |dirfd: i32,
                               path: CString,
                               flags: OpenTreeAttrFlags,
                               attr: *const LinuxMountAttr,
                               size: usize| {
    if size < core::mem::size_of::<LinuxMountAttr>() {
        return Err(SyscallError::InvalidArguments);
    }
    if attr.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let attr = user_safe::read(attr)?;
    let open_tree_flags = OpenTreeFlags::from_bits(
        (flags.bits()
            & (OpenTreeFlags::OPEN_TREE_CLONE
                | OpenTreeFlags::AT_SYMLINK_NOFOLLOW
                | OpenTreeFlags::AT_NO_AUTOMOUNT
                | OpenTreeFlags::AT_EMPTY_PATH
                | OpenTreeFlags::OPEN_TREE_CLOEXEC)
                .bits() as u64) as u32,
    )
    .ok_or(SyscallError::InvalidArguments)?;
    let fd = OpenTree::handle_call(
        dirfd as u64,
        path as u64,
        open_tree_flags.bits() as u64,
        0,
        0,
        0,
    )?;
    let target_path = get_object_current_process(fd as u64)
        .map_err(SyscallError::from)?
        .as_file_like()?
        .path()
        .normalize();
    let (remount_flags, remount_mask, propagation) = mount_attr_flag_update(&attr)?;
    VirtualFS
        .lock()
        .remount_bind_in_current_namespace(
            target_path.clone(),
            remount_flags,
            remount_mask,
            flags.contains(OpenTreeAttrFlags::RECURSIVE),
        )
        .map_err(SyscallError::from)?;
    if let Some(propagation) = propagation {
        VirtualFS
            .lock()
            .set_mount_propagation(
                target_path,
                propagation,
                flags.contains(OpenTreeAttrFlags::RECURSIVE),
            )
            .map_err(SyscallError::from)?;
    }
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
    let mount_path = VirtualFS
        .lock()
        .mount_path(target_path.clone())
        .map_err(SyscallError::from)?;
    if mount_path != target_path {
        return Err(SyscallError::InvalidArguments);
    }
    let (remount_flags, remount_mask, propagation) = mount_attr_flag_update(&attr)?;

    VirtualFS
        .lock()
        .remount_bind_in_current_namespace(
            target_path.clone(),
            remount_flags,
            remount_mask,
            flags.contains(AtFlags::RECURSIVE),
        )
        .map_err(SyscallError::from)?;
    if let Some(propagation) = propagation {
        VirtualFS
            .lock()
            .set_mount_propagation(target_path, propagation, flags.contains(AtFlags::RECURSIVE))
            .map_err(SyscallError::from)?;
    }

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
        object::{
            misc::get_object_current_process,
            traits::{Readable, Statable},
        },
        process::FdFlags,
        systemcall::{
            implementations::{
                Fsconfig, Fsmount, Fsopen, Fspick, Mkdir, Mount, MountSetattr, MoveMount, OpenTree,
                Statx, Umount2,
            },
            test::{
                TestLinuxStatx, assert_fd_flags, close_test_fd, expect_fd, read_user_value,
                write_user_cstr,
            },
            test_helpers::{
                SyscallArgs, allocate_user_test_page, expect_errno, expect_ok, write_user_value,
            },
            utils::SyscallError,
        },
    };
    use alloc::string::String;

    crate::test!(
        mount_api_syscalls,
        "mount and new mount api syscalls follow linux rules",
        mount_api_syscalls_follow_linux_rules
    );

    fn mount_api_syscalls_follow_linux_rules() {
        const AT_FDCWD: u64 = (-100i32) as u64;
        const AT_RECURSIVE: u64 = 0x8000;
        const OPEN_TREE_CLONE: u64 = 0x0000_0001;
        const OPEN_TREE_CLOEXEC: u64 = 0x0008_0000;
        const MOVE_MOUNT_F_EMPTY_PATH: u64 = 0x0000_0004;
        const FSCONFIG_SET_STRING: u64 = 1;
        const FSCONFIG_CMD_CREATE: u64 = 6;
        const MS_BIND: u64 = 4096;
        const STATX_BASIC_STATS: u64 = 0x0000_07ff;
        const STATX_ATTR_MOUNT_ROOT: u64 = 0x0000_2000;
        const STATX_BUF: u64 = 3584;

        #[repr(C)]
        #[derive(Clone, Copy, Default)]
        struct TestLinuxMountAttr {
            attr_set: u64,
            attr_clr: u64,
            propagation: u64,
            userns_fd: u64,
        }

        fn assert_mount_root(page: u64, path_ptr: u64, expected: bool) {
            expect_ok(
                SyscallArgs::new([
                    AT_FDCWD,
                    path_ptr,
                    0,
                    STATX_BASIC_STATS,
                    page + STATX_BUF,
                    0,
                ])
                .call::<Statx>(),
                0,
            );
            let statx = read_user_value::<TestLinuxStatx>(page + STATX_BUF);
            assert_eq!(statx.stx_attributes & STATX_ATTR_MOUNT_ROOT != 0, expected);
        }

        fn proc_mountinfo() -> String {
            let mountinfo = {
                let mut vfs = VirtualFS.lock();
                vfs.open(Path::new("/proc/self/mountinfo")).unwrap()
            };
            let mut buffer = [0u8; 4096];
            let read = mountinfo.read(&mut buffer).unwrap();
            String::from(core::str::from_utf8(&buffer[..read]).unwrap())
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
        VirtualFS
            .lock()
            .create_dir(Path::new("/tmp/syscall-mount-test/cloned-src"))
            .unwrap();
        for path in [
            "/tmp/syscall-mount-test/proc",
            "/tmp/syscall-mount-test/sys",
            "/tmp/syscall-mount-test/dev",
            "/tmp/syscall-mount-test/pts",
            "/tmp/syscall-mount-test/cgroup",
            "/tmp/syscall-mount-test/run",
            "/tmp/syscall-mount-test/tmp",
            "/tmp/syscall-mount-test/shm",
        ] {
            VirtualFS.lock().create_dir(Path::new(path)).unwrap();
        }
        write_user_cstr(page, b"/tmp/syscall-mount-test/src\0");
        write_user_cstr(page + 128, b"/tmp/syscall-mount-test/dst\0");
        write_user_cstr(page + 256, b"/tmp/syscall-mount-test/newdst\0");
        write_user_cstr(page + 320, b"/tmp/syscall-mount-test/cloned-src\0");
        write_user_cstr(page + 384, b"tmpfs\0");
        write_user_cstr(page + 448, b"mode=700\0");
        write_user_cstr(page + 512, b"mode\0");
        write_user_cstr(page + 576, b"755\0");
        write_user_cstr(page + 640, b"source\0");
        write_user_cstr(page + 832, b"tmpfs\0");
        write_user_cstr(page + 704, b"\0");
        write_user_cstr(page + 896, b"/tmp/syscall-mount-test/proc\0");
        write_user_cstr(page + 1024, b"/tmp/syscall-mount-test/sys\0");
        write_user_cstr(page + 1152, b"/tmp/syscall-mount-test/dev\0");
        write_user_cstr(page + 1280, b"/tmp/syscall-mount-test/pts\0");
        write_user_cstr(page + 1408, b"/tmp/syscall-mount-test/cgroup\0");
        write_user_cstr(page + 1536, b"proc\0");
        write_user_cstr(page + 1664, b"sysfs\0");
        write_user_cstr(page + 1792, b"devtmpfs\0");
        write_user_cstr(page + 1920, b"devpts\0");
        write_user_cstr(page + 2048, b"cgroup2\0");
        write_user_cstr(page + 2176, b"missingfs\0");
        write_user_cstr(page + 2304, b"/tmp/syscall-mount-test/dev\0");
        write_user_cstr(page + 2432, b"/tmp/syscall-mount-test/run\0");
        write_user_cstr(page + 2560, b"/tmp/syscall-mount-test/tmp\0");
        write_user_cstr(page + 2688, b"/tmp/syscall-mount-test/shm\0");
        write_user_cstr(page + 2816, b"/tmp/syscall-mount-test/dev/hugepages\0");
        write_user_cstr(page + 2944, b"/dev/hugepages\0");

        for path_ptr in [page + 2304, page + 2432, page + 2560, page + 2688] {
            assert_mount_root(page, path_ptr, false);
        }

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

        let subtree_fd = expect_fd(
            SyscallArgs::new([AT_FDCWD, page, OPEN_TREE_CLONE | OPEN_TREE_CLOEXEC, 0, 0, 0])
                .call::<OpenTree>(),
        );
        expect_ok(
            SyscallArgs::new([
                subtree_fd as u64,
                page + 704,
                AT_FDCWD,
                page + 320,
                MOVE_MOUNT_F_EMPTY_PATH,
                0,
            ])
            .call::<MoveMount>(),
            0,
        );
        let cloned_root = {
            let mut vfs = VirtualFS.lock();
            vfs.open(Path::new("/tmp/syscall-mount-test/cloned-src"))
                .unwrap()
        };
        assert_eq!(
            cloned_root.path(),
            Path::new("/tmp/syscall-mount-test/cloned-src")
        );

        let picked_fd =
            expect_fd(SyscallArgs::new([AT_FDCWD, page + 256, 1, 0, 0, 0]).call::<Fspick>());
        assert_fd_flags(picked_fd, FdFlags::CLOEXEC);
        let picked_mount_fd =
            expect_fd(SyscallArgs::new([picked_fd as u64, 0, 0, 0, 0, 0]).call::<Fsmount>());
        expect_ok(
            SyscallArgs::new([
                picked_mount_fd as u64,
                page + 704,
                AT_FDCWD,
                page,
                MOVE_MOUNT_F_EMPTY_PATH,
                0,
            ])
            .call::<MoveMount>(),
            0,
        );

        let proc_fsfd = expect_fd(SyscallArgs::new([page + 1536, 0, 0, 0, 0, 0]).call::<Fsopen>());
        expect_ok(
            SyscallArgs::new([proc_fsfd as u64, FSCONFIG_CMD_CREATE, 0, 0, 0, 0])
                .call::<Fsconfig>(),
            0,
        );
        let proc_mount_fd =
            expect_fd(SyscallArgs::new([proc_fsfd as u64, 0, 0, 0, 0, 0]).call::<Fsmount>());
        expect_ok(
            SyscallArgs::new([
                proc_mount_fd as u64,
                page + 704,
                AT_FDCWD,
                page + 896,
                MOVE_MOUNT_F_EMPTY_PATH,
                0,
            ])
            .call::<MoveMount>(),
            0,
        );

        let tmpfs_fsfd = expect_fd(SyscallArgs::new([page + 384, 0, 0, 0, 0, 0]).call::<Fsopen>());
        expect_ok(
            SyscallArgs::new([
                tmpfs_fsfd as u64,
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
            SyscallArgs::new([
                tmpfs_fsfd as u64,
                FSCONFIG_SET_STRING,
                page + 640,
                page + 832,
                0,
                0,
            ])
            .call::<Fsconfig>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([tmpfs_fsfd as u64, FSCONFIG_CMD_CREATE, 0, 0, 0, 0])
                .call::<Fsconfig>(),
            0,
        );
        let tmpfs_mount_fd =
            expect_fd(SyscallArgs::new([tmpfs_fsfd as u64, 0, 0, 0, 0, 0]).call::<Fsmount>());
        expect_ok(
            SyscallArgs::new([
                tmpfs_mount_fd as u64,
                page + 704,
                AT_FDCWD,
                page + 128,
                MOVE_MOUNT_F_EMPTY_PATH,
                0,
            ])
            .call::<MoveMount>(),
            0,
        );

        expect_errno(
            SyscallArgs::new([0, page + 896, page + 1536, 0, 0, 0]).call::<Mount>(),
            SyscallError::DeviceOrResourceBusy,
        );
        expect_ok(
            SyscallArgs::new([0, page + 1024, page + 1664, 0, 0, 0]).call::<Mount>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([0, page + 1152, page + 1792, 0, 0, 0]).call::<Mount>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([0, page + 1280, page + 1920, 0, 0, 0]).call::<Mount>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([0, page + 1408, page + 2048, 0, 0, 0]).call::<Mount>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([0, page + 2432, page + 384, 0, 0, 0]).call::<Mount>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([0, page + 2560, page + 384, 0, 0, 0]).call::<Mount>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([0, page + 2688, page + 384, 0, 0, 0]).call::<Mount>(),
            0,
        );
        for path_ptr in [page + 2304, page + 2432, page + 2560, page + 2688] {
            assert_mount_root(page, path_ptr, true);
        }
        expect_ok(
            SyscallArgs::new([page + 2816, 0o755, 0, 0, 0, 0]).call::<Mkdir>(),
            0,
        );
        let _ = SyscallArgs::new([page + 2944, 0, 0, 0, 0, 0]).call::<Umount2>();
        let _ = VirtualFS.lock().delete_file(Path::new("/dev/hugepages"));
        expect_ok(
            SyscallArgs::new([page + 2944, 0o755, 0, 0, 0, 0]).call::<Mkdir>(),
            0,
        );
        let _ = VirtualFS.lock().delete_file(Path::new("/dev/hugepages"));
        let mountinfo = proc_mountinfo();
        for mount_path in [
            "/tmp/syscall-mount-test/dev",
            "/tmp/syscall-mount-test/run",
            "/tmp/syscall-mount-test/tmp",
            "/tmp/syscall-mount-test/shm",
        ] {
            assert!(mountinfo.contains(mount_path));
        }
        expect_errno(
            SyscallArgs::new([0, page + 128, page + 2176, 0, 0, 0]).call::<Mount>(),
            SyscallError::NoDevice,
        );

        let proc_filesystems = {
            let mut vfs = VirtualFS.lock();
            vfs.open(Path::new("/tmp/syscall-mount-test/proc/filesystems"))
                .unwrap()
        };
        let mut buffer = [0u8; 128];
        let read = proc_filesystems.read(&mut buffer).unwrap();
        let filesystems = core::str::from_utf8(&buffer[..read]).unwrap();
        assert!(filesystems.contains("nodev\tproc"));
        assert!(filesystems.contains("nodev\tdevtmpfs"));

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
        close_test_fd(tmpfs_mount_fd);
        close_test_fd(tmpfs_fsfd);
        close_test_fd(proc_mount_fd);
        close_test_fd(proc_fsfd);
        close_test_fd(mount_fd);
        close_test_fd(fsfd);
        for path in [
            page + 2688,
            page + 2560,
            page + 2432,
            page + 256,
            page + 1408,
            page + 1280,
            page + 1152,
            page + 1024,
            page + 896,
            page + 128,
        ] {
            let _ = SyscallArgs::new([path, 0, 0, 0, 0, 0]).call::<Umount2>();
        }
        for path in [
            "/tmp/syscall-mount-test/cgroup",
            "/tmp/syscall-mount-test/pts",
            "/tmp/syscall-mount-test/dev",
            "/tmp/syscall-mount-test/sys",
            "/tmp/syscall-mount-test/proc",
            "/tmp/syscall-mount-test/shm",
            "/tmp/syscall-mount-test/tmp",
            "/tmp/syscall-mount-test/run",
        ] {
            let _ = VirtualFS.lock().delete_file(Path::new(path));
        }
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
