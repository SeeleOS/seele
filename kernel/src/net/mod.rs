use alloc::{sync::Arc, vec, vec::Vec};
use conquer_once::spin::OnceCell;
use smoltcp::{
    iface::{Config, Interface, PollResult, SocketHandle, SocketSet},
    phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken},
    socket::{tcp, udp},
    time::Instant,
    wire::{
        EthernetAddress, HardwareAddress, IpAddress, IpCidr, IpEndpoint, IpListenEndpoint,
        Ipv4Address,
    },
};
use spin::Mutex;

use crate::{misc::time::Time, thread::THREAD_MANAGER};

const STATIC_IPV4: [u8; 4] = [10, 0, 2, 15];
const DEFAULT_GATEWAY_IPV4: [u8; 4] = [10, 0, 2, 2];
const TCP_BUFFER_SIZE: usize = 64 * 1024;
const UDP_PACKET_CAPACITY: usize = 64;
const UDP_BUFFER_SIZE: usize = 64 * 1024;
const EPHEMERAL_PORT_START: u16 = 49152;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetError {
    TryAgain,
    InvalidArguments,
    NotConnected,
    AddressInUse,
    ConnectionRefused,
    BrokenPipe,
    NoDevice,
}

pub type NetResult<T> = Result<T, NetError>;
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NetSocketHandle(SocketHandle);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportKind {
    Tcp,
    Udp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InetAddress {
    pub addr: [u8; 4],
    pub port: u16,
}

impl InetAddress {
    pub const fn new(addr: [u8; 4], port: u16) -> Self {
        Self { addr, port }
    }

    pub const fn any(port: u16) -> Self {
        Self::new([0, 0, 0, 0], port)
    }

    pub fn is_unspecified(self) -> bool {
        self.addr == [0, 0, 0, 0]
    }

    pub fn ip(self) -> Ipv4Address {
        Ipv4Address::new(self.addr[0], self.addr[1], self.addr[2], self.addr[3])
    }

    pub fn endpoint(self) -> IpEndpoint {
        IpEndpoint::new(IpAddress::Ipv4(self.ip()), self.port)
    }

    pub fn listen_endpoint(self) -> IpListenEndpoint {
        if self.is_unspecified() {
            IpListenEndpoint::from(self.port)
        } else {
            IpListenEndpoint {
                addr: Some(IpAddress::Ipv4(self.ip())),
                port: self.port,
            }
        }
    }
}

impl From<IpEndpoint> for InetAddress {
    fn from(value: IpEndpoint) -> Self {
        let IpAddress::Ipv4(ip) = value.addr;
        let octets = ip.octets();
        Self::new(octets, value.port)
    }
}

pub trait NetworkDevice: Send + Sync {
    fn name(&self) -> &'static str;
    fn mac_address(&self) -> [u8; 6];
    fn mtu(&self) -> usize;
    fn receive(&self) -> Option<Vec<u8>>;
    fn transmit(&self, frame: &[u8]) -> NetResult<()>;
}

struct DeviceAdapter {
    device: Arc<dyn NetworkDevice>,
}

struct NetRxToken {
    frame: Vec<u8>,
}

struct NetTxToken {
    device: Arc<dyn NetworkDevice>,
}

impl RxToken for NetRxToken {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        f(&self.frame)
    }
}

impl TxToken for NetTxToken {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut frame = vec![0; len];
        let result = f(&mut frame);
        let _ = self.device.transmit(&frame);
        result
    }
}

impl Device for DeviceAdapter {
    type RxToken<'a>
        = NetRxToken
    where
        Self: 'a;
    type TxToken<'a>
        = NetTxToken
    where
        Self: 'a;

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let frame = self.device.receive()?;
        Some((
            NetRxToken { frame },
            NetTxToken {
                device: self.device.clone(),
            },
        ))
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        Some(NetTxToken {
            device: self.device.clone(),
        })
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.medium = Medium::Ethernet;
        caps.max_transmission_unit = self.device.mtu() + 14;
        caps
    }
}

struct NetStack {
    device: DeviceAdapter,
    iface: Interface,
    sockets: SocketSet<'static>,
    next_ephemeral_port: u16,
}

#[derive(Default)]
struct NetManager {
    stack: Option<NetStack>,
}

static NET_MANAGER: OnceCell<Mutex<NetManager>> = OnceCell::uninit();

fn manager() -> &'static Mutex<NetManager> {
    NET_MANAGER.get_or_init(|| Mutex::new(NetManager::default()))
}

fn smoltcp_now() -> Instant {
    Instant::from_millis(Time::since_boot().as_milliseconds() as i64)
}

fn wake_io() {
    if let Some(manager) = THREAD_MANAGER.get() {
        manager.lock().wake_io();
    }
}

impl NetStack {
    fn new(device: Arc<dyn NetworkDevice>) -> Self {
        let mac = device.mac_address();
        let mut adapter = DeviceAdapter { device };
        let mut config = Config::new(HardwareAddress::Ethernet(EthernetAddress(mac)));
        config.random_seed = Time::since_boot().as_nanoseconds();

        let mut iface = Interface::new(config, &mut adapter, smoltcp_now());
        iface.update_ip_addrs(|addrs| {
            addrs
                .push(IpCidr::new(
                    IpAddress::v4(
                        STATIC_IPV4[0],
                        STATIC_IPV4[1],
                        STATIC_IPV4[2],
                        STATIC_IPV4[3],
                    ),
                    24,
                ))
                .unwrap();
        });
        let _ = iface.routes_mut().add_default_ipv4_route(Ipv4Address::new(
            DEFAULT_GATEWAY_IPV4[0],
            DEFAULT_GATEWAY_IPV4[1],
            DEFAULT_GATEWAY_IPV4[2],
            DEFAULT_GATEWAY_IPV4[3],
        ));

        Self {
            device: adapter,
            iface,
            sockets: SocketSet::new(vec![]),
            next_ephemeral_port: EPHEMERAL_PORT_START,
        }
    }

    fn poll(&mut self) -> bool {
        matches!(
            self.iface
                .poll(smoltcp_now(), &mut self.device, &mut self.sockets),
            PollResult::SocketStateChanged
        )
    }

    fn allocate_tcp_socket(&mut self) -> SocketHandle {
        let socket = tcp::Socket::new(
            tcp::SocketBuffer::new(vec![0; TCP_BUFFER_SIZE]),
            tcp::SocketBuffer::new(vec![0; TCP_BUFFER_SIZE]),
        );
        self.sockets.add(socket)
    }

    fn allocate_udp_socket(&mut self) -> SocketHandle {
        let socket = udp::Socket::new(
            udp::PacketBuffer::new(
                vec![udp::PacketMetadata::EMPTY; UDP_PACKET_CAPACITY],
                vec![0; UDP_BUFFER_SIZE],
            ),
            udp::PacketBuffer::new(
                vec![udp::PacketMetadata::EMPTY; UDP_PACKET_CAPACITY],
                vec![0; UDP_BUFFER_SIZE],
            ),
        );
        self.sockets.add(socket)
    }

    fn remove_socket(&mut self, handle: SocketHandle) {
        let _ = self.sockets.remove(handle);
    }

    fn next_ephemeral_port(&mut self) -> u16 {
        let port = self.next_ephemeral_port;
        self.next_ephemeral_port = if port == u16::MAX {
            EPHEMERAL_PORT_START
        } else {
            port.saturating_add(1)
        };
        port
    }
}

impl NetManager {
    fn with_stack_mut<T, F>(&mut self, f: F) -> NetResult<T>
    where
        F: FnOnce(&mut NetStack) -> NetResult<T>,
    {
        let stack = self.stack.as_mut().ok_or(NetError::NoDevice)?;
        f(stack)
    }
}

fn map_tcp_listen_error(err: tcp::ListenError) -> NetError {
    match err {
        tcp::ListenError::InvalidState => NetError::InvalidArguments,
        tcp::ListenError::Unaddressable => NetError::InvalidArguments,
    }
}

fn map_tcp_connect_error(err: tcp::ConnectError) -> NetError {
    match err {
        tcp::ConnectError::InvalidState => NetError::InvalidArguments,
        tcp::ConnectError::Unaddressable => NetError::ConnectionRefused,
    }
}

fn map_tcp_send_error(_err: tcp::SendError) -> NetError {
    NetError::TryAgain
}

fn map_udp_bind_error(err: udp::BindError) -> NetError {
    match err {
        udp::BindError::InvalidState => NetError::AddressInUse,
        udp::BindError::Unaddressable => NetError::InvalidArguments,
    }
}

fn map_udp_send_error(err: udp::SendError) -> NetError {
    match err {
        udp::SendError::Unaddressable => NetError::ConnectionRefused,
        udp::SendError::BufferFull => NetError::TryAgain,
    }
}

pub fn init() {
    let _ = manager();
    log::info!("net: init");
}

pub fn register_device(device: Arc<dyn NetworkDevice>) {
    let mac = device.mac_address();
    let mut manager = manager().lock();
    if manager.stack.is_some() {
        log::warn!("net: dropping extra device {}", device.name());
        return;
    }

    log::info!(
        "net: registered {} mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        device.name(),
        mac[0],
        mac[1],
        mac[2],
        mac[3],
        mac[4],
        mac[5],
    );
    manager.stack = Some(NetStack::new(device));
}

pub fn poll() {
    let changed = manager().lock().stack.as_mut().is_some_and(NetStack::poll);
    if changed {
        wake_io();
    }
}

pub fn create_socket(kind: TransportKind) -> NetResult<NetSocketHandle> {
    manager().lock().with_stack_mut(|stack| {
        Ok(NetSocketHandle(match kind {
            TransportKind::Tcp => stack.allocate_tcp_socket(),
            TransportKind::Udp => stack.allocate_udp_socket(),
        }))
    })
}

pub fn remove_socket(handle: NetSocketHandle) {
    let mut manager = manager().lock();
    if let Some(stack) = manager.stack.as_mut() {
        stack.remove_socket(handle.0);
    }
}

pub fn allocate_ephemeral_port() -> NetResult<u16> {
    manager()
        .lock()
        .with_stack_mut(|stack| Ok(stack.next_ephemeral_port()))
}

impl NetSocketHandle {
    pub fn tcp_listen(self, local: InetAddress) -> NetResult<()> {
        manager().lock().with_stack_mut(|stack| {
            let socket = stack.sockets.get_mut::<tcp::Socket<'static>>(self.0);
            socket
                .listen(local.listen_endpoint())
                .map_err(map_tcp_listen_error)
        })
    }

    pub fn tcp_connect(self, remote: InetAddress, local: InetAddress) -> NetResult<()> {
        manager().lock().with_stack_mut(|stack| {
            let socket = stack.sockets.get_mut::<tcp::Socket<'static>>(self.0);
            socket
                .connect(
                    stack.iface.context(),
                    remote.endpoint(),
                    local.listen_endpoint(),
                )
                .map_err(map_tcp_connect_error)
        })
    }

    pub fn tcp_send(self, data: &[u8]) -> NetResult<usize> {
        manager().lock().with_stack_mut(|stack| {
            let socket = stack.sockets.get_mut::<tcp::Socket<'static>>(self.0);
            socket.send_slice(data).map_err(map_tcp_send_error)
        })
    }

    pub fn tcp_recv(self, data: &mut [u8]) -> NetResult<usize> {
        manager().lock().with_stack_mut(|stack| {
            let socket = stack.sockets.get_mut::<tcp::Socket<'static>>(self.0);
            match socket.recv_slice(data) {
                Ok(read) => Ok(read),
                Err(tcp::RecvError::Finished) => Ok(0),
                Err(tcp::RecvError::InvalidState) => Err(NetError::TryAgain),
            }
        })
    }

    pub fn tcp_close(self) -> NetResult<()> {
        manager().lock().with_stack_mut(|stack| {
            let socket = stack.sockets.get_mut::<tcp::Socket<'static>>(self.0);
            socket.close();
            Ok(())
        })
    }

    pub fn tcp_is_active(self) -> bool {
        manager().lock().stack.as_mut().is_some_and(|stack| {
            stack
                .sockets
                .get::<tcp::Socket<'static>>(self.0)
                .is_active()
        })
    }

    pub fn tcp_can_send(self) -> bool {
        manager().lock().stack.as_mut().is_some_and(|stack| {
            let socket = stack.sockets.get::<tcp::Socket<'static>>(self.0);
            socket.can_send() || socket.may_send()
        })
    }

    pub fn tcp_can_recv(self) -> bool {
        manager()
            .lock()
            .stack
            .as_mut()
            .is_some_and(|stack| stack.sockets.get::<tcp::Socket<'static>>(self.0).can_recv())
    }

    pub fn tcp_is_closed(self) -> bool {
        manager()
            .lock()
            .stack
            .as_mut()
            .is_none_or(|stack| !stack.sockets.get::<tcp::Socket<'static>>(self.0).is_open())
    }

    pub fn tcp_is_listening(self) -> bool {
        manager().lock().stack.as_mut().is_some_and(|stack| {
            matches!(
                stack.sockets.get::<tcp::Socket<'static>>(self.0).state(),
                tcp::State::Listen
            )
        })
    }

    pub fn tcp_local_addr(self) -> Option<InetAddress> {
        manager().lock().stack.as_mut().and_then(|stack| {
            let socket = stack.sockets.get::<tcp::Socket<'static>>(self.0);
            socket
                .local_endpoint()
                .or_else(|| {
                    let endpoint = socket.listen_endpoint();
                    (endpoint.port != 0).then(|| IpEndpoint {
                        addr: endpoint.addr.unwrap_or(IpAddress::v4(
                            STATIC_IPV4[0],
                            STATIC_IPV4[1],
                            STATIC_IPV4[2],
                            STATIC_IPV4[3],
                        )),
                        port: endpoint.port,
                    })
                })
                .map(InetAddress::from)
        })
    }

    pub fn tcp_remote_addr(self) -> Option<InetAddress> {
        manager().lock().stack.as_mut().and_then(|stack| {
            stack
                .sockets
                .get::<tcp::Socket<'static>>(self.0)
                .remote_endpoint()
                .map(InetAddress::from)
        })
    }

    pub fn tcp_accept(
        self,
        local: InetAddress,
    ) -> NetResult<(NetSocketHandle, InetAddress, InetAddress)> {
        manager().lock().with_stack_mut(|stack| {
            let active = {
                let socket = stack.sockets.get::<tcp::Socket<'static>>(self.0);
                socket.is_active()
            };
            if !active {
                return Err(NetError::TryAgain);
            }

            let local_addr = stack
                .sockets
                .get::<tcp::Socket<'static>>(self.0)
                .local_endpoint()
                .map(InetAddress::from)
                .unwrap_or(local);
            let peer_addr = stack
                .sockets
                .get::<tcp::Socket<'static>>(self.0)
                .remote_endpoint()
                .map(InetAddress::from)
                .ok_or(NetError::TryAgain)?;

            let new_listener = stack.allocate_tcp_socket();
            let listener = stack.sockets.get_mut::<tcp::Socket<'static>>(new_listener);
            listener
                .listen(local.listen_endpoint())
                .map_err(map_tcp_listen_error)?;
            Ok((NetSocketHandle(new_listener), local_addr, peer_addr))
        })
    }

    pub fn udp_bind(self, local: InetAddress) -> NetResult<()> {
        manager().lock().with_stack_mut(|stack| {
            let socket = stack.sockets.get_mut::<udp::Socket<'static>>(self.0);
            socket
                .bind(local.listen_endpoint())
                .map_err(map_udp_bind_error)
        })
    }

    pub fn udp_send(self, data: &[u8], remote: InetAddress) -> NetResult<usize> {
        manager().lock().with_stack_mut(|stack| {
            let socket = stack.sockets.get_mut::<udp::Socket<'static>>(self.0);
            socket
                .send_slice(data, remote.endpoint())
                .map(|_| data.len())
                .map_err(map_udp_send_error)
        })
    }

    pub fn udp_recv(self, data: &mut [u8]) -> NetResult<(usize, InetAddress, bool)> {
        manager().lock().with_stack_mut(|stack| {
            let socket = stack.sockets.get_mut::<udp::Socket<'static>>(self.0);
            match socket.recv_slice(data) {
                Ok((read, meta)) => Ok((read, InetAddress::from(meta.endpoint), false)),
                Err(udp::RecvError::Exhausted) => Err(NetError::TryAgain),
                Err(udp::RecvError::Truncated) => Err(NetError::InvalidArguments),
            }
        })
    }

    pub fn udp_can_send(self) -> bool {
        manager()
            .lock()
            .stack
            .as_mut()
            .is_some_and(|stack| stack.sockets.get::<udp::Socket<'static>>(self.0).can_send())
    }

    pub fn udp_can_recv(self) -> bool {
        manager()
            .lock()
            .stack
            .as_mut()
            .is_some_and(|stack| stack.sockets.get::<udp::Socket<'static>>(self.0).can_recv())
    }

    pub fn udp_is_open(self) -> bool {
        manager()
            .lock()
            .stack
            .as_mut()
            .is_some_and(|stack| stack.sockets.get::<udp::Socket<'static>>(self.0).is_open())
    }

    pub fn udp_local_addr(self) -> Option<InetAddress> {
        manager().lock().stack.as_mut().and_then(|stack| {
            let endpoint = stack.sockets.get::<udp::Socket<'static>>(self.0).endpoint();
            (endpoint.port != 0).then(|| InetAddress {
                addr: endpoint.addr.map_or(STATIC_IPV4, |addr| {
                    let IpAddress::Ipv4(ip) = addr;
                    ip.octets()
                }),
                port: endpoint.port,
            })
        })
    }
}
