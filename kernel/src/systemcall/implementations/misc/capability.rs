use super::*;

define_syscall!(Capget, |header: *mut LinuxCapHeader,
                         data: *mut LinuxCapData| {
    if header.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let mut header_value = user_safe::read(header)?;
    let Some(capability_u32s) = capability_u32s(header_value.version) else {
        header_value.version = LINUX_CAPABILITY_VERSION_3;
        user_safe::write(header, &header_value)?;
        return Err(SyscallError::InvalidArguments);
    };
    let capability_data = if let Some(pid) = capability_header_target_pid(&header_value)? {
        let current_pid = get_current_process().lock().pid;
        if pid == current_pid {
            current_capability_data()
        } else {
            capability_data_for_process(&get_process_with_pid(pid)?)
        }
    } else {
        current_capability_data()
    };
    user_safe::write(header, &header_value)?;
    if !data.is_null() {
        for (index, cap_data) in capability_data.iter().take(capability_u32s).enumerate() {
            user_safe::write(unsafe { data.add(index) }, cap_data)?;
        }
    }

    Ok(0)
});

define_syscall!(Capset, |header: *const LinuxCapHeader,
                         data: *const LinuxCapData| {
    if header.is_null() || data.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let header_value = user_safe::read(header)?;
    let Some(capability_u32s) = capability_u32s(header_value.version) else {
        let mut preferred = header_value;
        preferred.version = LINUX_CAPABILITY_VERSION_3;
        user_safe::write(header.cast_mut(), &preferred)?;
        return Err(SyscallError::InvalidArguments);
    };
    if !capability_header_targets_current_process(&header_value)? {
        return Err(SyscallError::PermissionDenied);
    }

    let mut cap_data = current_capability_data();
    for (index, cap_slot) in cap_data.iter_mut().take(capability_u32s).enumerate() {
        *cap_slot = user_safe::read(unsafe { data.add(index) })?;
    }
    validate_capset_data(&cap_data)?;
    let process = get_current_process();
    let mut process = process.lock();
    for (index, caps) in cap_data.iter().enumerate() {
        process.capability_effective[index] = caps.effective;
        process.capability_permitted[index] = caps.permitted;
        process.capability_inheritable[index] = caps.inheritable;
    }

    Ok(0)
});
