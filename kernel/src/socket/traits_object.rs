use alloc::sync::Arc;

use crate::{
    impl_cast_function, impl_cast_function_non_trait,
    memory::user_safe,
    object::{
        FileFlags, Object,
        config::ConfigurateRequest,
        error::ObjectError,
        linux_ioctl::{LinuxIoctlOp, socket_raw_ioctl_op},
        misc::ObjectResult,
        traits::{Configuratable, Readable, Statable, Writable},
    },
    polling::object::Pollable,
};

use super::{SocketLike, UnixSocketObject};

impl Object for UnixSocketObject {
    fn get_flags(self: Arc<Self>) -> ObjectResult<FileFlags> {
        Ok(*self.flags.lock())
    }

    fn set_flags(self: Arc<Self>, flags: FileFlags) -> ObjectResult<()> {
        *self.flags.lock() = flags;
        Ok(())
    }

    impl_cast_function!("configuratable", Configuratable);
    impl_cast_function!("readable", Readable);
    impl_cast_function!("writable", Writable);
    impl_cast_function!("pollable", Pollable);
    impl_cast_function!("statable", Statable);
    impl_cast_function!("socket_like", SocketLike);
    impl_cast_function_non_trait!("unix_socket", UnixSocketObject);
}

impl Configuratable for UnixSocketObject {
    fn configure(&self, request: ConfigurateRequest) -> ObjectResult<isize> {
        match request {
            ConfigurateRequest::RawIoctl { request, arg }
                if matches!(socket_raw_ioctl_op(request), Some(LinuxIoctlOp::RawFionbio)) =>
            {
                let nonblocking =
                    user_safe::read(arg as *const i32).map_err(|_| ObjectError::BadAddress)?;
                let mut flags = self.flags.lock();
                if nonblocking != 0 {
                    flags.insert(FileFlags::NONBLOCK);
                } else {
                    flags.remove(FileFlags::NONBLOCK);
                }
                Ok(0)
            }
            ConfigurateRequest::RawIoctl { request, .. }
                if matches!(socket_raw_ioctl_op(request), Some(LinuxIoctlOp::RawFioclex)) =>
            {
                Ok(0)
            }
            ConfigurateRequest::LinuxTiocoutq(outq_ptr) => {
                user_safe::write(outq_ptr, &0i32).map_err(|_| ObjectError::BadAddress)?;
                Ok(0)
            }
            _ => Err(ObjectError::InvalidRequest),
        }
    }
}
