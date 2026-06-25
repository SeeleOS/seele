use super::*;

define_syscall!(Prctl, |option: i32,
                        arg2: u64,
                        arg3: u64,
                        _arg4: u64,
                        _arg5: u64| {
    match PrctlOption::try_from(option).map_err(|_| SyscallError::InvalidArguments)? {
        PrctlOption::SetSeccomp => Err(SyscallError::InvalidArguments),
        PrctlOption::SetMdwe => Err(SyscallError::InvalidArguments),
        PrctlOption::SetPdeathsig => {
            let signal = if arg2 == 0 {
                None
            } else {
                Some(Signal::try_from(arg2).map_err(|_| SyscallError::InvalidArguments)?)
            };
            get_current_process().lock().parent_death_signal = signal;
            Ok(0)
        }
        PrctlOption::SetDumpable => {
            if arg2 > 1 {
                return Err(SyscallError::InvalidArguments);
            }
            get_current_process().lock().dumpable = arg2 != 0;
            Ok(0)
        }
        PrctlOption::SetName => {
            let name = read_prctl_name(arg2 as *const u8)?;
            crate::thread::get_current_thread().lock().name = name;
            Ok(0)
        }
        PrctlOption::SetChildSubreaper => {
            get_current_process().lock().child_subreaper = arg2 != 0;
            Ok(0)
        }
        PrctlOption::SetNoNewPrivs => {
            if arg2 != 1 || arg3 != 0 {
                return Err(SyscallError::InvalidArguments);
            }
            get_current_process().lock().no_new_privs = true;
            Ok(0)
        }
        PrctlOption::SetKeepCaps => {
            if arg2 > 1 {
                return Err(SyscallError::InvalidArguments);
            }
            get_current_process().lock().keep_capabilities = arg2 != 0;
            Ok(0)
        }
        PrctlOption::SetSecureBits => {
            get_current_process().lock().secure_bits = arg2 as u32;
            Ok(0)
        }
        PrctlOption::GetPdeathsig => {
            if arg2 == 0 {
                return Err(SyscallError::BadAddress);
            }
            let signal = get_current_process()
                .lock()
                .parent_death_signal
                .map(|signal| signal as i32)
                .unwrap_or(0);
            user_safe::write(arg2 as *mut i32, &signal)?;
            Ok(0)
        }
        PrctlOption::GetDumpable => Ok(get_current_process().lock().dumpable as usize),
        PrctlOption::GetChildSubreaper => {
            if arg2 == 0 {
                return Err(SyscallError::BadAddress);
            }
            let child_subreaper = get_current_process().lock().child_subreaper as i32;
            user_safe::write(arg2 as *mut i32, &child_subreaper)?;
            Ok(0)
        }
        PrctlOption::GetNoNewPrivs => Ok(get_current_process().lock().no_new_privs as usize),
        PrctlOption::GetSeccomp => Err(SyscallError::InvalidArguments),
        PrctlOption::GetMdwe => Ok(0),
        PrctlOption::GetKeepCaps => Ok(get_current_process().lock().keep_capabilities as usize),
        PrctlOption::GetSecureBits => Ok(get_current_process().lock().secure_bits as usize),
        PrctlOption::GetName => {
            if arg2 == 0 {
                return Err(SyscallError::BadAddress);
            }
            let name = current_thread_name();
            user_safe::write(arg2 as *mut u8, &name)?;
            Ok(0)
        }
        PrctlOption::CapbsetRead => {
            let (slot, mask) = capability_slot_and_mask(arg2)?;
            let process = get_current_process();
            Ok(((process.lock().capability_bounding[slot] & mask) != 0) as usize)
        }
        PrctlOption::CapbsetDrop => {
            let (slot, mask) = capability_slot_and_mask(arg2)?;
            let process = get_current_process();
            process.lock().capability_bounding[slot] &= !mask;
            Ok(0)
        }
        PrctlOption::CapAmbient => {
            let op =
                PrctlCapAmbientOp::try_from(arg2).map_err(|_| SyscallError::InvalidArguments)?;
            match op {
                PrctlCapAmbientOp::ClearAll => {
                    get_current_process().lock().capability_ambient = [0; LINUX_CAPABILITY_U32S_3];
                    Ok(0)
                }
                PrctlCapAmbientOp::IsSet => {
                    let (slot, mask) = capability_slot_and_mask(arg3)?;
                    let process = get_current_process();
                    Ok(((process.lock().capability_ambient[slot] & mask) != 0) as usize)
                }
                PrctlCapAmbientOp::Raise => {
                    let (slot, mask) = capability_slot_and_mask(arg3)?;
                    let process = get_current_process();
                    let mut process = process.lock();
                    process.capability_ambient[slot] |= mask;
                    Ok(0)
                }
                PrctlCapAmbientOp::Lower => {
                    let (slot, mask) = capability_slot_and_mask(arg3)?;
                    let process = get_current_process();
                    let mut process = process.lock();
                    process.capability_ambient[slot] &= !mask;
                    Ok(0)
                }
            }
        }
    }
});

fn read_prctl_name(ptr: *const u8) -> Result<[u8; 16], SyscallError> {
    if ptr.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let mut name = [0u8; 16];
    for (index, slot) in name.iter_mut().enumerate() {
        let byte = user_safe::read(unsafe { ptr.add(index) })?;
        *slot = byte;
        if byte == 0 {
            return Ok(name);
        }
    }
    name[15] = 0;
    Ok(name)
}

fn current_thread_name() -> [u8; 16] {
    let current = crate::thread::get_current_thread();
    let thread_name = current.lock().name;
    if thread_name.iter().any(|&byte| byte != 0) {
        return thread_name;
    }

    let process = get_current_process();
    let process = process.lock();
    let command = process
        .command_line
        .first()
        .map(String::as_str)
        .unwrap_or("main");
    let basename = command.rsplit('/').next().unwrap_or(command);
    let mut name = [0u8; 16];
    let bytes = basename.as_bytes();
    let copy_len = bytes.len().min(15);
    name[..copy_len].copy_from_slice(&bytes[..copy_len]);
    name
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systemcall::test::*;

    crate::test!(
        process_session_and_prctl_syscalls,
        "process session and prctl syscalls follow linux state rules",
        process_session_and_prctl_syscalls_follow_linux_state_rules
    );
    fn process_session_and_prctl_syscalls_follow_linux_state_rules() {
        let saved = CredentialSnapshot::save_current();
        let process = get_current_process();
        let (
            pid,
            old_group,
            old_session,
            old_terminal,
            old_parent_death_signal,
            old_dumpable,
            old_no_new_privs,
            old_keep_caps,
            old_secure_bits,
            old_bounding,
            old_ambient,
            old_child_subreaper,
            old_umask,
        ) = {
            let process = process.lock();
            let old_umask = process.fs_context.lock().file_mode_creation_mask;
            (
                process.pid.0,
                process.group_id,
                process.session_id,
                process.controlling_terminal,
                process.parent_death_signal,
                process.dumpable,
                process.no_new_privs,
                process.keep_capabilities,
                process.secure_bits,
                process.capability_bounding,
                process.capability_ambient,
                process.child_subreaper,
                old_umask,
            )
        };

        {
            let mut process = process.lock();
            process.real_uid = 100;
            process.effective_uid = 101;
            process.saved_uid = 102;
            process.real_gid = 200;
            process.effective_gid = 201;
            process.saved_gid = 202;
        }

        expect_ok(
            SyscallArgs::none().call::<Gettid>(),
            crate::thread::get_current_thread().lock().id.0 as usize,
        );
        expect_ok(
            SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Getsid>(),
            old_session.0 as usize,
        );
        expect_errno(
            SyscallArgs::new([u64::from(u32::MAX), 0, 0, 0, 0, 0]).call::<Getsid>(),
            SyscallError::NoProcess,
        );

        expect_ok(
            SyscallArgs::new([0o777, 0, 0, 0, 0, 0]).call::<Umask>(),
            old_umask as usize,
        );
        assert_eq!(
            process.lock().fs_context.lock().file_mode_creation_mask,
            0o777
        );
        expect_ok(
            SyscallArgs::new([0o1000, 0, 0, 0, 0, 0]).call::<Umask>(),
            0o777,
        );
        assert_eq!(process.lock().fs_context.lock().file_mode_creation_mask, 0);

        let uid_page = allocate_user_test_page();
        expect_ok(
            SyscallArgs::new([uid_page, uid_page + 4, uid_page + 8, 0, 0, 0]).call::<Getresuid>(),
            0,
        );
        assert_eq!(read_user_value::<u32>(uid_page), 100);
        assert_eq!(read_user_value::<u32>(uid_page + 4), 101);
        assert_eq!(read_user_value::<u32>(uid_page + 8), 102);
        expect_errno(
            SyscallArgs::new([0, uid_page + 4, uid_page + 8, 0, 0, 0]).call::<Getresuid>(),
            SyscallError::BadAddress,
        );

        let gid_page = allocate_user_test_page();
        expect_ok(
            SyscallArgs::new([gid_page, gid_page + 4, gid_page + 8, 0, 0, 0]).call::<Getresgid>(),
            0,
        );
        assert_eq!(read_user_value::<u32>(gid_page), 200);
        assert_eq!(read_user_value::<u32>(gid_page + 4), 201);
        assert_eq!(read_user_value::<u32>(gid_page + 8), 202);
        expect_errno(
            SyscallArgs::new([gid_page, 0, gid_page + 8, 0, 0, 0]).call::<Getresgid>(),
            SyscallError::BadAddress,
        );

        {
            let mut process = process.lock();
            process.group_id = ProcessGroupID(pid);
        }
        expect_errno(
            SyscallArgs::none().call::<Setsid>(),
            SyscallError::PermissionDenied,
        );
        {
            let mut process = process.lock();
            process.group_id = ProcessGroupID(pid + 7);
            process.session_id = SessionID(pid + 11);
            process.controlling_terminal = Some(ControllingTerminal(123));
        }
        expect_ok(SyscallArgs::none().call::<Setsid>(), pid as usize);
        {
            let process = process.lock();
            assert_eq!(process.group_id, ProcessGroupID(pid));
            assert_eq!(process.session_id, SessionID(pid));
            assert_eq!(process.controlling_terminal, None);
        }

        const PR_SET_PDEATHSIG: u64 = 1;
        const PR_GET_PDEATHSIG: u64 = 2;
        const PR_GET_DUMPABLE: u64 = 3;
        const PR_SET_DUMPABLE: u64 = 4;
        const PR_GET_KEEPCAPS: u64 = 7;
        const PR_SET_KEEPCAPS: u64 = 8;
        const PR_SET_NAME: u64 = 15;
        const PR_GET_NAME: u64 = 16;
        const PR_CAPBSET_READ: u64 = 23;
        const PR_CAPBSET_DROP: u64 = 24;
        const PR_GET_SECUREBITS: u64 = 27;
        const PR_SET_SECUREBITS: u64 = 28;
        const PR_SET_CHILD_SUBREAPER: u64 = 36;
        const PR_GET_CHILD_SUBREAPER: u64 = 37;
        const PR_SET_NO_NEW_PRIVS: u64 = 38;
        const PR_GET_NO_NEW_PRIVS: u64 = 39;
        const PR_CAP_AMBIENT: u64 = 47;
        const PR_CAP_AMBIENT_IS_SET: u64 = 1;
        const PR_CAP_AMBIENT_RAISE: u64 = 2;
        const PR_CAP_AMBIENT_LOWER: u64 = 3;
        const PR_CAP_AMBIENT_CLEAR_ALL: u64 = 4;

        expect_ok(
            SyscallArgs::new([PR_SET_PDEATHSIG, Signal::SIGTERM as u64, 0, 0, 0, 0])
                .call::<Prctl>(),
            0,
        );
        let pdeath_page = allocate_user_test_page();
        expect_ok(
            SyscallArgs::new([PR_GET_PDEATHSIG, pdeath_page, 0, 0, 0, 0]).call::<Prctl>(),
            0,
        );
        assert_eq!(read_user_value::<i32>(pdeath_page), Signal::SIGTERM as i32);
        expect_errno(
            SyscallArgs::new([PR_SET_PDEATHSIG, 999, 0, 0, 0, 0]).call::<Prctl>(),
            SyscallError::InvalidArguments,
        );

        expect_ok(
            SyscallArgs::new([PR_SET_DUMPABLE, 0, 0, 0, 0, 0]).call::<Prctl>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([PR_GET_DUMPABLE, 0, 0, 0, 0, 0]).call::<Prctl>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([PR_SET_DUMPABLE, 2, 0, 0, 0, 0]).call::<Prctl>(),
            SyscallError::InvalidArguments,
        );
        expect_ok(
            SyscallArgs::new([PR_SET_KEEPCAPS, 1, 0, 0, 0, 0]).call::<Prctl>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([PR_GET_KEEPCAPS, 0, 0, 0, 0, 0]).call::<Prctl>(),
            1,
        );
        expect_ok(
            SyscallArgs::new([PR_SET_SECUREBITS, 0x24, 0, 0, 0, 0]).call::<Prctl>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([PR_GET_SECUREBITS, 0, 0, 0, 0, 0]).call::<Prctl>(),
            0x24,
        );
        expect_ok(
            SyscallArgs::new([PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0, 0]).call::<Prctl>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([PR_GET_CHILD_SUBREAPER, pdeath_page, 9, 8, 7, 0]).call::<Prctl>(),
            0,
        );
        assert_eq!(read_user_value::<i32>(pdeath_page), 1);
        expect_ok(
            SyscallArgs::new([PR_SET_CHILD_SUBREAPER, 0, 0, 0, 0, 0]).call::<Prctl>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([PR_GET_CHILD_SUBREAPER, pdeath_page, 0, 0, 0, 0]).call::<Prctl>(),
            0,
        );
        assert_eq!(read_user_value::<i32>(pdeath_page), 0);
        expect_errno(
            SyscallArgs::new([PR_GET_CHILD_SUBREAPER, 0, 0, 0, 0, 0]).call::<Prctl>(),
            SyscallError::BadAddress,
        );
        expect_ok(
            SyscallArgs::new([PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0, 0]).call::<Prctl>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([PR_GET_NO_NEW_PRIVS, 0, 0, 0, 0, 0]).call::<Prctl>(),
            1,
        );
        expect_errno(
            SyscallArgs::new([PR_SET_NO_NEW_PRIVS, 1, 1, 0, 0, 0]).call::<Prctl>(),
            SyscallError::InvalidArguments,
        );

        let name_page = allocate_user_test_page();
        write_user_value(name_page, b"linux-name\0");
        expect_ok(
            SyscallArgs::new([PR_SET_NAME, name_page, 0, 0, 0, 0]).call::<Prctl>(),
            0,
        );
        let out_name_page = allocate_user_test_page();
        expect_ok(
            SyscallArgs::new([PR_GET_NAME, out_name_page, 0, 0, 0, 0]).call::<Prctl>(),
            0,
        );
        assert_user_bytes(out_name_page, b"linux-name\0\0\0\0\0\0");

        expect_ok(
            SyscallArgs::new([PR_CAPBSET_READ, 1, 0, 0, 0, 0]).call::<Prctl>(),
            1,
        );
        expect_ok(
            SyscallArgs::new([PR_CAPBSET_DROP, 1, 0, 0, 0, 0]).call::<Prctl>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([PR_CAPBSET_READ, 1, 0, 0, 0, 0]).call::<Prctl>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([PR_CAPBSET_READ, 64, 0, 0, 0, 0]).call::<Prctl>(),
            SyscallError::InvalidArguments,
        );

        let current = get_current_process();
        let original_parent = current.lock().parent.clone();
        let subreaper_parent = Process::empty();
        {
            let mut subreaper_parent = subreaper_parent.lock();
            subreaper_parent.pid = ProcessID::new();
            subreaper_parent.child_subreaper = true;
            subreaper_parent.parent = original_parent.clone();
        }
        let exiting_parent = Process::empty();
        {
            let mut exiting_parent = exiting_parent.lock();
            exiting_parent.pid = ProcessID::new();
            exiting_parent.parent = Some(subreaper_parent.clone());
        }
        current.lock().parent = Some(exiting_parent.clone());
        let (forked_process, _) = Process::fork(current.clone());
        assert!(!forked_process.lock().child_subreaper);
        terminate_process(exiting_parent.clone(), ProcessExitStatus::Exited(0));
        assert!(
            current
                .lock()
                .parent
                .as_ref()
                .is_some_and(|parent| alloc::sync::Arc::ptr_eq(parent, &subreaper_parent))
        );
        current.lock().parent = original_parent;

        expect_ok(
            SyscallArgs::new([PR_CAP_AMBIENT, PR_CAP_AMBIENT_RAISE, 2, 0, 0, 0]).call::<Prctl>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([PR_CAP_AMBIENT, PR_CAP_AMBIENT_IS_SET, 2, 0, 0, 0]).call::<Prctl>(),
            1,
        );
        expect_ok(
            SyscallArgs::new([PR_CAP_AMBIENT, PR_CAP_AMBIENT_LOWER, 2, 0, 0, 0]).call::<Prctl>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([PR_CAP_AMBIENT, PR_CAP_AMBIENT_IS_SET, 2, 0, 0, 0]).call::<Prctl>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([PR_CAP_AMBIENT, PR_CAP_AMBIENT_CLEAR_ALL, 0, 0, 0, 0])
                .call::<Prctl>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([999, 0, 0, 0, 0, 0]).call::<Prctl>(),
            SyscallError::InvalidArguments,
        );

        const ARCH_SET_FS: u64 = 0x1002;
        const ARCH_GET_FS: u64 = 0x1003;
        const ARCH_GET_CPUID: u64 = 0x1011;
        const ARCH_SET_CPUID: u64 = 0x1012;
        let fs_page = allocate_user_test_page();
        expect_ok(
            SyscallArgs::new([ARCH_GET_FS, fs_page, 0, 0, 0, 0]).call::<ArchPrctl>(),
            0,
        );
        let old_fs_base = read_user_value::<u64>(fs_page);
        expect_ok(
            SyscallArgs::new([ARCH_SET_FS, 0x1234_5000, 0, 0, 0, 0]).call::<ArchPrctl>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([ARCH_GET_FS, fs_page, 0, 0, 0, 0]).call::<ArchPrctl>(),
            0,
        );
        assert_eq!(read_user_value::<u64>(fs_page), 0x1234_5000);
        expect_ok(
            SyscallArgs::new([ARCH_SET_FS, old_fs_base, 0, 0, 0, 0]).call::<ArchPrctl>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([ARCH_GET_FS, 0, 0, 0, 0, 0]).call::<ArchPrctl>(),
            SyscallError::BadAddress,
        );
        expect_errno(
            SyscallArgs::new([ARCH_SET_CPUID, 0, 0, 0, 0, 0]).call::<ArchPrctl>(),
            SyscallError::NoDevice,
        );
        expect_ok(
            SyscallArgs::new([ARCH_GET_CPUID, 0, 0, 0, 0, 0]).call::<ArchPrctl>(),
            1,
        );
        expect_errno(
            SyscallArgs::new([0x9999, 0, 0, 0, 0, 0]).call::<ArchPrctl>(),
            SyscallError::InvalidArguments,
        );

        {
            let mut process = process.lock();
            process.group_id = old_group;
            process.session_id = old_session;
            process.controlling_terminal = old_terminal;
            process.parent_death_signal = old_parent_death_signal;
            process.dumpable = old_dumpable;
            process.no_new_privs = old_no_new_privs;
            process.keep_capabilities = old_keep_caps;
            process.secure_bits = old_secure_bits;
            process.capability_bounding = old_bounding;
            process.capability_ambient = old_ambient;
            process.child_subreaper = old_child_subreaper;
            process.fs_context.lock().file_mode_creation_mask = old_umask;
        }
        saved.restore();
    }
}
