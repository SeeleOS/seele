pub type SyscallResult<T = usize> = Result<T, SyscallError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(isize)]
pub enum SyscallError {
    PermissionDenied = -1,
    FileNotFound = -2,
    NoProcess = -3,
    Interrupted = -4,
    IOError = -5,
    BadFileDescriptor = -9,
    TryAgain = -11,
    NoMemory = -12,
    AccessDenied = -13,
    BadAddress = -14,
    DeviceOrResourceBusy = -16,
    FileAlreadyExists = -17,
    NotADirectory = -20,
    IsADirectory = -21,
    InvalidArguments = -22,
    TooManyOpenFilesSystem = -23,
    TooManyOpenFilesProcess = -24,
    InappropriateIoctl = -25,
    FileTooLarge = -27,
    NoSpaceLeft = -28,
    ReadOnlyFileSystem = -30,
    BrokenPipe = -32,
    PathTooLong = -36,
    NoSyscall = -38,
    DirectoryNotEmpty = -39,
    TooManySymbolicLinks = -40,
    NoData = -61,
    OperationNotSupported = -95,
    ProtocolNotSupported = -93,
    AddressFamilyNotSupported = -97,
    AddressInUse = -98,
    IsConnected = -106,
    ConnectionRefused = -111,
    Other = -256,
}

impl SyscallError {
    pub fn as_isize(self) -> isize {
        self as isize
    }

    pub fn other(_message: &str) -> SyscallError {
        Self::Other
    }
}

impl From<isize> for SyscallError {
    fn from(value: isize) -> Self {
        match value {
            -1 => Self::PermissionDenied,
            -2 => Self::FileNotFound,
            -3 => Self::NoProcess,
            -4 => Self::Interrupted,
            -5 => Self::IOError,
            -9 => Self::BadFileDescriptor,
            -11 => Self::TryAgain,
            -12 => Self::NoMemory,
            -13 => Self::AccessDenied,
            -14 => Self::BadAddress,
            -16 => Self::DeviceOrResourceBusy,
            -17 => Self::FileAlreadyExists,
            -20 => Self::NotADirectory,
            -21 => Self::IsADirectory,
            -22 => Self::InvalidArguments,
            -23 => Self::TooManyOpenFilesSystem,
            -24 => Self::TooManyOpenFilesProcess,
            -25 => Self::InappropriateIoctl,
            -27 => Self::FileTooLarge,
            -28 => Self::NoSpaceLeft,
            -30 => Self::ReadOnlyFileSystem,
            -32 => Self::BrokenPipe,
            -36 => Self::PathTooLong,
            -38 => Self::NoSyscall,
            -39 => Self::DirectoryNotEmpty,
            -40 => Self::TooManySymbolicLinks,
            -61 => Self::NoData,
            -95 => Self::OperationNotSupported,
            -93 => Self::ProtocolNotSupported,
            -97 => Self::AddressFamilyNotSupported,
            -98 => Self::AddressInUse,
            -106 => Self::IsConnected,
            -111 => Self::ConnectionRefused,
            _ => Self::Other,
        }
    }
}

#[macro_export]
macro_rules! register_syscalls {
    // 注意这里的 $( ... ),* 模式
    ($table: expr, $($no: ident),*) => {
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
