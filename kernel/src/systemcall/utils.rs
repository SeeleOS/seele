use core::any::type_name;

use alloc::format;

use crate::{
    process::misc::with_current_process, systemcall::numbers::SyscallNumber,
    thread::get_current_thread,
};

pub type SyscallResult<T = usize> = Result<T, SyscallError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(isize)]
pub enum SyscallError {
    PermissionDenied = -1,
    FileNotFound = -2,
    NoProcess = -3,
    Interrupted = -4,
    IOError = -5,
    ArgumentListTooLong = -7,
    ExecFormatError = -8,
    BadFileDescriptor = -9,
    NoChildProcesses = -10,
    TryAgain = -11,
    NoMemory = -12,
    AccessDenied = -13,
    BadAddress = -14,
    BlockDeviceRequired = -15,
    DeviceOrResourceBusy = -16,
    FileAlreadyExists = -17,
    CrossDeviceLink = -18,
    NoDevice = -19,
    NotADirectory = -20,
    IsADirectory = -21,
    InvalidArguments = -22,
    TooManyOpenFilesSystem = -23,
    TooManyOpenFilesProcess = -24,
    InappropriateIoctl = -25,
    FileTooLarge = -27,
    NoSpaceLeft = -28,
    IllegalSeek = -29,
    ReadOnlyFileSystem = -30,
    BrokenPipe = -32,
    NumericalArgumentOutOfDomain = -33,
    RangeError = -34,
    ValueTooLarge = -75,
    PathTooLong = -36,
    ResourceDeadlock = -35,
    NoMessage = -42,
    IdentifierRemoved = -43,
    NoSyscall = -38,
    DirectoryNotEmpty = -39,
    TooManySymbolicLinks = -40,
    NoData = -61,
    NotSocket = -88,
    OperationNotSupported = -95,
    SocketTypeNotSupported = -94,
    ProtocolNotSupported = -93,
    AddressFamilyNotSupported = -97,
    AddressInUse = -98,
    AddressNotAvailable = -99,
    NetworkDown = -100,
    IsConnected = -106,
    NotConnected = -107,
    TimedOut = -110,
    ConnectionRefused = -111,
}

impl SyscallError {
    pub fn as_isize(self) -> isize {
        self as isize
    }
}

pub fn invalid_syscall_flag_error<T>(raw: u64) -> SyscallError {
    crate::s_println!(
        "unsupported syscall flags type={} raw={:#x}",
        type_name::<T>(),
        raw
    );
    SyscallError::InvalidArguments
}

pub fn log_unsupported_syscall_result(syscall_no: isize, args: [u64; 6], err: SyscallError) {
    if !matches!(err, SyscallError::NoSyscall) {
        return;
    }

    with_current_process(|process| {
        let tid = get_current_thread().lock().id.0;
        let comm = process
            .command_line
            .first()
            .and_then(|command| command.rsplit('/').next())
            .unwrap_or("?");
        let syscall_name = SyscallNumber::from_number(syscall_no as usize)
            .map(|number| format!("{number:?}"))
            .unwrap_or_else(|| format!("nr={syscall_no}"));
        crate::s_println!(
            "unsupported syscall comm={} pid={} tid={} name={} ret={:?} a1={:#x} a2={:#x} a3={:#x} a4={:#x} a5={:#x} a6={:#x}",
            comm,
            process.pid.0,
            tid,
            syscall_name,
            err,
            args[0],
            args[1],
            args[2],
            args[3],
            args[4],
            args[5]
        );
    });
}

pub fn log_syscall_trace_enter(_syscall_no: isize, _args: [u64; 6]) {}

pub fn log_syscall_trace_exit(_syscall_no: isize, _args: [u64; 6], _result: isize) {}

impl From<isize> for SyscallError {
    fn from(value: isize) -> Self {
        match value {
            -1 => Self::PermissionDenied,
            -2 => Self::FileNotFound,
            -3 => Self::NoProcess,
            -4 => Self::Interrupted,
            -5 => Self::IOError,
            -7 => Self::ArgumentListTooLong,
            -8 => Self::ExecFormatError,
            -9 => Self::BadFileDescriptor,
            -10 => Self::NoChildProcesses,
            -11 => Self::TryAgain,
            -12 => Self::NoMemory,
            -13 => Self::AccessDenied,
            -14 => Self::BadAddress,
            -16 => Self::DeviceOrResourceBusy,
            -17 => Self::FileAlreadyExists,
            -18 => Self::CrossDeviceLink,
            -19 => Self::NoDevice,
            -20 => Self::NotADirectory,
            -21 => Self::IsADirectory,
            -22 => Self::InvalidArguments,
            -23 => Self::TooManyOpenFilesSystem,
            -24 => Self::TooManyOpenFilesProcess,
            -25 => Self::InappropriateIoctl,
            -27 => Self::FileTooLarge,
            -28 => Self::NoSpaceLeft,
            -29 => Self::IllegalSeek,
            -30 => Self::ReadOnlyFileSystem,
            -32 => Self::BrokenPipe,
            -33 => Self::NumericalArgumentOutOfDomain,
            -34 => Self::RangeError,
            -75 => Self::ValueTooLarge,
            -35 => Self::ResourceDeadlock,
            -36 => Self::PathTooLong,
            -42 => Self::NoMessage,
            -43 => Self::IdentifierRemoved,
            -38 => Self::NoSyscall,
            -39 => Self::DirectoryNotEmpty,
            -40 => Self::TooManySymbolicLinks,
            -61 => Self::NoData,
            -88 => Self::NotSocket,
            -95 => Self::OperationNotSupported,
            -94 => Self::SocketTypeNotSupported,
            -93 => Self::ProtocolNotSupported,
            -97 => Self::AddressFamilyNotSupported,
            -98 => Self::AddressInUse,
            -99 => Self::AddressNotAvailable,
            -100 => Self::NetworkDown,
            -106 => Self::IsConnected,
            -107 => Self::NotConnected,
            -110 => Self::TimedOut,
            -111 => Self::ConnectionRefused,
            _ => Self::IOError,
        }
    }
}

#[macro_export]
macro_rules! register_syscalls {
    ($table: expr, $($no: ident),+ $(,)?) => {
        $(
            $table[$crate::systemcall::numbers::SyscallNumber::$no as usize] = Some(
                <$no as SyscallImpl>::handle_call
                    as fn(u64, u64, u64, u64, u64, u64) -> $crate::systemcall::utils::SyscallResult,
            );
        )*
    };
}

#[macro_export]
macro_rules! define_syscall {
    ($name:ident, |$($arg_name:ident : $arg_type:ty),*| $body:block) => {
        pub struct $name;

        impl SyscallImpl for $name {
            fn handle_call(
                arg1: u64, arg2: u64, arg3: u64,
                arg4: u64, arg5: u64, arg6: u64,
            ) -> $crate::systemcall::utils::SyscallResult {
                let args = [arg1, arg2, arg3, arg4, arg5, arg6];
                let mut _idx = 0;

                $(
                    // Type converting
                    let $arg_name: $arg_type = <$arg_type as $crate::systemcall::arg_types::SyscallArg>::from_u64(args[_idx])?;
                    #[allow(unused_assignments)]
                    { _idx += 1; }
                )*

                $body
            }
        }
    };

    ($name:ident, $body:block) => {
        pub struct $name;

        impl SyscallImpl for $name {
            fn handle_call(
                _arg1: u64, _arg2: u64, _arg3: u64,
                _arg4: u64, _arg5: u64, _arg6: u64,
            ) -> $crate::systemcall::utils::SyscallResult {
                $body
            }
        }
    };
}

pub trait SyscallImpl {
    fn handle_call(
        arg1: u64,
        arg2: u64,
        arg3: u64,
        arg4: u64,
        arg5: u64,
        arg6: u64,
    ) -> SyscallResult;
}
