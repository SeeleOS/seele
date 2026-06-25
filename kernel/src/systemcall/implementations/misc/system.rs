use super::*;

define_syscall!(Getrusage, |who: i32, usage: *mut LinuxRusage| {
    let _ = LinuxRusageWho::try_from(who).map_err(|_| SyscallError::InvalidArguments)?;
    if usage.is_null() {
        return Err(SyscallError::BadAddress);
    }

    user_safe::write(usage, &LinuxRusage::default())?;
    Ok(0)
});

define_syscall!(Umask, |mask: u32| {
    let process = get_current_process();
    let process = process.lock();
    let mut fs_context = process.fs_context.lock();
    let old_mask = fs_context.file_mode_creation_mask;
    fs_context.file_mode_creation_mask = mask & 0o777;
    Ok(old_mask as usize)
});

define_syscall!(Brk, |addr: u64| {
    let process = get_current_process();
    let mut process = process.lock();

    if process.program_break == 0 {
        process.program_break = process
            .addrspace
            .user_mem
            .as_u64()
            .saturating_sub(INITIAL_BRK_RESERVE);
        process.program_break_base = process.program_break;
    }

    let current = process.program_break;
    if addr == 0 {
        return Ok(current as usize);
    }

    let old_aligned = current.div_ceil(4096) * 4096;
    let Some(new_aligned) = addr.checked_add(4095).map(|addr| addr / 4096 * 4096) else {
        return Ok(current as usize);
    };
    if new_aligned >= crate::memory::addrspace::USER_MEM_END {
        return Ok(current as usize);
    }
    let brk_base = process.program_break_base;
    if new_aligned.saturating_sub(brk_base) > process.rlimit_data_cur {
        return Ok(current as usize);
    }

    if new_aligned > old_aligned {
        process.addrspace.register_area(MemoryArea::new(
            VirtAddr::new(old_aligned),
            (new_aligned - old_aligned) / 4096,
            protection_to_page_flags(Protection::READ | Protection::WRITE),
            Protection::READ | Protection::WRITE,
            Data::Normal(Default::default()),
            true,
        ));
    } else if new_aligned < old_aligned {
        process
            .addrspace
            .unmap(VirtAddr::new(new_aligned), old_aligned - new_aligned);
    }

    if process.addrspace.user_mem.as_u64() < new_aligned {
        process.addrspace.user_mem = VirtAddr::new(new_aligned);
    }

    process.program_break = addr;
    Ok(addr as usize)
});

define_syscall!(Uname, |info: *mut UtsName| {
    if info.is_null() {
        return Err(SyscallError::BadAddress);
    }
    let mut uts = UtsName::new(
        "Linux",
        crate::KERNEL_RELEASE,
        crate::KERNEL_VERSION,
        crate::misc::utsname::DEFAULT_MACHINE,
    );
    uts.nodename = crate::misc::utsname::current_hostname(NAME);
    uts.domainname = crate::misc::utsname::current_domainname("(none)");
    user_safe::write(info, &uts)?;
    Ok(0)
});

define_syscall!(Sethostname, |name: *const u8, len: usize| {
    if len > 64 {
        return Err(SyscallError::InvalidArguments);
    }

    let mut hostname = Vec::with_capacity(len);
    for offset in 0..len {
        hostname.push(user_safe::read(unsafe { name.add(offset) })?);
    }

    crate::misc::utsname::set_hostname(&hostname).map_err(|_| SyscallError::InvalidArguments)?;
    Ok(0)
});

define_syscall!(Reboot, |magic1: u32,
                         magic2: u32,
                         cmd: u32,
                         _arg: *const u8| {
    if magic1 != LINUX_REBOOT_MAGIC1 || magic2 != LINUX_REBOOT_MAGIC2 {
        return Err(SyscallError::InvalidArguments);
    }

    match cmd {
        LINUX_REBOOT_CMD_CAD_OFF => {
            reboot_state::set_ctrl_alt_del_enabled(false);
            Ok(0)
        }
        LINUX_REBOOT_CMD_CAD_ON => {
            reboot_state::set_ctrl_alt_del_enabled(true);
            Ok(0)
        }
        _ => Err(SyscallError::InvalidArguments),
    }
});

define_syscall!(Sync, {
    crate::filesystem::vfs::VirtualFS.lock().sync_all()?;
    Ok(0)
});
define_syscall!(
    Getrandom,
    |buf: *mut u8, len: usize, flags: GetRandomFlags| {
        if flags.bits()
            != flags.bits()
                & (GetRandomFlags::NONBLOCK | GetRandomFlags::RANDOM | GetRandomFlags::INSECURE)
                    .bits()
        {
            return Err(SyscallError::InvalidArguments);
        }
        if len == 0 {
            return Ok(0);
        }
        if buf.is_null() {
            return Err(SyscallError::BadAddress);
        }

        let mut state = KernelTime::since_boot().as_nanoseconds()
            ^ KernelTime::current().as_nanoseconds()
            ^ (buf as u64).rotate_left(17)
            ^ (len as u64).rotate_left(33);
        let mut out = vec![0; len];

        for byte in &mut out {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            *byte = state as u8;
        }

        user_safe::write(buf, &out[..])?;

        Ok(len)
    }
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systemcall::test::*;

    crate::test!(
        misc_state_syscalls,
        "misc state syscalls follow linux pointer and state rules",
        misc_state_syscalls_follow_linux_pointer_and_state_rules
    );

    fn misc_state_syscalls_follow_linux_pointer_and_state_rules() {
        assert_linux_layout::<TestLinuxCapHeader>(8, 4);
        assert_linux_layout::<TestLinuxCapData>(12, 4);
        assert_linux_layout::<TestLinuxTimeval>(16, 8);
        assert_linux_layout::<TestLinuxTimezone>(8, 4);
        assert_linux_layout::<TestLinuxRusage>(144, 8);
        assert_linux_layout::<TestLinuxSchedParam>(4, 4);
        assert_linux_layout::<TestLinuxSysinfo>(112, 8);
        assert_linux_layout::<TestLinuxRseq>(32, 8);

        const LINUX_CAPABILITY_VERSION_3: u32 = 0x2008_0522;
        const RSEQ_LEN_X86_64: u64 = 32;
        const RSEQ_FLAG_UNREGISTER: u64 = 1;
        const RSEQ_CPU_ID_UNINITIALIZED: u32 = u32::MAX;
        const RSEQ_CPU_ID_SINGLE_CORE: u32 = 0;

        let saved = CredentialSnapshot::save_current();
        let process = get_current_process();
        let current = crate::thread::get_current_thread();
        let (
            old_clear_child_tid,
            old_robust_list_head,
            old_robust_list_len,
            old_rseq_area,
            old_rseq_len,
            old_rseq_flags,
            old_rseq_sig,
        ) = {
            let current = current.lock();
            (
                current.clear_child_tid,
                current.robust_list_head,
                current.robust_list_len,
                current.rseq_area,
                current.rseq_len,
                current.rseq_flags,
                current.rseq_sig,
            )
        };
        let old_timezone = crate::misc::time::timezone();
        let old_timers = core::mem::take(&mut process.lock().timers);

        {
            let mut process = process.lock();
            process.capability_effective = [0x1111_1111, 0x22];
            process.capability_permitted = [0x3333_3333, 0x44];
            process.capability_inheritable = [0x5555_5555, 0x66];
        }

        let cap_page = allocate_user_test_page();
        write_user_value(cap_page, &TestLinuxCapHeader { version: 0, pid: 0 });
        expect_ok(
            SyscallArgs::new([cap_page, cap_page + 16, 0, 0, 0, 0]).call::<Capget>(),
            0,
        );
        let header = read_user_value::<TestLinuxCapHeader>(cap_page);
        assert_eq!(header.version, LINUX_CAPABILITY_VERSION_3);
        assert_eq!(header.pid, 0);
        let cap0 = read_user_value::<TestLinuxCapData>(cap_page + 16);
        let cap1 = read_user_value::<TestLinuxCapData>(cap_page + 28);
        assert_eq!(cap0.effective, 0x1111_1111);
        assert_eq!(cap0.permitted, 0x3333_3333);
        assert_eq!(cap0.inheritable, 0x5555_5555);
        assert_eq!(cap1.effective, 0x22);
        assert_eq!(cap1.permitted, 0x44);
        assert_eq!(cap1.inheritable, 0x66);
        expect_errno(
            SyscallArgs::new([0, cap_page + 16, 0, 0, 0, 0]).call::<Capget>(),
            SyscallError::BadAddress,
        );

        write_user_value(
            cap_page,
            &TestLinuxCapHeader {
                version: LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            },
        );
        let new_caps = [
            TestLinuxCapData {
                effective: 0xaa,
                permitted: 0xbb,
                inheritable: 0xcc,
            },
            TestLinuxCapData {
                effective: 0xdd,
                permitted: 0xee,
                inheritable: 0xff,
            },
        ];
        write_user_value(cap_page + 16, &new_caps);
        expect_ok(
            SyscallArgs::new([cap_page, cap_page + 16, 0, 0, 0, 0]).call::<Capset>(),
            0,
        );
        {
            let process = process.lock();
            assert_eq!(process.capability_effective, [0xaa, 0xdd]);
            assert_eq!(process.capability_permitted, [0xbb, 0xee]);
            assert_eq!(process.capability_inheritable, [0xcc, 0xff]);
        }
        write_user_value(
            cap_page,
            &TestLinuxCapHeader {
                version: 0x1998_0522,
                pid: 0,
            },
        );
        expect_errno(
            SyscallArgs::new([cap_page, cap_page + 16, 0, 0, 0, 0]).call::<Capset>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([0, cap_page + 16, 0, 0, 0, 0]).call::<Capset>(),
            SyscallError::BadAddress,
        );

        let tid_page = allocate_user_test_page();
        let tid = crate::thread::get_current_thread().lock().id.0 as i32;
        expect_ok(
            SyscallArgs::new([tid_page, 0, 0, 0, 0, 0]).call::<SetTidAddress>(),
            tid as usize,
        );
        assert_eq!(read_user_value::<i32>(tid_page), tid);
        assert_eq!(
            crate::thread::get_current_thread().lock().clear_child_tid,
            tid_page
        );
        expect_ok(
            SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<SetTidAddress>(),
            tid as usize,
        );
        assert_eq!(
            crate::thread::get_current_thread().lock().clear_child_tid,
            0
        );

        expect_ok(
            SyscallArgs::new([0x1234_5000, 24, 0, 0, 0, 0]).call::<SetRobustList>(),
            0,
        );
        {
            let current = crate::thread::get_current_thread();
            let current = current.lock();
            assert_eq!(current.robust_list_head, 0x1234_5000);
            assert_eq!(current.robust_list_len, 24);
        }
        expect_errno(
            SyscallArgs::new([0x1234_6000, usize::MAX as u64, 0, 0, 0, 0]).call::<SetRobustList>(),
            SyscallError::InvalidArguments,
        );
        {
            let current = crate::thread::get_current_thread();
            let current = current.lock();
            assert_eq!(current.robust_list_head, 0x1234_5000);
            assert_eq!(current.robust_list_len, 24);
        }

        {
            let current = crate::thread::get_current_thread();
            let mut current = current.lock();
            current.rseq_area = 0;
            current.rseq_len = 0;
            current.rseq_flags = 0;
            current.rseq_sig = 0;
        }
        let rseq_page = allocate_user_test_page();
        expect_ok(
            SyscallArgs::new([rseq_page, RSEQ_LEN_X86_64, 0, 0x5305_5305, 0, 0]).call::<Rseq>(),
            0,
        );
        let rseq = read_user_value::<TestLinuxRseq>(rseq_page);
        assert_eq!(rseq.cpu_id_start, RSEQ_CPU_ID_SINGLE_CORE);
        assert_eq!(rseq.cpu_id, RSEQ_CPU_ID_SINGLE_CORE);
        {
            let current = crate::thread::get_current_thread();
            let current = current.lock();
            assert_eq!(current.rseq_area, rseq_page);
            assert_eq!(current.rseq_len, RSEQ_LEN_X86_64 as u32);
            assert_eq!(current.rseq_sig, 0x5305_5305);
        }
        expect_errno(
            SyscallArgs::new([rseq_page, RSEQ_LEN_X86_64, 0, 0x5305_5305, 0, 0]).call::<Rseq>(),
            SyscallError::DeviceOrResourceBusy,
        );
        expect_ok(
            SyscallArgs::new([
                rseq_page,
                RSEQ_LEN_X86_64,
                RSEQ_FLAG_UNREGISTER,
                0x5305_5305,
                0,
                0,
            ])
            .call::<Rseq>(),
            0,
        );
        let rseq = read_user_value::<TestLinuxRseq>(rseq_page);
        assert_eq!(rseq.cpu_id_start, RSEQ_CPU_ID_UNINITIALIZED);
        assert_eq!(rseq.cpu_id, RSEQ_CPU_ID_UNINITIALIZED);
        expect_errno(
            SyscallArgs::new([0, RSEQ_LEN_X86_64, 0, 0, 0, 0]).call::<Rseq>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([rseq_page, 16, 0, 0, 0, 0]).call::<Rseq>(),
            SyscallError::InvalidArguments,
        );

        let random_page = allocate_user_test_page();
        expect_ok(
            SyscallArgs::new([random_page, 16, 0, 0, 0, 0]).call::<Getrandom>(),
            16,
        );
        expect_ok(SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Getrandom>(), 0);
        expect_errno(
            SyscallArgs::new([0, 1, 0, 0, 0, 0]).call::<Getrandom>(),
            SyscallError::BadAddress,
        );
        expect_errno(
            SyscallArgs::new([random_page, 1, 8, 0, 0, 0]).call::<Getrandom>(),
            SyscallError::InvalidArguments,
        );

        let time_page = allocate_user_test_page();
        let seconds = SyscallArgs::new([time_page, 0, 0, 0, 0, 0])
            .call::<Time>()
            .expect("time should succeed");
        assert_eq!(read_user_value::<i64>(time_page) as usize, seconds);
        let null_seconds = SyscallArgs::new([0, 0, 0, 0, 0, 0])
            .call::<Time>()
            .expect("time null should succeed");
        assert!(null_seconds >= seconds);

        let tod_page = allocate_user_test_page();
        expect_ok(
            SyscallArgs::new([tod_page, tod_page + 32, 0, 0, 0, 0]).call::<Gettimeofday>(),
            0,
        );
        let timeval = read_user_value::<TestLinuxTimeval>(tod_page);
        assert!(timeval.tv_sec >= 0);
        assert!((0..1_000_000).contains(&timeval.tv_usec));
        let timezone = read_user_value::<TestLinuxTimezone>(tod_page + 32);
        assert_eq!(timezone.tz_minuteswest, old_timezone.0);
        assert_eq!(timezone.tz_dsttime, old_timezone.1);

        let set_time_page = allocate_user_test_page();
        write_user_value(
            set_time_page,
            &TestLinuxTimeval {
                tv_sec: -1,
                tv_usec: 0,
            },
        );
        expect_errno(
            SyscallArgs::new([set_time_page, 0, 0, 0, 0, 0]).call::<Settimeofday>(),
            SyscallError::InvalidArguments,
        );
        write_user_value(
            set_time_page + 32,
            &TestLinuxTimezone {
                tz_minuteswest: 90,
                tz_dsttime: 1,
            },
        );
        expect_ok(
            SyscallArgs::new([0, set_time_page + 32, 0, 0, 0, 0]).call::<Settimeofday>(),
            0,
        );
        assert_eq!(crate::misc::time::timezone(), (90, 1));
        expect_ok(
            SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Settimeofday>(),
            0,
        );

        let rusage_page = allocate_user_test_page();
        expect_ok(
            SyscallArgs::new([0, rusage_page, 0, 0, 0, 0]).call::<Getrusage>(),
            0,
        );
        assert_eq!(read_user_value::<TestLinuxRusage>(rusage_page).ru_maxrss, 0);
        expect_errno(
            SyscallArgs::new([99, rusage_page, 0, 0, 0, 0]).call::<Getrusage>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Getrusage>(),
            SyscallError::BadAddress,
        );

        let sysinfo_page = allocate_user_test_page();
        expect_ok(
            SyscallArgs::new([sysinfo_page, 0, 0, 0, 0, 0]).call::<Sysinfo>(),
            0,
        );
        let info = read_user_value::<TestLinuxSysinfo>(sysinfo_page);
        assert!(info.totalram > 0);
        assert_eq!(info.mem_unit, 1);
        expect_errno(
            SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Sysinfo>(),
            SyscallError::BadAddress,
        );

        let sched_page = allocate_user_test_page();
        write_user_value(sched_page, &TestLinuxSchedParam { sched_priority: 0 });
        expect_ok(
            SyscallArgs::new([0, sched_page, 0, 0, 0, 0]).call::<SchedSetparam>(),
            0,
        );
        write_user_value(sched_page, &TestLinuxSchedParam { sched_priority: -1 });
        expect_errno(
            SyscallArgs::new([0, sched_page, 0, 0, 0, 0]).call::<SchedSetparam>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<SchedSetparam>(),
            SyscallError::BadAddress,
        );
        expect_ok(
            SyscallArgs::new([0, sched_page, 0, 0, 0, 0]).call::<SchedGetparam>(),
            0,
        );
        assert_eq!(
            read_user_value::<TestLinuxSchedParam>(sched_page).sched_priority,
            0
        );
        expect_errno(
            SyscallArgs::new([u64::MAX, sched_page, 0, 0, 0, 0]).call::<SchedGetparam>(),
            SyscallError::InvalidArguments,
        );

        expect_ok(SyscallArgs::new([30, 0, 0, 0, 0, 0]).call::<Alarm>(), 0);
        expect_ok(SyscallArgs::none().call::<Sync>(), 0);

        let (old_break, old_break_base, old_user_mem) = {
            let process = process.lock();
            (
                process.program_break,
                process.program_break_base,
                process.addrspace.user_mem,
            )
        };
        let current_break = SyscallArgs::new([0, 0, 0, 0, 0, 0])
            .call::<Brk>()
            .expect("brk query should succeed");
        expect_ok(
            SyscallArgs::new([crate::memory::addrspace::USER_MEM_END, 0, 0, 0, 0, 0]).call::<Brk>(),
            current_break,
        );
        expect_ok(
            SyscallArgs::new([u64::MAX, 0, 0, 0, 0, 0]).call::<Brk>(),
            current_break,
        );
        {
            let mut process = process.lock();
            process.program_break = old_break;
            process.program_break_base = old_break_base;
            process.addrspace.user_mem = old_user_mem;
        }

        process.lock().timers = old_timers;
        crate::misc::time::set_timezone(old_timezone.0, old_timezone.1);
        {
            let current = crate::thread::get_current_thread();
            let mut current = current.lock();
            current.clear_child_tid = old_clear_child_tid;
            current.robust_list_head = old_robust_list_head;
            current.robust_list_len = old_robust_list_len;
            current.rseq_area = old_rseq_area;
            current.rseq_len = old_rseq_len;
            current.rseq_flags = old_rseq_flags;
            current.rseq_sig = old_rseq_sig;
        }
        saved.restore();
    }
    crate::test!(
        uname_reboot_and_rlimit_syscalls,
        "uname reboot and rlimit syscalls follow linux abi rules",
        uname_reboot_and_rlimit_syscalls_follow_linux_abi_rules
    );
    fn uname_reboot_and_rlimit_syscalls_follow_linux_abi_rules() {
        assert_linux_layout::<TestUtsName>(390, 1);
        assert_linux_layout::<TestLinuxTimespec>(16, 8);
        assert_linux_layout::<TestLinuxRlimit64>(16, 8);

        const LINUX_REBOOT_MAGIC1: u64 = 0xfee1_dead;
        const LINUX_REBOOT_MAGIC2: u64 = 0x2812_1969;
        const LINUX_REBOOT_CMD_CAD_OFF: u64 = 0x0000_0000;
        const LINUX_REBOOT_CMD_CAD_ON: u64 = 0x89ab_cdef;
        const RLIMIT_STACK: u64 = 3;
        const RLIMIT_NOFILE: u64 = 7;
        const RLIMIT_MEMLOCK: u64 = 8;
        const RLIMIT_RTPRIO: u64 = 14;

        let process = get_current_process();
        let (
            old_stack_cur,
            old_stack_max,
            old_nofile_cur,
            old_nofile_max,
            old_memlock_cur,
            old_memlock_max,
            old_rtprio_cur,
            old_rtprio_max,
        ) = {
            let process = process.lock();
            (
                process.rlimit_stack_cur,
                process.rlimit_stack_max,
                process.rlimit_nofile_cur,
                process.rlimit_nofile_max,
                process.rlimit_memlock_cur,
                process.rlimit_memlock_max,
                process.rlimit_rtprio_cur,
                process.rlimit_rtprio_max,
            )
        };
        let old_cad = crate::misc::reboot::ctrl_alt_del_enabled();

        let host_page = allocate_user_test_page();
        write_user_value(host_page, b"linuxhost");
        expect_ok(
            SyscallArgs::new([host_page, 9, 0, 0, 0, 0]).call::<Sethostname>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([host_page, 65, 0, 0, 0, 0]).call::<Sethostname>(),
            SyscallError::InvalidArguments,
        );
        write_user_value(host_page, b"bad\0host");
        expect_errno(
            SyscallArgs::new([host_page, 8, 0, 0, 0, 0]).call::<Sethostname>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([0, 1, 0, 0, 0, 0]).call::<Sethostname>(),
            SyscallError::BadAddress,
        );

        let uts_page = allocate_user_test_page();
        expect_ok(
            SyscallArgs::new([uts_page, 0, 0, 0, 0, 0]).call::<Uname>(),
            0,
        );
        let uts = read_user_value::<TestUtsName>(uts_page);
        assert_eq!(&uts.sysname[..6], b"Linux\0");
        assert_eq!(&uts.nodename[..10], b"linuxhost\0");
        assert_eq!(&uts.release[..13], b"6.12.0-seele\0");
        assert_eq!(&uts.machine[..7], b"x86_64\0");
        expect_errno(
            SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Uname>(),
            SyscallError::BadAddress,
        );

        expect_errno(
            SyscallArgs::new([0, LINUX_REBOOT_MAGIC2, LINUX_REBOOT_CMD_CAD_OFF, 0, 0, 0])
                .call::<Reboot>(),
            SyscallError::InvalidArguments,
        );
        expect_ok(
            SyscallArgs::new([
                LINUX_REBOOT_MAGIC1,
                LINUX_REBOOT_MAGIC2,
                LINUX_REBOOT_CMD_CAD_OFF,
                0,
                0,
                0,
            ])
            .call::<Reboot>(),
            0,
        );
        assert!(!crate::misc::reboot::ctrl_alt_del_enabled());
        expect_ok(
            SyscallArgs::new([
                LINUX_REBOOT_MAGIC1,
                LINUX_REBOOT_MAGIC2,
                LINUX_REBOOT_CMD_CAD_ON,
                0,
                0,
                0,
            ])
            .call::<Reboot>(),
            0,
        );
        assert!(crate::misc::reboot::ctrl_alt_del_enabled());
        expect_errno(
            SyscallArgs::new([LINUX_REBOOT_MAGIC1, LINUX_REBOOT_MAGIC2, 0x1234, 0, 0, 0])
                .call::<Reboot>(),
            SyscallError::InvalidArguments,
        );

        let timespec_page = allocate_user_test_page();
        expect_ok(
            SyscallArgs::new([0, timespec_page, 0, 0, 0, 0]).call::<SchedRrGetInterval>(),
            0,
        );
        let interval = read_user_value::<TestLinuxTimespec>(timespec_page);
        assert_eq!(interval.tv_sec, 0);
        assert_eq!(interval.tv_nsec, 100_000_000);
        expect_errno(
            SyscallArgs::new([u64::MAX, timespec_page, 0, 0, 0, 0]).call::<SchedRrGetInterval>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<SchedRrGetInterval>(),
            SyscallError::BadAddress,
        );

        let rlimit_page = allocate_user_test_page();
        write_user_value(
            rlimit_page,
            &TestLinuxRlimit64 {
                rlim_cur: 4096,
                rlim_max: 8192,
            },
        );
        expect_ok(
            SyscallArgs::new([RLIMIT_STACK, rlimit_page, 0, 0, 0, 0]).call::<Setrlimit>(),
            0,
        );
        {
            let process = process.lock();
            assert_eq!(process.rlimit_stack_cur, 4096);
            assert_eq!(process.rlimit_stack_max, 8192);
        }
        expect_ok(
            SyscallArgs::new([RLIMIT_STACK, rlimit_page + 64, 0, 0, 0, 0]).call::<Getrlimit>(),
            0,
        );
        let stack_limit = read_user_value::<TestLinuxRlimit64>(rlimit_page + 64);
        assert_eq!(stack_limit.rlim_cur, 4096);
        assert_eq!(stack_limit.rlim_max, 8192);
        expect_errno(
            SyscallArgs::new([99, rlimit_page + 64, 0, 0, 0, 0]).call::<Getrlimit>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([RLIMIT_STACK, 0, 0, 0, 0, 0]).call::<Getrlimit>(),
            SyscallError::BadAddress,
        );
        expect_errno(
            SyscallArgs::new([99, rlimit_page, 0, 0, 0, 0]).call::<Setrlimit>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([RLIMIT_STACK, 0, 0, 0, 0, 0]).call::<Setrlimit>(),
            SyscallError::BadAddress,
        );

        write_user_value(
            rlimit_page,
            &TestLinuxRlimit64 {
                rlim_cur: 256,
                rlim_max: 512,
            },
        );
        expect_ok(
            SyscallArgs::new([0, RLIMIT_NOFILE, rlimit_page, rlimit_page + 32, 0, 0])
                .call::<Prlimit64>(),
            0,
        );
        let old_nofile = read_user_value::<TestLinuxRlimit64>(rlimit_page + 32);
        assert_eq!(old_nofile.rlim_cur, old_nofile_cur);
        assert_eq!(old_nofile.rlim_max, old_nofile_max);
        {
            let process = process.lock();
            assert_eq!(process.rlimit_nofile_cur, 256);
            assert_eq!(process.rlimit_nofile_max, 512);
        }
        expect_ok(
            SyscallArgs::new([0, RLIMIT_MEMLOCK, 0, rlimit_page + 32, 0, 0]).call::<Prlimit64>(),
            0,
        );
        let old_memlock = read_user_value::<TestLinuxRlimit64>(rlimit_page + 32);
        assert_eq!(old_memlock.rlim_cur, old_memlock_cur);
        assert_eq!(old_memlock.rlim_max, old_memlock_max);
        write_user_value(
            rlimit_page,
            &TestLinuxRlimit64 {
                rlim_cur: 7,
                rlim_max: 9,
            },
        );
        expect_ok(
            SyscallArgs::new([0, RLIMIT_RTPRIO, rlimit_page, 0, 0, 0]).call::<Prlimit64>(),
            0,
        );
        {
            let process = process.lock();
            assert_eq!(process.rlimit_rtprio_cur, 7);
            assert_eq!(process.rlimit_rtprio_max, 9);
        }
        expect_errno(
            SyscallArgs::new([1, RLIMIT_NOFILE, 0, 0, 0, 0]).call::<Prlimit64>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([0, 99, 0, 0, 0, 0]).call::<Prlimit64>(),
            SyscallError::InvalidArguments,
        );

        expect_ok(SyscallArgs::none().call::<Vhangup>(), 0);

        {
            let mut process = process.lock();
            process.rlimit_stack_cur = old_stack_cur;
            process.rlimit_stack_max = old_stack_max;
            process.rlimit_nofile_cur = old_nofile_cur;
            process.rlimit_nofile_max = old_nofile_max;
            process.rlimit_memlock_cur = old_memlock_cur;
            process.rlimit_memlock_max = old_memlock_max;
            process.rlimit_rtprio_cur = old_rtprio_cur;
            process.rlimit_rtprio_max = old_rtprio_max;
        }
        crate::misc::reboot::set_ctrl_alt_del_enabled(old_cad);
    }
}
