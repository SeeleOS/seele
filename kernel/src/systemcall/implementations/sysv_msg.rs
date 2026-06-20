use crate::{
    define_syscall,
    ipc::sysv_msg::{self, LinuxMsqidDs, SysvMsgCredentials},
    process::manager::get_current_process,
    systemcall::utils::SyscallImpl,
};

define_syscall!(Msgget, |key: i32, msgflg: i32| {
    let credentials = current_credentials();
    sysv_msg::msgget(&credentials, key, msgflg)
});

define_syscall!(Msgsnd, |msqid: i32,
                         msgp: *const u8,
                         msgsz: usize,
                         msgflg: i32| {
    let credentials = current_credentials();
    sysv_msg::msgsnd(&credentials, msqid, msgp, msgsz, msgflg)
});

define_syscall!(Msgrcv, |msqid: i32,
                         msgp: *mut u8,
                         msgsz: usize,
                         msgtyp: i64,
                         msgflg: i32| {
    let credentials = current_credentials();
    sysv_msg::msgrcv(&credentials, msqid, msgp, msgsz, msgtyp, msgflg)
});

define_syscall!(Msgctl, |msqid: i32, cmd: i32, buf: *mut LinuxMsqidDs| {
    let credentials = current_credentials();
    sysv_msg::msgctl(&credentials, msqid, cmd, buf)
});

fn current_credentials() -> SysvMsgCredentials {
    let process = get_current_process();
    let process = process.lock();
    SysvMsgCredentials {
        pid: process.pid.0 as i32,
        effective_uid: process.effective_uid,
        effective_gid: process.effective_gid,
        supplementary_groups: process.supplementary_groups.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systemcall::{
        test_helpers::{
            SyscallArgs, allocate_user_test_page, assert_linux_layout, expect_errno, expect_ok,
            read_user_value, write_user_value,
        },
        utils::SyscallError,
    };

    crate::test!(
        sysv_msg_syscalls,
        "sysv message queue syscalls follow linux rules",
        sysv_msg_syscalls_follow_linux_rules
    );

    #[repr(C)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Message<const N: usize> {
        ty: i64,
        data: [u8; N],
    }

    fn sysv_msg_syscalls_follow_linux_rules() {
        const IPC_CREAT: u64 = 0o1000;
        const IPC_EXCL: u64 = 0o2000;
        const IPC_NOWAIT: u64 = 0o4000;
        const MSG_NOERROR: u64 = 0o10000;
        const IPC_RMID: u64 = 0;
        const IPC_STAT: u64 = 2;

        assert_linux_layout::<LinuxMsqidDs>(120, 8);

        let key = 0x4d53_4701;
        expect_errno(
            SyscallArgs::new([key, 0, 0, 0, 0, 0]).call::<Msgget>(),
            SyscallError::FileNotFound,
        );
        let msqid = expect_ok_fd(
            SyscallArgs::new([key, IPC_CREAT | IPC_EXCL | 0o600, 0, 0, 0, 0]).call::<Msgget>(),
        );
        expect_errno(
            SyscallArgs::new([key, IPC_CREAT | IPC_EXCL | 0o600, 0, 0, 0, 0]).call::<Msgget>(),
            SyscallError::FileAlreadyExists,
        );
        expect_ok(
            SyscallArgs::new([key, IPC_CREAT | 0o600, 0, 0, 0, 0]).call::<Msgget>(),
            msqid,
        );

        let page = allocate_user_test_page();
        let first = Message {
            ty: 2,
            data: *b"abc",
        };
        let second = Message {
            ty: 1,
            data: *b"wxyz",
        };
        write_user_value(page, &first);
        write_user_value(page + 32, &second);
        expect_ok(
            SyscallArgs::new([msqid as u64, page, 3, 0, 0, 0]).call::<Msgsnd>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([msqid as u64, page + 32, 4, 0, 0, 0]).call::<Msgsnd>(),
            0,
        );

        expect_ok(
            SyscallArgs::new([msqid as u64, IPC_STAT, page + 96, 0, 0, 0]).call::<Msgctl>(),
            0,
        );
        let stat = read_user_value::<LinuxMsqidDs>(page + 96);
        assert_eq!(stat.msg_perm.__ipc_perm_key, key as i32);
        assert_eq!(stat.msg_perm.mode & 0o777, 0o600);
        assert_eq!(stat.msg_qnum, 2);
        assert_eq!(stat.__msg_cbytes, 7);

        expect_errno(
            SyscallArgs::new([msqid as u64, page + 160, 2, 0, IPC_NOWAIT, 0]).call::<Msgrcv>(),
            SyscallError::InvalidArguments,
        );
        expect_ok(
            SyscallArgs::new([msqid as u64, page + 160, 2, 0, IPC_NOWAIT | MSG_NOERROR, 0])
                .call::<Msgrcv>(),
            2,
        );
        let truncated = read_user_value::<Message<2>>(page + 160);
        assert_eq!(truncated.ty, 2);
        assert_eq!(truncated.data, *b"ab");

        expect_ok(
            SyscallArgs::new([msqid as u64, page + 192, 4, -2i64 as u64, IPC_NOWAIT, 0])
                .call::<Msgrcv>(),
            4,
        );
        assert_eq!(read_user_value::<Message<4>>(page + 192), second);
        expect_errno(
            SyscallArgs::new([msqid as u64, page + 224, 4, 0, IPC_NOWAIT, 0]).call::<Msgrcv>(),
            SyscallError::NoMessage,
        );
        expect_errno(
            SyscallArgs::new([msqid as u64, 0, 1, 0, IPC_NOWAIT, 0]).call::<Msgsnd>(),
            SyscallError::BadAddress,
        );
        expect_errno(
            SyscallArgs::new([msqid as u64, 0, 1, 0, IPC_NOWAIT, 0]).call::<Msgrcv>(),
            SyscallError::BadAddress,
        );

        expect_ok(
            SyscallArgs::new([msqid as u64, IPC_RMID, 0, 0, 0, 0]).call::<Msgctl>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([msqid as u64, IPC_STAT, page + 96, 0, 0, 0]).call::<Msgctl>(),
            SyscallError::InvalidArguments,
        );
    }

    fn expect_ok_fd(result: crate::systemcall::utils::SyscallResult) -> usize {
        let value = result.expect("syscall should succeed");
        assert!(value <= i32::MAX as usize);
        value
    }
}
