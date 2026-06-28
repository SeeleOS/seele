use crate::memory::utils::Mut;
use alloc::{string::String, sync::Arc, vec::Vec};

use crate::{
    impl_cast_function,
    memory::user_safe,
    object::{
        FileFlags, Object,
        config::ConfigurateRequest,
        error::ObjectError,
        linux_ioctl::{LinuxIoctlOp, socket_raw_ioctl_op},
        misc::ObjectResult,
        traits::Configuratable,
    },
};

use super::{
    AF_PACKET, SO_DOMAIN, SO_ERROR, SO_PROTOCOL, SO_TYPE, SOCK_CLOEXEC, SOCK_DGRAM, SOCK_NONBLOCK,
    SOCK_RAW, SOL_SOCKET, SocketError, SocketLike, SocketResult,
};

const SIOCGIFINDEX: u64 = 0x8933;
const SIOCSIFFLAGS: u64 = 0x8914;
const LOOPBACK_IFINDEX: i32 = 1;
const IFF_LOOPBACK: u64 = 0x8;
const IFF_RUNNING: u64 = 0x40;

#[repr(C)]
#[derive(Clone, Copy)]
struct LinuxSockAddrLl {
    sll_family: u16,
    sll_protocol: u16,
    sll_ifindex: i32,
    sll_hatype: u16,
    sll_pkttype: u8,
    sll_halen: u8,
    sll_addr: [u8; 8],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct LinuxIfreq {
    name: [u8; 16],
    value: i32,
}

#[derive(Debug)]
pub struct PacketSocketObject {
    flags: Mut<FileFlags>,
    protocol: u64,
    bound_ifindex: Mut<Option<i32>>,
}

impl PacketSocketObject {
    pub fn create(kind: u64, protocol: u64) -> SocketResult<Arc<Self>> {
        let socket_type = kind & !(SOCK_NONBLOCK | SOCK_CLOEXEC);
        match socket_type {
            SOCK_DGRAM => Ok(Arc::new(Self {
                flags: Mut::new(FileFlags::empty()),
                protocol,
                bound_ifindex: Mut::new(None),
            })),
            SOCK_RAW => Err(SocketError::ProtocolNotSupported),
            _ => Err(SocketError::InvalidArguments),
        }
    }

    fn decode_sockaddr_ll(address: &[u8]) -> SocketResult<LinuxSockAddrLl> {
        if address.len() < core::mem::size_of::<LinuxSockAddrLl>() {
            return Err(SocketError::InvalidArguments);
        }
        let sockaddr = unsafe { &*(address.as_ptr().cast::<LinuxSockAddrLl>()) };
        if u64::from(sockaddr.sll_family) != AF_PACKET {
            return Err(SocketError::AddressFamilyNotSupported);
        }
        Ok(*sockaddr)
    }

    fn decode_ifreq(arg: u64) -> ObjectResult<LinuxIfreq> {
        user_safe::read(arg as *const LinuxIfreq).map_err(|_| ObjectError::BadAddress)
    }

    fn write_ifreq(arg: u64, ifreq: &LinuxIfreq) -> ObjectResult<()> {
        user_safe::write(arg as *mut LinuxIfreq, ifreq).map_err(|_| ObjectError::BadAddress)
    }

    fn ifreq_name(ifreq: &LinuxIfreq) -> String {
        let len = ifreq
            .name
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(ifreq.name.len());
        String::from_utf8_lossy(&ifreq.name[..len]).into_owned()
    }

    fn encode_i32(option_len: usize, value: i32) -> SocketResult<Vec<u8>> {
        if option_len < core::mem::size_of::<i32>() {
            return Err(SocketError::InvalidArguments);
        }
        Ok(value.to_ne_bytes().to_vec())
    }
}

impl SocketLike for PacketSocketObject {
    fn bind_bytes(self: Arc<Self>, address: &[u8]) -> SocketResult<()> {
        let sockaddr = Self::decode_sockaddr_ll(address)?;
        if !matches!(sockaddr.sll_ifindex, 0 | LOOPBACK_IFINDEX) {
            return Err(SocketError::NetworkDown);
        }
        *self.bound_ifindex.lock() = Some(sockaddr.sll_ifindex);
        Ok(())
    }

    fn getsockname_bytes(&self) -> SocketResult<Vec<u8>> {
        let sockaddr = LinuxSockAddrLl {
            sll_family: AF_PACKET as u16,
            sll_protocol: self.protocol as u16,
            sll_ifindex: self.bound_ifindex.lock().unwrap_or(0),
            sll_hatype: 0,
            sll_pkttype: 0,
            sll_halen: 0,
            sll_addr: [0; 8],
        };
        Ok(unsafe {
            core::slice::from_raw_parts(
                (&sockaddr as *const LinuxSockAddrLl).cast::<u8>(),
                core::mem::size_of::<LinuxSockAddrLl>(),
            )
        }
        .to_vec())
    }

    fn setsockopt(&self, level: u64, _option_name: u64, _option_value: &[u8]) -> SocketResult<()> {
        if level == SOL_SOCKET {
            return Err(SocketError::InvalidArguments);
        }
        Err(SocketError::ProtocolOptionNotSupported)
    }

    fn getsockopt(&self, level: u64, option_name: u64, option_len: usize) -> SocketResult<Vec<u8>> {
        if level != SOL_SOCKET {
            return Err(SocketError::ProtocolOptionNotSupported);
        }
        match option_name {
            SO_ERROR => Self::encode_i32(option_len, 0),
            SO_TYPE => Self::encode_i32(option_len, SOCK_DGRAM as i32),
            SO_DOMAIN => Self::encode_i32(option_len, AF_PACKET as i32),
            SO_PROTOCOL => Self::encode_i32(option_len, self.protocol as i32),
            _ => Err(SocketError::InvalidArguments),
        }
    }
}

impl Object for PacketSocketObject {
    fn get_flags(self: Arc<Self>) -> ObjectResult<FileFlags> {
        Ok(*self.flags.lock())
    }

    fn set_flags(self: Arc<Self>, flags: FileFlags) -> ObjectResult<()> {
        *self.flags.lock() = flags;
        Ok(())
    }

    impl_cast_function!("configuratable", Configuratable);
    impl_cast_function!("socket_like", SocketLike);
}

impl Configuratable for PacketSocketObject {
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
            ConfigurateRequest::RawIoctl {
                request: SIOCGIFINDEX,
                arg,
            } => {
                let mut ifreq = Self::decode_ifreq(arg)?;
                if Self::ifreq_name(&ifreq) != "lo" {
                    return Err(ObjectError::DoesNotExist);
                }
                ifreq.value = LOOPBACK_IFINDEX;
                Self::write_ifreq(arg, &ifreq)?;
                Ok(0)
            }
            ConfigurateRequest::RawIoctl {
                request: SIOCSIFFLAGS,
                arg,
            } => {
                let ifreq = Self::decode_ifreq(arg)?;
                if Self::ifreq_name(&ifreq) != "lo" {
                    return Err(ObjectError::DoesNotExist);
                }
                let requested_flags = ifreq.value as u64;
                let loopback_flags = requested_flags & (IFF_LOOPBACK | IFF_RUNNING);
                crate::process::manager::get_current_process()
                    .lock()
                    .net_namespace
                    .set_loopback_flags(loopback_flags);
                Ok(0)
            }
            _ => Err(ObjectError::InvalidRequest),
        }
    }
}
