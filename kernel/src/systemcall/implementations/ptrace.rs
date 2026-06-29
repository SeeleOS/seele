use crate::{
    define_syscall,
    memory::user_safe,
    misc::{
        signal::{SigInfo, Signal},
        snapshot::Snapshot,
    },
    process::{
        manager::get_current_process,
        misc::ProcessID,
        ptrace::{
            PtraceResumeMode, PtraceStopKind, get_traced_process, resume, seize, set_options,
            traceme_current,
        },
    },
    systemcall::utils::{SyscallError, SyscallImpl},
};
use core::mem;

const PTRACE_TRACEME: u64 = 0;
const PTRACE_PEEKTEXT: u64 = 1;
const PTRACE_PEEKDATA: u64 = 2;
const PTRACE_CONT: u64 = 7;
const PTRACE_GETREGS: u64 = 12;
const PTRACE_SYSCALL: u64 = 24;
const PTRACE_SETOPTIONS: u64 = 0x4200;
const PTRACE_GETEVENTMSG: u64 = 0x4201;
const PTRACE_GETSIGINFO: u64 = 0x4202;
const PTRACE_GETREGSET: u64 = 0x4204;
const PTRACE_SEIZE: u64 = 0x4206;
const PTRACE_GET_SYSCALL_INFO: u64 = 0x420e;
const NT_PRSTATUS: u64 = 1;
const AUDIT_ARCH_X86_64: u32 = 0xC000_003E;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LinuxPtraceSyscallInfo {
    op: u8,
    _pad: [u8; 3],
    arch: u32,
    instruction_pointer: u64,
    stack_pointer: u64,
    payload: LinuxPtraceSyscallInfoPayload,
}

#[repr(C)]
#[derive(Clone, Copy)]
union LinuxPtraceSyscallInfoPayload {
    entry: LinuxPtraceSyscallInfoEntry,
    exit: LinuxPtraceSyscallInfoExit,
    bytes: [u8; 64],
}

impl Default for LinuxPtraceSyscallInfoPayload {
    fn default() -> Self {
        Self { bytes: [0; 64] }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LinuxPtraceSyscallInfoEntry {
    nr: u64,
    args: [u64; 6],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LinuxPtraceSyscallInfoExit {
    rval: i64,
    is_error: u8,
    _pad: [u8; 7],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct LinuxIovec {
    iov_base: *mut u8,
    iov_len: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LinuxUserRegsStruct {
    r15: u64,
    r14: u64,
    r13: u64,
    r12: u64,
    rbp: u64,
    rbx: u64,
    r11: u64,
    r10: u64,
    r9: u64,
    r8: u64,
    rax: u64,
    rcx: u64,
    rdx: u64,
    rsi: u64,
    rdi: u64,
    orig_rax: u64,
    rip: u64,
    cs: u64,
    eflags: u64,
    rsp: u64,
    ss: u64,
    fs_base: u64,
    gs_base: u64,
    ds: u64,
    es: u64,
    fs: u64,
    gs: u64,
}

fn tracer_pid() -> ProcessID {
    get_current_process().lock().pid
}

fn traced_regs(process: &crate::process::ProcessRef) -> Result<LinuxUserRegsStruct, SyscallError> {
    let thread_ref = {
        let process = process.lock();
        process
            .threads
            .iter()
            .find_map(|thread| thread.upgrade())
            .ok_or(SyscallError::NoProcess)?
    };
    let thread = thread_ref.lock();
    let snapshot = thread.last_user_snapshot;
    let Snapshot {
        r15,
        r14,
        r13,
        r12,
        r11,
        r10,
        r9,
        r8,
        rdi,
        rsi,
        rbp,
        rbx,
        rdx,
        rcx,
        rax,
        rip,
        cs,
        rflags,
        rsp,
        ss,
    } = snapshot;
    Ok(LinuxUserRegsStruct {
        r15,
        r14,
        r13,
        r12,
        rbp,
        rbx,
        r11,
        r10,
        r9,
        r8,
        rax: rax as u64,
        rcx,
        rdx,
        rsi,
        rdi,
        orig_rax: thread.last_syscall_no,
        rip,
        cs,
        eflags: rflags,
        rsp,
        ss,
        fs_base: thread.last_user_fs_base,
        gs_base: 0,
        ds: 0,
        es: 0,
        fs: 0,
        gs: 0,
    })
}

fn peek_target_word(pid: i32, addr: u64) -> Result<usize, SyscallError> {
    let process = get_traced_process(pid, tracer_pid())?;
    let word = process.lock().addrspace.read(addr as *const u64)?;
    Ok(word as usize)
}

fn write_traced_regs(pid: i32, out_ptr: *mut u8, out_len: usize) -> Result<(), SyscallError> {
    let process = get_traced_process(pid, tracer_pid())?;
    let regs = traced_regs(&process)?;
    let regs_bytes = unsafe {
        core::slice::from_raw_parts(
            (&regs as *const LinuxUserRegsStruct).cast::<u8>(),
            mem::size_of::<LinuxUserRegsStruct>(),
        )
    };
    let copy_len = out_len.min(regs_bytes.len());
    if copy_len > 0 {
        user_safe::write(out_ptr, &regs_bytes[..copy_len])?;
    }
    Ok(())
}

fn write_traced_regset(pid: i32, addr: u64, data: u64) -> Result<usize, SyscallError> {
    if addr != NT_PRSTATUS {
        return Err(SyscallError::InvalidArguments);
    }

    let mut iov: LinuxIovec = user_safe::read(data as *const LinuxIovec)?;
    let process = get_traced_process(pid, tracer_pid())?;
    let regs = traced_regs(&process)?;
    let regs_bytes = unsafe {
        core::slice::from_raw_parts(
            (&regs as *const LinuxUserRegsStruct).cast::<u8>(),
            mem::size_of::<LinuxUserRegsStruct>(),
        )
    };
    let copy_len = iov.iov_len.min(regs_bytes.len());
    if copy_len > 0 {
        user_safe::write(iov.iov_base, &regs_bytes[..copy_len])?;
    }
    iov.iov_len = regs_bytes.len();
    user_safe::write(data as *mut LinuxIovec, &iov)?;
    Ok(0)
}

fn write_siginfo(pid: i32, data: u64) -> Result<usize, SyscallError> {
    let process = get_traced_process(pid, tracer_pid())?;
    let (status, uid) = {
        let process = process.lock();
        let status = match process.wait_event {
            Some(crate::process::wait::ProcessWaitEvent::Stopped { status, .. }) => status,
            None if process.ptrace.last_stop_status != 0 => process.ptrace.last_stop_status,
            None => return Err(SyscallError::TryAgain),
        };
        (status, process.real_uid)
    };
    let raw_signal = (status >> 8) & 0xff;
    let signal = Signal::try_from((raw_signal & 0x7f) as u64).unwrap_or(Signal::SIGTRAP);
    let siginfo = SigInfo::for_process_signal(signal, pid, uid);
    user_safe::write(data as *mut SigInfo, &siginfo)?;
    Ok(0)
}

fn write_syscall_info(pid: i32, size: u64, data: u64) -> Result<usize, SyscallError> {
    let process = get_traced_process(pid, tracer_pid())?;
    let thread_ref = {
        let process = process.lock();
        process
            .threads
            .iter()
            .find_map(|thread| thread.upgrade())
            .ok_or(SyscallError::NoProcess)?
    };
    let thread = thread_ref.lock();
    let snapshot = thread.last_user_snapshot;
    let (op, payload) = {
        let process = process.lock();
        match process.ptrace.last_stop_kind {
            PtraceStopKind::SyscallEnter => (
                1,
                LinuxPtraceSyscallInfoPayload {
                    entry: LinuxPtraceSyscallInfoEntry {
                        nr: thread.last_syscall_no,
                        args: [
                            snapshot.rdi,
                            snapshot.rsi,
                            snapshot.rdx,
                            snapshot.r10,
                            snapshot.r8,
                            snapshot.r9,
                        ],
                    },
                },
            ),
            PtraceStopKind::SyscallExit => (
                2,
                LinuxPtraceSyscallInfoPayload {
                    exit: LinuxPtraceSyscallInfoExit {
                        rval: snapshot.rax as i64,
                        is_error: (((snapshot.rax as i64) < 0) && ((snapshot.rax as i64) >= -4095))
                            as u8,
                        _pad: [0; 7],
                    },
                },
            ),
            _ => (0, LinuxPtraceSyscallInfoPayload::default()),
        }
    };
    let info = LinuxPtraceSyscallInfo {
        op,
        _pad: [0; 3],
        arch: AUDIT_ARCH_X86_64,
        instruction_pointer: snapshot.rip,
        stack_pointer: snapshot.rsp,
        payload,
    };
    let info_bytes = unsafe {
        core::slice::from_raw_parts(
            (&info as *const LinuxPtraceSyscallInfo).cast::<u8>(),
            mem::size_of::<LinuxPtraceSyscallInfo>(),
        )
    };
    let copy_len = usize::try_from(size)
        .map_err(|_| SyscallError::InvalidArguments)?
        .min(info_bytes.len());
    if copy_len > 0 {
        user_safe::write(data as *mut u8, &info_bytes[..copy_len])?;
    }
    Ok(copy_len)
}

define_syscall!(Ptrace, |request: u64, pid: i32, addr: u64, data: u64| {
    match request {
        PTRACE_TRACEME => {
            traceme_current()?;
            Ok(0)
        }
        PTRACE_PEEKTEXT | PTRACE_PEEKDATA => peek_target_word(pid, addr),
        PTRACE_CONT => {
            if data != 0 {
                return Err(SyscallError::InvalidArguments);
            }
            let process = get_traced_process(pid, tracer_pid())?;
            resume(&process, tracer_pid(), PtraceResumeMode::Continue)?;
            Ok(0)
        }
        PTRACE_SYSCALL => {
            if data != 0 {
                return Err(SyscallError::InvalidArguments);
            }
            let process = get_traced_process(pid, tracer_pid())?;
            resume(&process, tracer_pid(), PtraceResumeMode::Syscall)?;
            Ok(0)
        }
        PTRACE_GETREGS => {
            write_traced_regs(pid, data as *mut u8, mem::size_of::<LinuxUserRegsStruct>())?;
            Ok(0)
        }
        PTRACE_SETOPTIONS => {
            let process = get_traced_process(pid, tracer_pid())?;
            set_options(&process, tracer_pid(), data as u32)?;
            Ok(0)
        }
        PTRACE_SEIZE => {
            let process = if pid <= 0 {
                return Err(SyscallError::InvalidArguments);
            } else {
                crate::process::misc::get_process_with_pid(crate::process::misc::ProcessID(
                    pid as u64,
                ))?
            };
            seize(&process, tracer_pid(), data as u32)?;
            Ok(0)
        }
        PTRACE_GETEVENTMSG => {
            user_safe::write(data as *mut usize, &0usize)?;
            Ok(0)
        }
        PTRACE_GETSIGINFO => write_siginfo(pid, data),
        PTRACE_GETREGSET => write_traced_regset(pid, addr, data),
        PTRACE_GET_SYSCALL_INFO => write_syscall_info(pid, addr, data),
        _ => Err(SyscallError::InvalidArguments),
    }
});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        process::{Process, manager::MANAGER, misc::ProcessID},
        signal::{SigInfo, Signal},
        systemcall::{
            implementations::Ptrace,
            numbers::SyscallNumber,
            test::expect_fd,
            test_helpers::{
                SyscallArgs, allocate_user_test_page, expect_errno, expect_ok, read_user_value,
                write_user_value,
            },
            utils::SyscallError,
        },
    };

    crate::test!(
        ptrace_syscalls,
        "ptrace syscalls follow linux rules",
        ptrace_syscalls_follow_linux_rules
    );

    fn ptrace_syscalls_follow_linux_rules() {
        const PTRACE_TRACEME: u64 = 0;
        const PTRACE_SETOPTIONS: u64 = 0x4200;
        const PTRACE_GETEVENTMSG: u64 = 0x4201;
        const PTRACE_GETSIGINFO: u64 = 0x4202;
        const PTRACE_GETREGSET: u64 = 0x4204;
        const PTRACE_SEIZE: u64 = 0x4206;
        const PTRACE_GET_SYSCALL_INFO: u64 = 0x420e;
        const PTRACE_CONT: u64 = 7;
        const NT_PRSTATUS: u64 = 1;

        #[repr(C)]
        #[derive(Clone, Copy, Default)]
        struct TestLinuxIovec {
            iov_base: *mut u8,
            iov_len: usize,
        }

        let current = get_current_process();
        let tracer_pid = current.lock().pid;
        let original_parent = current.lock().parent.clone();
        let original_ptrace = current.lock().ptrace;
        let parent = Process::empty();
        parent.lock().pid = ProcessID::new();
        current.lock().parent = Some(parent.clone());

        expect_ok(
            SyscallArgs::new([PTRACE_TRACEME, 0, 0, 0, 0, 0]).call::<Ptrace>(),
            0,
        );
        assert_eq!(current.lock().ptrace.tracer, Some(parent.lock().pid));
        expect_errno(
            SyscallArgs::new([PTRACE_TRACEME, 0, 0, 0, 0, 0]).call::<Ptrace>(),
            SyscallError::PermissionDenied,
        );

        let traced = Process::empty();
        let traced_pid = {
            let mut traced_locked = traced.lock();
            traced_locked.pid = ProcessID::new();
            traced_locked.parent = Some(current.clone());
            traced_locked.ptrace.tracer = Some(tracer_pid);
            traced_locked.ptrace.resume_mode = crate::process::ptrace::PtraceResumeMode::Stopped;
            traced_locked.ptrace.last_stop_status = ((Signal::SIGTRAP as i32) << 8) | 0x7f;
            traced_locked.wait_event = Some(crate::process::wait::ProcessWaitEvent::Stopped {
                status: (((Signal::SIGTRAP as i32) << 8) | 0x7f),
                ptrace: true,
            });
            traced_locked.pid.0
        };
        let traced_thread = crate::thread::thread::Thread::empty();
        {
            let mut thread = traced_thread.lock();
            thread.set_parent(traced.clone());
            thread.last_syscall_no = SyscallNumber::Read as u64;
            thread.last_user_snapshot.rax = -38;
            thread.last_user_snapshot.rip = 0x1234;
            thread.last_user_snapshot.rsp = 0x5678;
        }
        traced
            .lock()
            .threads
            .push(alloc::sync::Arc::downgrade(&traced_thread));
        MANAGER
            .lock()
            .processes
            .insert(ProcessID(traced_pid), traced.clone());
        crate::thread::THREAD_MANAGER
            .get()
            .unwrap()
            .lock()
            .threads
            .insert(traced_thread.lock().id, traced_thread.clone());

        let page = allocate_user_test_page();
        expect_ok(
            SyscallArgs::new([PTRACE_SETOPTIONS, traced_pid, 0, 1, 0, 0]).call::<Ptrace>(),
            0,
        );
        assert_eq!(traced.lock().ptrace.options, 1);

        expect_ok(
            SyscallArgs::new([PTRACE_GETEVENTMSG, traced_pid, 0, page, 0, 0]).call::<Ptrace>(),
            0,
        );
        assert_eq!(read_user_value::<usize>(page), 0);

        expect_ok(
            SyscallArgs::new([PTRACE_GETSIGINFO, traced_pid, 0, page + 64, 0, 0]).call::<Ptrace>(),
            0,
        );
        let siginfo = read_user_value::<SigInfo>(page + 64);
        assert_eq!(siginfo.si_signo, Signal::SIGTRAP as i32);

        let iov = TestLinuxIovec {
            iov_base: (page + 256) as *mut u8,
            iov_len: 216,
        };
        write_user_value(page + 192, &iov);
        expect_ok(
            SyscallArgs::new([PTRACE_GETREGSET, traced_pid, NT_PRSTATUS, page + 192, 0, 0])
                .call::<Ptrace>(),
            0,
        );
        assert_eq!(read_user_value::<TestLinuxIovec>(page + 192).iov_len, 216);

        traced.lock().ptrace.last_stop_kind = crate::process::ptrace::PtraceStopKind::SyscallExit;
        let copied = expect_fd(Ok(SyscallArgs::new([
            PTRACE_GET_SYSCALL_INFO,
            traced_pid,
            88,
            page + 512,
            0,
            0,
        ])
        .call::<Ptrace>()
        .expect("ptrace get syscall info should succeed")));
        assert_eq!(copied, 88);

        expect_ok(
            SyscallArgs::new([PTRACE_CONT, traced_pid, 0, 0, 0, 0]).call::<Ptrace>(),
            0,
        );
        assert_eq!(
            traced.lock().ptrace.resume_mode,
            crate::process::ptrace::PtraceResumeMode::Continue
        );

        let seize_target = Process::empty();
        let seize_pid = {
            let mut process = seize_target.lock();
            process.pid = ProcessID::new();
            process.pid.0 as i32
        };
        MANAGER
            .lock()
            .processes
            .insert(ProcessID(seize_pid as u64), seize_target.clone());
        expect_ok(
            SyscallArgs::new([PTRACE_SEIZE, seize_pid as u64, 0, 0, 0, 0]).call::<Ptrace>(),
            0,
        );
        assert_eq!(seize_target.lock().ptrace.tracer, Some(tracer_pid));
        expect_errno(
            SyscallArgs::new([PTRACE_CONT, seize_pid as u64, 0, 1, 0, 0]).call::<Ptrace>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([PTRACE_GETREGSET, traced_pid, 2, page + 192, 0, 0]).call::<Ptrace>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([9999, traced_pid, 0, 0, 0, 0]).call::<Ptrace>(),
            SyscallError::InvalidArguments,
        );

        current.lock().parent = original_parent;
        current.lock().ptrace = original_ptrace;
        MANAGER.lock().processes.remove(&ProcessID(traced_pid));
        MANAGER
            .lock()
            .processes
            .remove(&ProcessID(seize_pid as u64));
        crate::thread::THREAD_MANAGER
            .get()
            .unwrap()
            .lock()
            .threads
            .remove(&traced_thread.lock().id);
    }
}
