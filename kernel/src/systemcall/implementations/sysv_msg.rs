use crate::{
    define_syscall,
    ipc::sysv_msg::{self, LinuxMsqidDs},
    process::manager::get_current_process,
    systemcall::utils::SyscallImpl,
};

define_syscall!(Msgget, |key: i32, msgflg: i32| {
    let process = get_current_process();
    let process = process.lock();
    sysv_msg::msgget(&process, key, msgflg)
});

define_syscall!(Msgsnd, |msqid: i32,
                         msgp: *const u8,
                         msgsz: usize,
                         msgflg: i32| {
    let process = get_current_process();
    let process = process.lock();
    sysv_msg::msgsnd(&process, msqid, msgp, msgsz, msgflg)
});

define_syscall!(Msgrcv, |msqid: i32,
                         msgp: *mut u8,
                         msgsz: usize,
                         msgtyp: i64,
                         msgflg: i32| {
    let process = get_current_process();
    let process = process.lock();
    sysv_msg::msgrcv(&process, msqid, msgp, msgsz, msgtyp, msgflg)
});

define_syscall!(Msgctl, |msqid: i32, cmd: i32, buf: *mut LinuxMsqidDs| {
    let process = get_current_process();
    let process = process.lock();
    sysv_msg::msgctl(&process, msqid, cmd, buf)
});
