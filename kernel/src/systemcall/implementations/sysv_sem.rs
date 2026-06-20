use crate::{
    define_syscall,
    ipc::sysv_sem::{self, LinuxSembuf, LinuxTimespec},
    process::manager::get_current_process,
    systemcall::utils::SyscallImpl,
};

define_syscall!(Semget, |key: i32, nsems: i32, semflg: i32| {
    let process = get_current_process();
    let process = process.lock();
    sysv_sem::semget(&process, key, nsems, semflg)
});

define_syscall!(Semop, |semid: i32,
                        sops: *const LinuxSembuf,
                        nsops: usize| {
    let process = get_current_process();
    let process = process.lock();
    sysv_sem::semop(&process, semid, sops, nsops)
});

define_syscall!(
    Semtimedop,
    |semid: i32, sops: *const LinuxSembuf, nsops: usize, timeout: *const LinuxTimespec| {
        let process = get_current_process();
        let process = process.lock();
        sysv_sem::semtimedop(&process, semid, sops, nsops, timeout)
    }
);

define_syscall!(Semctl, |semid: i32, semnum: i32, cmd: i32, arg: usize| {
    let process = get_current_process();
    let process = process.lock();
    sysv_sem::semctl(&process, semid, semnum, cmd, arg)
});
