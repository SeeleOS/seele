use crate::{
    define_syscall,
    ipc::sysv_shm::{self, LinuxShmidDs},
    process::manager::get_current_process,
    systemcall::utils::SyscallImpl,
};

define_syscall!(Shmget, |key: i32, size: usize, shmflg: i32| {
    let process = get_current_process();
    let process = process.lock();
    sysv_shm::shmget(&process, key, size, shmflg)
});

define_syscall!(Shmat, |shmid: i32, shmaddr: *const u8, shmflg: i32| {
    let process = get_current_process();
    let mut process = process.lock();
    sysv_shm::shmat(&mut process, shmid, shmaddr, shmflg)
});

define_syscall!(Shmctl, |shmid: i32, cmd: i32, buf: *mut LinuxShmidDs| {
    let process = get_current_process();
    let process = process.lock();
    let effective_uid = process.effective_uid;
    drop(process);
    sysv_shm::shmctl(effective_uid, shmid, cmd, buf)
});

define_syscall!(Shmdt, |shmaddr: *const u8| {
    let process = get_current_process();
    let mut process = process.lock();
    sysv_shm::shmdt(&mut process, shmaddr)
});

#[cfg(test)]
mod tests {
    use crate::{
        ipc::sysv_shm::LinuxShmidDs,
        process::manager::get_current_process,
        systemcall::{
            implementations::{Shmat, Shmctl, Shmdt, Shmget},
            test::assert_user_bytes,
            test_helpers::{
                SyscallArgs, allocate_user_test_page, expect_errno, expect_ok, read_user_value,
            },
            utils::SyscallError,
        },
    };

    crate::test!(
        sysv_shm_syscalls,
        "sysv shm syscalls follow linux rules",
        sysv_shm_syscalls_follow_linux_rules
    );

    fn sysv_shm_syscalls_follow_linux_rules() {
        const IPC_PRIVATE: u64 = 0;
        const IPC_CREAT: u64 = 0o1000;
        const IPC_EXCL: u64 = 0o2000;
        const IPC_RMID: u64 = 0;
        const IPC_STAT: u64 = 2;
        const SHM_RDONLY: u64 = 0o10000;
        const SHM_RND: u64 = 0o20000;

        let key = 0x55aa_u64;
        let shmid = SyscallArgs::new([key, 4097, IPC_CREAT | IPC_EXCL | 0o600, 0, 0, 0])
            .call::<Shmget>()
            .expect("shmget should create segment") as u64;
        expect_ok(
            SyscallArgs::new([key, 4096, IPC_CREAT, 0, 0, 0]).call::<Shmget>(),
            shmid as usize,
        );
        expect_errno(
            SyscallArgs::new([key, 4096, IPC_CREAT | IPC_EXCL, 0, 0, 0]).call::<Shmget>(),
            SyscallError::FileAlreadyExists,
        );
        expect_errno(
            SyscallArgs::new([key, 8192, IPC_CREAT, 0, 0, 0]).call::<Shmget>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([0xdead, 4096, 0, 0, 0, 0]).call::<Shmget>(),
            SyscallError::FileNotFound,
        );
        expect_errno(
            SyscallArgs::new([IPC_PRIVATE, 0, IPC_CREAT, 0, 0, 0]).call::<Shmget>(),
            SyscallError::InvalidArguments,
        );

        let attach_addr = SyscallArgs::new([shmid, 0, 0, 0, 0, 0])
            .call::<Shmat>()
            .expect("shmat should attach") as u64;
        get_current_process()
            .lock()
            .addrspace
            .write_buffer(attach_addr as *mut u8, b"shm!")
            .unwrap();
        assert_user_bytes(attach_addr, b"shm!");

        let stat_page = allocate_user_test_page();
        expect_ok(
            SyscallArgs::new([shmid, IPC_STAT, stat_page, 0, 0, 0]).call::<Shmctl>(),
            0,
        );
        let stat = read_user_value::<LinuxShmidDs>(stat_page);
        assert_eq!(stat.shm_perm.__ipc_perm_key, key as i32);
        assert_eq!(stat.shm_perm.mode & 0o777, 0o600);
        assert_eq!(stat.shm_segsz, 4097);
        assert_eq!(stat.shm_nattch, 1);

        expect_errno(
            SyscallArgs::new([shmid, IPC_STAT, 0, 0, 0, 0]).call::<Shmctl>(),
            SyscallError::BadAddress,
        );
        expect_errno(
            SyscallArgs::new([shmid, 99, 0, 0, 0, 0]).call::<Shmctl>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([shmid, 0, SHM_RDONLY | 0x8, 0, 0, 0]).call::<Shmat>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([shmid, 123, SHM_RDONLY, 0, 0, 0]).call::<Shmat>(),
            SyscallError::InvalidArguments,
        );

        let rounded_addr = SyscallArgs::new([shmid, 0x12345, SHM_RDONLY | SHM_RND, 0, 0, 0])
            .call::<Shmat>()
            .expect("shmat with SHM_RND should round address") as u64;
        assert_eq!(rounded_addr, 0x12000);
        let readonly_area = get_current_process()
            .lock()
            .addrspace
            .get_area(x86_64::VirtAddr::new(rounded_addr))
            .cloned()
            .expect("readonly shm attach should create area");
        assert!(
            !readonly_area
                .flags
                .contains(x86_64::structures::paging::PageTableFlags::WRITABLE)
        );

        expect_ok(
            SyscallArgs::new([shmid, IPC_RMID, 0, 0, 0, 0]).call::<Shmctl>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([rounded_addr, 0, 0, 0, 0, 0]).call::<Shmdt>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([attach_addr, 0, 0, 0, 0, 0]).call::<Shmdt>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([attach_addr, 0, 0, 0, 0, 0]).call::<Shmdt>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([shmid, IPC_STAT, stat_page, 0, 0, 0]).call::<Shmctl>(),
            SyscallError::InvalidArguments,
        );
    }
}
