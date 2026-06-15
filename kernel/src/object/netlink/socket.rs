use crate::memory::utils::Mut;
use alloc::{
    collections::VecDeque,
    sync::{Arc, Weak},
    vec,
    vec::Vec,
};
use core::sync::atomic::{AtomicU64, Ordering};
use lazy_static::lazy_static;

use crate::{
    filesystem::info::LinuxStat,
    impl_cast_function, impl_cast_function_non_trait,
    memory::user_safe,
    object::{
        FileFlags, Object,
        config::ConfigurateRequest,
        error::ObjectError,
        linux_anon::wake_linux_io_waiters,
        linux_ioctl::{LinuxIoctlOp, socket_raw_ioctl_op},
        misc::{ObjectRef, ObjectResult},
        traits::{Configuratable, Readable, Statable},
    },
    polling::{event::PollableEvent, object::Pollable},
    process::manager::get_current_process,
    socket::{
        AF_NETLINK, NETLINK_ADD_MEMBERSHIP, NETLINK_AUDIT, NETLINK_DROP_MEMBERSHIP,
        NETLINK_EXT_ACK, NETLINK_GET_STRICT_CHK, NETLINK_KOBJECT_UEVENT, NETLINK_LIST_MEMBERSHIPS,
        NETLINK_PKTINFO, NETLINK_ROUTE, SO_ATTACH_FILTER, SO_DETACH_FILTER, SO_DOMAIN, SO_ERROR,
        SO_PASSCRED, SO_PASSPIDFD, SO_PASSRIGHTS, SO_PASSSEC, SO_PRIORITY, SO_PROTOCOL, SO_RCVBUF,
        SO_RCVBUFFORCE, SO_RCVTIMEO_NEW, SO_RCVTIMEO_OLD, SO_REUSEADDR, SO_SNDBUF, SO_SNDBUFFORCE,
        SO_SNDTIMEO_NEW, SO_SNDTIMEO_OLD, SO_TIMESTAMP_NEW, SO_TIMESTAMP_OLD, SO_TIMESTAMPNS_NEW,
        SO_TIMESTAMPNS_OLD, SO_TYPE, SOCK_CLOEXEC, SOCK_DGRAM, SOCK_NONBLOCK, SOCK_RAW,
        SOL_NETLINK, SOL_SOCKET, SocketError, SocketLike, SocketResult, can_set_socket_priority,
        socket_timeout_option_len,
    },
};

const DEFAULT_SOCKET_BUFFER_SIZE: i32 = 64 * 1024;
const S_IFSOCK: u32 = 0o140000;
pub(super) const AF_INET: u8 = 2;
pub(super) const ARPHRD_ETHER: u16 = 1;
pub(super) const ARPHRD_LOOPBACK: u16 = 772;
pub(super) const IFA_ADDRESS: u16 = 1;
pub(super) const IFA_LOCAL: u16 = 2;
pub(super) const IFA_LABEL: u16 = 3;
pub(super) const IFA_FLAGS: u16 = 8;
pub(super) const IFA_F_PERMANENT: u8 = 0x80;
pub(super) const IFF_UP: u32 = 1 << 0;
pub(super) const IFF_BROADCAST: u32 = 1 << 1;
pub(super) const IFF_LOOPBACK: u32 = 1 << 3;
pub(super) const IFF_RUNNING: u32 = 1 << 6;
pub(super) const IFF_MULTICAST: u32 = 1 << 12;
pub(super) const IFF_LOWER_UP: u32 = 1 << 16;
pub(super) const IFLA_ADDRESS: u16 = 1;
pub(super) const IFLA_BROADCAST: u16 = 2;
pub(super) const IFLA_IFNAME: u16 = 3;
pub(super) const IFLA_MTU: u16 = 4;
pub(super) const IFLA_QDISC: u16 = 6;
pub(super) const IFLA_TXQLEN: u16 = 13;
pub(super) const IFLA_OPERSTATE: u16 = 16;
pub(super) const IFLA_LINKMODE: u16 = 17;
pub(super) const IFLA_NET_NS_FD: u16 = 28;
pub(super) const IFLA_NUM_TX_QUEUES: u16 = 31;
pub(super) const IFLA_NUM_RX_QUEUES: u16 = 32;
pub(super) const IFLA_ALT_IFNAME: u16 = 53;
pub(super) const IFLA_PERM_ADDRESS: u16 = 54;
const NLMSG_ERROR: u16 = 0x2;
pub(super) const NLMSG_DONE: u16 = 0x3;
pub(super) const NLM_F_MULTI: u16 = 0x2;
pub(super) const NLM_F_DUMP: u16 = 0x300;
pub(super) const RTM_NEWLINK: u16 = 16;
pub(super) const RTM_GETLINK: u16 = 18;
pub(super) const RTM_NEWADDR: u16 = 20;
pub(super) const RTM_GETADDR: u16 = 22;
pub(super) const RT_SCOPE_UNIVERSE: u8 = 0;
pub(super) const RT_SCOPE_HOST: u8 = 254;
pub(super) const IF_OPER_UP: u8 = 6;
static NEXT_NETLINK_PORT_ID: AtomicU64 = AtomicU64::new(1);

lazy_static! {
    pub(super) static ref NETLINK_SOCKETS: Mut<Vec<Weak<NetlinkSocketObject>>> =
        Mut::new(Vec::new());
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct NetlinkMessageHeader {
    pub(super) nlmsg_len: u32,
    pub(super) nlmsg_type: u16,
    pub(super) nlmsg_flags: u16,
    pub(super) nlmsg_seq: u32,
    pub(super) nlmsg_pid: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct NetlinkErrorMessage {
    error: i32,
    header: NetlinkMessageHeader,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct IfInfoMessage {
    pub(super) ifi_family: u8,
    pub(super) ifi_pad: u8,
    pub(super) ifi_type: u16,
    pub(super) ifi_index: i32,
    pub(super) ifi_flags: u32,
    pub(super) ifi_change: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct IfAddrMessage {
    pub(super) ifa_family: u8,
    pub(super) ifa_prefixlen: u8,
    pub(super) ifa_flags: u8,
    pub(super) ifa_scope: u8,
    pub(super) ifa_index: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct RouteAttributeHeader {
    pub(super) rta_len: u16,
    pub(super) rta_type: u16,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NetlinkSocketAddress {
    pub pid: u32,
    pub groups: u32,
}

#[derive(Clone, Debug)]
struct QueuedNetlinkMessage {
    bytes: Vec<u8>,
    source: NetlinkSocketAddress,
    uid: u32,
    gid: u32,
}

#[derive(Debug)]
pub struct NetlinkSocketObject {
    flags: Mut<FileFlags>,
    pass_cred: Mut<bool>,
    priority: Mut<i32>,
    socket_type: u64,
    protocol: u64,
    address: Mut<NetlinkSocketAddress>,
    memberships: Mut<Vec<u32>>,
    recv_queue: Mut<VecDeque<QueuedNetlinkMessage>>,
    self_ref: Mut<Option<Weak<NetlinkSocketObject>>>,
}

impl NetlinkSocketObject {
    fn parse_sockaddr(address: &[u8]) -> SocketResult<NetlinkSocketAddress> {
        if address.len() < 12 {
            return Err(SocketError::InvalidArguments);
        }
        if u16::from_ne_bytes(
            address[..2]
                .try_into()
                .map_err(|_| SocketError::InvalidArguments)?,
        ) != AF_NETLINK as u16
        {
            return Err(SocketError::InvalidArguments);
        }

        Ok(NetlinkSocketAddress {
            pid: u32::from_ne_bytes(
                address[4..8]
                    .try_into()
                    .map_err(|_| SocketError::InvalidArguments)?,
            ),
            groups: u32::from_ne_bytes(
                address[8..12]
                    .try_into()
                    .map_err(|_| SocketError::InvalidArguments)?,
            ),
        })
    }

    pub fn sockaddr_bytes(address: NetlinkSocketAddress) -> Vec<u8> {
        let mut out = Vec::with_capacity(12);
        out.extend_from_slice(&(AF_NETLINK as u16).to_ne_bytes());
        out.extend_from_slice(&0u16.to_ne_bytes());
        out.extend_from_slice(&address.pid.to_ne_bytes());
        out.extend_from_slice(&address.groups.to_ne_bytes());
        out
    }

    pub fn create(kind: u64, protocol: u64) -> SocketResult<Arc<Self>> {
        let socket_type = kind & !(SOCK_NONBLOCK | SOCK_CLOEXEC);
        if !matches!(
            protocol,
            NETLINK_ROUTE | NETLINK_AUDIT | NETLINK_KOBJECT_UEVENT
        ) {
            return Err(SocketError::ProtocolNotSupported);
        }
        if !matches!(socket_type, SOCK_RAW | SOCK_DGRAM) {
            return Err(SocketError::ProtocolNotSupported);
        }

        let socket = Arc::new(Self {
            flags: Mut::new(FileFlags::empty()),
            pass_cred: Mut::new(false),
            priority: Mut::new(0),
            socket_type,
            protocol,
            address: Mut::new(NetlinkSocketAddress::default()),
            memberships: Mut::new(Vec::new()),
            recv_queue: Mut::new(VecDeque::new()),
            self_ref: Mut::new(None),
        });
        *socket.self_ref.lock() = Some(Arc::downgrade(&socket));
        if protocol == NETLINK_KOBJECT_UEVENT {
            NETLINK_SOCKETS.lock().push(Arc::downgrade(&socket));
        }
        Ok(socket)
    }

    fn self_object(&self) -> Option<ObjectRef> {
        self.self_ref
            .lock()
            .as_ref()
            .and_then(Weak::upgrade)
            .map(|socket| socket as ObjectRef)
    }

    pub(super) fn wake_read_waiters(&self) {
        wake_linux_io_waiters();
        let Some(object) = self.self_object() else {
            return;
        };
        crate::thread::yielding::wake_pollers_for_object(object, PollableEvent::CanBeRead);
    }

    pub(super) fn queue_message(&self, message: Vec<u8>) {
        self.queue_message_with_source(message, NetlinkSocketAddress::default(), 0, 0);
    }

    pub(super) fn queue_message_with_source(
        &self,
        message: Vec<u8>,
        source: NetlinkSocketAddress,
        uid: u32,
        gid: u32,
    ) {
        self.recv_queue.lock().push_back(QueuedNetlinkMessage {
            bytes: message,
            source,
            uid,
            gid,
        });
        self.wake_read_waiters();
    }

    pub fn bind(&self, address: NetlinkSocketAddress) -> SocketResult<()> {
        let mut address = address;
        if address.pid == 0 {
            address.pid = NEXT_NETLINK_PORT_ID.fetch_add(1, Ordering::Relaxed) as u32;
        }
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

    pub fn pass_cred_enabled(&self) -> bool {
        *self.pass_cred.lock()
    }

    pub(super) fn protocol(&self) -> u64 {
        self.protocol
    }

    pub fn peek_message_len(&self) -> Option<usize> {
        self.recv_queue
            .lock()
            .front()
            .map(|message| message.bytes.len())
    }

    pub fn recv_message(
        &self,
        buffer: &mut [u8],
        peek: bool,
    ) -> ObjectResult<(usize, usize, NetlinkSocketAddress, u32, u32)> {
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

        let copy_len = buffer.len().min(message.bytes.len());
        buffer[..copy_len].copy_from_slice(&message.bytes[..copy_len]);
        Ok((
            copy_len,
            message.bytes.len(),
            message.source,
            message.uid,
            message.gid,
        ))
    }

    pub(super) fn receives_group(&self, group: u32) -> bool {
        let address_groups = self.address.lock().groups;
        if (address_groups & group) != 0 {
            return true;
        }

        self.memberships.lock().contains(&group)
    }

    pub(super) fn local_address(&self) -> NetlinkSocketAddress {
        let mut address = self.address.lock();
        if address.pid == 0 {
            address.pid = NEXT_NETLINK_PORT_ID.fetch_add(1, Ordering::Relaxed) as u32;
        }
        *address
    }

    pub fn send(
        &self,
        message: &[u8],
        destination: Option<NetlinkSocketAddress>,
    ) -> SocketResult<usize> {
        if self.protocol == NETLINK_ROUTE {
            self.handle_route_messages(message);
            return Ok(message.len());
        }

        if self.protocol == NETLINK_AUDIT {
            self.enqueue_ack(message);
            return Ok(message.len());
        }

        if self.protocol != NETLINK_KOBJECT_UEVENT {
            return Ok(message.len());
        }

        let Some(destination) = destination else {
            return Err(SocketError::InvalidArguments);
        };
        if destination.pid == 0 && destination.groups == 0 {
            return Err(SocketError::InvalidArguments);
        }

        let sender = self.local_address();
        let process = get_current_process();
        let process = process.lock();
        let uid = process.effective_uid;
        let gid = process.effective_gid;
        drop(process);

        let source = NetlinkSocketAddress {
            pid: sender.pid,
            groups: if destination.groups != 0 {
                destination.groups
            } else {
                0
            },
        };

        let mut delivered = 0usize;
        let mut sockets = NETLINK_SOCKETS.lock();
        sockets.retain(|socket| {
            let Some(socket) = socket.upgrade() else {
                return false;
            };
            if socket.protocol != NETLINK_KOBJECT_UEVENT {
                return true;
            }

            let should_deliver = if destination.groups != 0 {
                socket.receives_group(destination.groups)
            } else {
                socket.local_address().pid == destination.pid
            };

            if should_deliver {
                socket.queue_message_with_source(message.to_vec(), source, uid, gid);
                delivered += 1;
            }
            true
        });

        if delivered == 0 {
            return Err(SocketError::ConnectionRefused);
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
                SO_PASSCRED => {
                    let enabled = Self::decode_u32(option_value)? != 0;
                    *self.pass_cred.lock() = enabled;
                    Ok(())
                }
                SO_PRIORITY => {
                    let priority = Self::decode_i32(option_value)?;
                    can_set_socket_priority(priority)?;
                    *self.priority.lock() = priority;
                    Ok(())
                }
                SO_REUSEADDR | SO_SNDBUF | SO_RCVBUF | SO_SNDBUFFORCE | SO_RCVBUFFORCE
                | SO_ATTACH_FILTER | SO_DETACH_FILTER | SO_PASSSEC | SO_PASSRIGHTS
                | SO_PASSPIDFD | SO_TIMESTAMP_OLD | SO_TIMESTAMP_NEW | SO_TIMESTAMPNS_OLD
                | SO_TIMESTAMPNS_NEW => Ok(()),
                SO_RCVTIMEO_OLD | SO_SNDTIMEO_OLD | SO_RCVTIMEO_NEW | SO_SNDTIMEO_NEW => {
                    let expected_len = socket_timeout_option_len(option_name)
                        .ok_or(SocketError::InvalidArguments)?;
                    if option_value.len() < expected_len {
                        return Err(SocketError::InvalidArguments);
                    }
                    Ok(())
                }
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
                SO_PRIORITY => Self::encode_i32(option_len, *self.priority.lock()),
                SO_PASSCRED => Self::encode_i32(option_len, self.pass_cred_enabled() as i32),
                SO_REUSEADDR | SO_PASSSEC | SO_PASSRIGHTS | SO_PASSPIDFD | SO_TIMESTAMP_OLD
                | SO_TIMESTAMP_NEW | SO_TIMESTAMPNS_OLD | SO_TIMESTAMPNS_NEW => {
                    Self::encode_i32(option_len, 0)
                }
                SO_RCVTIMEO_OLD | SO_SNDTIMEO_OLD | SO_RCVTIMEO_NEW | SO_SNDTIMEO_NEW => {
                    let expected_len = socket_timeout_option_len(option_name)
                        .ok_or(SocketError::InvalidArguments)?;
                    Self::encode_zeroed_bytes(option_len, expected_len)
                }
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

    fn decode_i32(option_value: &[u8]) -> SocketResult<i32> {
        if option_value.len() < core::mem::size_of::<i32>() {
            return Err(SocketError::InvalidArguments);
        }

        Ok(i32::from_ne_bytes(
            option_value[..core::mem::size_of::<i32>()]
                .try_into()
                .map_err(|_| SocketError::InvalidArguments)?,
        ))
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

    fn encode_zeroed_bytes(option_len: usize, expected_len: usize) -> SocketResult<Vec<u8>> {
        if option_len < expected_len {
            return Err(SocketError::InvalidArguments);
        }

        Ok(vec![0; expected_len])
    }

    fn enqueue_ack(&self, message: &[u8]) {
        if message.len() < core::mem::size_of::<NetlinkMessageHeader>() {
            return;
        }

        let header =
            unsafe { core::ptr::read_unaligned(message.as_ptr().cast::<NetlinkMessageHeader>()) };
        self.enqueue_ack_from_header(header);
    }

    pub(super) fn enqueue_ack_from_header(&self, header: NetlinkMessageHeader) {
        self.enqueue_error_response(header, 0);
    }

    pub(super) fn enqueue_error_response(&self, header: NetlinkMessageHeader, error: i32) {
        let reply_len = core::mem::size_of::<NetlinkMessageHeader>()
            + core::mem::size_of::<NetlinkErrorMessage>();
        let reply_header = NetlinkMessageHeader {
            nlmsg_len: reply_len as u32,
            nlmsg_type: NLMSG_ERROR,
            nlmsg_flags: 0,
            nlmsg_seq: header.nlmsg_seq,
            nlmsg_pid: self.local_address().pid,
        };
        let error = NetlinkErrorMessage { error, header };

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
        self.queue_message(bytes);
    }

    fn is_nonblocking(&self) -> bool {
        self.flags.lock().contains(FileFlags::NONBLOCK)
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
