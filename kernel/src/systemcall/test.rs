use crate::{
    memory::protection::Protection,
    misc::timer::ClockId,
    signal::{Signal, Signals},
    systemcall::{
        arg_types::SyscallArg,
        implementations::{OpenFlags, PollEvents},
        numbers::SyscallNumber,
        table::SYSCALL_TABLE,
        utils::SyscallError,
    },
};

crate::test!(
    syscall_number_lookup,
    "syscall number lookup matches x86_64 abi values",
    syscall_number_lookup_matches_x86_64_abi_values
);
crate::test!(
    syscall_table_coverage,
    "syscall table contains registered and rejects unknown numbers",
    syscall_table_contains_registered_and_rejects_unknown_numbers
);
crate::test!(
    typed_syscall_arg_conversion,
    "typed syscall args convert flags and enums at boundary",
    typed_syscall_args_convert_flags_and_enums_at_boundary
);

fn syscall_number_lookup_matches_x86_64_abi_values() {
    assert_eq!(SyscallNumber::from_number(0), Some(SyscallNumber::Read));
    assert_eq!(SyscallNumber::from_number(1), Some(SyscallNumber::Write));
    assert_eq!(SyscallNumber::from_number(257), Some(SyscallNumber::OpenAt));
    assert_eq!(SyscallNumber::from_number(999), None);
}

fn syscall_table_contains_registered_and_rejects_unknown_numbers() {
    assert!(SYSCALL_TABLE[SyscallNumber::Read as usize].is_some());
    assert!(SYSCALL_TABLE[SyscallNumber::OpenAt as usize].is_some());
    assert!(SYSCALL_TABLE[999].is_none());
}

fn typed_syscall_args_convert_flags_and_enums_at_boundary() {
    assert_eq!(<u32 as SyscallArg>::from_u64(u64::MAX).unwrap(), u32::MAX);
    assert!(<bool as SyscallArg>::from_u64(2).unwrap());
    assert_eq!(
        <Signal as SyscallArg>::from_u64(Signal::SIGTERM as u64).unwrap(),
        Signal::SIGTERM
    );
    assert!(matches!(
        <Signal as SyscallArg>::from_u64(0),
        Err(SyscallError::InvalidArguments)
    ));
    assert_eq!(
        <ClockId as SyscallArg>::from_u64(ClockId::Realtime as u64).unwrap(),
        ClockId::Realtime
    );
    assert_eq!(
        <Protection as SyscallArg>::from_u64((Protection::READ | Protection::WRITE).bits())
            .unwrap()
            .bits(),
        (Protection::READ | Protection::WRITE).bits()
    );
    assert_eq!(
        <Signals as SyscallArg>::from_u64(Signal::SIGINT.mask())
            .unwrap()
            .bits(),
        Signals::SIGINT.bits()
    );
    assert_eq!(
        <OpenFlags as SyscallArg>::from_u64(
            (OpenFlags::CLOEXEC | OpenFlags::NONBLOCK).bits() as u64
        )
        .unwrap()
        .bits(),
        (OpenFlags::CLOEXEC | OpenFlags::NONBLOCK).bits()
    );
    assert!(<PollEvents as SyscallArg>::from_u64(PollEvents::POLLIN.bits() as u64).is_ok());
}
