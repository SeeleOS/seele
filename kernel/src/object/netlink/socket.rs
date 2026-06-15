use crate::memory::utils::Mut;
use alloc::{
    collections::VecDeque,
    sync::{Arc, Weak},
    vec::Vec,
};
use core::sync::atomic::{AtomicU64, Ordering};
use lazy_static::lazy_static;

use crate::{
    object::{
        FileFlags,
        error::ObjectError,
        linux_anon::wake_linux_io_waiters,
        misc::{ObjectRef, ObjectResult},
    },
    polling::event::PollableEvent,
    process::manager::get_current_process,
    socket::{
        AF_NETLINK, NETLINK_AUDIT, NETLINK_KOBJECT_UEVENT, NETLINK_ROUTE, SOCK_CLOEXEC, SOCK_DGRAM,
        SOCK_NONBLOCK, SOCK_RAW, SocketError, SocketResult,
    },
};

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
    pub(super) flags: Mut<FileFlags>,
    pub(super) pass_cred: Mut<bool>,
    pub(super) priority: Mut<i32>,
    pub(super) socket_type: u64,
    pub(super) protocol: u64,
    address: Mut<NetlinkSocketAddress>,
    pub(super) memberships: Mut<Vec<u32>>,
    recv_queue: Mut<VecDeque<QueuedNetlinkMessage>>,
    self_ref: Mut<Option<Weak<NetlinkSocketObject>>>,
}

impl NetlinkSocketObject {
    pub(super) fn parse_sockaddr(address: &[u8]) -> SocketResult<NetlinkSocketAddress> {
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

    pub(super) fn has_pending_messages(&self) -> bool {
        !self.recv_queue.lock().is_empty()
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
