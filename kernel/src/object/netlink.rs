use alloc::{
    collections::VecDeque,
    format,
    sync::{Arc, Weak},
    vec,
    vec::Vec,
};
use core::sync::atomic::{AtomicU64, Ordering};
use lazy_static::lazy_static;
use spin::Mutex;

use crate::{
    filesystem::info::LinuxStat,
    impl_cast_function, impl_cast_function_non_trait, net,
    object::{
        FileFlags, Object,
        config::ConfigurateRequest,
        error::ObjectError,
        linux_anon::wake_linux_io_waiters,
        misc::{ObjectRef, ObjectResult},
        traits::{Configuratable, Readable, Statable},
    },
    polling::{event::PollableEvent, object::Pollable},
    process::manager::get_current_process,
    s_println,
    socket::{
        AF_NETLINK, NETLINK_ADD_MEMBERSHIP, NETLINK_AUDIT, NETLINK_DROP_MEMBERSHIP,
        NETLINK_EXT_ACK, NETLINK_GET_STRICT_CHK, NETLINK_KOBJECT_UEVENT, NETLINK_LIST_MEMBERSHIPS,
        NETLINK_PKTINFO, NETLINK_ROUTE, SO_ATTACH_FILTER, SO_DETACH_FILTER, SO_DOMAIN, SO_ERROR,
        SO_PASSCRED, SO_PASSPIDFD, SO_PASSRIGHTS, SO_PASSSEC, SO_PROTOCOL, SO_RCVBUF,
        SO_RCVBUFFORCE, SO_RCVTIMEO_NEW, SO_RCVTIMEO_OLD, SO_REUSEADDR, SO_SNDBUF, SO_SNDBUFFORCE,
        SO_SNDTIMEO_NEW, SO_SNDTIMEO_OLD, SO_TIMESTAMP_NEW, SO_TIMESTAMP_OLD, SO_TIMESTAMPNS_NEW,
        SO_TIMESTAMPNS_OLD, SO_TYPE, SOCK_CLOEXEC, SOCK_DGRAM, SOCK_NONBLOCK, SOCK_RAW,
        SOL_NETLINK, SOL_SOCKET, SocketError, SocketLike, SocketResult, socket_timeout_option_len,
    },
    thread::THREAD_MANAGER,
};

const DEFAULT_SOCKET_BUFFER_SIZE: i32 = 64 * 1024;
const FIONBIO: u64 = 0x5421;
const FIOCLEX: u64 = 0x5451;
const S_IFSOCK: u32 = 0o140000;
const AF_INET: u8 = 2;
const ARPHRD_ETHER: u16 = 1;
const ARPHRD_LOOPBACK: u16 = 772;
const IFA_ADDRESS: u16 = 1;
const IFA_LOCAL: u16 = 2;
const IFA_LABEL: u16 = 3;
const IFA_FLAGS: u16 = 8;
const IFA_F_PERMANENT: u8 = 0x80;
const IFF_UP: u32 = 1 << 0;
const IFF_BROADCAST: u32 = 1 << 1;
const IFF_LOOPBACK: u32 = 1 << 3;
const IFF_RUNNING: u32 = 1 << 6;
const IFF_MULTICAST: u32 = 1 << 12;
const IFF_LOWER_UP: u32 = 1 << 16;
const IFLA_ADDRESS: u16 = 1;
const IFLA_BROADCAST: u16 = 2;
const IFLA_IFNAME: u16 = 3;
const IFLA_MTU: u16 = 4;
const IFLA_QDISC: u16 = 6;
const IFLA_TXQLEN: u16 = 13;
const IFLA_OPERSTATE: u16 = 16;
const IFLA_LINKMODE: u16 = 17;
const IFLA_NUM_TX_QUEUES: u16 = 31;
const IFLA_NUM_RX_QUEUES: u16 = 32;
const IFLA_ALT_IFNAME: u16 = 53;
const IFLA_PERM_ADDRESS: u16 = 54;
const NLMSG_ERROR: u16 = 0x2;
const NLMSG_DONE: u16 = 0x3;
const NLM_F_MULTI: u16 = 0x2;
const NLM_F_DUMP: u16 = 0x300;
const RTM_NEWLINK: u16 = 16;
const RTM_GETLINK: u16 = 18;
const RTM_NEWADDR: u16 = 20;
const RTM_GETADDR: u16 = 22;
const RT_SCOPE_UNIVERSE: u8 = 0;
const RT_SCOPE_HOST: u8 = 254;
const IF_OPER_UP: u8 = 6;
static NEXT_UEVENT_SEQNUM: AtomicU64 = AtomicU64::new(1);
static NEXT_NETLINK_PORT_ID: AtomicU64 = AtomicU64::new(1);

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

#[repr(C)]
#[derive(Clone, Copy)]
struct UdevMonitorNetlinkHeader {
    prefix: [u8; 8],
    magic: u32,
    header_size: u32,
    properties_off: u32,
    properties_len: u32,
    filter_subsystem_hash: u32,
    filter_devtype_hash: u32,
    filter_tag_bloom_hi: u32,
    filter_tag_bloom_lo: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct IfInfoMessage {
    ifi_family: u8,
    ifi_pad: u8,
    ifi_type: u16,
    ifi_index: i32,
    ifi_flags: u32,
    ifi_change: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct IfAddrMessage {
    ifa_family: u8,
    ifa_prefixlen: u8,
    ifa_flags: u8,
    ifa_scope: u8,
    ifa_index: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct RouteAttributeHeader {
    rta_len: u16,
    rta_type: u16,
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
    flags: Mutex<FileFlags>,
    pass_cred: Mutex<bool>,
    socket_type: u64,
    protocol: u64,
    address: Mutex<NetlinkSocketAddress>,
    memberships: Mutex<Vec<u32>>,
    recv_queue: Mutex<VecDeque<QueuedNetlinkMessage>>,
    self_ref: Mutex<Option<Weak<NetlinkSocketObject>>>,
}

impl NetlinkSocketObject {
    fn debug_key<'a>(payload: &'a [u8], key: &str) -> Option<&'a str> {
        payload
            .split(|byte| *byte == 0)
            .find_map(|field| field.strip_prefix(key.as_bytes()))
            .and_then(|value| core::str::from_utf8(value).ok())
    }

    fn debug_libudev_message(message: &[u8]) -> Option<(&str, &str)> {
        if !message.starts_with(b"libudev\0") {
            return None;
        }
        if message.len() < core::mem::size_of::<UdevMonitorNetlinkHeader>() {
            return None;
        }
        let header = unsafe { &*(message.as_ptr() as *const UdevMonitorNetlinkHeader) };
        let offset = header.properties_off as usize;
        if offset >= message.len() {
            return None;
        }
        let payload = &message[offset..];
        Some((
            Self::debug_key(payload, "ACTION=")?,
            Self::debug_key(payload, "DEVPATH=")?,
        ))
    }

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
            flags: Mutex::new(FileFlags::empty()),
            pass_cred: Mutex::new(false),
            socket_type,
            protocol,
            address: Mutex::new(NetlinkSocketAddress::default()),
            memberships: Mutex::new(Vec::new()),
            recv_queue: Mutex::new(VecDeque::new()),
            self_ref: Mutex::new(None),
        });
        *socket.self_ref.lock() = Some(Arc::downgrade(&socket));
        if protocol == NETLINK_KOBJECT_UEVENT {
            s_println!("netlink kobject create type={socket_type:#x}");
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

    fn wake_read_waiters(&self) {
        wake_linux_io_waiters();
        let Some(object) = self.self_object() else {
            return;
        };
        THREAD_MANAGER
            .get()
            .unwrap()
            .lock()
            .wake_poller(object, PollableEvent::CanBeRead);
    }

    fn queue_message(&self, message: Vec<u8>) {
        self.queue_message_with_source(message, NetlinkSocketAddress::default(), 0, 0);
    }

    fn queue_message_with_source(
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
        if self.protocol == NETLINK_KOBJECT_UEVENT {
            s_println!(
                "netlink kobject bind pid={} groups=0x{:x}",
                address.pid,
                address.groups
            );
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

    fn receives_group(&self, group: u32) -> bool {
        let address_groups = self.address.lock().groups;
        if (address_groups & group) != 0 {
            return true;
        }

        self.memberships.lock().contains(&group)
    }

    fn local_address(&self) -> NetlinkSocketAddress {
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
            self.handle_route_message(message);
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

        if let Some((action, devpath)) = Self::debug_libudev_message(message) {
            s_println!(
                "netlink kobject send pid={} groups=0x{:x} libudev action={} devpath={} delivered={}",
                destination.pid,
                destination.groups,
                action,
                devpath,
                delivered
            );
        } else {
            s_println!(
                "netlink kobject send pid={} groups=0x{:x} libudev={} delivered={}",
                destination.pid,
                destination.groups,
                message.starts_with(b"libudev\0"),
                delivered
            );
        }

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
                if self.protocol == NETLINK_KOBJECT_UEVENT {
                    s_println!(
                        "netlink kobject membership opt={} group=0x{:x} now={:?}",
                        option_name,
                        group,
                        *memberships
                    );
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

    fn handle_route_message(&self, message: &[u8]) {
        let Some((header, payload)) = self.request_header_and_payload(message) else {
            return;
        };
        let reply_pid = self.local_address().pid;

        match header.nlmsg_type {
            RTM_GETLINK => self.handle_get_link(header, payload, reply_pid),
            RTM_GETADDR => self.handle_get_addr(header, payload, reply_pid),
            _ => self.enqueue_error_response(header, 0),
        }
    }

    fn request_header_and_payload<'a>(
        &self,
        message: &'a [u8],
    ) -> Option<(NetlinkMessageHeader, &'a [u8])> {
        if message.len() < core::mem::size_of::<NetlinkMessageHeader>() {
            return None;
        }

        let header =
            unsafe { core::ptr::read_unaligned(message.as_ptr().cast::<NetlinkMessageHeader>()) };
        let message_len = usize::try_from(header.nlmsg_len)
            .ok()
            .map(|len| len.min(message.len()))?;
        if message_len < core::mem::size_of::<NetlinkMessageHeader>() {
            return None;
        }

        Some((
            header,
            &message[core::mem::size_of::<NetlinkMessageHeader>()..message_len],
        ))
    }

    fn handle_get_link(&self, header: NetlinkMessageHeader, payload: &[u8], reply_pid: u32) {
        let request = Self::read_struct_prefix::<IfInfoMessage>(payload).unwrap_or(IfInfoMessage {
            ifi_family: 0,
            ifi_pad: 0,
            ifi_type: 0,
            ifi_index: 0,
            ifi_flags: 0,
            ifi_change: 0,
        });
        let attrs_offset = core::mem::size_of::<IfInfoMessage>().min(payload.len());
        let request_name = Self::find_attribute(payload, attrs_offset, IFLA_IFNAME)
            .and_then(Self::parse_netlink_string);
        let request_alt_name = Self::find_attribute(payload, attrs_offset, IFLA_ALT_IFNAME)
            .and_then(Self::parse_netlink_string);
        let dump = (header.nlmsg_flags & NLM_F_DUMP) != 0;

        let mut matched = Vec::new();
        for interface in net::interfaces() {
            if request.ifi_index > 0 && interface.index != request.ifi_index {
                continue;
            }
            if request_name.is_some_and(|name| interface.name != name) {
                continue;
            }
            if request_alt_name.is_some() {
                continue;
            }
            matched.push(interface);
        }

        let should_dump = dump
            || (request.ifi_index == 0 && request_name.is_none() && request_alt_name.is_none());
        if should_dump {
            for interface in matched {
                self.queue_message(Self::encode_link_message(
                    header, interface, true, reply_pid,
                ));
            }
            self.queue_message(Self::encode_done_message(header.nlmsg_seq, reply_pid));
            return;
        }

        if let Some(interface) = matched.into_iter().next() {
            self.queue_message(Self::encode_link_message(
                header, interface, false, reply_pid,
            ));
        } else {
            self.enqueue_error_response(header, -19);
        }
    }

    fn handle_get_addr(&self, header: NetlinkMessageHeader, payload: &[u8], reply_pid: u32) {
        let request = Self::read_struct_prefix::<IfAddrMessage>(payload).unwrap_or(IfAddrMessage {
            ifa_family: 0,
            ifa_prefixlen: 0,
            ifa_flags: 0,
            ifa_scope: 0,
            ifa_index: 0,
        });
        let dump = (header.nlmsg_flags & NLM_F_DUMP) != 0;
        let request_index = i32::try_from(request.ifa_index).unwrap_or(0);

        let mut matched = Vec::new();
        for interface in net::interfaces() {
            let Some((addr, prefix_len)) = interface.ipv4 else {
                continue;
            };
            if request.ifa_family != 0 && request.ifa_family != AF_INET {
                continue;
            }
            if request_index > 0 && interface.index != request_index {
                continue;
            }
            matched.push((interface, addr, prefix_len));
        }

        let should_dump = dump || request_index == 0;
        if should_dump {
            for (interface, addr, prefix_len) in matched {
                self.queue_message(Self::encode_addr_message(
                    header, interface, addr, prefix_len, true, reply_pid,
                ));
            }
            self.queue_message(Self::encode_done_message(header.nlmsg_seq, reply_pid));
            return;
        }

        if let Some((interface, addr, prefix_len)) = matched.into_iter().next() {
            self.queue_message(Self::encode_addr_message(
                header, interface, addr, prefix_len, false, reply_pid,
            ));
        } else {
            self.enqueue_error_response(header, 0);
        }
    }

    fn encode_link_message(
        request: NetlinkMessageHeader,
        interface: net::NetworkInterfaceInfo,
        multipart: bool,
        reply_pid: u32,
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        Self::append_struct(
            &mut bytes,
            &NetlinkMessageHeader {
                nlmsg_len: 0,
                nlmsg_type: RTM_NEWLINK,
                nlmsg_flags: if multipart { NLM_F_MULTI } else { 0 },
                nlmsg_seq: request.nlmsg_seq,
                nlmsg_pid: reply_pid,
            },
        );
        Self::append_struct(
            &mut bytes,
            &IfInfoMessage {
                ifi_family: 0,
                ifi_pad: 0,
                ifi_type: if interface.loopback {
                    ARPHRD_LOOPBACK
                } else {
                    ARPHRD_ETHER
                },
                ifi_index: interface.index,
                ifi_flags: if interface.loopback {
                    IFF_UP | IFF_LOOPBACK | IFF_RUNNING | IFF_LOWER_UP
                } else {
                    IFF_UP | IFF_BROADCAST | IFF_RUNNING | IFF_MULTICAST | IFF_LOWER_UP
                },
                ifi_change: u32::MAX,
            },
        );
        Self::append_string_attribute(&mut bytes, IFLA_IFNAME, interface.name);
        Self::append_attribute(&mut bytes, IFLA_ADDRESS, &interface.mac);
        Self::append_attribute(&mut bytes, IFLA_PERM_ADDRESS, &interface.mac);
        if !interface.loopback {
            Self::append_attribute(&mut bytes, IFLA_BROADCAST, &[0xff; 6]);
        }
        Self::append_u32_attribute(&mut bytes, IFLA_MTU, interface.mtu);
        Self::append_string_attribute(
            &mut bytes,
            IFLA_QDISC,
            if interface.loopback {
                "noqueue"
            } else {
                "fq_codel"
            },
        );
        Self::append_u32_attribute(&mut bytes, IFLA_TXQLEN, 1_000);
        Self::append_u8_attribute(&mut bytes, IFLA_OPERSTATE, IF_OPER_UP);
        Self::append_u8_attribute(&mut bytes, IFLA_LINKMODE, 0);
        Self::append_u32_attribute(&mut bytes, IFLA_NUM_TX_QUEUES, 1);
        Self::append_u32_attribute(&mut bytes, IFLA_NUM_RX_QUEUES, 1);
        Self::finalize_message_length(&mut bytes);
        bytes
    }

    fn encode_addr_message(
        request: NetlinkMessageHeader,
        interface: net::NetworkInterfaceInfo,
        addr: [u8; 4],
        prefix_len: u8,
        multipart: bool,
        reply_pid: u32,
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        Self::append_struct(
            &mut bytes,
            &NetlinkMessageHeader {
                nlmsg_len: 0,
                nlmsg_type: RTM_NEWADDR,
                nlmsg_flags: if multipart { NLM_F_MULTI } else { 0 },
                nlmsg_seq: request.nlmsg_seq,
                nlmsg_pid: reply_pid,
            },
        );
        Self::append_struct(
            &mut bytes,
            &IfAddrMessage {
                ifa_family: AF_INET,
                ifa_prefixlen: prefix_len,
                ifa_flags: IFA_F_PERMANENT,
                ifa_scope: if interface.loopback {
                    RT_SCOPE_HOST
                } else {
                    RT_SCOPE_UNIVERSE
                },
                ifa_index: interface.index as u32,
            },
        );
        Self::append_attribute(&mut bytes, IFA_ADDRESS, &addr);
        Self::append_attribute(&mut bytes, IFA_LOCAL, &addr);
        Self::append_string_attribute(&mut bytes, IFA_LABEL, interface.name);
        Self::append_u32_attribute(&mut bytes, IFA_FLAGS, u32::from(IFA_F_PERMANENT));
        Self::finalize_message_length(&mut bytes);
        bytes
    }

    fn encode_done_message(seq: u32, reply_pid: u32) -> Vec<u8> {
        let header = NetlinkMessageHeader {
            nlmsg_len: core::mem::size_of::<NetlinkMessageHeader>() as u32,
            nlmsg_type: NLMSG_DONE,
            nlmsg_flags: NLM_F_MULTI,
            nlmsg_seq: seq,
            nlmsg_pid: reply_pid,
        };
        let mut bytes = Vec::new();
        Self::append_struct(&mut bytes, &header);
        bytes
    }

    fn append_attribute(bytes: &mut Vec<u8>, attr_type: u16, payload: &[u8]) {
        let attr_len = core::mem::size_of::<RouteAttributeHeader>() + payload.len();
        let header = RouteAttributeHeader {
            rta_len: attr_len as u16,
            rta_type: attr_type,
        };
        Self::append_struct(bytes, &header);
        bytes.extend_from_slice(payload);
        while !bytes.len().is_multiple_of(4) {
            bytes.push(0);
        }
    }

    fn append_string_attribute(bytes: &mut Vec<u8>, attr_type: u16, value: &str) {
        let mut payload = value.as_bytes().to_vec();
        payload.push(0);
        Self::append_attribute(bytes, attr_type, &payload);
    }

    fn append_u8_attribute(bytes: &mut Vec<u8>, attr_type: u16, value: u8) {
        Self::append_attribute(bytes, attr_type, &[value]);
    }

    fn append_u32_attribute(bytes: &mut Vec<u8>, attr_type: u16, value: u32) {
        Self::append_attribute(bytes, attr_type, &value.to_ne_bytes());
    }

    fn append_struct<T>(bytes: &mut Vec<u8>, value: &T) {
        bytes.extend_from_slice(unsafe {
            core::slice::from_raw_parts((value as *const T).cast::<u8>(), core::mem::size_of::<T>())
        });
    }

    fn finalize_message_length(bytes: &mut [u8]) {
        let header = NetlinkMessageHeader {
            nlmsg_len: bytes.len() as u32,
            nlmsg_type: u16::from_ne_bytes([bytes[4], bytes[5]]),
            nlmsg_flags: u16::from_ne_bytes([bytes[6], bytes[7]]),
            nlmsg_seq: u32::from_ne_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
            nlmsg_pid: u32::from_ne_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
        };
        bytes[..core::mem::size_of::<NetlinkMessageHeader>()].copy_from_slice(unsafe {
            core::slice::from_raw_parts(
                (&header as *const NetlinkMessageHeader).cast::<u8>(),
                core::mem::size_of::<NetlinkMessageHeader>(),
            )
        });
    }

    fn find_attribute(payload: &[u8], mut offset: usize, attr_type: u16) -> Option<&[u8]> {
        while offset + core::mem::size_of::<RouteAttributeHeader>() <= payload.len() {
            let header = unsafe {
                core::ptr::read_unaligned(payload[offset..].as_ptr().cast::<RouteAttributeHeader>())
            };
            let attr_len = usize::from(header.rta_len);
            if attr_len < core::mem::size_of::<RouteAttributeHeader>() {
                return None;
            }
            let attr_end = offset.checked_add(attr_len)?;
            if attr_end > payload.len() {
                return None;
            }
            if header.rta_type == attr_type {
                return Some(
                    &payload[offset + core::mem::size_of::<RouteAttributeHeader>()..attr_end],
                );
            }
            offset = Self::align_to_4(attr_end);
        }
        None
    }

    fn parse_netlink_string(bytes: &[u8]) -> Option<&str> {
        let bytes = bytes.strip_suffix(&[0]).unwrap_or(bytes);
        core::str::from_utf8(bytes).ok()
    }

    fn read_struct_prefix<T: Copy>(bytes: &[u8]) -> Option<T> {
        if bytes.len() < core::mem::size_of::<T>() {
            return None;
        }
        Some(unsafe { core::ptr::read_unaligned(bytes.as_ptr().cast::<T>()) })
    }

    fn align_to_4(value: usize) -> usize {
        (value + 3) & !3
    }

    fn enqueue_ack(&self, message: &[u8]) {
        if message.len() < core::mem::size_of::<NetlinkMessageHeader>() {
            return;
        }

        let header =
            unsafe { core::ptr::read_unaligned(message.as_ptr().cast::<NetlinkMessageHeader>()) };
        self.enqueue_error_response(header, 0);
    }

    fn enqueue_error_response(&self, header: NetlinkMessageHeader, error: i32) {
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

pub fn broadcast_kobject_uevent(action: &str, devpath: &str, extra_env: &[u8]) {
    let seqnum = NEXT_UEVENT_SEQNUM.fetch_add(1, Ordering::Relaxed);
    let mut message =
        format!("{action}@{devpath}\0ACTION={action}\0DEVPATH={devpath}\0").into_bytes();
    for line in extra_env
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        if line.starts_with(b"ACTION=")
            || line.starts_with(b"DEVPATH=")
            || line.starts_with(b"SEQNUM=")
        {
            continue;
        }
        message.extend_from_slice(line);
        message.push(0);
    }
    message.extend_from_slice(format!("SEQNUM={seqnum}\0").as_bytes());

    let mut sockets = NETLINK_SOCKETS.lock();
    let mut delivered_sockets = Vec::new();
    let mut total = 0usize;
    sockets.retain(|socket| {
        let Some(socket) = socket.upgrade() else {
            return false;
        };
        total += 1;
        if socket.protocol == NETLINK_KOBJECT_UEVENT && socket.receives_group(1) {
            socket.queue_message_with_source(
                message.clone(),
                NetlinkSocketAddress { pid: 0, groups: 1 },
                0,
                0,
            );
            delivered_sockets.push(socket);
        }
        true
    });
    drop(sockets);
    s_println!(
        "kobject uevent action={} devpath={} sockets={} delivered={}",
        action,
        devpath,
        total,
        delivered_sockets.len()
    );

    for socket in delivered_sockets {
        socket.wake_read_waiters();
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
