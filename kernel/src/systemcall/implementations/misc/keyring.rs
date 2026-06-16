use super::*;

define_syscall!(AddKey, |type_name: String,
                         description: String,
                         payload: *const u8,
                         plen: usize,
                         keyring: i32| {
    let _ = (type_name, description);
    if plen != 0 {
        if payload.is_null() {
            return Err(SyscallError::BadAddress);
        }
        let _ = user_safe::read_buffer(payload, plen)?;
    }
    let _ = resolve_keyring(keyring, true)?;
    let serial = NEXT_KEY_SERIAL.fetch_add(1, Ordering::Relaxed);
    ensure_key_entry(serial);
    Ok(serial as usize)
});

define_syscall!(Keyctl, |cmd: u64,
                         arg2: u64,
                         arg3: u64,
                         _arg4: u64,
                         _arg5: u64| {
    match KeyctlCommand::try_from(cmd) {
        Ok(KeyctlCommand::GetKeyringId) => {
            let keyring = resolve_keyring(arg2 as i32, arg3 != 0)?;
            Ok(keyring as usize)
        }
        Ok(KeyctlCommand::JoinSessionKeyring) => Ok(current_session_keyring(true)? as usize),
        Ok(KeyctlCommand::Revoke) => {
            revoke_key(arg2 as i32)?;
            Ok(0)
        }
        Ok(KeyctlCommand::Setperm) => {
            let keyring = resolve_keyring(arg2 as i32, true)?;
            set_key_permissions(keyring, arg3 as u32)?;
            Ok(0)
        }
        Ok(KeyctlCommand::Link) => {
            let target = resolve_keyring(arg3 as i32, true)?;
            link_key_into_keyring(arg2 as i32, target)?;
            Ok(0)
        }
        Ok(KeyctlCommand::SessionToParent) => {
            let current_keyring = current_session_keyring(true)?;
            let current = get_current_process();
            let parent = current
                .lock()
                .parent
                .clone()
                .ok_or(SyscallError::NoProcess)?;
            parent.lock().session_keyring = current_keyring;
            ensure_keyring_entry(current_keyring);
            Ok(0)
        }
        Err(_) => Err(SyscallError::NoSyscall),
    }
});

#[cfg(test)]
mod tests {
    use crate::systemcall::test::*;

    crate::test!(
        key_and_bpf_syscalls,
        "add_key keyctl and bpf follow linux rules",
        key_and_bpf_syscalls_follow_linux_rules
    );
}
