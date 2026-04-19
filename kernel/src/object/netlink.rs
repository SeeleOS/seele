use alloc::{collections::VecDeque, format, sync::{Arc, Weak}, vec::Vec};
use core::sync::atomic::{AtomicU64, Ordering};
use lazy_static::lazy_static;
use spin::Mutex;

use crate::{
    filesystem::info::LinuxStat,
    impl_cast_function, impl_cast_function_non_trait,
    object::{
        FileFlags, Object,
        config::ConfigurateRequest,
        error::ObjectError,
        linux_anon::wake_linux_io_waiters,
        misc::ObjectResult,
        traits::{Configuratable, Readable, Statable},
    },
    polling::{event::PollableEvent, object::Pollable},
    socket::{
        AF_NETLINK, NETLINK_ADD_MEMBERSHIP, NETLINK_DROP_MEMBERSHIP, NETLINK_EXT_ACK,
        NETLINK_GET_STRICT_CHK, NETLINK_KOBJECT_UEVENT, NETLINK_LIST_MEMBERSHIPS, NETLINK_PKTINFO,
        NETLINK_ROUTE, SO_ATTACH_FILTER, SO_DETACH_FILTER, SO_DOMAIN, SO_ERROR, SO_PASSCRED,
        SO_PROTOCOL, SO_RCVBUF, SO_RCVBUFFORCE, SO_REUSEADDR, SO_SNDBUF, SO_SNDBUFFORCE, SO_TYPE,
        SOCK_CLOEXEC, SOCK_DGRAM, SOCK_NONBLOCK, SOCK_RAW, SOL_NETLINK, SOL_SOCKET, SocketError,
        SocketResult,
    },
};

const DEFAULT_SOCKET_BUFFER_SIZE: i32 = 64 * 1024;
const FIONBIO: u64 = 0x5421;
const FIOCLEX: u64 = 0x5451;
const S_IFSOCK: u32 = 0o140000;
const NLMSG_ERROR: u16 = 0x2;
static NEXT_UEVENT_SEQNUM: AtomicU64 = AtomicU64::new(1);

lazy_static! {
    static ref NETLINK_SOCKETS: Mutex<Vec<Weak<NetlinkSocketObject>>> = Mutex::new(Vec::new());
}

#[repr(C)]
#[derive(Clone, Copy)]
struct NetlinkMessageHeader {
    nlmsg_len: u32,
    nlmsg_type: u16,
    nlmsg_flags: u16,
    nlmsg_seq: u32,
    nlmsg_pid: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct NetlinkErrorMessage {
    error: i32,
    header: NetlinkMessageHeader,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NetlinkSocketAddress {
    pub pid: u32,
    pub groups: u32,
}

#[derive(Debug)]
pub struct NetlinkSocketObject {
    flags: Mutex<FileFlags>,
    socket_type: u64,
    protocol: u64,
    address: Mutex<NetlinkSocketAddress>,
    memberships: Mutex<Vec<u32>>,
    recv_queue: Mutex<VecDeque<Vec<u8>>>,
}

impl NetlinkSocketObject {
    pub fn create(kind: u64, protocol: u64) -> SocketResult<Arc<Self>> {
        let socket_type = kind & !(SOCK_NONBLOCK | SOCK_CLOEXEC);
        if !matches!(protocol, NETLINK_ROUTE | NETLINK_KOBJECT_UEVENT) {
            return Err(SocketError::ProtocolNotSupported);
        }
        if !matches!(socket_type, SOCK_RAW | SOCK_DGRAM) {
            return Err(SocketError::ProtocolNotSupported);
        }

        let socket = Arc::new(Self {
            flags: Mutex::new(FileFlags::empty()),
            socket_type,
            protocol,
            address: Mutex::new(NetlinkSocketAddress::default()),
            memberships: Mutex::new(Vec::new()),
            recv_queue: Mutex::new(VecDeque::new()),
        });
        if protocol == NETLINK_KOBJECT_UEVENT {
            NETLINK_SOCKETS.lock().push(Arc::downgrade(&socket));
        }
        Ok(socket)
    }

    pub fn bind(&self, address: NetlinkSocketAddress) -> SocketResult<()> {
        *self.address.lock() = address;
        Ok(())
    }

    pub fn getsockname_bytes(&self) -> Vec<u8> {
        let address = *self.address.lock();
        let mut out = Vec::with_capacity(12);
        out.extend_from_slice(&(AF_NETLINK as u16).to_ne_bytes());
        out.extend_from_slice(&0u16.to_ne_bytes());
        out.extend_from_slice(&address.pid.to_ne_bytes());
        out.extend_from_slice(&address.groups.to_ne_bytes());
        out
    }

    pub fn peek_message_len(&self) -> Option<usize> {
        self.recv_queue.lock().front().map(Vec::len)
    }

    pub fn recv_message(&self, buffer: &mut [u8], peek: bool) -> ObjectResult<(usize, usize)> {
        let mut queue = self.recv_queue.lock();
        let message = if peek {
            queue.front().cloned()
        } else {
            queue.pop_front()
        };
        let Some(message) = message else {
            let _ = self.is_nonblocking();
            return Err(ObjectError::TryAgain);
        };

        let copy_len = buffer.len().min(message.len());
        buffer[..copy_len].copy_from_slice(&message[..copy_len]);
        Ok((copy_len, message.len()))
    }

    pub fn send(&self, message: &[u8]) -> SocketResult<usize> {
        if self.protocol == NETLINK_ROUTE {
            self.enqueue_ack(message);
        }
        Ok(message.len())
    }

    pub fn setsockopt(
        &self,
        level: u64,
        option_name: u64,
        option_value: &[u8],
    ) -> SocketResult<()> {
        if level == SOL_SOCKET {
            return match option_name {
                SO_REUSEADDR | SO_PASSCRED | SO_SNDBUF | SO_RCVBUF | SO_SNDBUFFORCE
                | SO_RCVBUFFORCE | SO_ATTACH_FILTER | SO_DETACH_FILTER => Ok(()),
                _ => Err(SocketError::InvalidArguments),
            };
        }

        if level != SOL_NETLINK {
            return Err(SocketError::ProtocolNotSupported);
        }

        match option_name {
            NETLINK_PKTINFO | NETLINK_EXT_ACK | NETLINK_GET_STRICT_CHK => Ok(()),
            NETLINK_ADD_MEMBERSHIP | NETLINK_DROP_MEMBERSHIP => {
                let group = Self::decode_u32(option_value)?;
                let mut memberships = self.memberships.lock();
                if option_name == NETLINK_ADD_MEMBERSHIP {
                    if !memberships.contains(&group) {
                        memberships.push(group);
                    }
                } else {
                    memberships.retain(|existing| *existing != group);
                }
                Ok(())
            }
            _ => Err(SocketError::InvalidArguments),
        }
    }

    pub fn getsockopt(
        &self,
        level: u64,
        option_name: u64,
        option_len: usize,
    ) -> SocketResult<Vec<u8>> {
        if level == SOL_SOCKET {
            return match option_name {
                SO_ERROR => Self::encode_i32(option_len, 0),
                SO_TYPE => Self::encode_i32(option_len, self.socket_type as i32),
                SO_DOMAIN => Self::encode_i32(option_len, AF_NETLINK as i32),
                SO_PROTOCOL => Self::encode_i32(option_len, self.protocol as i32),
                SO_SNDBUF | SO_RCVBUF | SO_SNDBUFFORCE | SO_RCVBUFFORCE => {
                    Self::encode_i32(option_len, DEFAULT_SOCKET_BUFFER_SIZE)
                }
                SO_REUSEADDR | SO_PASSCRED => Self::encode_i32(option_len, 0),
                _ => Err(SocketError::InvalidArguments),
            };
        }

        if level != SOL_NETLINK {
            return Err(SocketError::ProtocolNotSupported);
        }

        match option_name {
            NETLINK_LIST_MEMBERSHIPS => Ok(self.membership_bytes(option_len)),
            _ => Err(SocketError::InvalidArguments),
        }
    }

    fn encode_i32(option_len: usize, value: i32) -> SocketResult<Vec<u8>> {
        if option_len < core::mem::size_of::<i32>() {
            return Err(SocketError::InvalidArguments);
        }
        Ok(value.to_ne_bytes().to_vec())
    }

    fn decode_u32(option_value: &[u8]) -> SocketResult<u32> {
        if option_value.len() < core::mem::size_of::<u32>() {
            return Err(SocketError::InvalidArguments);
        }

        Ok(u32::from_ne_bytes(
            option_value[..core::mem::size_of::<u32>()]
                .try_into()
                .map_err(|_| SocketError::InvalidArguments)?,
        ))
    }

    fn membership_bytes(&self, option_len: usize) -> Vec<u8> {
        let memberships = self.memberships.lock();
        if option_len == 0 {
            return Vec::new();
        }

        let capacity = option_len / core::mem::size_of::<u32>();
        let mut out = Vec::with_capacity(capacity * core::mem::size_of::<u32>());
        for group in memberships.iter().take(capacity) {
            out.extend_from_slice(&group.to_ne_bytes());
        }
        out
    }

    fn enqueue_ack(&self, message: &[u8]) {
        if message.len() < core::mem::size_of::<NetlinkMessageHeader>() {
            return;
        }

        let header =
            unsafe { core::ptr::read_unaligned(message.as_ptr().cast::<NetlinkMessageHeader>()) };
        let reply_len = core::mem::size_of::<NetlinkMessageHeader>()
            + core::mem::size_of::<NetlinkErrorMessage>();
        let reply_header = NetlinkMessageHeader {
            nlmsg_len: reply_len as u32,
            nlmsg_type: NLMSG_ERROR,
            nlmsg_flags: 0,
            nlmsg_seq: header.nlmsg_seq,
            nlmsg_pid: 0,
        };
        let error = NetlinkErrorMessage { error: 0, header };

        let mut bytes = Vec::with_capacity(reply_len);
        bytes.extend_from_slice(unsafe {
            core::slice::from_raw_parts(
                (&reply_header as *const NetlinkMessageHeader).cast::<u8>(),
                core::mem::size_of::<NetlinkMessageHeader>(),
            )
        });
        bytes.extend_from_slice(unsafe {
            core::slice::from_raw_parts(
                (&error as *const NetlinkErrorMessage).cast::<u8>(),
                core::mem::size_of::<NetlinkErrorMessage>(),
            )
        });
        self.recv_queue.lock().push_back(bytes);
        wake_linux_io_waiters();
    }

    fn is_nonblocking(&self) -> bool {
        self.flags.lock().contains(FileFlags::NONBLOCK)
    }
}

pub fn broadcast_kobject_uevent(
    action: &str,
    devpath: &str,
    subsystem: &str,
    devname: Option<&str>,
) {
    let seqnum = NEXT_UEVENT_SEQNUM.fetch_add(1, Ordering::Relaxed);
    let mut message = format!("{action}@{devpath}\0ACTION={action}\0DEVPATH={devpath}\0SUBSYSTEM={subsystem}\0").into_bytes();
    if let Some(devname) = devname {
        message.extend_from_slice(format!("DEVNAME={devname}\0").as_bytes());
    }
    message.extend_from_slice(format!("SEQNUM={seqnum}\0").as_bytes());

    let mut sockets = NETLINK_SOCKETS.lock();
    let mut delivered = false;
    sockets.retain(|socket| {
        let Some(socket) = socket.upgrade() else {
            return false;
        };
        if socket.protocol == NETLINK_KOBJECT_UEVENT {
            socket.recv_queue.lock().push_back(message.clone());
            delivered = true;
        }
        true
    });
    if delivered {
        wake_linux_io_waiters();
    }
}

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
    impl_cast_function_non_trait!("netlink_socket", NetlinkSocketObject);
}

impl Configuratable for NetlinkSocketObject {
    fn configure(&self, request: ConfigurateRequest) -> ObjectResult<isize> {
        match request {
            ConfigurateRequest::RawIoctl {
                request: FIOCLEX, ..
            } => Ok(0),
            ConfigurateRequest::RawIoctl {
                request: FIONBIO,
                arg,
            } => {
                let nonblocking = unsafe { *(arg as *const i32) };
                let mut flags = self.flags.lock();
                if nonblocking != 0 {
                    flags.insert(FileFlags::NONBLOCK);
                } else {
                    flags.remove(FileFlags::NONBLOCK);
                }
                Ok(0)
            }
            _ => Err(ObjectError::InvalidRequest),
        }
    }
}

impl Readable for NetlinkSocketObject {
    fn read(&self, buffer: &mut [u8]) -> ObjectResult<usize> {
        let (copied, _) = self.recv_message(buffer, false)?;
        Ok(copied)
    }
}

impl Pollable for NetlinkSocketObject {
    fn is_event_ready(&self, event: PollableEvent) -> bool {
        match event {
            PollableEvent::CanBeWritten => true,
            PollableEvent::CanBeRead => !self.recv_queue.lock().is_empty(),
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
