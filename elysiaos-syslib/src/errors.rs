use num_derive::FromPrimitive;
use num_traits::FromPrimitive;

#[derive(FromPrimitive, Debug)]
pub enum SyscallError {
    /// 操作不允许 (EPERM)
    PermissionDenied = -1,
    /// 文件或目录不存在 (ENOENT)
    FileNotFound = -2,
    /// 进程不存在 (ESRCH)
    NoProcess = -3,
    /// 系统调用被中断 (EINTR)
    Interrupted = -4,
    /// 输入/输出错误 (EIO)
    IOError = -5,
    /// 坏的文件描述符 (EBADF)
    BadFileDescriptor = -9,
    TryAgain = -11,
    /// 内存不足 (ENOMEM)
    NoMemory = -12,
    /// 权限不足 (EACCES)
    AccessDenied = -13,
    /// 地址错误 (EFAULT)
    BadAddress = -14,
    /// 资源忙 (EBUSY)
    DeviceOrResourceBusy = -16,
    /// 文件已存在 (EEXIST)
    FileAlreadyExists = -17,
    /// 不是一个目录 (ENOTDIR)
    NotADirectory = -20,
    /// 是一个目录 (EISDIR)
    IsADirectory = -21,
    /// 参数非法 (EINVAL)
    InvalidArguments = -22,
    /// 系统打开文件过多 (ENFILE)
    TooManyOpenFilesSystem = -23,
    /// 进程打开文件过多 (EMFILE)
    TooManyOpenFilesProcess = -24,
    /// ENOTTY
    InappropriateIoctl = -25,
    /// 文件太大 (EFBIG)
    FileTooLarge = -27,
    /// 磁盘空间不足 (ENOSPC)
    NoSpaceLeft = -28,
    /// 只读文件系统 (EROFS)
    ReadOnlyFileSystem = -30,
    /// 系统调用未实现 (ENOSYS)
    NoSyscall = -38,
    /// 目录不为空 (ENOTEMPTY)
    DirectoryNotEmpty = -39,
    /// 链接层数过多 (ELOOP)
    TooManySymbolicLinks = -40,
    /// 名字太长 (ENAMETOOLONG)
    PathTooLong = -36,
    Other = -256,
}

impl SyscallError {
    pub fn as_isize(self) -> isize {
        self as isize
    }
}

impl From<isize> for SyscallError {
    fn from(value: isize) -> Self {
        SyscallError::from_isize(value).unwrap()
    }
}
