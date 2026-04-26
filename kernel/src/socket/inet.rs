use alloc::{sync::Arc, vec, vec::Vec};
use core::{mem, slice};

use spin::Mutex;

use crate::{
    filesystem::info::LinuxStat,
    impl_cast_function, impl_cast_function_non_trait,
    net::{self, InetAddress, NetError, NetSocketHandle, TransportKind},
    object::{
        FileFlags, Object,
        config::ConfigurateRequest,
        error::ObjectError,
        misc::ObjectResult,
        traits::{Configuratable, Readable, Statable, Writable},
    },
    polling::{event::PollableEvent, object::Pollable},
    thread::yielding::{
        BlockType, WakeType, cancel_block, finish_block_current, prepare_block_current,
    },
};

use super::{
    AF_INET, IPPROTO_TCP, IPPROTO_UDP, SO_ACCEPTCONN, SO_DOMAIN, SO_ERROR, SO_PROTOCOL, SO_RCVBUF,
    SO_RCVBUFFORCE, SO_RCVTIMEO_NEW, SO_RCVTIMEO_OLD, SO_REUSEADDR, SO_SNDBUF, SO_SNDBUFFORCE,
    SO_SNDTIMEO_NEW, SO_SNDTIMEO_OLD, SO_TYPE, SOCK_CLOEXEC, SOCK_DGRAM, SOCK_NONBLOCK,
    SOCK_STREAM, SOL_SOCKET, SOL_TCP, SocketError, SocketLike, SocketResult,
    socket_timeout_option_len,
};

const DEFAULT_SOCKET_BUFFER_SIZE: i32 = 64 * 1024;
const FIONBIO: u64 = 0x5421;
const FIOCLEX: u64 = 0x5451;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InetSocketKind {
    Stream,
    Datagram,
}

#[derive(Debug, Clone, Copy)]
struct InetState {
    handle: NetSocketHandle,
    local: Option<InetAddress>,
    peer: Option<InetAddress>,
    listening: bool,
    read_shutdown: bool,
    write_shutdown: bool,
}

#[derive(Debug)]
pub struct InetSocketObject {
    pub kind: InetSocketKind,
    state: Mutex<InetState>,
    flags: Mutex<FileFlags>,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct LinuxSockAddrIn {
    sin_family: u16,
    sin_port: u16,
    sin_addr: [u8; 4],
    sin_zero: [u8; 8],
}

impl InetSocketObject {
    fn decode_addr(address: &[u8]) -> SocketResult<InetAddress> {
        if address.len() < mem::size_of::<LinuxSockAddrIn>() {
            return Err(SocketError::InvalidArguments);
        }
        let sockaddr = unsafe { &*(address.as_ptr().cast::<LinuxSockAddrIn>()) };
        Ok(InetAddress::new(
            sockaddr.sin_addr,
            u16::from_be(sockaddr.sin_port),
        ))
    }

    pub fn create(domain: u64, kind: u64, protocol: u64) -> SocketResult<Arc<Self>> {
        if domain != AF_INET {
            return Err(SocketError::AddressFamilyNotSupported);
        }

        let socket_type = kind & !(SOCK_NONBLOCK | SOCK_CLOEXEC);
        let transport = match socket_type {
            SOCK_STREAM => {
                if protocol != 0 && protocol != IPPROTO_TCP {
                    return Err(SocketError::ProtocolNotSupported);
                }
                TransportKind::Tcp
            }
            SOCK_DGRAM => {
                if protocol != 0 && protocol != IPPROTO_UDP {
                    return Err(SocketError::ProtocolNotSupported);
                }
                TransportKind::Udp
            }
            _ => return Err(SocketError::ProtocolNotSupported),
        };

        let handle = net::create_socket(transport).map_err(Self::map_net_error)?;
        Ok(Arc::new(Self {
            kind: match transport {
                TransportKind::Tcp => InetSocketKind::Stream,
                TransportKind::Udp => InetSocketKind::Datagram,
            },
            state: Mutex::new(InetState {
                handle,
                local: None,
                peer: None,
                listening: false,
                read_shutdown: false,
                write_shutdown: false,
            }),
            flags: Mutex::new(FileFlags::empty()),
        }))
    }

    fn from_accepted(handle: NetSocketHandle, local: InetAddress, peer: InetAddress) -> Arc<Self> {
        Arc::new(Self {
            kind: InetSocketKind::Stream,
            state: Mutex::new(InetState {
                handle,
                local: Some(local),
                peer: Some(peer),
                listening: false,
                read_shutdown: false,
                write_shutdown: false,
            }),
            flags: Mutex::new(FileFlags::empty()),
        })
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

    fn encode_addr(addr: InetAddress) -> Vec<u8> {
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

    fn prepare_wait(&self) {
        let current = prepare_block_current(BlockType::WakeRequired {
            wake_type: WakeType::IO,
            deadline: None,
        });

        net::poll();
        if self.is_ready_for_io() {
            cancel_block(&current);
        } else {
            finish_block_current();
        }
    }

    fn is_ready_for_io(&self) -> bool {
        match self.kind {
            InetSocketKind::Stream => {
                let handle = self.current_handle();
                handle.tcp_can_recv() || handle.tcp_can_send() || handle.tcp_is_closed()
            }
            InetSocketKind::Datagram => {
                let handle = self.current_handle();
                handle.udp_can_recv() || handle.udp_can_send()
            }
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
        self.state.lock().local = Some(local);
        Ok(local)
    }

    pub fn bind(&self, addr: InetAddress) -> SocketResult<()> {
        if addr.port == 0 {
            return Err(SocketError::AddressNotAvailable);
        }

        let mut state = self.state.lock();
        if state.local.is_some() {
            return Err(SocketError::AddressInUse);
        }

        match self.kind {
            InetSocketKind::Stream => {
                state.local = Some(addr);
                Ok(())
            }
            InetSocketKind::Datagram => {
                state.handle.udp_bind(addr).map_err(Self::map_net_error)?;
                state.local = Some(addr);
                Ok(())
            }
        }
    }

    pub fn listen(&self, _backlog: usize) -> SocketResult<()> {
        if self.kind != InetSocketKind::Stream {
            return Err(SocketError::OperationNotSupported);
        }

        let local = {
            let state = self.state.lock();
            state.local.ok_or(SocketError::AddressNotAvailable)?
        };

        self.current_handle()
            .tcp_listen(local)
            .map_err(Self::map_net_error)?;
        self.state.lock().listening = true;
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
            self.prepare_wait();
        }
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
                    return Ok(Self::from_accepted(old_handle, accepted_local, peer));
                }
                Err(NetError::TryAgain) => {
                    if self.is_nonblocking() {
                        return Err(SocketError::TryAgain);
                    }
                    self.prepare_wait();
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
                    self.prepare_wait();
                }
                Err(err) => return Err(Self::map_net_error(err)),
            }
        }
    }

    fn send_datagram(&self, buffer: &[u8], remote: InetAddress) -> SocketResult<usize> {
        if self.state.lock().write_shutdown {
            return Err(SocketError::BrokenPipe);
        }

        let local = self.ensure_udp_bound()?;
        self.state.lock().local = Some(local);

        loop {
            net::poll();
            match self.current_handle().udp_send(buffer, remote) {
                Ok(written) => return Ok(written),
                Err(NetError::TryAgain) => {
                    if self.is_nonblocking() {
                        return Err(SocketError::TryAgain);
                    }
                    self.prepare_wait();
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
                    self.prepare_wait();
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
            net::poll();
            match self.current_handle().udp_recv(buffer) {
                Ok((read, remote, _)) => return Ok((read, Some(remote))),
                Err(NetError::TryAgain) => {
                    if self.is_nonblocking() {
                        return Err(SocketError::TryAgain);
                    }
                    self.prepare_wait();
                }
                Err(err) => return Err(Self::map_net_error(err)),
            }
        }
    }

    pub fn shutdown(&self, how: u64) -> SocketResult<()> {
        let mut state = self.state.lock();
        match how {
            0 => state.read_shutdown = true,
            1 => {
                state.write_shutdown = true;
                if self.kind == InetSocketKind::Stream {
                    state.handle.tcp_close().map_err(Self::map_net_error)?;
                }
            }
            2 => {
                state.read_shutdown = true;
                state.write_shutdown = true;
                if self.kind == InetSocketKind::Stream {
                    state.handle.tcp_close().map_err(Self::map_net_error)?;
                }
            }
            _ => return Err(SocketError::InvalidArguments),
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
        net::remove_socket(self.state.lock().handle);
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
                    state.read_shutdown
                        || state.handle.tcp_can_recv()
                        || state.handle.tcp_is_closed()
                }
                PollableEvent::CanBeWritten => {
                    !state.write_shutdown && !state.listening && state.handle.tcp_can_send()
                }
                PollableEvent::Closed => state.handle.tcp_is_closed(),
                _ => false,
            },
            InetSocketKind::Datagram => match event {
                PollableEvent::CanBeRead => !state.read_shutdown && state.handle.udp_can_recv(),
                PollableEvent::CanBeWritten => !state.write_shutdown && state.handle.udp_can_send(),
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
        self.bind(Self::decode_addr(address)?)
    }

    fn listen(self: Arc<Self>, backlog: usize) -> SocketResult<()> {
        InetSocketObject::listen(&self, backlog)
    }

    fn connect_bytes(self: Arc<Self>, address: &[u8]) -> SocketResult<()> {
        self.connect(Self::decode_addr(address)?)
    }

    fn accept(self: Arc<Self>) -> SocketResult<crate::object::misc::ObjectRef> {
        Ok(InetSocketObject::accept(&self)?)
    }

    fn sendto(self: Arc<Self>, buffer: &[u8], address: Option<&[u8]>) -> SocketResult<usize> {
        match address {
            Some(address) => self.send_to(buffer, Self::decode_addr(address)?),
            None => self.send(buffer),
        }
    }

    fn recvfrom(&self, buffer: &mut [u8]) -> SocketResult<(usize, Option<Vec<u8>>)> {
        let (read, source) = self.recv_from(buffer)?;
        Ok((read, source.map(Self::encode_addr)))
    }

    fn getsockname_bytes(&self) -> SocketResult<Vec<u8>> {
        let state = self.state.lock();
        let addr = match self.kind {
            InetSocketKind::Stream => state.handle.tcp_local_addr().or(state.local),
            InetSocketKind::Datagram => state.handle.udp_local_addr().or(state.local),
        }
        .unwrap_or_else(|| InetAddress::any(0));
        Ok(Self::encode_addr(addr))
    }

    fn getpeername_bytes(&self) -> SocketResult<Vec<u8>> {
        let state = self.state.lock();
        let addr = match self.kind {
            InetSocketKind::Stream => state.handle.tcp_remote_addr().or(state.peer),
            InetSocketKind::Datagram => state.peer,
        }
        .ok_or(SocketError::NotConnected)?;
        Ok(Self::encode_addr(addr))
    }

    fn shutdown(&self, how: u64) -> SocketResult<()> {
        InetSocketObject::shutdown(self, how)
    }

    fn setsockopt(&self, level: u64, option_name: u64, option_value: &[u8]) -> SocketResult<()> {
        if level == SOL_TCP {
            if option_name == super::TCP_NODELAY {
                let _ = Self::decode_i32(option_value)?;
                return Ok(());
            }
            return Err(SocketError::InvalidArguments);
        }

        if level != SOL_SOCKET {
            return Err(SocketError::InvalidArguments);
        }

        match option_name {
            SO_REUSEADDR | SO_SNDBUF | SO_RCVBUF | SO_SNDBUFFORCE | SO_RCVBUFFORCE => {
                let _ = Self::decode_i32(option_value)?;
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
        if level == SOL_TCP {
            if option_name == super::TCP_NODELAY {
                return Self::encode_i32(option_len, 1);
            }
            return Err(SocketError::InvalidArguments);
        }

        if level != SOL_SOCKET {
            return Err(SocketError::InvalidArguments);
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
            SO_DOMAIN => Self::encode_i32(option_len, AF_INET as i32),
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
