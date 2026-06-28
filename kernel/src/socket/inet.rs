use alloc::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::{Arc, Weak},
    vec,
    vec::Vec,
};
use core::{mem, slice};

use crate::memory::utils::Mut;
use conquer_once::spin::OnceCell;

use crate::{
    filesystem::info::LinuxStat,
    impl_cast_function, impl_cast_function_non_trait,
    memory::user_safe,
    net::{self, InetAddress, NetError, NetSocketHandle, TransportKind},
    object::{
        FileFlags, Object,
        config::ConfigurateRequest,
        error::ObjectError,
        linux_ioctl::{LinuxIoctlOp, socket_raw_ioctl_op},
        misc::ObjectRef,
        misc::ObjectResult,
        traits::{Configuratable, Readable, Statable, Writable},
    },
    polling::{event::PollableEvent, object::Pollable},
    process::manager::get_current_process,
    thread::yielding::{
        BlockType, WakeType, cancel_block, finish_block_current, prepare_block_current,
        wake_pollers_for_object,
    },
};

use super::{
    AF_INET, AF_INET6, IPPROTO_SCTP, IPPROTO_TCP, IPPROTO_UDP, IPPROTO_UDPLITE, SO_ACCEPTCONN,
    SO_DOMAIN, SO_ERROR, SO_PRIORITY, SO_PROTOCOL, SO_RCVBUF, SO_RCVBUFFORCE, SO_RCVTIMEO_NEW,
    SO_RCVTIMEO_OLD, SO_REUSEADDR, SO_SNDBUF, SO_SNDBUFFORCE, SO_SNDTIMEO_NEW, SO_SNDTIMEO_OLD,
    SO_TYPE, SOCK_CLOEXEC, SOCK_DGRAM, SOCK_NONBLOCK, SOCK_RAW, SOCK_STREAM, SOL_IP, SOL_SOCKET,
    SOL_TCP, SocketError, SocketLike, SocketResult, can_set_socket_priority, self_ref::object_ref,
    socket_timeout_option_len, wait::wait_for_object_event,
};

const DEFAULT_SOCKET_BUFFER_SIZE: i32 = 64 * 1024;
const CAP_NET_BIND_SERVICE: usize = 10;
const SOL_IPV6: u64 = 41;
const IPV6_ADDRFORM: u64 = 1;
const IP_ADD_MEMBERSHIP: u64 = 35;
const IP_DROP_MEMBERSHIP: u64 = 36;
const MCAST_JOIN_GROUP: u64 = 42;
const MCAST_LEAVE_GROUP: u64 = 45;
const IP_MULTICAST_ALL: u64 = 49;
const MAX_UDP_PAYLOAD_SIZE: usize = 65_507;

type LocalListenerKey = (u64, [u8; 4], u16);

static LOCAL_LISTENERS: OnceCell<Mut<BTreeMap<LocalListenerKey, Weak<InetSocketObject>>>> =
    OnceCell::uninit();
static LOCAL_DATAGRAMS: OnceCell<Mut<BTreeMap<LocalListenerKey, Weak<InetSocketObject>>>> =
    OnceCell::uninit();

fn local_listeners() -> &'static Mut<BTreeMap<LocalListenerKey, Weak<InetSocketObject>>> {
    LOCAL_LISTENERS.get_or_init(|| Mut::new(BTreeMap::new()))
}

fn local_datagrams() -> &'static Mut<BTreeMap<LocalListenerKey, Weak<InetSocketObject>>> {
    LOCAL_DATAGRAMS.get_or_init(|| Mut::new(BTreeMap::new()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InetSocketKind {
    Stream,
    Datagram,
}

#[derive(Debug, Clone)]
struct InetState {
    handle: NetSocketHandle,
    local: Option<InetAddress>,
    peer: Option<InetAddress>,
    local_stream: Option<LocalStreamEndpoint>,
    local_datagrams: VecDeque<LocalDatagramMessage>,
    listening: bool,
    read_shutdown: bool,
    write_shutdown: bool,
}

#[derive(Debug, Clone)]
struct LocalDatagramMessage {
    data: Vec<u8>,
    source: InetAddress,
}

#[derive(Debug, Clone)]
struct LocalStreamEndpoint {
    incoming: Arc<Mut<VecDeque<u8>>>,
    outgoing: Arc<Mut<VecDeque<u8>>>,
    local_write_closed: Arc<Mut<bool>>,
    peer_write_closed: Arc<Mut<bool>>,
    peer_closed: Arc<Mut<bool>>,
    peer: Weak<InetSocketObject>,
}

impl LocalStreamEndpoint {
    fn pair(client: &Arc<InetSocketObject>, server: &Arc<InetSocketObject>) -> (Self, Self) {
        let client_to_server = Arc::new(Mut::new(VecDeque::new()));
        let server_to_client = Arc::new(Mut::new(VecDeque::new()));
        let client_write_closed = Arc::new(Mut::new(false));
        let server_write_closed = Arc::new(Mut::new(false));
        let client_closed = Arc::new(Mut::new(false));
        let server_closed = Arc::new(Mut::new(false));
        (
            Self {
                incoming: server_to_client.clone(),
                outgoing: client_to_server.clone(),
                local_write_closed: client_write_closed.clone(),
                peer_write_closed: server_write_closed.clone(),
                peer_closed: server_closed.clone(),
                peer: Arc::downgrade(server),
            },
            Self {
                incoming: client_to_server,
                outgoing: server_to_client,
                local_write_closed: server_write_closed,
                peer_write_closed: client_write_closed,
                peer_closed: client_closed,
                peer: Arc::downgrade(client),
            },
        )
    }

    fn close_write(&self) {
        *self.local_write_closed.lock() = true;
        self.wake_peer(PollableEvent::CanBeRead);
        self.wake_peer(PollableEvent::ReadClosed);
        crate::socket::wake_io();
    }

    fn close_local(&self) {
        *self.local_write_closed.lock() = true;
        if let Some(peer) = self.peer.upgrade()
            && let Some(peer_endpoint) = peer.state.lock().local_stream.clone()
        {
            *peer_endpoint.peer_closed.lock() = true;
        }
        self.wake_peer(PollableEvent::CanBeRead);
        self.wake_peer(PollableEvent::ReadClosed);
        self.wake_peer(PollableEvent::Closed);
        self.wake_peer(PollableEvent::CanBeWritten);
        crate::socket::wake_io();
    }

    fn wake_peer(&self, event: PollableEvent) {
        if let Some(peer) = self.peer.upgrade()
            && let Some(object) = object_ref(&peer.self_ref)
        {
            wake_pollers_for_object(object as ObjectRef, event);
        }
    }

    fn can_recv(&self) -> bool {
        !self.incoming.lock().is_empty()
            || *self.peer_write_closed.lock()
            || *self.peer_closed.lock()
            || self.peer.upgrade().is_none()
    }
}

#[derive(Debug)]
pub struct InetSocketObject {
    domain: Mut<u64>,
    pub kind: InetSocketKind,
    state: Mut<InetState>,
    flags: Mut<FileFlags>,
    priority: Mut<i32>,
    multicast_groups: Mut<BTreeSet<Vec<u8>>>,
    pending_local_streams: Mut<VecDeque<Arc<InetSocketObject>>>,
    local_listener_key: Mut<Option<LocalListenerKey>>,
    local_backlog: Mut<usize>,
    self_ref: Mut<Option<Weak<InetSocketObject>>>,
}

#[derive(Clone, Copy)]
enum InetWaitKind {
    Connect,
    Accept,
    Send,
    Recv,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct LinuxSockAddrIn {
    sin_family: u16,
    sin_port: u16,
    sin_addr: [u8; 4],
    sin_zero: [u8; 8],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct LinuxSockAddrIn6 {
    sin6_family: u16,
    sin6_port: u16,
    sin6_flowinfo: u32,
    sin6_addr: [u8; 16],
    sin6_scope_id: u32,
}

impl InetSocketObject {
    pub(crate) fn decode_addr_for_domain(domain: u64, address: &[u8]) -> SocketResult<InetAddress> {
        let family = if address.len() >= mem::size_of::<u16>() {
            u16::from_ne_bytes(address[..2].try_into().unwrap())
        } else {
            return Err(SocketError::InvalidArguments);
        };

        if domain == AF_INET {
            if address.len() < mem::size_of::<LinuxSockAddrIn>() {
                return Err(SocketError::InvalidArguments);
            }
            let sockaddr = unsafe { &*(address.as_ptr().cast::<LinuxSockAddrIn>()) };
            if u64::from(sockaddr.sin_family) != AF_INET {
                return Err(SocketError::AddressFamilyNotSupported);
            }
            return Ok(InetAddress::new(
                sockaddr.sin_addr,
                u16::from_be(sockaddr.sin_port),
            ));
        }

        if domain != AF_INET6 {
            return Err(SocketError::AddressFamilyNotSupported);
        }
        if address.len() < mem::size_of::<LinuxSockAddrIn6>() {
            return Err(SocketError::InvalidArguments);
        }
        let sockaddr = unsafe { &*(address.as_ptr().cast::<LinuxSockAddrIn6>()) };
        if u64::from(family) != AF_INET6 || u64::from(sockaddr.sin6_family) != AF_INET6 {
            return Err(SocketError::AddressFamilyNotSupported);
        }

        let mapped = match sockaddr.sin6_addr {
            [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0] => [0, 0, 0, 0],
            [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1] => [127, 0, 0, 1],
            [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff, a, b, c, d] => [a, b, c, d],
            _ => return Err(SocketError::AddressNotAvailable),
        };
        Ok(InetAddress::new(mapped, u16::from_be(sockaddr.sin6_port)))
    }

    pub fn create(domain: u64, kind: u64, protocol: u64) -> SocketResult<Arc<Self>> {
        if !matches!(domain, AF_INET | AF_INET6) {
            return Err(SocketError::AddressFamilyNotSupported);
        }

        let socket_type = kind & !(SOCK_NONBLOCK | SOCK_CLOEXEC);
        let transport = match socket_type {
            SOCK_STREAM => match protocol {
                0 | IPPROTO_TCP => TransportKind::Tcp,
                IPPROTO_SCTP => return Err(SocketError::ProtocolNotSupported),
                _ => return Err(SocketError::ProtocolNotSupported),
            },
            SOCK_DGRAM => match protocol {
                0 | IPPROTO_UDP => TransportKind::Udp,
                IPPROTO_UDPLITE => return Err(SocketError::ProtocolNotSupported),
                _ => return Err(SocketError::ProtocolNotSupported),
            },
            SOCK_RAW => return Err(SocketError::ProtocolNotSupported),
            _ => return Err(SocketError::InvalidArguments),
        };

        let handle = net::create_socket(transport).map_err(Self::map_net_error)?;
        let socket = Arc::new(Self {
            domain: Mut::new(domain),
            kind: match transport {
                TransportKind::Tcp => InetSocketKind::Stream,
                TransportKind::Udp => InetSocketKind::Datagram,
            },
            state: Mut::new(InetState {
                handle,
                local: None,
                peer: None,
                local_stream: None,
                local_datagrams: VecDeque::new(),
                listening: false,
                read_shutdown: false,
                write_shutdown: false,
            }),
            flags: Mut::new(FileFlags::empty()),
            priority: Mut::new(0),
            multicast_groups: Mut::new(BTreeSet::new()),
            pending_local_streams: Mut::new(VecDeque::new()),
            local_listener_key: Mut::new(None),
            local_backlog: Mut::new(0),
            self_ref: Mut::new(None),
        });
        *socket.self_ref.lock() = Some(Arc::downgrade(&socket));
        Ok(socket)
    }

    fn from_accepted(
        domain: u64,
        handle: NetSocketHandle,
        local: InetAddress,
        peer: InetAddress,
    ) -> Arc<Self> {
        let socket = Arc::new(Self {
            domain: Mut::new(domain),
            kind: InetSocketKind::Stream,
            state: Mut::new(InetState {
                handle,
                local: Some(local),
                peer: Some(peer),
                local_stream: None,
                local_datagrams: VecDeque::new(),
                listening: false,
                read_shutdown: false,
                write_shutdown: false,
            }),
            flags: Mut::new(FileFlags::empty()),
            priority: Mut::new(0),
            multicast_groups: Mut::new(BTreeSet::new()),
            pending_local_streams: Mut::new(VecDeque::new()),
            local_listener_key: Mut::new(None),
            local_backlog: Mut::new(0),
            self_ref: Mut::new(None),
        });
        *socket.self_ref.lock() = Some(Arc::downgrade(&socket));
        socket
    }

    fn local_listener_keys(namespace_inode: u64, local: InetAddress) -> [LocalListenerKey; 2] {
        [
            (namespace_inode, local.addr, local.port),
            (namespace_inode, [0, 0, 0, 0], local.port),
        ]
    }

    fn register_local_listener(&self, local: InetAddress) {
        let Some(owner) = self.self_ref.lock().clone() else {
            return;
        };
        let key = (
            self.current_handle().namespace_inode(),
            local.addr,
            local.port,
        );
        local_listeners().lock().insert(key, owner);
        *self.local_listener_key.lock() = Some(key);
    }

    fn ensure_local_listener_available(&self, local: InetAddress) -> SocketResult<()> {
        let namespace_inode = self.current_handle().namespace_inode();
        let keys = Self::local_listener_keys(namespace_inode, local);
        let listeners = local_listeners().lock();
        if keys.iter().any(|key| {
            listeners
                .get(key)
                .and_then(Weak::upgrade)
                .is_some_and(|listener| !core::ptr::eq(listener.as_ref(), self))
        }) {
            return Err(SocketError::AddressInUse);
        }
        Ok(())
    }

    fn unregister_local_listener(&self) {
        let Some(key) = self.local_listener_key.lock().take() else {
            return;
        };
        local_listeners().lock().remove(&key);
    }

    fn register_local_datagram(&self, local: InetAddress) {
        let Some(owner) = self.self_ref.lock().clone() else {
            return;
        };
        let key = (
            self.current_handle().namespace_inode(),
            local.addr,
            local.port,
        );
        local_datagrams().lock().insert(key, owner);
    }

    fn unregister_local_datagram(&self, local: InetAddress) {
        let namespace_inode = self.current_handle().namespace_inode();
        local_datagrams()
            .lock()
            .remove(&(namespace_inode, local.addr, local.port));
    }

    fn find_local_listener(namespace_inode: u64, remote: InetAddress) -> Option<Arc<Self>> {
        let listeners = local_listeners().lock();
        for key in Self::local_listener_keys(namespace_inode, remote) {
            if let Some(listener) = listeners.get(&key).and_then(Weak::upgrade) {
                return Some(listener);
            }
        }
        None
    }

    fn find_local_datagram(namespace_inode: u64, remote: InetAddress) -> Option<Arc<Self>> {
        let datagrams = local_datagrams().lock();
        for key in Self::local_listener_keys(namespace_inode, remote) {
            if let Some(socket) = datagrams.get(&key).and_then(Weak::upgrade) {
                return Some(socket);
            }
        }
        None
    }

    fn local_for_remote(mut local: InetAddress, remote: InetAddress) -> InetAddress {
        if local.is_unspecified() {
            local.addr = remote.addr;
        }
        local
    }

    fn is_local_address(addr: [u8; 4]) -> bool {
        net::interfaces()
            .iter()
            .filter_map(|interface| interface.ipv4.map(|(ipv4, _)| ipv4))
            .any(|ipv4| ipv4 == addr)
    }

    fn map_net_error(err: NetError) -> SocketError {
        match err {
            NetError::TryAgain => SocketError::TryAgain,
            NetError::InvalidArguments => SocketError::InvalidArguments,
            NetError::NotConnected => SocketError::NotConnected,
            NetError::AddressInUse => SocketError::AddressInUse,
            NetError::ConnectionRefused => SocketError::ConnectionRefused,
            NetError::BrokenPipe => SocketError::BrokenPipe,
            NetError::NoDevice => SocketError::NetworkDown,
        }
    }

    fn current_handle(&self) -> NetSocketHandle {
        self.state.lock().handle
    }

    fn is_nonblocking(&self) -> bool {
        self.flags.lock().contains(FileFlags::NONBLOCK)
    }

    pub(crate) fn encode_addr_for_domain(domain: u64, addr: InetAddress) -> Vec<u8> {
        if domain == AF_INET6 {
            let sin6_addr = if addr.is_unspecified() {
                [0; 16]
            } else if addr.addr[0] == 127 {
                let mut loopback = [0; 16];
                loopback[15] = 1;
                loopback
            } else {
                let mut mapped = [0; 16];
                mapped[10] = 0xff;
                mapped[11] = 0xff;
                mapped[12..16].copy_from_slice(&addr.addr);
                mapped
            };
            let sockaddr = LinuxSockAddrIn6 {
                sin6_family: AF_INET6 as u16,
                sin6_port: addr.port.to_be(),
                sin6_flowinfo: 0,
                sin6_addr,
                sin6_scope_id: 0,
            };
            return unsafe {
                slice::from_raw_parts(
                    (&sockaddr as *const LinuxSockAddrIn6).cast::<u8>(),
                    mem::size_of::<LinuxSockAddrIn6>(),
                )
            }
            .to_vec();
        }

        let sockaddr = LinuxSockAddrIn {
            sin_family: AF_INET as u16,
            sin_port: addr.port.to_be(),
            sin_addr: addr.addr,
            sin_zero: [0; 8],
        };
        unsafe {
            slice::from_raw_parts(
                (&sockaddr as *const LinuxSockAddrIn).cast::<u8>(),
                mem::size_of::<LinuxSockAddrIn>(),
            )
        }
        .to_vec()
    }

    fn prepare_wait(&self, kind: InetWaitKind) {
        let current = prepare_block_current(BlockType::WakeRequired {
            wake_type: WakeType::IO,
            deadline: None,
        });

        net::poll();
        if self.is_ready_for_io(kind) {
            cancel_block(&current);
        } else {
            finish_block_current();
        }
    }

    fn is_ready_for_io(&self, kind: InetWaitKind) -> bool {
        let state = self.state.lock();
        match (self.kind, kind) {
            (InetSocketKind::Stream, InetWaitKind::Connect) => {
                state.handle.tcp_is_active() || state.handle.tcp_is_closed()
            }
            (InetSocketKind::Stream, InetWaitKind::Accept) => {
                state.listening
                    && (!self.pending_local_streams.lock().is_empty()
                        || state.handle.tcp_listener_accept_ready())
            }
            (InetSocketKind::Stream, InetWaitKind::Send) => {
                state.local_stream.is_some()
                    || state.handle.tcp_can_send()
                    || state.handle.tcp_is_closed()
            }
            (InetSocketKind::Stream, InetWaitKind::Recv) => {
                if let Some(local_stream) = &state.local_stream {
                    state.read_shutdown || local_stream.can_recv()
                } else {
                    state.read_shutdown
                        || state.handle.tcp_can_recv()
                        || state.handle.tcp_is_closed()
                }
            }
            (InetSocketKind::Datagram, InetWaitKind::Send) => {
                !state.write_shutdown && state.handle.udp_can_send()
            }
            (InetSocketKind::Datagram, InetWaitKind::Recv) => {
                state.read_shutdown
                    || !state.local_datagrams.is_empty()
                    || state.handle.udp_can_recv()
            }
            (InetSocketKind::Datagram, InetWaitKind::Connect | InetWaitKind::Accept) => false,
        }
    }

    fn readiness_event(kind: InetWaitKind) -> PollableEvent {
        match kind {
            InetWaitKind::Accept | InetWaitKind::Recv => PollableEvent::CanBeRead,
            InetWaitKind::Send => PollableEvent::CanBeWritten,
            InetWaitKind::Connect => PollableEvent::CanBeWritten,
        }
    }

    fn wait_for_event_or_io(&self, kind: InetWaitKind) {
        if let Some(object) = object_ref(&self.self_ref) {
            let object_ref = object as ObjectRef;
            wait_for_object_event(object_ref, Self::readiness_event(kind));
        } else {
            self.prepare_wait(kind);
        }
    }

    fn ensure_udp_bound(&self) -> SocketResult<InetAddress> {
        {
            let state = self.state.lock();
            if let Some(local) = state.local {
                return Ok(local);
            }
        }

        let local = InetAddress::any(net::allocate_ephemeral_port().map_err(Self::map_net_error)?);
        self.current_handle()
            .udp_bind(local)
            .map_err(Self::map_net_error)?;
        self.register_local_datagram(local);
        self.state.lock().local = Some(local);
        Ok(local)
    }

    pub fn bind(&self, addr: InetAddress) -> SocketResult<()> {
        let addr = if addr.port == 0 {
            InetAddress::new(
                addr.addr,
                net::allocate_ephemeral_port().map_err(Self::map_net_error)?,
            )
        } else {
            addr
        };

        let mut state = self.state.lock();
        if state.local.is_some() {
            return Err(SocketError::AddressInUse);
        }
        if !net::is_local_ipv4_address(addr.addr) {
            return Err(SocketError::AddressNotAvailable);
        }
        if addr.port < 1024 {
            let process = get_current_process();
            let process = process.lock();
            let slot = CAP_NET_BIND_SERVICE / 32;
            let mask = 1u32 << (CAP_NET_BIND_SERVICE % 32);
            if process.capability_effective[slot] & mask == 0 {
                return Err(SocketError::AccessDenied);
            }
        }

        match self.kind {
            InetSocketKind::Stream => {
                state.local = Some(addr);
                Ok(())
            }
            InetSocketKind::Datagram => {
                state.handle.udp_bind(addr).map_err(Self::map_net_error)?;
                drop(state);
                self.register_local_datagram(addr);
                let mut state = self.state.lock();
                state.local = Some(addr);
                Ok(())
            }
        }
    }

    pub fn listen(&self, backlog: usize) -> SocketResult<()> {
        if self.kind != InetSocketKind::Stream {
            return Err(SocketError::OperationNotSupported);
        }

        let local = {
            let state = self.state.lock();
            state.local.ok_or(SocketError::AddressNotAvailable)?
        };

        self.ensure_local_listener_available(local)?;
        self.current_handle()
            .tcp_listen(local)
            .map_err(Self::map_net_error)?;
        self.state.lock().listening = true;
        *self.local_backlog.lock() = backlog;
        self.register_local_listener(local);
        Ok(())
    }

    pub fn connect(&self, remote: InetAddress) -> SocketResult<()> {
        if remote.port == 0 || remote.is_unspecified() {
            return Err(SocketError::ConnectionRefused);
        }

        match self.kind {
            InetSocketKind::Stream => self.connect_stream(remote),
            InetSocketKind::Datagram => self.connect_datagram(remote),
        }
    }

    fn connect_stream(&self, remote: InetAddress) -> SocketResult<()> {
        let domain = *self.domain.lock();
        let local = {
            let state = self.state.lock();
            if state.peer.is_some() || state.listening {
                return Err(SocketError::IsConnected);
            }
            state.local
        };
        let local = match local {
            Some(local) => local,
            None => InetAddress::any(net::allocate_ephemeral_port().map_err(Self::map_net_error)?),
        };

        if Self::is_local_address(remote.addr)
            && let Some(listener) =
                Self::find_local_listener(self.current_handle().namespace_inode(), remote)
        {
            let local = Self::local_for_remote(local, remote);
            if listener.pending_local_streams.lock().len() >= *listener.local_backlog.lock() {
                return Err(SocketError::ConnectionRefused);
            }
            let server_handle =
                net::create_socket(TransportKind::Tcp).map_err(Self::map_net_error)?;
            let server_socket = Self::from_accepted(domain, server_handle, remote, local);
            let Some(client_socket) =
                object_ref(&self.self_ref).and_then(|object| object.as_inet_socket().ok())
            else {
                return Err(SocketError::InvalidArguments);
            };
            let (client_endpoint, server_endpoint) =
                LocalStreamEndpoint::pair(&client_socket, &server_socket);
            server_socket.state.lock().local_stream = Some(server_endpoint);
            listener
                .pending_local_streams
                .lock()
                .push_back(server_socket);
            {
                let mut state = self.state.lock();
                state.local = Some(local);
                state.peer = Some(remote);
                state.local_stream = Some(client_endpoint);
            }
            if let Some(object) = object_ref(&listener.self_ref) {
                wake_pollers_for_object(object as ObjectRef, PollableEvent::CanBeRead);
            }
            return Ok(());
        }

        self.current_handle()
            .tcp_connect(remote, local)
            .map_err(Self::map_net_error)?;

        {
            let mut state = self.state.lock();
            state.local = Some(local);
            state.peer = Some(remote);
        }

        if self.is_nonblocking() {
            return Err(SocketError::TryAgain);
        }

        loop {
            net::poll();
            let handle = self.current_handle();
            if handle.tcp_is_active() {
                if let Some(local_addr) = handle.tcp_local_addr() {
                    self.state.lock().local = Some(local_addr);
                }
                return Ok(());
            }
            if handle.tcp_is_closed() {
                return Err(SocketError::ConnectionRefused);
            }
            self.prepare_wait(InetWaitKind::Connect);
        }
    }

    fn disconnect_stream(&self) -> SocketResult<()> {
        if self.kind != InetSocketKind::Stream {
            return Err(SocketError::ProtocolNotSupported);
        }

        self.unregister_local_listener();
        let old = {
            let mut state = self.state.lock();
            if let Some(local_stream) = state.local_stream.take() {
                local_stream.close_local();
            } else {
                state.handle.tcp_close().map_err(Self::map_net_error)?;
            }
            let old = state.handle;
            state.handle = net::create_socket(TransportKind::Tcp).map_err(Self::map_net_error)?;
            state.local = None;
            state.peer = None;
            state.listening = false;
            state.read_shutdown = false;
            state.write_shutdown = false;
            old
        };
        net::remove_socket(old);
        Ok(())
    }

    fn connect_datagram(&self, remote: InetAddress) -> SocketResult<()> {
        let local = self.ensure_udp_bound()?;
        let mut state = self.state.lock();
        state.local = Some(local);
        state.peer = Some(remote);
        Ok(())
    }

    pub fn accept(&self) -> SocketResult<Arc<Self>> {
        if self.kind != InetSocketKind::Stream {
            return Err(SocketError::OperationNotSupported);
        }

        let local = {
            let state = self.state.lock();
            if !state.listening {
                return Err(SocketError::InvalidArguments);
            }
            state.local.ok_or(SocketError::AddressNotAvailable)?
        };

        loop {
            if let Some(socket) = self.pending_local_streams.lock().pop_front() {
                return Ok(socket);
            }

            net::poll();
            match self.current_handle().tcp_accept(local) {
                Ok((new_listener, accepted_local, peer)) => {
                    let old_handle = {
                        let mut state = self.state.lock();
                        let old = state.handle;
                        state.handle = new_listener;
                        state.listening = true;
                        old
                    };
                    return Ok(Self::from_accepted(
                        *self.domain.lock(),
                        old_handle,
                        accepted_local,
                        peer,
                    ));
                }
                Err(NetError::TryAgain) => {
                    if self.is_nonblocking() {
                        return Err(SocketError::TryAgain);
                    }
                    self.wait_for_event_or_io(InetWaitKind::Accept);
                }
                Err(err) => return Err(Self::map_net_error(err)),
            }
        }
    }

    pub fn send(&self, buffer: &[u8]) -> SocketResult<usize> {
        match self.kind {
            InetSocketKind::Stream => self.send_stream(buffer),
            InetSocketKind::Datagram => {
                let peer = self.state.lock().peer.ok_or(SocketError::NotConnected)?;
                self.send_to(buffer, peer)
            }
        }
    }

    pub fn send_to(&self, buffer: &[u8], remote: InetAddress) -> SocketResult<usize> {
        match self.kind {
            InetSocketKind::Stream => self.send_stream(buffer),
            InetSocketKind::Datagram => self.send_datagram(buffer, remote),
        }
    }

    fn send_stream(&self, buffer: &[u8]) -> SocketResult<usize> {
        if self.state.lock().write_shutdown {
            return Err(SocketError::BrokenPipe);
        }
        let local_stream = { self.state.lock().local_stream.clone() };
        if let Some(local_stream) = local_stream {
            if *local_stream.local_write_closed.lock()
                || local_stream.peer.upgrade().is_none()
                || *local_stream.peer_closed.lock()
            {
                return Err(SocketError::BrokenPipe);
            }
            local_stream.outgoing.lock().extend(buffer.iter().copied());
            local_stream.wake_peer(PollableEvent::CanBeRead);
            crate::socket::wake_io();
            return Ok(buffer.len());
        }

        if self.state.lock().peer.is_none() {
            return Err(SocketError::BrokenPipe);
        }

        loop {
            net::poll();
            let handle = self.current_handle();
            if handle.tcp_is_closed() {
                return Err(SocketError::BrokenPipe);
            }
            match handle.tcp_send(buffer) {
                Ok(written) => return Ok(written),
                Err(NetError::TryAgain) => {
                    if self.is_nonblocking() {
                        return Err(SocketError::TryAgain);
                    }
                    self.wait_for_event_or_io(InetWaitKind::Send);
                }
                Err(err) => return Err(Self::map_net_error(err)),
            }
        }
    }

    fn send_datagram(&self, buffer: &[u8], remote: InetAddress) -> SocketResult<usize> {
        if self.state.lock().write_shutdown {
            return Err(SocketError::BrokenPipe);
        }
        if buffer.len() > MAX_UDP_PAYLOAD_SIZE {
            return Err(SocketError::MessageTooLong);
        }

        let local = self.ensure_udp_bound()?;
        self.state.lock().local = Some(local);

        if Self::is_local_address(remote.addr)
            && let Some(target) =
                Self::find_local_datagram(self.current_handle().namespace_inode(), remote)
        {
            let source = Self::local_for_remote(local, remote);
            {
                let mut target_state = target.state.lock();
                if target_state.read_shutdown {
                    return Err(SocketError::ConnectionRefused);
                }
                target_state
                    .local_datagrams
                    .push_back(LocalDatagramMessage {
                        data: buffer.to_vec(),
                        source,
                    });
            }
            if let Some(object) = object_ref(&target.self_ref) {
                wake_pollers_for_object(object as ObjectRef, PollableEvent::CanBeRead);
            }
            crate::socket::wake_io();
            return Ok(buffer.len());
        }

        loop {
            net::poll();
            match self.current_handle().udp_send(buffer, remote) {
                Ok(written) => return Ok(written),
                Err(NetError::TryAgain) => {
                    if self.is_nonblocking() {
                        return Err(SocketError::TryAgain);
                    }
                    self.wait_for_event_or_io(InetWaitKind::Send);
                }
                Err(err) => return Err(Self::map_net_error(err)),
            }
        }
    }

    pub fn recv(&self, buffer: &mut [u8]) -> SocketResult<usize> {
        self.recv_from(buffer).map(|(read, _)| read)
    }

    pub fn recv_from(&self, buffer: &mut [u8]) -> SocketResult<(usize, Option<InetAddress>)> {
        match self.kind {
            InetSocketKind::Stream => self.recv_stream(buffer).map(|read| (read, None)),
            InetSocketKind::Datagram => self.recv_datagram(buffer),
        }
    }

    fn recv_stream(&self, buffer: &mut [u8]) -> SocketResult<usize> {
        if self.state.lock().read_shutdown {
            return Ok(0);
        }
        let local_stream = { self.state.lock().local_stream.clone() };
        if let Some(local_stream) = local_stream {
            loop {
                let mut incoming = local_stream.incoming.lock();
                if !incoming.is_empty() {
                    let read = buffer.len().min(incoming.len());
                    for slot in buffer.iter_mut().take(read) {
                        *slot = incoming.pop_front().ok_or(SocketError::InvalidArguments)?;
                    }
                    return Ok(read);
                }
                drop(incoming);

                if *local_stream.peer_write_closed.lock()
                    || *local_stream.peer_closed.lock()
                    || local_stream.peer.upgrade().is_none()
                {
                    return Ok(0);
                }
                if self.state.lock().read_shutdown {
                    return Ok(0);
                }
                if self.is_nonblocking() {
                    return Err(SocketError::TryAgain);
                }
                self.wait_for_event_or_io(InetWaitKind::Recv);
            }
        }

        loop {
            net::poll();
            let handle = self.current_handle();
            if handle.tcp_is_closed() && !handle.tcp_can_recv() {
                return Ok(0);
            }
            match handle.tcp_recv(buffer) {
                Ok(read) => return Ok(read),
                Err(NetError::TryAgain) => {
                    if self.is_nonblocking() {
                        return Err(SocketError::TryAgain);
                    }
                    self.wait_for_event_or_io(InetWaitKind::Recv);
                }
                Err(err) => return Err(Self::map_net_error(err)),
            }
        }
    }

    fn recv_datagram(&self, buffer: &mut [u8]) -> SocketResult<(usize, Option<InetAddress>)> {
        if self.state.lock().read_shutdown {
            return Ok((0, None));
        }

        loop {
            if let Some(message) = self.state.lock().local_datagrams.pop_front() {
                let read = buffer.len().min(message.data.len());
                buffer[..read].copy_from_slice(&message.data[..read]);
                return Ok((read, Some(message.source)));
            }
            net::poll();
            match self.current_handle().udp_recv(buffer) {
                Ok((read, remote, _)) => return Ok((read, Some(remote))),
                Err(NetError::TryAgain) => {
                    if self.is_nonblocking() {
                        return Err(SocketError::TryAgain);
                    }
                    self.wait_for_event_or_io(InetWaitKind::Recv);
                }
                Err(err) => return Err(Self::map_net_error(err)),
            }
        }
    }

    pub fn shutdown(&self, how: u64) -> SocketResult<()> {
        let mut state = self.state.lock();
        match how {
            0 => {
                state.read_shutdown = true;
                if let Some(local_stream) = &state.local_stream {
                    local_stream.wake_peer(PollableEvent::CanBeWritten);
                }
            }
            1 => {
                state.write_shutdown = true;
                if let Some(local_stream) = &state.local_stream {
                    local_stream.close_write();
                } else if self.kind == InetSocketKind::Stream {
                    state.handle.tcp_close().map_err(Self::map_net_error)?;
                }
            }
            2 => {
                state.read_shutdown = true;
                state.write_shutdown = true;
                if let Some(local_stream) = &state.local_stream {
                    local_stream.close_local();
                } else if self.kind == InetSocketKind::Stream {
                    state.handle.tcp_close().map_err(Self::map_net_error)?;
                }
            }
            _ => return Err(SocketError::InvalidArguments),
        }
        drop(state);
        if let Some(object) = object_ref(&self.self_ref) {
            let object_ref = object as ObjectRef;
            wake_pollers_for_object(object_ref.clone(), PollableEvent::CanBeRead);
            wake_pollers_for_object(object_ref.clone(), PollableEvent::ReadClosed);
            wake_pollers_for_object(object_ref, PollableEvent::Closed);
        }
        Ok(())
    }

    fn encode_i32(option_len: usize, value: i32) -> SocketResult<Vec<u8>> {
        if option_len < mem::size_of::<i32>() {
            return Err(SocketError::InvalidArguments);
        }
        Ok(value.to_ne_bytes().to_vec())
    }

    fn decode_i32(option_value: &[u8]) -> SocketResult<i32> {
        if option_value.len() < mem::size_of::<i32>() {
            return Err(SocketError::InvalidArguments);
        }
        Ok(i32::from_ne_bytes(
            option_value[..mem::size_of::<i32>()]
                .try_into()
                .map_err(|_| SocketError::InvalidArguments)?,
        ))
    }
}

impl Drop for InetSocketObject {
    fn drop(&mut self) {
        self.unregister_local_listener();
        let (handle, local, local_stream) = {
            let state = self.state.lock();
            (state.handle, state.local, state.local_stream.clone())
        };
        if self.kind == InetSocketKind::Datagram
            && let Some(local) = local
        {
            self.unregister_local_datagram(local);
        }
        if let Some(local_stream) = &local_stream {
            local_stream.close_local();
        }
        net::remove_socket(handle);
    }
}

impl Object for InetSocketObject {
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
    impl_cast_function_non_trait!("inet_socket", InetSocketObject);
}

impl Configuratable for InetSocketObject {
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

impl Readable for InetSocketObject {
    fn read(&self, buffer: &mut [u8]) -> Result<usize, ObjectError> {
        self.recv(buffer).map_err(Into::into)
    }
}

impl Writable for InetSocketObject {
    fn write(&self, buffer: &[u8]) -> Result<usize, ObjectError> {
        self.send(buffer).map_err(Into::into)
    }
}

impl Pollable for InetSocketObject {
    fn is_event_ready(&self, event: PollableEvent) -> bool {
        let state = self.state.lock();
        match self.kind {
            InetSocketKind::Stream => match event {
                PollableEvent::CanBeRead => {
                    if state.listening {
                        !self.pending_local_streams.lock().is_empty()
                            || state.handle.tcp_listener_accept_ready()
                    } else if let Some(local_stream) = &state.local_stream {
                        state.read_shutdown || local_stream.can_recv()
                    } else {
                        state.read_shutdown
                            || state.handle.tcp_can_recv()
                            || state.handle.tcp_is_closed()
                    }
                }
                PollableEvent::CanBeWritten => {
                    !state.write_shutdown
                        && !state.listening
                        && (state.local_stream.is_some() || state.handle.tcp_can_send())
                }
                PollableEvent::ReadClosed => {
                    state.read_shutdown
                        || state
                            .local_stream
                            .as_ref()
                            .is_some_and(|stream| *stream.peer_write_closed.lock())
                        || state.handle.tcp_is_closed()
                }
                PollableEvent::Closed => {
                    state
                        .local_stream
                        .as_ref()
                        .is_some_and(|stream| *stream.peer_closed.lock())
                        || state.handle.tcp_is_closed()
                }
                _ => false,
            },
            InetSocketKind::Datagram => match event {
                PollableEvent::CanBeRead => {
                    !state.read_shutdown
                        && (!state.local_datagrams.is_empty() || state.handle.udp_can_recv())
                }
                PollableEvent::CanBeWritten => !state.write_shutdown && state.handle.udp_can_send(),
                PollableEvent::ReadClosed => state.read_shutdown,
                PollableEvent::Closed => state.read_shutdown && state.write_shutdown,
                _ => false,
            },
        }
    }
}

impl Statable for InetSocketObject {
    fn stat(&self) -> LinuxStat {
        const S_IFSOCK: u32 = 0o140000;

        LinuxStat {
            st_dev: 1,
            st_nlink: 1,
            st_mode: S_IFSOCK | 0o777,
            st_blksize: 4096,
            ..Default::default()
        }
    }
}

impl SocketLike for InetSocketObject {
    fn bind_bytes(self: Arc<Self>, address: &[u8]) -> SocketResult<()> {
        self.bind(Self::decode_addr_for_domain(*self.domain.lock(), address)?)
    }

    fn listen(self: Arc<Self>, backlog: usize) -> SocketResult<()> {
        InetSocketObject::listen(&self, backlog)
    }

    fn connect_bytes(self: Arc<Self>, address: &[u8]) -> SocketResult<()> {
        if address.len() >= mem::size_of::<u16>() {
            let family = u16::from_ne_bytes(address[..2].try_into().unwrap());
            if family == 0 {
                return self.disconnect_stream();
            }
        }
        self.connect(Self::decode_addr_for_domain(*self.domain.lock(), address)?)
    }

    fn accept(self: Arc<Self>) -> SocketResult<crate::object::misc::ObjectRef> {
        Ok(InetSocketObject::accept(&self)?)
    }

    fn sendto(self: Arc<Self>, buffer: &[u8], address: Option<&[u8]>) -> SocketResult<usize> {
        match self.kind {
            InetSocketKind::Stream => self.send(buffer),
            InetSocketKind::Datagram => match address {
                Some(address) => self.send_to(
                    buffer,
                    Self::decode_addr_for_domain(*self.domain.lock(), address)?,
                ),
                None => self.send(buffer),
            },
        }
    }

    fn recvfrom(&self, buffer: &mut [u8]) -> SocketResult<(usize, Option<Vec<u8>>)> {
        let (read, source) = self.recv_from(buffer)?;
        Ok((
            read,
            source.map(|addr| Self::encode_addr_for_domain(*self.domain.lock(), addr)),
        ))
    }

    fn getsockname_bytes(&self) -> SocketResult<Vec<u8>> {
        let state = self.state.lock();
        let addr = match self.kind {
            InetSocketKind::Stream => state.handle.tcp_local_addr().or(state.local),
            InetSocketKind::Datagram => state.handle.udp_local_addr().or(state.local),
        }
        .unwrap_or_else(|| InetAddress::any(0));
        Ok(Self::encode_addr_for_domain(*self.domain.lock(), addr))
    }

    fn getpeername_bytes(&self) -> SocketResult<Vec<u8>> {
        let state = self.state.lock();
        let addr = match self.kind {
            InetSocketKind::Stream => state.handle.tcp_remote_addr().or(state.peer),
            InetSocketKind::Datagram => state.peer,
        }
        .ok_or(SocketError::NotConnected)?;
        Ok(Self::encode_addr_for_domain(*self.domain.lock(), addr))
    }

    fn shutdown(&self, how: u64) -> SocketResult<()> {
        InetSocketObject::shutdown(self, how)
    }

    fn setsockopt(&self, level: u64, option_name: u64, option_value: &[u8]) -> SocketResult<()> {
        if level == SOL_IPV6 {
            if option_name != IPV6_ADDRFORM {
                return Err(SocketError::ProtocolOptionNotSupported);
            }
            let family = Self::decode_i32(option_value)?;
            if *self.domain.lock() != AF_INET6
                || self.kind != InetSocketKind::Stream
                || family != AF_INET as i32
            {
                return Err(SocketError::InvalidArguments);
            }
            let state = self.state.lock();
            if state.peer.is_none() {
                return Err(SocketError::NotConnected);
            }
            drop(state);
            *self.domain.lock() = AF_INET;
            return Ok(());
        }

        if level == SOL_IP {
            return match option_name {
                IP_MULTICAST_ALL => {
                    let _ = Self::decode_i32(option_value)?;
                    Ok(())
                }
                IP_ADD_MEMBERSHIP | MCAST_JOIN_GROUP => {
                    if option_value.is_empty() {
                        return Err(SocketError::InvalidArguments);
                    }
                    self.multicast_groups.lock().insert(option_value.to_vec());
                    Ok(())
                }
                IP_DROP_MEMBERSHIP | MCAST_LEAVE_GROUP => {
                    if option_value.is_empty() {
                        return Err(SocketError::InvalidArguments);
                    }
                    if self.multicast_groups.lock().remove(option_value) {
                        Ok(())
                    } else {
                        Err(SocketError::AddressNotAvailable)
                    }
                }
                _ => Err(SocketError::ProtocolOptionNotSupported),
            };
        }

        if level == super::SOL_UDP {
            return Err(SocketError::ProtocolOptionNotSupported);
        }

        if level == SOL_TCP {
            if option_name == super::TCP_NODELAY {
                let _ = Self::decode_i32(option_value)?;
                return Ok(());
            }
            return Err(SocketError::ProtocolOptionNotSupported);
        }

        if level != SOL_SOCKET {
            return Err(SocketError::ProtocolOptionNotSupported);
        }

        match option_name {
            SO_REUSEADDR | SO_SNDBUF | SO_RCVBUF | SO_SNDBUFFORCE | SO_RCVBUFFORCE => {
                let _ = Self::decode_i32(option_value)?;
                Ok(())
            }
            SO_PRIORITY => {
                let priority = Self::decode_i32(option_value)?;
                can_set_socket_priority(priority)?;
                *self.priority.lock() = priority;
                Ok(())
            }
            SO_RCVTIMEO_OLD | SO_SNDTIMEO_OLD | SO_RCVTIMEO_NEW | SO_SNDTIMEO_NEW => {
                let expected_len =
                    socket_timeout_option_len(option_name).ok_or(SocketError::InvalidArguments)?;
                if option_value.len() < expected_len {
                    return Err(SocketError::InvalidArguments);
                }
                Ok(())
            }
            _ => Err(SocketError::InvalidArguments),
        }
    }

    fn getsockopt(&self, level: u64, option_name: u64, option_len: usize) -> SocketResult<Vec<u8>> {
        if level == SOL_IP {
            return match option_name {
                IP_MULTICAST_ALL => Self::encode_i32(option_len, 1),
                _ => Err(SocketError::ProtocolOptionNotSupported),
            };
        }

        if level == super::SOL_UDP {
            return Err(SocketError::OperationNotSupported);
        }

        if level == SOL_TCP {
            if option_name == super::TCP_NODELAY {
                return Self::encode_i32(option_len, 1);
            }
            return Err(SocketError::ProtocolOptionNotSupported);
        }

        if level != SOL_SOCKET {
            return Err(SocketError::OperationNotSupported);
        }

        match option_name {
            SO_ERROR => Self::encode_i32(option_len, 0),
            SO_TYPE => Self::encode_i32(
                option_len,
                match self.kind {
                    InetSocketKind::Stream => SOCK_STREAM as i32,
                    InetSocketKind::Datagram => SOCK_DGRAM as i32,
                },
            ),
            SO_ACCEPTCONN => Self::encode_i32(option_len, self.state.lock().listening as i32),
            SO_DOMAIN => Self::encode_i32(option_len, *self.domain.lock() as i32),
            SO_PROTOCOL => Self::encode_i32(
                option_len,
                match self.kind {
                    InetSocketKind::Stream => IPPROTO_TCP as i32,
                    InetSocketKind::Datagram => IPPROTO_UDP as i32,
                },
            ),
            SO_SNDBUF | SO_RCVBUF | SO_SNDBUFFORCE | SO_RCVBUFFORCE => {
                Self::encode_i32(option_len, DEFAULT_SOCKET_BUFFER_SIZE)
            }
            SO_PRIORITY => Self::encode_i32(option_len, *self.priority.lock()),
            SO_REUSEADDR => Self::encode_i32(option_len, 0),
            SO_RCVTIMEO_OLD | SO_SNDTIMEO_OLD | SO_RCVTIMEO_NEW | SO_SNDTIMEO_NEW => {
                let expected_len =
                    socket_timeout_option_len(option_name).ok_or(SocketError::InvalidArguments)?;
                if option_len < expected_len {
                    return Err(SocketError::InvalidArguments);
                }
                Ok(vec![0; expected_len])
            }
            _ => Err(SocketError::InvalidArguments),
        }
    }
}
