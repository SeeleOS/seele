use super::*;

fn kernel_uid_from_namespace(process: &Process, uid: u32) -> Result<u32, SyscallError> {
    process
        .kernel_uid_from_namespace(uid)
        .ok_or(SyscallError::InvalidArguments)
}

fn kernel_gid_from_namespace(process: &Process, gid: u32) -> Result<u32, SyscallError> {
    process
        .kernel_gid_from_namespace(gid)
        .ok_or(SyscallError::InvalidArguments)
}

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
        (
            process.namespace_uid(process.real_uid),
            process.namespace_uid(process.effective_uid),
            process.namespace_uid(process.saved_uid),
        )
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
        (
            process.namespace_gid(process.real_gid),
            process.namespace_gid(process.effective_gid),
            process.namespace_gid(process.saved_gid),
        )
    };
    user_safe::write(rgid, &real_gid)?;
    user_safe::write(egid, &effective_gid)?;
    user_safe::write(sgid, &saved_gid)?;

    Ok(0)
});

define_syscall!(Setresuid, |ruid: i32, euid: i32, suid: i32| {
    let process = get_current_process();
    let mut process = process.lock();
    let old_effective_uid = process.effective_uid;
    if ruid != -1 {
        process.real_uid = kernel_uid_from_namespace(&process, ruid as u32)?;
    }
    if euid != -1 {
        let euid = kernel_uid_from_namespace(&process, euid as u32)?;
        process.effective_uid = euid;
        process.fs_uid = euid;
    }
    if suid != -1 {
        process.saved_uid = kernel_uid_from_namespace(&process, suid as u32)?;
    }
    process.update_uid_capabilities(old_effective_uid);
    Ok(0)
});

define_syscall!(Setresgid, |rgid: i32, egid: i32, sgid: i32| {
    let process = get_current_process();
    let mut process = process.lock();
    if rgid != -1 {
        process.real_gid = kernel_gid_from_namespace(&process, rgid as u32)?;
    }
    if egid != -1 {
        let egid = kernel_gid_from_namespace(&process, egid as u32)?;
        process.effective_gid = egid;
        process.fs_gid = egid;
    }
    if sgid != -1 {
        process.saved_gid = kernel_gid_from_namespace(&process, sgid as u32)?;
    }
    Ok(0)
});

define_syscall!(Getuid, {
    let process = get_current_process();
    let process = process.lock();
    Ok(process.namespace_uid(process.real_uid) as usize)
});

define_syscall!(Getgid, {
    let process = get_current_process();
    let process = process.lock();
    Ok(process.namespace_gid(process.real_gid) as usize)
});

define_syscall!(Setuid, |uid: u32| {
    let process = get_current_process();
    let mut process = process.lock();
    let old_effective_uid = process.effective_uid;
    let uid = kernel_uid_from_namespace(&process, uid)?;
    process.real_uid = uid;
    process.effective_uid = uid;
    process.saved_uid = uid;
    process.fs_uid = uid;
    process.update_uid_capabilities(old_effective_uid);
    Ok(0)
});

define_syscall!(Setreuid, |ruid: i32, euid: i32| {
    let process = get_current_process();
    let mut process = process.lock();
    let old_effective_uid = process.effective_uid;
    if ruid != -1 {
        process.real_uid = kernel_uid_from_namespace(&process, ruid as u32)?;
    }
    if euid != -1 {
        let euid = kernel_uid_from_namespace(&process, euid as u32)?;
        process.effective_uid = euid;
        process.saved_uid = euid;
        process.fs_uid = euid;
    }
    process.update_uid_capabilities(old_effective_uid);
    Ok(0)
});

define_syscall!(Setgid, |gid: u32| {
    let process = get_current_process();
    let mut process = process.lock();
    let gid = kernel_gid_from_namespace(&process, gid)?;
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
        process.real_gid = kernel_gid_from_namespace(&process, rgid as u32)?;
    }
    if egid != -1 {
        let egid = kernel_gid_from_namespace(&process, egid as u32)?;
        process.effective_gid = egid;
        process.saved_gid = egid;
        process.fs_gid = egid;
    }
    Ok(0)
});

define_syscall!(Geteuid, {
    let process = get_current_process();
    let process = process.lock();
    Ok(process.namespace_uid(process.effective_uid) as usize)
});

define_syscall!(Getegid, {
    let process = get_current_process();
    let process = process.lock();
    Ok(process.namespace_gid(process.effective_gid) as usize)
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
    let old_uid = process.namespace_uid(process.fs_uid);
    if let Some(uid) = process.kernel_uid_from_namespace(uid) {
        process.fs_uid = uid;
    }
    Ok(old_uid as usize)
});

define_syscall!(Setfsgid, |gid: u32| {
    let process = get_current_process();
    let mut process = process.lock();
    let old_gid = process.namespace_gid(process.fs_gid);
    if let Some(gid) = process.kernel_gid_from_namespace(gid) {
        process.fs_gid = gid;
    }
    Ok(old_gid as usize)
});

define_syscall!(Vhangup, { Ok(0) });

#[cfg(test)]
mod tests {
    use crate::process::{DEFAULT_CAPABILITY_SET, manager::get_current_process};
    use crate::systemcall::{
        implementations::{
            Getegid, Geteuid, Getgid, Getgroups, Getuid, Setfsgid, Setfsuid, Setgid, Setgroups,
            Setregid, Setresgid, Setresuid, Setreuid, Setuid,
        },
        test::CredentialSnapshot,
        test_helpers::{SyscallArgs, expect_errno, expect_ok},
        utils::SyscallError,
    };
    use alloc::string::String;

    crate::test!(
        credential_getter_syscalls,
        "credential getters return current linux ids",
        credential_getters_return_current_linux_ids
    );
    crate::test!(
        credential_setter_syscalls,
        "credential setters update linux real effective saved and fs ids",
        credential_setters_update_linux_real_effective_saved_and_fs_ids
    );
    crate::test!(
        fsuid_fsgid_syscalls,
        "fsuid and fsgid syscalls return previous ids and update state",
        fsuid_fsgid_syscalls_return_previous_ids_and_update_state
    );
    crate::test!(
        fsuid_fsgid_syscalls_unmapped,
        "fsuid and fsgid syscalls ignore unmapped ids and return previous values",
        fsuid_fsgid_syscalls_ignore_unmapped_ids_and_return_previous_values
    );
    crate::test!(
        group_syscalls,
        "group syscalls validate linux size rules",
        group_syscalls_validate_linux_size_rules
    );
    crate::test!(
        setuid_root_exec_capabilities,
        "setuid root exec restores linux permitted capabilities",
        setuid_root_exec_restores_linux_permitted_capabilities
    );

    fn credential_getters_return_current_linux_ids() {
        let process = get_current_process();
        let mut process = process.lock();
        let saved = CredentialSnapshot::save(&process);
        process.real_uid = 1001;
        process.effective_uid = 1002;
        process.real_gid = 1003;
        process.effective_gid = 1004;
        process.user_namespace_uid_map = None;
        process.user_namespace_gid_map = None;
        drop(process);

        expect_ok(SyscallArgs::none().call::<Getuid>(), 1001);
        expect_ok(SyscallArgs::none().call::<Geteuid>(), 1002);
        expect_ok(SyscallArgs::none().call::<Getgid>(), 1003);
        expect_ok(SyscallArgs::none().call::<Getegid>(), 1004);

        saved.restore();
    }

    fn credential_setters_update_linux_real_effective_saved_and_fs_ids() {
        let saved = CredentialSnapshot::save_current();
        {
            let process = get_current_process();
            let mut process = process.lock();
            process.user_namespace_uid_map = None;
            process.user_namespace_gid_map = None;
        }

        expect_ok(SyscallArgs::new([42, 0, 0, 0, 0, 0]).call::<Setuid>(), 0);
        {
            let process = get_current_process();
            let process = process.lock();
            assert_eq!(process.real_uid, 42);
            assert_eq!(process.effective_uid, 42);
            assert_eq!(process.saved_uid, 42);
            assert_eq!(process.fs_uid, 42);
        }

        expect_ok(SyscallArgs::new([43, 0, 0, 0, 0, 0]).call::<Setgid>(), 0);
        {
            let process = get_current_process();
            let process = process.lock();
            assert_eq!(process.real_gid, 43);
            assert_eq!(process.effective_gid, 43);
            assert_eq!(process.saved_gid, 43);
            assert_eq!(process.fs_gid, 43);
        }

        expect_ok(
            SyscallArgs::new([u64::MAX, 44, 0, 0, 0, 0]).call::<Setreuid>(),
            0,
        );
        {
            let process = get_current_process();
            let process = process.lock();
            assert_eq!(process.real_uid, 42);
            assert_eq!(process.effective_uid, 44);
            assert_eq!(process.saved_uid, 44);
            assert_eq!(process.fs_uid, 44);
        }

        expect_ok(
            SyscallArgs::new([u64::MAX, 45, 0, 0, 0, 0]).call::<Setregid>(),
            0,
        );
        {
            let process = get_current_process();
            let process = process.lock();
            assert_eq!(process.real_gid, 43);
            assert_eq!(process.effective_gid, 45);
            assert_eq!(process.saved_gid, 45);
            assert_eq!(process.fs_gid, 45);
        }

        expect_ok(
            SyscallArgs::new([50, 51, 52, 0, 0, 0]).call::<Setresuid>(),
            0,
        );
        {
            let process = get_current_process();
            let process = process.lock();
            assert_eq!(process.real_uid, 50);
            assert_eq!(process.effective_uid, 51);
            assert_eq!(process.saved_uid, 52);
            assert_eq!(process.fs_uid, 51);
        }

        expect_ok(
            SyscallArgs::new([60, 61, 62, 0, 0, 0]).call::<Setresgid>(),
            0,
        );
        {
            let process = get_current_process();
            let process = process.lock();
            assert_eq!(process.real_gid, 60);
            assert_eq!(process.effective_gid, 61);
            assert_eq!(process.saved_gid, 62);
            assert_eq!(process.fs_gid, 61);
        }

        {
            let process = get_current_process();
            let mut process = process.lock();
            process.user_namespace_uid_map = Some(String::from("0 100000 1000\n"));
            process.user_namespace_gid_map = Some(String::from("0 200000 1000\n"));
        }
        expect_ok(SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Setuid>(), 0);
        expect_ok(SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Setgid>(), 0);
        {
            let process = get_current_process();
            let process = process.lock();
            assert_eq!(process.real_uid, 100000);
            assert_eq!(process.effective_uid, 100000);
            assert_eq!(process.saved_uid, 100000);
            assert_eq!(process.fs_uid, 100000);
            assert_eq!(process.real_gid, 200000);
            assert_eq!(process.effective_gid, 200000);
            assert_eq!(process.saved_gid, 200000);
            assert_eq!(process.fs_gid, 200000);
        }

        saved.restore();
    }

    fn fsuid_fsgid_syscalls_return_previous_ids_and_update_state() {
        let saved = CredentialSnapshot::save_current();

        {
            let process = get_current_process();
            let mut process = process.lock();
            process.fs_uid = 700;
            process.fs_gid = 800;
        }

        expect_ok(
            SyscallArgs::new([701, 0, 0, 0, 0, 0]).call::<Setfsuid>(),
            700,
        );
        expect_ok(
            SyscallArgs::new([801, 0, 0, 0, 0, 0]).call::<Setfsgid>(),
            800,
        );
        {
            let process = get_current_process();
            let process = process.lock();
            assert_eq!(process.fs_uid, 701);
            assert_eq!(process.fs_gid, 801);
        }

        saved.restore();
    }

    fn fsuid_fsgid_syscalls_ignore_unmapped_ids_and_return_previous_values() {
        let saved = CredentialSnapshot::save_current();

        {
            let process = get_current_process();
            let mut process = process.lock();
            process.fs_uid = 700;
            process.fs_gid = 800;
            process.user_namespace_uid_map = Some(String::from("0 100000 1\n"));
            process.user_namespace_gid_map = Some(String::from("0 200000 1\n"));
        }

        expect_ok(
            SyscallArgs::new([701, 0, 0, 0, 0, 0]).call::<Setfsuid>(),
            700,
        );
        expect_ok(
            SyscallArgs::new([801, 0, 0, 0, 0, 0]).call::<Setfsgid>(),
            800,
        );
        {
            let process = get_current_process();
            let process = process.lock();
            assert_eq!(process.fs_uid, 700);
            assert_eq!(process.fs_gid, 800);
        }

        saved.restore();
    }

    fn group_syscalls_validate_linux_size_rules() {
        let process = get_current_process();
        let saved_groups = process.lock().supplementary_groups.clone();
        let groups = [10u32, 20u32, 30u32];

        expect_ok(
            SyscallArgs::new([groups.len() as u64, groups.as_ptr() as u64, 0, 0, 0, 0])
                .call::<Setgroups>(),
            0,
        );
        expect_ok(SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Getgroups>(), 3);
        expect_errno(
            SyscallArgs::new([2, 0, 0, 0, 0, 0]).call::<Getgroups>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([u64::MAX, 0, 0, 0, 0, 0]).call::<Getgroups>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([1, 0, 0, 0, 0, 0]).call::<Setgroups>(),
            SyscallError::BadAddress,
        );
        expect_ok(SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Setgroups>(), 0);
        assert!(process.lock().supplementary_groups.is_empty());

        process.lock().supplementary_groups = saved_groups;
    }

    fn setuid_root_exec_restores_linux_permitted_capabilities() {
        let saved = CredentialSnapshot::save_current();

        {
            let process = get_current_process();
            let mut process = process.lock();
            process.real_uid = 1000;
            process.effective_uid = 0;
            process.saved_uid = 0;
            process.fs_uid = 0;
            process.capability_effective = [0; 2];
            process.capability_permitted = [0; 2];
            process.capability_bounding = DEFAULT_CAPABILITY_SET;
            process.no_new_privs = false;

            process.update_exec_uid_capabilities(1000, true);

            assert_eq!(process.capability_permitted, DEFAULT_CAPABILITY_SET);
            assert_eq!(process.capability_effective, DEFAULT_CAPABILITY_SET);

            process.real_uid = 0;
            process.effective_uid = 0;
            process.saved_uid = 0;
            process.user_namespace_uid_map = Some(String::from("200 0 1\n"));
            process.capability_effective = DEFAULT_CAPABILITY_SET;
            process.capability_permitted = DEFAULT_CAPABILITY_SET;

            process.update_exec_uid_capabilities(0, false);

            assert_eq!(process.capability_permitted, [0; 2]);
            assert_eq!(process.capability_effective, [0; 2]);
        }

        saved.restore();
    }
}
