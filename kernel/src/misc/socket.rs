use alloc::{
    collections::{BTreeMap, VecDeque},
    string::String,
    sync::{Arc, Weak},
    vec::Vec,
};
use core::{mem, slice};
use lazy_static::lazy_static;
use seele_sys::{abi::object::ObjectFlags, abi::socket};
use spin::Mutex;

use crate::{
    impl_cast_function, impl_cast_function_non_trait,
    object::{
        Object,
        control::ControlRequest,
        error::ObjectError,
        misc::{ObjectRef, ObjectResult},
        traits::{Controllable, Readable, Writable},
    },
    polling::{event::PollableEvent, object::Pollable},
    process::manager::get_current_process,
    thread::{
        THREAD_MANAGER,
        yielding::{BlockType, WakeType, block_current},
    },
};

lazy_static! {
    static ref UNIX_SOCKET_REGISTRY: Mutex<BTreeMap<String, Option<Arc<UnixListenerInner>>>> =
        Mutex::new(BTreeMap::new());
}

const DEFAULT_SOCKET_BUFFER_SIZE: i32 = 64 * 1024;

#[derive(Clone, Copy, Debug, Default)]
pub struct SocketPeerCred {
    pub pid: u64,
    pub uid: u32,
    pub gid: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct SocketUcred {
    pid: i32,
    uid: u32,
    gid: u32,
}

#[derive(Debug)]
pub struct UnixSocketObject {
    pub state: Mutex<UnixSocketState>,
    pub flags: Mutex<ObjectFlags>,
}

impl UnixSocketObject {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(UnixSocketState::Unbound),
            flags: Mutex::new(ObjectFlags::empty()),
        }
    }

    pub fn create(domain: u64, kind: u64, protocol: u64) -> ObjectResult<Arc<Self>> {
        if domain != socket::AF_UNIX {
            return Err(ObjectError::AddressFamilyNotSupported);
        }
        if kind != socket::SOCK_STREAM {
            return Err(ObjectError::ProtocolNotSupported);
        }
        if protocol != 0 {
            return Err(ObjectError::ProtocolNotSupported);
        }

        Ok(Arc::new(Self::new()))
    }

    fn is_nonblocking(&self) -> bool {
        self.flags.lock().contains(ObjectFlags::NONBLOCK)
    }

    fn wake_io() {
        if let Some(manager) = THREAD_MANAGER.get() {
            manager.lock().wake_io();
        }
    }

    fn wake_pollers(target: &Arc<Self>, event: PollableEvent) {
        if let Some(manager) = THREAD_MANAGER.get() {
            let object_ref: ObjectRef = target.clone();
            manager.lock().wake_poller(object_ref, event);
        }
    }

    pub fn bind(self: &Arc<Self>, path: String) -> ObjectResult<()> {
        let mut state = self.state.lock();

        if !matches!(*state, UnixSocketState::Unbound) {
            return Err(ObjectError::InvalidArguments);
        }

        let mut registry = UNIX_SOCKET_REGISTRY.lock();
        if registry.contains_key(&path) {
            return Err(ObjectError::AddressInUse);
        }

        registry.insert(path.clone(), None);
        *state = UnixSocketState::Bound { path };
        Ok(())
    }

    pub fn listen(self: &Arc<Self>, backlog: usize) -> ObjectResult<()> {
        let path = match &*self.state.lock() {
            UnixSocketState::Bound { path } => path.clone(),
            UnixSocketState::Listener(_) => return Err(ObjectError::InvalidArguments),
            _ => return Err(ObjectError::InvalidArguments),
        };

        let listener = Arc::new(UnixListenerInner::new(path.clone(), backlog.max(1)));
        *listener.owner.lock() = Some(Arc::downgrade(self));

        {
            let mut registry = UNIX_SOCKET_REGISTRY.lock();
            let slot = registry.get_mut(&path).ok_or(ObjectError::InvalidArguments)?;
            *slot = Some(listener.clone());
        }

        *self.state.lock() = UnixSocketState::Listener(listener);
        Ok(())
    }

    pub fn connect(self: &Arc<Self>, path: String) -> ObjectResult<()> {
        let listener = {
            let registry = UNIX_SOCKET_REGISTRY.lock();
            match registry.get(&path) {
                Some(Some(listener)) => listener.clone(),
                Some(None) => return Err(ObjectError::ConnectionRefused),
                None => return Err(ObjectError::ConnectionRefused),
            }
        };

        {
            let state = self.state.lock();
            match &*state {
                UnixSocketState::Unbound => {}
                UnixSocketState::Stream(_) => return Err(ObjectError::IsConnected),
                _ => return Err(ObjectError::InvalidArguments),
            }
        }

        let (client_stream, server_stream) = UnixStreamInner::pair();
        let peer_pid = get_current_process().lock().pid.0;
        *client_stream.owner.lock() = Some(Arc::downgrade(self));

        let server_socket = Arc::new(Self {
            state: Mutex::new(UnixSocketState::Stream(server_stream.clone())),
            flags: Mutex::new(ObjectFlags::empty()),
        });
        *server_stream.owner.lock() = Some(Arc::downgrade(&server_socket));
        *server_stream.peer_cred.lock() = SocketPeerCred {
            pid: peer_pid,
            uid: 0,
            gid: 0,
        };

        {
            let mut pending = listener.pending.lock();
            if pending.len() >= listener.backlog {
                return Err(ObjectError::TryAgain);
            }
            pending.push_back(server_socket);
        }

        *self.state.lock() = UnixSocketState::Stream(client_stream);
        if let Some(owner) = listener.owner.lock().as_ref().and_then(Weak::upgrade) {
            Self::wake_pollers(&owner, PollableEvent::CanBeRead);
        }
        Self::wake_io();
        Ok(())
    }

    pub fn getsockopt(&self, level: u64, option_name: u64, option_len: usize) -> ObjectResult<Vec<u8>> {
        if level != socket::SOL_SOCKET {
            return Err(ObjectError::InvalidArguments);
        }

        match option_name {
            socket::SO_ERROR => Self::encode_i32(option_len, 0),
            socket::SO_TYPE => Self::encode_i32(option_len, socket::SOCK_STREAM as i32),
            socket::SO_ACCEPTCONN => Self::encode_i32(
                option_len,
                matches!(&*self.state.lock(), UnixSocketState::Listener(_)) as i32,
            ),
            socket::SO_DOMAIN => Self::encode_i32(option_len, socket::AF_UNIX as i32),
            socket::SO_PROTOCOL => Self::encode_i32(option_len, 0),
            socket::SO_SNDBUF | socket::SO_RCVBUF => {
                Self::encode_i32(option_len, DEFAULT_SOCKET_BUFFER_SIZE)
            }
            socket::SO_REUSEADDR | socket::SO_PASSCRED => Self::encode_i32(option_len, 0),
            socket::SO_PEERCRED => match &*self.state.lock() {
                UnixSocketState::Stream(stream) => {
                    let cred = *stream.peer_cred.lock();
                    Self::encode_ucred(
                        option_len,
                        SocketUcred {
                            pid: i32::try_from(cred.pid).unwrap_or(i32::MAX),
                            uid: cred.uid,
                            gid: cred.gid,
                        },
                    )
                }
                _ => Err(ObjectError::InvalidArguments),
            },
            _ => Err(ObjectError::InvalidArguments),
        }
    }

    fn encode_i32(option_len: usize, value: i32) -> ObjectResult<Vec<u8>> {
        if option_len < mem::size_of::<i32>() {
            return Err(ObjectError::InvalidArguments);
        }

        Ok(value.to_ne_bytes().to_vec())
    }

    fn encode_ucred(option_len: usize, value: SocketUcred) -> ObjectResult<Vec<u8>> {
        if option_len < mem::size_of::<SocketUcred>() {
            return Err(ObjectError::InvalidArguments);
        }

        Ok(unsafe {
            slice::from_raw_parts(
                (&value as *const SocketUcred).cast::<u8>(),
                mem::size_of::<SocketUcred>(),
            )
        }
        .to_vec())
    }

    pub fn accept(self: &Arc<Self>) -> ObjectResult<usize> {
        loop {
            let listener = match &*self.state.lock() {
                UnixSocketState::Listener(listener) => listener.clone(),
                _ => return Err(ObjectError::InvalidArguments),
            };

            if let Some(socket) = listener.pending.lock().pop_front() {
                let slot = get_current_process().lock().push_object(socket);
                return Ok(slot);
            }

            if self.is_nonblocking() {
                return Err(ObjectError::TryAgain);
            }

            block_current(BlockType::WakeRequired {
                wake_type: WakeType::IO,
                deadline: None,
            });
        }
    }
}

impl Default for UnixSocketObject {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for UnixSocketObject {
    fn drop(&mut self) {
        match &*self.state.lock() {
            UnixSocketState::Bound { path } => {
                UNIX_SOCKET_REGISTRY.lock().remove(path);
            }
            UnixSocketState::Listener(listener) => {
                UNIX_SOCKET_REGISTRY.lock().remove(&listener.path);
                Self::wake_io();
            }
            UnixSocketState::Stream(stream) => {
                stream.close_local();
            }
            UnixSocketState::Unbound | UnixSocketState::Closed => {}
        }
    }
}

#[derive(Debug)]
pub enum UnixSocketState {
    Unbound,
    Bound { path: String },
    Listener(Arc<UnixListenerInner>),
    Stream(Arc<UnixStreamInner>),
    Closed,
}

#[derive(Debug)]
pub struct UnixListenerInner {
    pub path: String,
    pub backlog: usize,
    pub pending: Mutex<VecDeque<Arc<UnixSocketObject>>>,
    pub owner: Mutex<Option<Weak<UnixSocketObject>>>,
}

impl UnixListenerInner {
    pub fn new(path: String, backlog: usize) -> Self {
        Self {
            path,
            backlog,
            pending: Mutex::new(VecDeque::new()),
            owner: Mutex::new(None),
        }
    }
}

#[derive(Debug)]
pub struct UnixStreamInner {
    pub recv_buf: Mutex<VecDeque<u8>>,
    pub peer: Mutex<Option<Weak<UnixStreamInner>>>,
    pub owner: Mutex<Option<Weak<UnixSocketObject>>>,
    pub peer_cred: Mutex<SocketPeerCred>,
    pub write_closed: Mutex<bool>,
}

impl UnixStreamInner {
    pub fn new() -> Self {
        Self {
            recv_buf: Mutex::new(VecDeque::new()),
            peer: Mutex::new(None),
            owner: Mutex::new(None),
            peer_cred: Mutex::new(SocketPeerCred::default()),
            write_closed: Mutex::new(false),
        }
    }

    pub fn pair() -> (Arc<Self>, Arc<Self>) {
        let left = Arc::new(Self::new());
        let right = Arc::new(Self::new());

        *left.peer.lock() = Some(Arc::downgrade(&right));
        *right.peer.lock() = Some(Arc::downgrade(&left));

        (left, right)
    }

    fn close_local(&self) {
        if let Some(peer) = self.peer.lock().as_ref().and_then(Weak::upgrade) {
            *peer.write_closed.lock() = true;
            if let Some(owner) = peer.owner.lock().as_ref().and_then(Weak::upgrade) {
                UnixSocketObject::wake_pollers(&owner, PollableEvent::CanBeRead);
                UnixSocketObject::wake_pollers(&owner, PollableEvent::Closed);
            }
        }
        UnixSocketObject::wake_io();
    }
}

impl Object for UnixSocketObject {
    impl_cast_function!("readable", Readable);
    impl_cast_function!("writable", Writable);
    impl_cast_function!("controllable", Controllable);
    impl_cast_function!("pollable", Pollable);
    impl_cast_function_non_trait!("unix_socket", UnixSocketObject);
}

impl Readable for UnixSocketObject {
    fn read(&self, buffer: &mut [u8]) -> ObjectResult<usize> {
        loop {
            let stream = match &*self.state.lock() {
                UnixSocketState::Stream(stream) => stream.clone(),
                _ => return Err(ObjectError::InvalidArguments),
            };

            {
                let mut recv_buf = stream.recv_buf.lock();
                if !recv_buf.is_empty() {
                    let mut bytes_read = 0;
                    while bytes_read < buffer.len() {
                        match recv_buf.pop_front() {
                            Some(byte) => {
                                buffer[bytes_read] = byte;
                                bytes_read += 1;
                            }
                            None => break,
                        }
                    }
                    return Ok(bytes_read);
                }
            }

            let peer_gone = stream
                .peer
                .lock()
                .as_ref()
                .and_then(Weak::upgrade)
                .is_none();
            if peer_gone || *stream.write_closed.lock() {
                return Ok(0);
            }

            if self.is_nonblocking() {
                return Err(ObjectError::TryAgain);
            }

            block_current(BlockType::WakeRequired {
                wake_type: WakeType::IO,
                deadline: None,
            });
        }
    }
}

impl Writable for UnixSocketObject {
    fn write(&self, buffer: &[u8]) -> ObjectResult<usize> {
        let stream = match &*self.state.lock() {
            UnixSocketState::Stream(stream) => stream.clone(),
            _ => return Err(ObjectError::InvalidArguments),
        };

        let peer = stream
            .peer
            .lock()
            .as_ref()
            .and_then(Weak::upgrade)
            .ok_or(ObjectError::BrokenPipe)?;

        peer.recv_buf.lock().extend(buffer.iter().copied());

        if let Some(owner) = peer.owner.lock().as_ref().and_then(Weak::upgrade) {
            Self::wake_pollers(&owner, PollableEvent::CanBeRead);
        }
        Self::wake_io();
        Ok(buffer.len())
    }
}

impl Pollable for UnixSocketObject {
    fn is_event_ready(&self, event: PollableEvent) -> bool {
        match &*self.state.lock() {
            UnixSocketState::Listener(listener) => match event {
                PollableEvent::CanBeRead => !listener.pending.lock().is_empty(),
                _ => false,
            },
            UnixSocketState::Stream(stream) => match event {
                PollableEvent::CanBeRead => {
                    !stream.recv_buf.lock().is_empty()
                        || *stream.write_closed.lock()
                        || stream
                            .peer
                            .lock()
                            .as_ref()
                            .and_then(Weak::upgrade)
                            .is_none()
                }
                PollableEvent::CanBeWritten => stream
                    .peer
                    .lock()
                    .as_ref()
                    .and_then(Weak::upgrade)
                    .is_some(),
                PollableEvent::Closed => {
                    *stream.write_closed.lock()
                        || stream
                            .peer
                            .lock()
                            .as_ref()
                            .and_then(Weak::upgrade)
                            .is_none()
                }
                _ => false,
            },
            _ => false,
        }
    }
}

impl Controllable for UnixSocketObject {
    fn control(&self, request: ControlRequest) -> ObjectResult<isize> {
        match request {
            ControlRequest::GetFlags => Ok(self.flags.lock().bits() as isize),
            ControlRequest::SetFlags(flags) => {
                *self.flags.lock() = flags;
                Ok(0)
            }
        }
    }
}
