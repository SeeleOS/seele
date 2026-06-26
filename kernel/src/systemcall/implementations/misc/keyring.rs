use super::*;
use crate::misc::others::KernelFrom;

const KEYCTL_IOV_MAX: u32 = 1024;

#[repr(C)]
#[derive(Clone, Copy)]
struct KeyctlIovec {
    iov_base: *const u8,
    iov_len: usize,
}

define_syscall!(AddKey, |type_name: String,
                         description: String,
                         payload: *const u8,
                         plen: usize,
                         keyring: i32| {
    let key_type = key_type_from_name(&type_name);
    let payload_bytes = validate_key_payload(key_type, &description, payload, plen)?;
    let target_keyring = resolve_keyring(keyring, true)?;
    if matches!(
        key_type,
        KeyType::User | KeyType::Logon | KeyType::BigKey | KeyType::Encrypted
    ) {
        let uid = get_current_process().lock().effective_uid;
        reserve_user_key_quota(uid, &description, plen)?;
    }
    let serial = NEXT_KEY_SERIAL.fetch_add(1, Ordering::Relaxed);
    if key_type == KeyType::Keyring {
        ensure_keyring_entry(serial, &description);
    } else {
        ensure_key_entry(serial, &type_name, &description, payload_bytes);
    }
    link_key_into_keyring(serial, target_keyring)?;
    Ok(serial as usize)
});

define_syscall!(RequestKey, |type_name_ptr: *const u8,
                             description_ptr: *const u8,
                             callout_info: *const u8,
                             dest_keyring: i32| {
    let type_name = String::k_from(type_name_ptr).map_err(|_| SyscallError::BadAddress)?;
    let description = String::k_from(description_ptr).map_err(|_| SyscallError::BadAddress)?;
    if type_name.starts_with('.') || description.starts_with('.') {
        return Err(SyscallError::PermissionDenied);
    }
    if !callout_info.is_null() {
        let _ = user_safe::read_buffer(callout_info, 1)?;
    }

    let key_type = key_type_from_name(&type_name);
    if key_type == KeyType::Unsupported {
        return Err(SyscallError::NoDevice);
    }

    let effective_dest_keyring = if dest_keyring == KEY_REQKEY_DEFL_DEFAULT {
        let default_keyring = get_current_process().lock().request_key_default_keyring;
        if default_keyring == KEY_REQKEY_DEFL_DEFAULT {
            dest_keyring
        } else {
            default_keyring
        }
    } else {
        dest_keyring
    };

    let keyring = match effective_dest_keyring {
        KEY_REQKEY_DEFL_DEFAULT => {
            let search_keyrings = [
                current_thread_keyring(false).ok(),
                current_process_keyring(false).ok(),
                current_session_keyring(false).ok(),
            ];
            for keyring in search_keyrings.into_iter().flatten() {
                match search_keyring(keyring, &type_name, &description) {
                    Ok(serial) => return Ok(serial as usize),
                    Err(SyscallError::NoKey) => {}
                    Err(err) => return Err(err),
                }
            }
            return Err(SyscallError::NoKey);
        }
        KEY_REQKEY_DEFL_THREAD_KEYRING => current_thread_keyring(true)?,
        KEY_REQKEY_DEFL_PROCESS_KEYRING => current_process_keyring(true)?,
        KEY_REQKEY_DEFL_SESSION_KEYRING => current_session_keyring(true)?,
        KEY_REQKEY_DEFL_USER_KEYRING => current_user_keyring(UserKeyringKind::User, true)?,
        KEY_REQKEY_DEFL_USER_SESSION_KEYRING => {
            current_user_keyring(UserKeyringKind::UserSession, true)?
        }
        KEY_SPEC_THREAD_KEYRING
        | KEY_SPEC_PROCESS_KEYRING
        | KEY_SPEC_SESSION_KEYRING
        | KEY_SPEC_USER_KEYRING
        | KEY_SPEC_USER_SESSION_KEYRING => resolve_keyring(effective_dest_keyring, true)?,
        _ => resolve_existing_keyring(effective_dest_keyring)?,
    };

    match search_keyring(keyring, &type_name, &description) {
        Ok(serial) => return Ok(serial as usize),
        Err(SyscallError::NoKey) => {}
        Err(err) => return Err(err),
    };

    if !keyring_allows_search(keyring)? {
        return Err(SyscallError::AccessDenied);
    }
    if !keyring_allows_write(keyring)? {
        return Err(SyscallError::AccessDenied);
    }

    if key_type == KeyType::Keyring {
        let serial = NEXT_KEY_SERIAL.fetch_add(1, Ordering::Relaxed);
        ensure_keyring_entry(serial, &description);
        link_key_into_keyring(serial, keyring)?;
        return Ok(serial as usize);
    }

    let serial = NEXT_KEY_SERIAL.fetch_add(1, Ordering::Relaxed);
    ensure_negative_key_entry(serial, &type_name, &description);
    link_key_into_keyring(serial, keyring)?;
    Err(SyscallError::NoKey)
});

fn keyctl_key_serial(spec: u64) -> Result<i32, SyscallError> {
    resolve_key_serial(spec as i32)
}

fn keyctl_keyring_serial(spec: u64, create: bool) -> Result<i32, SyscallError> {
    if create {
        resolve_keyring(spec as i32, true)
    } else {
        resolve_existing_keyring(spec as i32)
    }
}

fn keyctl_unlink_target(spec: u64) -> Result<i32, SyscallError> {
    match spec as i32 {
        KEY_SPEC_THREAD_KEYRING
        | KEY_SPEC_PROCESS_KEYRING
        | KEY_SPEC_SESSION_KEYRING
        | KEY_SPEC_USER_KEYRING
        | KEY_SPEC_USER_SESSION_KEYRING => resolve_keyring(spec as i32, false),
        serial if serial > 0 => Ok(serial),
        _ => Err(SyscallError::NoKey),
    }
}

fn read_keyctl_iovec_payload(
    iov_ptr: *const KeyctlIovec,
    ioc: u32,
) -> Result<Vec<u8>, SyscallError> {
    if ioc > KEYCTL_IOV_MAX {
        return Err(SyscallError::InvalidArguments);
    }
    if ioc > 0 && iov_ptr.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let mut iovs = Vec::with_capacity(ioc as usize);
    for index in 0..ioc as usize {
        iovs.push(user_safe::read(unsafe { iov_ptr.add(index) })?);
    }

    let total_len = iovs.iter().try_fold(0usize, |acc, iov| {
        let next = acc
            .checked_add(iov.iov_len)
            .ok_or(SyscallError::InvalidArguments)?;
        if next > isize::MAX as usize {
            return Err(SyscallError::InvalidArguments);
        }
        Ok(next)
    })?;
    if total_len == 0 {
        return Ok(Vec::new());
    }

    let mut payload = Vec::with_capacity(total_len);
    for iov in iovs {
        if iov.iov_len == 0 {
            continue;
        }
        if iov.iov_base.is_null() {
            return Err(SyscallError::BadAddress);
        }
        payload.extend_from_slice(&user_safe::read_buffer(iov.iov_base, iov.iov_len)?);
    }
    Ok(payload)
}

define_syscall!(Keyctl, |cmd: u64,
                         arg2: u64,
                         arg3: u64,
                         arg4: u64,
                         arg5: u64| {
    match KeyctlCommand::try_from(cmd) {
        Ok(KeyctlCommand::GetKeyringId) => {
            let keyring = resolve_keyring(arg2 as i32, arg3 != 0)?;
            Ok(keyring as usize)
        }
        Ok(KeyctlCommand::JoinSessionKeyring) => {
            if arg2 != 0 {
                let description =
                    String::k_from(arg2 as *const u8).map_err(|_| SyscallError::BadAddress)?;
                if description.starts_with('.') {
                    return Err(SyscallError::PermissionDenied);
                }
                return Ok(join_named_session_keyring(&description)? as usize);
            }
            Ok(current_session_keyring(true)? as usize)
        }
        Ok(KeyctlCommand::Update) => {
            update_key_payload(keyctl_key_serial(arg2)?, arg3 as *const u8, arg4 as usize)?;
            Ok(0)
        }
        Ok(KeyctlCommand::Revoke) => {
            revoke_key(keyctl_key_serial(arg2)?)?;
            Ok(0)
        }
        Ok(KeyctlCommand::Chown) => {
            chown_key(keyctl_key_serial(arg2)?, arg3 as u32, arg4 as u32)?;
            Ok(0)
        }
        Ok(KeyctlCommand::Setperm) => {
            set_key_permissions(keyctl_key_serial(arg2)?, arg3 as u32)?;
            Ok(0)
        }
        Ok(KeyctlCommand::Describe) => {
            let description = describe_key(keyctl_key_serial(arg2)?)?;
            copy_keyctl_bytes_to_user(&description, arg3 as *mut u8, arg4 as usize)
        }
        Ok(KeyctlCommand::Clear) => {
            clear_keyring(keyctl_keyring_serial(arg2, false)?)?;
            Ok(0)
        }
        Ok(KeyctlCommand::Link) => {
            let target = resolve_keyring(arg3 as i32, true)?;
            link_key_into_keyring(keyctl_key_serial(arg2)?, target)?;
            Ok(0)
        }
        Ok(KeyctlCommand::Unlink) => {
            unlink_key_from_keyring(
                keyctl_unlink_target(arg2)?,
                keyctl_keyring_serial(arg3, false)?,
            )?;
            Ok(0)
        }
        Ok(KeyctlCommand::Search) => {
            let keyring = keyctl_keyring_serial(arg2, false)?;
            let type_name =
                String::k_from(arg3 as *const u8).map_err(|_| SyscallError::BadAddress)?;
            let description =
                String::k_from(arg4 as *const u8).map_err(|_| SyscallError::BadAddress)?;
            let serial = search_keyring(keyring, &type_name, &description)?;
            if arg5 != 0 {
                let target = resolve_keyring(arg5 as i32, true)?;
                link_key_into_keyring(serial, target)?;
            }
            Ok(serial as usize)
        }
        Ok(KeyctlCommand::Read) => {
            let bytes = read_key(keyctl_key_serial(arg2)?)?;
            copy_keyctl_bytes_to_user(&bytes, arg3 as *mut u8, arg4 as usize)
        }
        Ok(KeyctlCommand::Instantiate) => {
            let serial = keyctl_key_serial(arg2)?;
            instantiate_key(serial, arg3 as *const u8, arg4 as usize)?;
            if arg5 != 0 {
                let target = resolve_keyring(arg5 as i32, true)?;
                link_key_into_keyring(serial, target)?;
            }
            Ok(0)
        }
        Ok(KeyctlCommand::Negate) => {
            reject_key(keyctl_key_serial(arg2)?, arg3 as u32)?;
            if arg4 != 0 {
                let target = resolve_keyring(arg4 as i32, true)?;
                link_key_into_keyring(keyctl_key_serial(arg2)?, target)?;
            }
            Ok(0)
        }
        Ok(KeyctlCommand::SetReqkeyKeyring) => {
            let requested = arg2 as i32;
            if requested == KEY_REQKEY_DEFL_NO_CHANGE {
                return Ok(get_current_process().lock().request_key_default_keyring as usize);
            }
            match requested {
                KEY_REQKEY_DEFL_DEFAULT
                | KEY_REQKEY_DEFL_THREAD_KEYRING
                | KEY_REQKEY_DEFL_PROCESS_KEYRING
                | KEY_REQKEY_DEFL_SESSION_KEYRING
                | KEY_REQKEY_DEFL_USER_KEYRING
                | KEY_REQKEY_DEFL_USER_SESSION_KEYRING => {}
                _ => return Err(SyscallError::InvalidArguments),
            }
            let current = get_current_process();
            let mut process = current.lock();
            let old = process.request_key_default_keyring;
            process.request_key_default_keyring = requested;
            Ok(old as usize)
        }
        Ok(KeyctlCommand::SetTimeout) => {
            set_key_timeout(keyctl_key_serial(arg2)?, arg3 as u32)?;
            Ok(0)
        }
        Ok(KeyctlCommand::AssumeAuthority) => unsupported_keyctl(),
        Ok(KeyctlCommand::GetSecurity) => {
            let security = get_key_security(keyctl_key_serial(arg2)?)?;
            copy_keyctl_bytes_to_user(&security, arg3 as *mut u8, arg4 as usize)
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
            ensure_keyring_entry(current_keyring, "");
            Ok(0)
        }
        Ok(KeyctlCommand::Reject) => {
            reject_key(keyctl_key_serial(arg2)?, arg3 as u32)?;
            if arg5 != 0 {
                let target = resolve_keyring(arg5 as i32, true)?;
                link_key_into_keyring(keyctl_key_serial(arg2)?, target)?;
            }
            Ok(0)
        }
        Ok(KeyctlCommand::InstantiateIov) => {
            let serial = keyctl_key_serial(arg2)?;
            let payload = read_keyctl_iovec_payload(arg3 as *const KeyctlIovec, arg4 as u32)?;
            instantiate_key_from_payload(serial, payload)?;
            if arg5 != 0 {
                let target = resolve_keyring(arg5 as i32, true)?;
                link_key_into_keyring(serial, target)?;
            }
            Ok(0)
        }
        Ok(KeyctlCommand::Invalidate) => {
            invalidate_key(keyctl_key_serial(arg2)?)?;
            Ok(0)
        }
        Ok(KeyctlCommand::GetPersistent) => {
            let keyring = current_user_keyring(UserKeyringKind::UserSession, true)?;
            Ok(keyring as usize)
        }
        Ok(KeyctlCommand::DhCompute)
        | Ok(KeyctlCommand::PkeyQuery)
        | Ok(KeyctlCommand::PkeyEncrypt)
        | Ok(KeyctlCommand::PkeyDecrypt)
        | Ok(KeyctlCommand::PkeySign)
        | Ok(KeyctlCommand::PkeyVerify) => unsupported_keyctl(),
        Ok(KeyctlCommand::RestrictKeyring) => unsupported_keyctl(),
        Ok(KeyctlCommand::Move) => {
            if arg5 != 0 {
                return Err(SyscallError::OperationNotSupported);
            }
            let source = keyctl_unlink_target(arg2)?;
            let from = keyctl_keyring_serial(arg3, false)?;
            let to = resolve_keyring(arg4 as i32, true)?;
            unlink_key_from_keyring(source, from)?;
            link_key_into_keyring(source, to)?;
            Ok(0)
        }
        Ok(KeyctlCommand::Capabilities) => {
            let caps = [0xb3u8, 0, 0, 0];
            copy_keyctl_bytes_to_user(&caps, arg2 as *mut u8, arg3 as usize)
        }
        Ok(KeyctlCommand::WatchKey) => unsupported_keyctl(),
        Err(_) => Err(SyscallError::InvalidArguments),
    }
});

#[cfg(test)]
mod tests {
    use super::super::{
        KEY_USER_DEFAULT_MAX_BYTES, KEY_USER_DEFAULT_MAX_KEYS, ensure_negative_key_entry,
        proc_key_users_bytes, reserve_user_key_quota,
    };

    use crate::systemcall::{
        implementations::{AddKey, Bpf, Eventfd, Keyctl},
        test::{close_test_fd, expect_fd, write_user_cstr},
        test_helpers::{
            SyscallArgs, allocate_user_test_page, assert_user_bytes, expect_errno, expect_ok,
            read_user_value, write_user_value,
        },
        utils::SyscallError,
    };

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct TestBpfMapCreateAttr {
        map_type: u32,
        key_size: u32,
        value_size: u32,
        max_entries: u32,
        map_flags: u32,
        inner_map_fd: u32,
        numa_node: u32,
        map_name: [u8; 16],
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct TestBpfMapElemAttr {
        map_fd: u32,
        padding: u32,
        key: u64,
        value: u64,
        flags: u64,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct TestBpfProgLoadAttr {
        prog_type: u32,
        insn_cnt: u32,
        insns: u64,
        license: u64,
        log_level: u32,
        log_size: u32,
        log_buf: u64,
        kern_version: u32,
        prog_flags: u32,
        prog_name: [u8; 16],
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct TestBpfProgAttachAttr {
        target_fd: u32,
        attach_bpf_fd: u32,
        attach_type: u32,
        attach_flags: u32,
        replace_bpf_fd: u32,
        relative_fd: u32,
        expected_revision: u64,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct TestKeyctlIovec {
        iov_base: u64,
        iov_len: usize,
    }

    crate::test!(
        key_and_bpf_syscalls,
        "add_key keyctl and bpf follow linux rules",
        key_and_bpf_syscalls_follow_linux_rules
    );

    crate::test!(
        key_user_quota_proc_format,
        "key user quotas expose Linux-compatible proc fields",
        key_user_quota_proc_format_matches_linux
    );

    fn key_user_quota_proc_format_matches_linux() {
        let uid = 60_001;
        for index in 0..KEY_USER_DEFAULT_MAX_KEYS {
            let description = alloc::format!("abc{index}");
            reserve_user_key_quota(uid, &description, 64)
                .expect("64-byte keys should reach the max-key limit before max-bytes");
        }

        let proc = alloc::string::String::from_utf8(proc_key_users_bytes())
            .expect("/proc/key-users should be UTF-8");
        let line = proc
            .lines()
            .find(|line| line.trim_start().starts_with(&alloc::format!("{uid}:")))
            .expect("quota entry should be visible in /proc/key-users");
        let fields = line.split_whitespace().collect::<alloc::vec::Vec<_>>();
        assert_eq!(fields[0], alloc::format!("{uid}:"));
        assert_eq!(
            fields[3],
            alloc::format!("{0}/{0}", KEY_USER_DEFAULT_MAX_KEYS)
        );
        let expected_bytes = (0..KEY_USER_DEFAULT_MAX_KEYS)
            .map(|index| alloc::format!("abc{index}").len() + 1 + 64)
            .sum::<usize>();
        assert_eq!(
            fields[4],
            alloc::format!("{expected_bytes}/{KEY_USER_DEFAULT_MAX_BYTES}")
        );

        expect_errno(
            reserve_user_key_quota(uid, "overflow", 64).map(|()| 0),
            SyscallError::QuotaExceeded,
        );

        let bytes_uid = uid + 1;
        reserve_user_key_quota(bytes_uid, "near_limit", KEY_USER_DEFAULT_MAX_BYTES - 11)
            .expect("payload at the byte quota should fit");
        expect_errno(
            reserve_user_key_quota(bytes_uid, "x", 0).map(|()| 0),
            SyscallError::QuotaExceeded,
        );
    }

    fn key_and_bpf_syscalls_follow_linux_rules() {
        const KEY_SPEC_THREAD_KEYRING: u64 = (-1i32) as u64;
        const KEY_SPEC_PROCESS_KEYRING: u64 = (-2i32) as u64;
        const KEY_SPEC_SESSION_KEYRING: u64 = (-3i32) as u64;
        const KEY_SPEC_USER_KEYRING: u64 = (-4i32) as u64;
        const KEY_SPEC_USER_SESSION_KEYRING: u64 = (-5i32) as u64;
        const BPF_MAP_CREATE: u64 = 0;
        const BPF_MAP_LOOKUP_ELEM: u64 = 1;
        const BPF_MAP_UPDATE_ELEM: u64 = 2;
        const BPF_PROG_LOAD: u64 = 5;
        const BPF_PROG_ATTACH: u64 = 8;
        const BPF_PROG_DETACH: u64 = 9;
        const BPF_MAP_TYPE_ARRAY: u32 = 2;
        const KEYCTL_SEARCH: u64 = 10;
        const KEYCTL_READ: u64 = 11;
        const KEYCTL_INSTANTIATE_IOV: u64 = 20;
        const ENCRYPTED_KEY_VALID_PAYLOAD_LEN: u64 =
            b"new enc32 user:masterkey 32 abcdefABCDEF1234567890aaaaaaaaaaabcdefABCDEF1234567890aaaaaaaaaa"
                .len() as u64;
        const ENCRYPTED_KEY_INVALID_PAYLOAD_LEN: u64 =
            b"new enc32 user:masterkey 32 plaintext123@123!123@123!123@123plaintext123@123!123@123!123@123"
                .len() as u64;

        let page = allocate_user_test_page();
        write_user_cstr(page, b"user\0");
        write_user_cstr(page + 64, b"demo\0");
        write_user_cstr(page + 96, b"keyring\0");
        write_user_cstr(page + 352, b"logon\0");
        write_user_cstr(page + 384, b"big_key\0");
        write_user_cstr(page + 416, b"encrypted\0");
        write_user_cstr(page + 448, b"service:secret\0");
        write_user_cstr(
            page + 512,
            b"new enc32 user:masterkey 32 abcdefABCDEF1234567890aaaaaaaaaaabcdefABCDEF1234567890aaaaaaaaaa\0",
        );
        write_user_cstr(
            page + 640,
            b"new enc32 user:masterkey 32 plaintext123@123!123@123!123@123plaintext123@123!123@123!123@123\0",
        );
        expect_errno(
            SyscallArgs::new([page, page + 64, 1, 1, KEY_SPEC_SESSION_KEYRING, 0]).call::<AddKey>(),
            SyscallError::BadAddress,
        );
        write_user_cstr(page + 320, b"asymmetric\0");
        expect_errno(
            SyscallArgs::new([page + 320, page + 64, 0, 0, KEY_SPEC_THREAD_KEYRING, 0])
                .call::<AddKey>(),
            SyscallError::NoDevice,
        );
        expect_errno(
            SyscallArgs::new([page + 320, page + 64, 0, 64, KEY_SPEC_PROCESS_KEYRING, 0])
                .call::<AddKey>(),
            SyscallError::BadAddress,
        );
        expect_errno(
            SyscallArgs::new([
                page + 320,
                page + 64,
                page + 128,
                64,
                KEY_SPEC_PROCESS_KEYRING,
                0,
            ])
            .call::<AddKey>(),
            SyscallError::NoDevice,
        );
        expect_errno(
            SyscallArgs::new([page + 96, page + 64, 0, 0, 0x7fff_ffff, 0]).call::<AddKey>(),
            SyscallError::NoKey,
        );
        let _small_user_key =
            SyscallArgs::new([page, page + 64, page + 128, 16, KEY_SPEC_THREAD_KEYRING, 0])
                .call::<AddKey>()
                .expect("add_key should accept user payload under the limit");
        expect_errno(
            SyscallArgs::new([
                page + 352,
                page + 64,
                page + 128,
                16,
                KEY_SPEC_THREAD_KEYRING,
                0,
            ])
            .call::<AddKey>(),
            SyscallError::InvalidArguments,
        );
        SyscallArgs::new([
            page + 352,
            page + 448,
            page + 128,
            16,
            KEY_SPEC_THREAD_KEYRING,
            0,
        ])
        .call::<AddKey>()
        .expect("add_key should accept logon payloads with a qualified description");
        SyscallArgs::new([
            page + 384,
            page + 64,
            page + 128,
            16,
            KEY_SPEC_THREAD_KEYRING,
            0,
        ])
        .call::<AddKey>()
        .expect("add_key should accept big_key payloads under the limit");
        SyscallArgs::new([
            page + 416,
            page + 64,
            page + 512,
            ENCRYPTED_KEY_VALID_PAYLOAD_LEN,
            KEY_SPEC_THREAD_KEYRING,
            0,
        ])
        .call::<AddKey>()
        .expect("add_key should accept encrypted keys with hex encoded decrypted data");
        expect_errno(
            SyscallArgs::new([
                page + 416,
                page + 448,
                page + 640,
                ENCRYPTED_KEY_INVALID_PAYLOAD_LEN,
                KEY_SPEC_THREAD_KEYRING,
                0,
            ])
            .call::<AddKey>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([
                page + 96,
                page + 64,
                page + 128,
                1,
                KEY_SPEC_THREAD_KEYRING,
                0,
            ])
            .call::<AddKey>(),
            SyscallError::InvalidArguments,
        );
        let process_keyring = SyscallArgs::new([0, KEY_SPEC_PROCESS_KEYRING, 1, 0, 0, 0])
            .call::<Keyctl>()
            .expect("get_keyring_id should create process keyring")
            as u64;
        let keyring_serial = SyscallArgs::new([page + 96, page + 64, 0, 0, process_keyring, 0])
            .call::<AddKey>()
            .expect("add_key should create keyring") as u64;
        assert_ne!(keyring_serial, process_keyring);
        let key_serial = SyscallArgs::new([page, page + 64, 0, 0, KEY_SPEC_SESSION_KEYRING, 0])
            .call::<AddKey>()
            .expect("add_key should create key") as u64;
        let session_keyring = SyscallArgs::new([0, KEY_SPEC_SESSION_KEYRING, 0, 0, 0, 0])
            .call::<Keyctl>()
            .expect("get_keyring_id should create session keyring")
            as u64;
        assert_eq!(
            SyscallArgs::new([1, 0, 0, 0, 0, 0])
                .call::<Keyctl>()
                .expect("join_session_keyring should return current session keyring")
                as u64,
            session_keyring
        );
        expect_ok(
            SyscallArgs::new([5, session_keyring, 0x1234_5678, 0, 0, 0]).call::<Keyctl>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([15, session_keyring, 1, 0, 0, 0]).call::<Keyctl>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([8, key_serial, session_keyring, 0, 0, 0]).call::<Keyctl>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([3, key_serial, 0, 0, 0, 0]).call::<Keyctl>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([8, key_serial, session_keyring, 0, 0, 0]).call::<Keyctl>(),
            SyscallError::NoKey,
        );
        expect_errno(
            SyscallArgs::new([0, KEY_SPEC_USER_KEYRING, 0, 0, 0, 0]).call::<Keyctl>(),
            SyscallError::NoData,
        );
        let user_keyring = SyscallArgs::new([0, KEY_SPEC_USER_KEYRING, 1, 0, 0, 0])
            .call::<Keyctl>()
            .expect("get_keyring_id should create user keyring");
        let user_session_keyring = SyscallArgs::new([0, KEY_SPEC_USER_SESSION_KEYRING, 1, 0, 0, 0])
            .call::<Keyctl>()
            .expect("get_keyring_id should create user session keyring");
        assert_ne!(user_keyring, user_session_keyring);
        let old_default = SyscallArgs::new([14, (-1i32) as u64, 0, 0, 0, 0])
            .call::<Keyctl>()
            .expect("KEY_REQKEY_DEFL_NO_CHANGE should return the old default");
        assert_eq!(old_default, 0);
        expect_ok(
            SyscallArgs::new([14, 5, 0, 0, 0, 0]).call::<Keyctl>(),
            old_default,
        );
        write_user_cstr(page + 672, b"iovdemo\0");
        write_user_value(page + 704, b"hello ");
        write_user_value(page + 736, b"keyring");
        write_user_value(
            page + 768,
            &[
                TestKeyctlIovec {
                    iov_base: page + 704,
                    iov_len: 6,
                },
                TestKeyctlIovec {
                    iov_base: page + 736,
                    iov_len: 7,
                },
            ],
        );
        let iov_key = 900_000;
        ensure_negative_key_entry(iov_key, "user", "iovdemo");
        expect_ok(
            SyscallArgs::new([
                KEYCTL_INSTANTIATE_IOV,
                iov_key as u64,
                page + 768,
                2,
                KEY_SPEC_THREAD_KEYRING,
                0,
            ])
            .call::<Keyctl>(),
            0,
        );
        assert_eq!(
            SyscallArgs::new([
                KEYCTL_SEARCH,
                KEY_SPEC_THREAD_KEYRING,
                page,
                page + 672,
                0,
                0,
            ])
            .call::<Keyctl>(),
            Ok(iov_key as usize)
        );
        assert_eq!(
            SyscallArgs::new([KEYCTL_READ, iov_key as u64, page + 832, 16, 0, 0]).call::<Keyctl>(),
            Ok(13)
        );
        assert_user_bytes(page + 832, b"hello keyring");
        ensure_negative_key_entry(iov_key + 1, "user", "iovdemo2");
        expect_errno(
            SyscallArgs::new([
                KEYCTL_INSTANTIATE_IOV,
                (iov_key + 1) as u64,
                page + 768,
                super::KEYCTL_IOV_MAX as u64 + 1,
                0,
                0,
            ])
            .call::<Keyctl>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([99, 0, 0, 0, 0, 0]).call::<Keyctl>(),
            SyscallError::InvalidArguments,
        );

        expect_errno(
            SyscallArgs::new([BPF_MAP_CREATE, 0, 0, 0, 0, 0]).call::<Bpf>(),
            SyscallError::BadAddress,
        );
        let mut create_attr = TestBpfMapCreateAttr {
            map_type: BPF_MAP_TYPE_ARRAY,
            key_size: 0,
            value_size: 4,
            max_entries: 2,
            ..Default::default()
        };
        write_user_value(page + 128, &create_attr);
        expect_errno(
            SyscallArgs::new([
                BPF_MAP_CREATE,
                page + 128,
                core::mem::size_of::<TestBpfMapCreateAttr>() as u64,
                0,
                0,
                0,
            ])
            .call::<Bpf>(),
            SyscallError::InvalidArguments,
        );
        create_attr.key_size = 4;
        write_user_value(page + 128, &create_attr);
        let map_fd = expect_fd(
            SyscallArgs::new([
                BPF_MAP_CREATE,
                page + 128,
                core::mem::size_of::<TestBpfMapCreateAttr>() as u64,
                0,
                0,
                0,
            ])
            .call::<Bpf>(),
        );

        write_user_value(page + 256, &0u32);
        write_user_value(page + 264, &0x1122_3344u32);
        let elem_attr = TestBpfMapElemAttr {
            map_fd: map_fd as u32,
            key: page + 256,
            value: page + 264,
            flags: 0,
            ..Default::default()
        };
        write_user_value(page + 272, &elem_attr);
        expect_ok(
            SyscallArgs::new([
                BPF_MAP_UPDATE_ELEM,
                page + 272,
                core::mem::size_of::<TestBpfMapElemAttr>() as u64,
                0,
                0,
                0,
            ])
            .call::<Bpf>(),
            0,
        );
        write_user_value(page + 320, &0u32);
        let lookup_attr = TestBpfMapElemAttr {
            map_fd: map_fd as u32,
            key: page + 256,
            value: page + 320,
            flags: 0,
            ..Default::default()
        };
        write_user_value(page + 328, &lookup_attr);
        expect_ok(
            SyscallArgs::new([
                BPF_MAP_LOOKUP_ELEM,
                page + 328,
                core::mem::size_of::<TestBpfMapElemAttr>() as u64,
                0,
                0,
                0,
            ])
            .call::<Bpf>(),
            0,
        );
        assert_eq!(read_user_value::<u32>(page + 320), 0x1122_3344);

        write_user_value(page + 256, &9u32);
        expect_errno(
            SyscallArgs::new([
                BPF_MAP_LOOKUP_ELEM,
                page + 328,
                core::mem::size_of::<TestBpfMapElemAttr>() as u64,
                0,
                0,
                0,
            ])
            .call::<Bpf>(),
            SyscallError::FileNotFound,
        );

        let bad_prog = TestBpfProgLoadAttr {
            prog_type: 0,
            insn_cnt: 0,
            ..Default::default()
        };
        write_user_value(page + 384, &bad_prog);
        expect_errno(
            SyscallArgs::new([
                BPF_PROG_LOAD,
                page + 384,
                core::mem::size_of::<TestBpfProgLoadAttr>() as u64,
                0,
                0,
                0,
            ])
            .call::<Bpf>(),
            SyscallError::InvalidArguments,
        );
        write_user_value(page + 448, &[0u8; 8]);
        write_user_cstr(page + 512, b"GPL\0");
        let prog = TestBpfProgLoadAttr {
            prog_type: 1,
            insn_cnt: 1,
            insns: page + 448,
            license: page + 512,
            ..Default::default()
        };
        write_user_value(page + 384, &prog);
        let prog_fd = expect_fd(
            SyscallArgs::new([
                BPF_PROG_LOAD,
                page + 384,
                core::mem::size_of::<TestBpfProgLoadAttr>() as u64,
                0,
                0,
                0,
            ])
            .call::<Bpf>(),
        );
        let target_fd = expect_fd(SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Eventfd>());
        let attach_attr = TestBpfProgAttachAttr {
            target_fd: target_fd as u32,
            attach_bpf_fd: prog_fd as u32,
            ..Default::default()
        };
        write_user_value(page + 576, &attach_attr);
        expect_ok(
            SyscallArgs::new([
                BPF_PROG_ATTACH,
                page + 576,
                core::mem::size_of::<TestBpfProgAttachAttr>() as u64,
                0,
                0,
                0,
            ])
            .call::<Bpf>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([
                BPF_PROG_DETACH,
                page + 576,
                core::mem::size_of::<TestBpfProgAttachAttr>() as u64,
                0,
                0,
                0,
            ])
            .call::<Bpf>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([
                99,
                page + 576,
                core::mem::size_of::<TestBpfProgAttachAttr>() as u64,
                0,
                0,
                0,
            ])
            .call::<Bpf>(),
            SyscallError::InvalidArguments,
        );

        close_test_fd(target_fd);
        close_test_fd(prog_fd);
        close_test_fd(map_fd);
    }
}
