use super::*;

define_syscall!(Capget, |header: *mut LinuxCapHeader,
                         data: *mut LinuxCapData| {
    if header.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let mut header_value = user_safe::read(header)?;
    if !capability_header_targets_current_process(&header_value) {
        return Err(SyscallError::InvalidArguments);
    }
    header_value.version = LINUX_CAPABILITY_VERSION_3;
    user_safe::write(header, &header_value)?;
    if !data.is_null() {
        user_safe::write(data, &current_capability_data())?;
    }

    Ok(0)
});

define_syscall!(Capset, |header: *const LinuxCapHeader,
                         data: *const LinuxCapData| {
    if header.is_null() || data.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let header_value = user_safe::read(header)?;
    if header_value.version != LINUX_CAPABILITY_VERSION_3 {
        return Err(SyscallError::InvalidArguments);
    }
    if !capability_header_targets_current_process(&header_value) {
        return Err(SyscallError::InvalidArguments);
    }

    let cap_data = user_safe::read(data as *const [LinuxCapData; LINUX_CAPABILITY_U32S_3])?;
    let process = get_current_process();
    let mut process = process.lock();
    for (index, caps) in cap_data.iter().enumerate() {
        process.capability_effective[index] = caps.effective;
        process.capability_permitted[index] = caps.permitted;
        process.capability_inheritable[index] = caps.inheritable;
    }

    Ok(0)
});
