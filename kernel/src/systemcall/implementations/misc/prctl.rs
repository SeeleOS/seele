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
