use crate::{
    define_syscall,
    ipc::sysv_sem::{self, LinuxSembuf, LinuxTimespec, SysvSemCredentials},
    process::manager::get_current_process,
    systemcall::utils::SyscallImpl,
};

define_syscall!(Semget, |key: i32, nsems: i32, semflg: i32| {
    let credentials = current_credentials();
    sysv_sem::semget(&credentials, key, nsems, semflg)
});

define_syscall!(Semop, |semid: i32,
                        sops: *const LinuxSembuf,
                        nsops: usize| {
    let credentials = current_credentials();
    sysv_sem::semop(&credentials, semid, sops, nsops)
});

define_syscall!(
    Semtimedop,
    |semid: i32, sops: *const LinuxSembuf, nsops: usize, timeout: *const LinuxTimespec| {
        let credentials = current_credentials();
        sysv_sem::semtimedop(&credentials, semid, sops, nsops, timeout)
    }
);

define_syscall!(Semctl, |semid: i32, semnum: i32, cmd: i32, arg: usize| {
    let credentials = current_credentials();
    sysv_sem::semctl(&credentials, semid, semnum, cmd, arg)
});

fn current_credentials() -> SysvSemCredentials {
    let process = get_current_process();
    let process = process.lock();
    SysvSemCredentials {
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
        sysv_sem_syscalls,
        "sysv semaphore syscalls follow linux rules",
        sysv_sem_syscalls_follow_linux_rules
    );

    fn sysv_sem_syscalls_follow_linux_rules() {
        const IPC_PRIVATE: u64 = 0;
        const IPC_CREAT: u64 = 0o1000;
        const IPC_EXCL: u64 = 0o2000;
        const IPC_NOWAIT: i16 = 0o4000;
        const IPC_RMID: u64 = 0;
        const IPC_STAT: u64 = 2;
        const GETVAL: u64 = 12;
        const GETALL: u64 = 13;
        const SETVAL: u64 = 16;
        const SETALL: u64 = 17;
        const SEMVMX: usize = 32767;

        assert_linux_layout::<LinuxSembuf>(6, 2);

        let page = allocate_user_test_page();
        expect_errno(
            SyscallArgs::new([IPC_PRIVATE, 0, IPC_CREAT | 0o600, 0, 0, 0]).call::<Semget>(),
            SyscallError::InvalidArguments,
        );

        let key = 0x5345_4d01;
        let semid = expect_ok_fd(
            SyscallArgs::new([key, 2, IPC_CREAT | IPC_EXCL | 0o600, 0, 0, 0]).call::<Semget>(),
        );
        expect_errno(
            SyscallArgs::new([key, 2, IPC_CREAT | IPC_EXCL | 0o600, 0, 0, 0]).call::<Semget>(),
            SyscallError::FileAlreadyExists,
        );
        expect_ok(
            SyscallArgs::new([key, 2, IPC_CREAT | 0o600, 0, 0, 0]).call::<Semget>(),
            semid,
        );
        expect_errno(
            SyscallArgs::new([key, 3, IPC_CREAT | 0o600, 0, 0, 0]).call::<Semget>(),
            SyscallError::InvalidArguments,
        );

        expect_ok(
            SyscallArgs::new([semid as u64, 0, SETVAL, 5, 0, 0]).call::<Semctl>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([semid as u64, 0, GETVAL, 0, 0, 0]).call::<Semctl>(),
            5,
        );
        expect_errno(
            SyscallArgs::new([semid as u64, 0, SETVAL, (SEMVMX + 1) as u64, 0, 0]).call::<Semctl>(),
            SyscallError::RangeError,
        );

        let ops = [
            LinuxSembuf {
                sem_num: 0,
                sem_op: -3,
                sem_flg: 0,
            },
            LinuxSembuf {
                sem_num: 1,
                sem_op: 4,
                sem_flg: 0,
            },
        ];
        write_user_value(page, &ops);
        expect_ok(
            SyscallArgs::new([semid as u64, page, 2, 0, 0, 0]).call::<Semop>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([semid as u64, 0, GETVAL, 0, 0, 0]).call::<Semctl>(),
            2,
        );
        expect_ok(
            SyscallArgs::new([semid as u64, 1, GETVAL, 0, 0, 0]).call::<Semctl>(),
            4,
        );

        let nowait = [LinuxSembuf {
            sem_num: 0,
            sem_op: -3,
            sem_flg: IPC_NOWAIT,
        }];
        write_user_value(page + 32, &nowait);
        expect_errno(
            SyscallArgs::new([semid as u64, page + 32, 1, 0, 0, 0]).call::<Semop>(),
            SyscallError::TryAgain,
        );
        expect_errno(
            SyscallArgs::new([semid as u64, 0, 0, 0, 0, 0]).call::<Semop>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([semid as u64, 0, 1, 0, 0, 0]).call::<Semop>(),
            SyscallError::BadAddress,
        );

        let all_values = [7u16, 8u16];
        write_user_value(page + 64, &all_values);
        expect_ok(
            SyscallArgs::new([semid as u64, 0, SETALL, page + 64, 0, 0]).call::<Semctl>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([semid as u64, 0, GETALL, page + 96, 0, 0]).call::<Semctl>(),
            0,
        );
        assert_eq!(read_user_value::<[u16; 2]>(page + 96), all_values);
        expect_ok(
            SyscallArgs::new([semid as u64, 0, IPC_STAT, page + 128, 0, 0]).call::<Semctl>(),
            0,
        );

        expect_ok(
            SyscallArgs::new([semid as u64, 0, IPC_RMID, 0, 0, 0]).call::<Semctl>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([semid as u64, 0, GETVAL, 0, 0, 0]).call::<Semctl>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([key, 0, 0, 0, 0, 0]).call::<Semget>(),
            SyscallError::FileNotFound,
        );
    }

    fn expect_ok_fd(result: crate::systemcall::utils::SyscallResult) -> usize {
        let value = result.expect("syscall should succeed");
        assert!(value <= i32::MAX as usize);
        value
    }
}
