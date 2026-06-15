use super::*;

define_syscall!(Setgroups, |size: usize, list: *const u32| {
    let groups = if size == 0 {
        Vec::new()
    } else {
        if list.is_null() {
            return Err(SyscallError::BadAddress);
        }
        unsafe { core::slice::from_raw_parts(list, size) }.to_vec()
    };

    get_current_process().lock().supplementary_groups = groups;
    Ok(0)
});

define_syscall!(Getresuid, |ruid: *mut u32,
                            euid: *mut u32,
                            suid: *mut u32| {
    let (real_uid, effective_uid, saved_uid) = {
        let process = get_current_process();
        let process = process.lock();
        (process.real_uid, process.effective_uid, process.saved_uid)
    };
    user_safe::write(ruid, &real_uid)?;
    user_safe::write(euid, &effective_uid)?;
    user_safe::write(suid, &saved_uid)?;

    Ok(0)
});

define_syscall!(Getresgid, |rgid: *mut u32,
                            egid: *mut u32,
                            sgid: *mut u32| {
    let (real_gid, effective_gid, saved_gid) = {
        let process = get_current_process();
        let process = process.lock();
        (process.real_gid, process.effective_gid, process.saved_gid)
    };
    user_safe::write(rgid, &real_gid)?;
    user_safe::write(egid, &effective_gid)?;
    user_safe::write(sgid, &saved_gid)?;

    Ok(0)
});

define_syscall!(Setresuid, |ruid: i32, euid: i32, suid: i32| {
    let process = get_current_process();
    let mut process = process.lock();
    if ruid != -1 {
        process.real_uid = ruid as u32;
    }
    if euid != -1 {
        process.effective_uid = euid as u32;
        process.fs_uid = euid as u32;
    }
    if suid != -1 {
        process.saved_uid = suid as u32;
    }
    Ok(0)
});

define_syscall!(Setresgid, |rgid: i32, egid: i32, sgid: i32| {
    let process = get_current_process();
    let mut process = process.lock();
    if rgid != -1 {
        process.real_gid = rgid as u32;
    }
    if egid != -1 {
        process.effective_gid = egid as u32;
        process.fs_gid = egid as u32;
    }
    if sgid != -1 {
        process.saved_gid = sgid as u32;
    }
    Ok(0)
});

define_syscall!(Getuid, {
    Ok(get_current_process().lock().real_uid as usize)
});

define_syscall!(Getgid, {
    Ok(get_current_process().lock().real_gid as usize)
});

define_syscall!(Setuid, |uid: u32| {
    let process = get_current_process();
    let mut process = process.lock();
    process.real_uid = uid;
    process.effective_uid = uid;
    process.saved_uid = uid;
    process.fs_uid = uid;
    Ok(0)
});

define_syscall!(Setreuid, |ruid: i32, euid: i32| {
    let process = get_current_process();
    let mut process = process.lock();
    if ruid != -1 {
        process.real_uid = ruid as u32;
    }
    if euid != -1 {
        process.effective_uid = euid as u32;
        process.saved_uid = euid as u32;
        process.fs_uid = euid as u32;
    }
    Ok(0)
});

define_syscall!(Setgid, |gid: u32| {
    let process = get_current_process();
    let mut process = process.lock();
    process.real_gid = gid;
    process.effective_gid = gid;
    process.saved_gid = gid;
    process.fs_gid = gid;
    Ok(0)
});

define_syscall!(Setregid, |rgid: i32, egid: i32| {
    let process = get_current_process();
    let mut process = process.lock();
    if rgid != -1 {
        process.real_gid = rgid as u32;
    }
    if egid != -1 {
        process.effective_gid = egid as u32;
        process.saved_gid = egid as u32;
        process.fs_gid = egid as u32;
    }
    Ok(0)
});

define_syscall!(Geteuid, {
    Ok(get_current_process().lock().effective_uid as usize)
});

define_syscall!(Getegid, {
    Ok(get_current_process().lock().effective_gid as usize)
});

define_syscall!(Getgroups, |size: i32, list: *mut u32| {
    if size < 0 {
        return Err(SyscallError::InvalidArguments);
    }

    let groups = get_current_process().lock().supplementary_groups.clone();
    if size == 0 {
        return Ok(groups.len());
    }

    let size = size as usize;
    if size < groups.len() {
        return Err(SyscallError::InvalidArguments);
    }

    if groups.is_empty() {
        return Ok(0);
    }

    user_safe::write(list, &groups[..])?;
    Ok(groups.len())
});

define_syscall!(Setfsuid, |uid: u32| {
    let process = get_current_process();
    let mut process = process.lock();
    let old_uid = process.fs_uid;
    process.fs_uid = uid;
    Ok(old_uid as usize)
});

define_syscall!(Setfsgid, |gid: u32| {
    let process = get_current_process();
    let mut process = process.lock();
    let old_gid = process.fs_gid;
    process.fs_gid = gid;
    Ok(old_gid as usize)
});

define_syscall!(Vhangup, { Ok(0) });
