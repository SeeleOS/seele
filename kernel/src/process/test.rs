use crate::{
    process::{FdFlags, Process, ProcessExitStatus, wait::ProcessWaitEvent},
    signal::Signal,
};

crate::test!("process newtypes and wait status helpers", || {
    default_process_starts_with_linux_root_credentials_and_limits();
    fd_flags_track_cloexec_bit();
    exit_and_wait_events_preserve_linux_status_encoding();
});

fn default_process_starts_with_linux_root_credentials_and_limits() {
    let process = Process::default();

    assert_eq!(process.real_uid, 0);
    assert_eq!(process.effective_uid, 0);
    assert_eq!(process.real_gid, 0);
    assert_eq!(process.effective_gid, 0);
    assert!(process.dumpable);
    assert!(!process.no_new_privs);
    assert_eq!(process.rlimit_nofile_cur, 1024);
}

fn fd_flags_track_cloexec_bit() {
    assert!(FdFlags::CLOEXEC.contains(FdFlags::CLOEXEC));
    assert!(
        FdFlags::from_bits(FdFlags::CLOEXEC.bits())
            .unwrap()
            .contains(FdFlags::CLOEXEC)
    );
    assert_eq!(FdFlags::from_bits(1 << 8), None);
}

fn exit_and_wait_events_preserve_linux_status_encoding() {
    assert_eq!(ProcessExitStatus::Exited(3).wait_status(), 3 << 8);
    assert_eq!(
        ProcessExitStatus::Signaled(Signal::SIGTERM).waitid_status(),
        Signal::SIGTERM as i32
    );

    let event = ProcessWaitEvent::Stopped {
        status: 0x7f,
        ptrace: true,
    };
    assert_eq!(event.wait_status(), 0x7f);
    assert!(event.is_ptrace());
}
