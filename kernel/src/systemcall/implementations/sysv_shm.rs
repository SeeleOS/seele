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
    sysv_shm::shmctl(&process, shmid, cmd, buf)
});

define_syscall!(Shmdt, |shmaddr: *const u8| {
    let process = get_current_process();
    let mut process = process.lock();
    sysv_shm::shmdt(&mut process, shmaddr)
});
