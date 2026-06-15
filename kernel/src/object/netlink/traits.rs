use alloc::{sync::Arc, vec::Vec};

use super::socket::NetlinkSocketObject;
use crate::{
    filesystem::info::LinuxStat,
    impl_cast_function, impl_cast_function_non_trait,
    memory::user_safe,
    object::{
        FileFlags, Object,
        config::ConfigurateRequest,
        error::ObjectError,
        linux_ioctl::{LinuxIoctlOp, socket_raw_ioctl_op},
        misc::ObjectResult,
        traits::{Configuratable, Readable, Statable},
    },
    polling::{event::PollableEvent, object::Pollable},
    socket::{SocketError, SocketLike, SocketResult},
};

const S_IFSOCK: u32 = 0o140000;

impl Object for NetlinkSocketObject {
    fn get_flags(self: Arc<Self>) -> ObjectResult<FileFlags> {
        Ok(*self.flags.lock())
    }

    fn set_flags(self: Arc<Self>, flags: FileFlags) -> ObjectResult<()> {
        *self.flags.lock() = flags;
        Ok(())
    }

    impl_cast_function!("configuratable", Configuratable);
    impl_cast_function!("readable", Readable);
    impl_cast_function!("pollable", Pollable);
    impl_cast_function!("statable", Statable);
    impl_cast_function!("socket_like", SocketLike);
    impl_cast_function_non_trait!("netlink_socket", NetlinkSocketObject);
}

impl Configuratable for NetlinkSocketObject {
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

impl Readable for NetlinkSocketObject {
    fn read(&self, buffer: &mut [u8]) -> ObjectResult<usize> {
        let (copied, _, _, _, _) = self.recv_message(buffer, false)?;
        Ok(copied)
    }
}

impl SocketLike for NetlinkSocketObject {
    fn bind_bytes(self: Arc<Self>, address: &[u8]) -> SocketResult<()> {
        self.bind(Self::parse_sockaddr(address)?)
    }

    fn sendto(self: Arc<Self>, buffer: &[u8], address: Option<&[u8]>) -> SocketResult<usize> {
        let destination = address.map(Self::parse_sockaddr).transpose()?;
        self.send(buffer, destination)
    }

    fn recvfrom(&self, buffer: &mut [u8]) -> SocketResult<(usize, Option<Vec<u8>>)> {
        let (copied, _, source, _, _) =
            self.recv_message(buffer, false).map_err(|err| match err {
                ObjectError::TryAgain => SocketError::TryAgain,
                _ => SocketError::InvalidArguments,
            })?;
        Ok((copied, Some(Self::sockaddr_bytes(source))))
    }

    fn getsockname_bytes(&self) -> SocketResult<Vec<u8>> {
        Ok(NetlinkSocketObject::getsockname_bytes(self))
    }

    fn setsockopt(&self, level: u64, option_name: u64, option_value: &[u8]) -> SocketResult<()> {
        NetlinkSocketObject::setsockopt(self, level, option_name, option_value)
    }

    fn getsockopt(&self, level: u64, option_name: u64, option_len: usize) -> SocketResult<Vec<u8>> {
        NetlinkSocketObject::getsockopt(self, level, option_name, option_len)
    }
}

impl Pollable for NetlinkSocketObject {
    fn is_event_ready(&self, event: PollableEvent) -> bool {
        match event {
            PollableEvent::CanBeWritten => true,
            PollableEvent::CanBeRead => self.has_pending_messages(),
            _ => false,
        }
    }
}

impl Statable for NetlinkSocketObject {
    fn stat(&self) -> LinuxStat {
        LinuxStat {
            st_dev: 1,
            st_nlink: 1,
            st_mode: S_IFSOCK | 0o777,
            st_blksize: 4096,
            ..Default::default()
        }
    }
}
