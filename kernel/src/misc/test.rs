use crate::{
    KERNEL_RELEASE, KERNEL_VERSION,
    misc::fb_object::FramebufferObject,
    misc::{
        auxv::AuxType,
        error::KernelError,
        framebuffer::FRAME_BUFFER,
        framebuffer_ioctl::{
            FB_TYPE_PACKED_PIXELS, FB_VISUAL_TRUECOLOR, FbCmap, FbFixScreeninfo, FbVarScreeninfo,
        },
        signal::{PendingSignalInfo, SI_QUEUE, SigInfo, Signal, Signals},
        time::Time,
        timer::{Sigevent, TimerMode, TimerNotify, TimerNotifyMethod, TimerSpec, TimerState},
        utsname::{DEFAULT_MACHINE, DEFAULT_SYSNAME, UtsName},
    },
    object::{config::ConfigurateRequest, error::ObjectError, traits::Configuratable},
    process::ProcessExitStatus,
    systemcall::utils::SyscallError,
};

crate::test!(
    time_arithmetic,
    "time arithmetic saturates and splits subseconds",
    time_arithmetic_saturates_and_splits_subseconds
);
crate::test!(
    timer_state_conversion,
    "timer spec round trips state and notify method",
    timer_spec_round_trips_state_and_notify_method
);
crate::test!(
    signal_siginfo_conversion,
    "signal masks and siginfo conversion match linux numbers",
    signal_masks_and_siginfo_conversion_match_linux_numbers
);
crate::test!(
    process_exit_wait_encodings,
    "process exit status exports wait encodings",
    process_exit_status_exports_wait_encodings
);
crate::test!(
    misc_layout_and_constants,
    "misc pure layout constants and kernel error mapping stay stable",
    misc_pure_layout_constants_and_kernel_error_mapping_stay_stable
);
crate::test!(
    framebuffer_ioctl_semantics,
    "framebuffer ioctls follow linux rules",
    framebuffer_ioctls_follow_linux_rules
);

fn time_arithmetic_saturates_and_splits_subseconds() {
    let time = Time::from_nanoseconds(1_234_567_890);

    assert_eq!(time.as_seconds(), 1);
    assert_eq!(time.as_milliseconds(), 1234);
    assert_eq!(time.as_microseconds(), 1_234_567);
    assert_eq!(time.subsec_nanoseconds(), 234_567_890);
    assert_eq!(
        Time::from_nanoseconds(5)
            .sub(Time::from_nanoseconds(9))
            .as_nanoseconds(),
        0
    );
    assert_eq!(
        Time::from_nanoseconds(u64::MAX).add_ns(1).as_nanoseconds(),
        u64::MAX
    );
}

fn timer_spec_round_trips_state_and_notify_method() {
    let spec = TimerSpec {
        state_type: TimerMode::Periodic,
        deadline: 10,
        interval: 3,
    };
    let state = TimerState::from(spec);
    assert!(matches!(
        state,
        TimerState::Periodic {
            deadline: Time(10),
            interval: Time(3)
        }
    ));
    assert_eq!(TimerSpec::from(state).deadline, 10);

    assert!(matches!(
        TimerNotifyMethod::from(Sigevent {
            notify_type: TimerNotify::Signal,
            signal: Signal::SIGUSR1
        }),
        TimerNotifyMethod::Signal(Signal::SIGUSR1)
    ));
}

fn signal_masks_and_siginfo_conversion_match_linux_numbers() {
    assert_eq!(Signal::SIGINT.index(), 1);
    assert_eq!(Signal::SIGINT.mask(), Signals::SIGINT.bits());
    assert!(Signal::SIGKILL.is_unblockable());
    assert!(Signal::SIGRTMIN.is_realtime());

    let info = SigInfo::for_process_signal(Signal::SIGTERM, 123, 1000);
    let pending = PendingSignalInfo::from_siginfo(info);
    assert_eq!(pending.si_signo, Signal::SIGTERM as i32);
    assert_eq!(pending.si_pid, 123);
    assert_eq!(pending.si_uid, 1000);

    let queued = PendingSignalInfo {
        si_signo: Signal::SIGUSR1 as i32,
        si_code: SI_QUEUE,
        si_pid: 77,
        si_uid: 88,
        si_value: 0xfeed,
        ..Default::default()
    }
    .to_siginfo();
    assert_eq!(queued.si_signo, Signal::SIGUSR1 as i32);
    assert_eq!(queued.si_code, SI_QUEUE);
    assert_eq!(queued.sender_pid(), 77);
    assert_eq!(queued.sender_uid(), 88);
    assert_eq!(queued.signal_value_ptr(), 0xfeed);
}

fn process_exit_status_exports_wait_encodings() {
    assert_eq!(
        ProcessExitStatus::from_exit_code(0x123).wait_status(),
        0x2300
    );
    assert_eq!(ProcessExitStatus::Exited(7).waitid_status(), 7);
    assert_eq!(
        ProcessExitStatus::from_signal(Signal::SIGKILL).wait_status(),
        Signal::SIGKILL as i32
    );
    assert_eq!(
        ProcessExitStatus::from_signal(Signal::SIGALRM).wait_status(),
        Signal::SIGALRM as i32
    );
    assert_eq!(
        (ProcessExitStatus::Signaled {
            signal: Signal::SIGABRT,
            core_dumped: true,
        })
        .wait_status(),
        Signal::SIGABRT as i32 | 0x80
    );
    assert!(Signal::SIGABRT.default_action_dumps_core());
    assert!(!Signal::SIGTERM.default_action_dumps_core());
}

fn misc_pure_layout_constants_and_kernel_error_mapping_stay_stable() {
    let uts = UtsName::new(
        DEFAULT_SYSNAME,
        KERNEL_RELEASE,
        KERNEL_VERSION,
        DEFAULT_MACHINE,
    );
    assert_eq!(core::mem::size_of::<UtsName>(), 390);
    assert_eq!(&uts.machine[..6], b"x86_64");

    assert_eq!(AuxType::ProgramHeaderTable as u64, 3);
    assert_eq!(AuxType::PageSize as u64, 6);
    assert_eq!(AuxType::Null as u64, 0);

    assert_eq!(
        KernelError::InvalidString.as_syscall_error(),
        SyscallError::InvalidArguments
    );
}

fn framebuffer_ioctls_follow_linux_rules() {
    let framebuffer = FramebufferObject::default();
    let info = FRAME_BUFFER.get().unwrap().lock().fb_info();

    let mut fix = FbFixScreeninfo::default();
    assert_eq!(
        framebuffer
            .configure(ConfigurateRequest::FbGetFixedScreenInfo(&mut fix))
            .unwrap(),
        0
    );
    assert_eq!(fix.type_, FB_TYPE_PACKED_PIXELS);
    assert_eq!(fix.visual, FB_VISUAL_TRUECOLOR);
    assert_eq!(fix.smem_len, info.byte_len as u32);
    assert_eq!(fix.line_length, (info.stride * info.bytes_per_pixel) as u32);
    assert_eq!(&fix.id[..8], &[115, 101, 101, 108, 101, 102, 98, 0]);

    let mut var = FbVarScreeninfo::default();
    assert_eq!(
        framebuffer
            .configure(ConfigurateRequest::FbGetVariableScreenInfo(&mut var))
            .unwrap(),
        0
    );
    assert_eq!(var.xres, info.width as u32);
    assert_eq!(var.yres, info.height as u32);
    assert_eq!(var.xres_virtual, info.stride as u32);
    assert_eq!(var.yres_virtual, info.height as u32);
    assert_eq!(var.bits_per_pixel, (info.bytes_per_pixel * 8) as u32);

    let mut roundtrip = var;
    assert_eq!(
        framebuffer
            .configure(ConfigurateRequest::FbPutVariableScreenInfo(&mut roundtrip))
            .unwrap(),
        0
    );
    assert_eq!(roundtrip.xres, var.xres);

    let mut invalid_var = var;
    invalid_var.grayscale = 1;
    assert!(matches!(
        framebuffer.configure(ConfigurateRequest::FbPutVariableScreenInfo(
            &mut invalid_var
        )),
        Err(ObjectError::InvalidArguments)
    ));

    let mut pan = var;
    assert_eq!(
        framebuffer
            .configure(ConfigurateRequest::FbPanDisplay(&mut pan))
            .unwrap(),
        0
    );
    assert_eq!(pan.xoffset, 0);
    assert_eq!(pan.yoffset, 0);

    pan.xoffset = 1;
    assert!(matches!(
        framebuffer.configure(ConfigurateRequest::FbPanDisplay(&mut pan)),
        Err(ObjectError::InvalidArguments)
    ));

    let mut red = [0xaaaa; 4];
    let mut green = [0xbbbb; 4];
    let mut blue = [0xcccc; 4];
    let mut transp = [0xdddd; 4];
    let mut cmap = FbCmap {
        start: 0,
        len: 4,
        red: red.as_mut_ptr(),
        green: green.as_mut_ptr(),
        blue: blue.as_mut_ptr(),
        transp: transp.as_mut_ptr(),
    };
    assert_eq!(
        framebuffer
            .configure(ConfigurateRequest::FbGetColorMap(&mut cmap))
            .unwrap(),
        0
    );
    assert_eq!(red, [0; 4]);
    assert_eq!(green, [0; 4]);
    assert_eq!(blue, [0; 4]);
    assert_eq!(transp, [0; 4]);

    assert_eq!(
        framebuffer
            .configure(ConfigurateRequest::FbPutColorMap(&mut cmap))
            .unwrap(),
        0
    );
    assert_eq!(
        framebuffer
            .configure(ConfigurateRequest::FbBlank(4))
            .unwrap(),
        0
    );

    assert!(matches!(
        framebuffer.configure(ConfigurateRequest::FbGetFixedScreenInfo(
            core::ptr::null_mut()
        )),
        Err(ObjectError::BadAddress)
    ));
    assert!(matches!(
        framebuffer.configure(ConfigurateRequest::FbGetVariableScreenInfo(
            core::ptr::null_mut()
        )),
        Err(ObjectError::BadAddress)
    ));
    assert!(matches!(
        framebuffer.configure(ConfigurateRequest::FbPutVariableScreenInfo(
            core::ptr::null_mut()
        )),
        Err(ObjectError::BadAddress)
    ));
    assert!(matches!(
        framebuffer.configure(ConfigurateRequest::FbPanDisplay(core::ptr::null_mut())),
        Err(ObjectError::BadAddress)
    ));
    assert!(matches!(
        framebuffer.configure(ConfigurateRequest::FbGetColorMap(core::ptr::null_mut())),
        Err(ObjectError::BadAddress)
    ));
    assert!(matches!(
        framebuffer.configure(ConfigurateRequest::FbPutColorMap(core::ptr::null_mut())),
        Err(ObjectError::BadAddress)
    ));
}
