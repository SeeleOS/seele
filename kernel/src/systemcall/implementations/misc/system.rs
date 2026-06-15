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
    }

    let current = process.program_break;
    if addr == 0 {
        return Ok(current as usize);
    }

    let old_aligned = current.div_ceil(4096) * 4096;
    let new_aligned = addr.div_ceil(4096) * 4096;

    if new_aligned > old_aligned {
        process.addrspace.register_area(MemoryArea::new(
            VirtAddr::new(old_aligned),
            (new_aligned - old_aligned) / 4096,
            protection_to_page_flags(Protection::READ | Protection::WRITE),
            Data::Normal,
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
        crate::misc::utsname::DEFAULT_SYSNAME,
        crate::misc::utsname::DEFAULT_RELEASE,
        crate::misc::utsname::DEFAULT_VERSION,
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

define_syscall!(Sync, { Ok(0) });
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
