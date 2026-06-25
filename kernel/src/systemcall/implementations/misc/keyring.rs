use super::*;

define_syscall!(AddKey, |type_name: String,
                         description: String,
                         payload: *const u8,
                         plen: usize,
                         keyring: i32| {
    if plen != 0 {
        if payload.is_null() {
            return Err(SyscallError::BadAddress);
        }
        let _ = user_safe::read_buffer(payload, plen)?;
    }
    match type_name.as_str() {
        "keyring" if plen != 0 => return Err(SyscallError::InvalidArguments),
        "keyring" => {}
        "user" if plen > KEY_USER_MAX_PAYLOAD => return Err(SyscallError::InvalidArguments),
        "user" => {}
        "logon" | "big_key" => return Err(SyscallError::NoDevice),
        _ => return Err(SyscallError::NoDevice),
    }
    let _ = resolve_keyring(keyring, true)?;
    if type_name == "user" {
        let uid = get_current_process().lock().effective_uid;
        reserve_user_key_quota(uid, &description, plen)?;
    }
    let serial = NEXT_KEY_SERIAL.fetch_add(1, Ordering::Relaxed);
    if type_name == "keyring" {
        ensure_keyring_entry(serial, &description);
    } else {
        ensure_key_entry(serial);
    }
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
            ensure_keyring_entry(current_keyring, "");
            Ok(0)
        }
        Err(_) => Err(SyscallError::NoSyscall),
    }
});

#[cfg(test)]
mod tests {
    use super::super::{
        KEY_USER_DEFAULT_MAX_BYTES, KEY_USER_DEFAULT_MAX_KEYS, proc_key_users_bytes,
        reserve_user_key_quota,
    };

    use crate::systemcall::{
        implementations::{AddKey, Bpf, Eventfd, Keyctl},
        test::{close_test_fd, expect_fd, write_user_cstr},
        test_helpers::{
            SyscallArgs, allocate_user_test_page, expect_errno, expect_ok, read_user_value,
            write_user_value,
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

        let page = allocate_user_test_page();
        write_user_cstr(page, b"user\0");
        write_user_cstr(page + 64, b"demo\0");
        write_user_cstr(page + 96, b"keyring\0");
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
        let _small_user_key =
            SyscallArgs::new([page, page + 64, page + 128, 16, KEY_SPEC_THREAD_KEYRING, 0])
                .call::<AddKey>()
                .expect("add_key should accept user payload under the limit");
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
            SyscallArgs::new([8, key_serial, session_keyring, 0, 0, 0]).call::<Keyctl>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([3, key_serial, 0, 0, 0, 0]).call::<Keyctl>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([8, key_serial, session_keyring, 0, 0, 0]).call::<Keyctl>(),
            SyscallError::InvalidArguments,
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
        expect_errno(
            SyscallArgs::new([99, 0, 0, 0, 0, 0]).call::<Keyctl>(),
            SyscallError::NoSyscall,
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
