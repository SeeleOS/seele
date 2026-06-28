use alloc::{
    collections::VecDeque,
    sync::{Arc, Weak},
    vec::Vec,
};

use crate::{
    filesystem::info::LinuxStat,
    filesystem::procfs::PROC_PIPE_MAX_SIZE,
    impl_cast_function, impl_cast_function_non_trait,
    memory::utils::Mut,
    object::{
        FileFlags, Object,
        config::ConfigurateRequest,
        error::ObjectError,
        misc::{ObjectRef, ObjectResult},
        traits::{Configuratable, Readable, Statable, Writable},
    },
    polling::{event::PollableEvent, object::Pollable, wait_for_object_event_interruptible},
    process::manager::get_current_process,
    socket::SocketError,
    systemcall::utils::{SyscallError, SyscallResult},
    thread::yielding::wake_pollers_for_object,
};
use core::sync::atomic::Ordering;

pub const PIPE_CAPACITY: usize = 64 * 1024;
const PIPE_BUF: usize = 4096;
const PIPE_MAX_SIZE_LIMIT: usize = 1 << 31;
const CAP_SYS_RESOURCE: usize = 24;

#[derive(Debug)]
struct PipeState {
    buffer: VecDeque<u8>,
    capacity: usize,
    readers: usize,
    writers: usize,
}

#[derive(Debug)]
struct PipeInner {
    state: Mut<PipeState>,
    read_endpoint: Mut<Option<Weak<PipeEndpoint>>>,
    write_endpoint: Mut<Option<Weak<PipeEndpoint>>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PipeEndpointKind {
    Read,
    Write,
}

#[derive(Debug)]
pub struct PipeEndpoint {
    inner: Arc<PipeInner>,
    kind: PipeEndpointKind,
    flags: Mut<FileFlags>,
}

impl PipeEndpoint {
    pub fn pair(flags: FileFlags) -> (Arc<Self>, Arc<Self>) {
        let inner = Arc::new(PipeInner {
            state: Mut::new(PipeState {
                buffer: VecDeque::new(),
                capacity: default_pipe_capacity(),
                readers: 0,
                writers: 0,
            }),
            read_endpoint: Mut::new(None),
            write_endpoint: Mut::new(None),
        });
        let read = Arc::new(Self {
            inner: inner.clone(),
            kind: PipeEndpointKind::Read,
            flags: Mut::new(flags),
        });
        let write = Arc::new(Self {
            inner: inner.clone(),
            kind: PipeEndpointKind::Write,
            flags: Mut::new(flags | FileFlags::WRONLY),
        });

        *inner.read_endpoint.lock() = Some(Arc::downgrade(&read));
        *inner.write_endpoint.lock() = Some(Arc::downgrade(&write));
        (read, write)
    }

    fn is_nonblocking(&self) -> bool {
        self.flags.lock().contains(FileFlags::NONBLOCK)
    }

    fn wake_readers(&self) {
        if let Some(read) = self
            .inner
            .read_endpoint
            .lock()
            .as_ref()
            .and_then(Weak::upgrade)
        {
            wake_pollers_for_object(read as ObjectRef, PollableEvent::CanBeRead);
        }
    }

    fn wake_writers(&self) {
        if let Some(write) = self
            .inner
            .write_endpoint
            .lock()
            .as_ref()
            .and_then(Weak::upgrade)
        {
            wake_pollers_for_object(write as ObjectRef, PollableEvent::CanBeWritten);
        }
    }

    fn wake_blocked_writers(&self) {
        if let Some(write) = self
            .inner
            .write_endpoint
            .lock()
            .as_ref()
            .and_then(Weak::upgrade)
        {
            wake_pollers_for_object(write as ObjectRef, PollableEvent::PipeWriteSpace);
        }
    }

    pub fn clone_fd_reference(&self) {
        let mut state = self.inner.state.lock();
        match self.kind {
            PipeEndpointKind::Read => state.readers += 1,
            PipeEndpointKind::Write => state.writers += 1,
        }
    }

    pub fn close_fd_reference(&self) {
        let mut state = self.inner.state.lock();
        match self.kind {
            PipeEndpointKind::Read => {
                state.readers = state.readers.saturating_sub(1);
                if state.readers == 0 {
                    drop(state);
                    self.wake_writers();
                    self.wake_blocked_writers();
                }
            }
            PipeEndpointKind::Write => {
                state.writers = state.writers.saturating_sub(1);
                if state.writers == 0 {
                    drop(state);
                    self.wake_readers();
                }
            }
        }
    }

    pub fn capacity(&self) -> usize {
        self.inner.state.lock().capacity
    }

    pub fn readable_len(&self) -> usize {
        self.inner.state.lock().buffer.len()
    }

    pub fn tee_to(&self, output: &PipeEndpoint, len: usize) -> Result<usize, SyscallError> {
        if self.kind != PipeEndpointKind::Read || output.kind != PipeEndpointKind::Write {
            return Err(SyscallError::InvalidArguments);
        }
        if Arc::ptr_eq(&self.inner, &output.inner) {
            return Err(SyscallError::InvalidArguments);
        }

        let input = self.inner.state.lock();
        if input.buffer.is_empty() {
            return Err(SyscallError::TryAgain);
        }

        let mut output_state = output.inner.state.lock();
        if output_state.readers == 0 {
            return Err(SyscallError::BrokenPipe);
        }
        let available = output_state
            .capacity
            .saturating_sub(output_state.buffer.len());
        if available == 0 {
            return Err(SyscallError::TryAgain);
        }

        let copy_len = len.min(input.buffer.len()).min(available);
        if copy_len == 0 {
            return Err(SyscallError::TryAgain);
        }
        output_state
            .buffer
            .extend(input.buffer.iter().take(copy_len).copied());
        drop(output_state);
        output.wake_readers();
        if let Some(read) = output
            .inner
            .read_endpoint
            .lock()
            .as_ref()
            .and_then(Weak::upgrade)
        {
            read.notify_readable();
        }

        Ok(copy_len)
    }

    pub fn peek_into(&self, len: usize, out: &mut Vec<u8>) -> usize {
        let state = self.inner.state.lock();
        if self.kind != PipeEndpointKind::Read {
            return 0;
        }
        let copy_len = len.min(state.buffer.len());
        out.extend(state.buffer.iter().take(copy_len).copied());
        copy_len
    }

    pub fn set_capacity(&self, capacity: usize) -> Result<usize, SyscallError> {
        if capacity > PIPE_MAX_SIZE_LIMIT {
            return Err(SyscallError::InvalidArguments);
        }

        let pipe_max_size = proc_pipe_max_size();
        let mut state = self.inner.state.lock();
        if capacity < state.buffer.len() {
            return Err(SyscallError::DeviceOrResourceBusy);
        }
        if capacity > pipe_max_size {
            return Err(SyscallError::PermissionDenied);
        }

        state.capacity = capacity.max(PIPE_BUF);
        Ok(state.capacity)
    }
}

fn proc_pipe_max_size() -> usize {
    PROC_PIPE_MAX_SIZE.load(Ordering::Relaxed) as usize
}

fn default_pipe_capacity() -> usize {
    if current_process_has_pipe_resource_privilege() {
        PIPE_CAPACITY
    } else {
        PIPE_CAPACITY.min(proc_pipe_max_size()).max(PIPE_BUF)
    }
}

fn current_process_has_pipe_resource_privilege() -> bool {
    let process = get_current_process();
    let process = process.lock();
    process.effective_uid == 0 || process.capability_effective[0] & (1 << CAP_SYS_RESOURCE) != 0
}

impl Object for PipeEndpoint {
    fn get_flags(self: Arc<Self>) -> ObjectResult<FileFlags> {
        Ok(*self.flags.lock())
    }

    fn set_flags(self: Arc<Self>, flags: FileFlags) -> ObjectResult<()> {
        let status_flags = flags & !(FileFlags::WRONLY | FileFlags::RDWR);
        *self.flags.lock() = match self.kind {
            PipeEndpointKind::Read => status_flags,
            PipeEndpointKind::Write => status_flags | FileFlags::WRONLY,
        };
        Ok(())
    }

    fn notify_readable(self: Arc<Self>) {
        if !self.flags.lock().contains(FileFlags::ASYNC) {
            return;
        }
        crate::object::control::notify_fcntl_async_readable(&(self as ObjectRef));
    }

    fn as_readable(self: Arc<Self>) -> SyscallResult<Arc<dyn Readable>> {
        if self.kind == PipeEndpointKind::Read {
            Ok(self)
        } else {
            Err(SyscallError::BadFileDescriptor)
        }
    }

    fn as_writable(self: Arc<Self>) -> SyscallResult<Arc<dyn Writable>> {
        if self.kind == PipeEndpointKind::Write {
            Ok(self)
        } else {
            Err(SyscallError::BadFileDescriptor)
        }
    }

    impl_cast_function!("configuratable", Configuratable);
    impl_cast_function!("pollable", Pollable);
    impl_cast_function!("statable", Statable);
    impl_cast_function_non_trait!("pipe", PipeEndpoint);
}

impl Statable for PipeEndpoint {
    fn stat(&self) -> LinuxStat {
        const S_IFIFO: u32 = 0o010000;

        LinuxStat {
            st_dev: 1,
            st_nlink: 1,
            st_mode: S_IFIFO | 0o600,
            st_blksize: 4096,
            ..Default::default()
        }
    }
}

impl Configuratable for PipeEndpoint {
    fn configure(&self, request: ConfigurateRequest) -> ObjectResult<isize> {
        match request {
            ConfigurateRequest::LinuxFionRead(out) => {
                if out.is_null() {
                    return Err(ObjectError::BadAddress);
                }
                crate::memory::user_safe::write(out, &(self.readable_len() as i32))
                    .map_err(|_| ObjectError::BadAddress)?;
                Ok(0)
            }
            _ => Err(ObjectError::InvalidRequest),
        }
    }
}

impl Readable for PipeEndpoint {
    fn read(&self, buffer: &mut [u8]) -> ObjectResult<usize> {
        if self.kind != PipeEndpointKind::Read {
            return Err(ObjectError::InvalidArguments);
        }
        if buffer.is_empty() {
            return Ok(0);
        }

        loop {
            let mut state = self.inner.state.lock();
            if !state.buffer.is_empty() {
                let was_full = state.buffer.len() >= state.capacity;
                let read_len = buffer.len().min(state.buffer.len());
                for dst in buffer.iter_mut().take(read_len) {
                    *dst = state
                        .buffer
                        .pop_front()
                        .expect("pipe buffer length changed while draining");
                }
                drop(state);
                if was_full {
                    self.wake_writers();
                    self.wake_blocked_writers();
                }
                return Ok(read_len);
            }

            if state.writers == 0 {
                return Ok(0);
            }
            if self.is_nonblocking() {
                return Err(ObjectError::TryAgain);
            }
            drop(state);
            let read = self
                .inner
                .read_endpoint
                .lock()
                .as_ref()
                .and_then(Weak::upgrade)
                .expect("pipe read endpoint must exist while reading");
            wait_for_object_event_interruptible(read as ObjectRef, PollableEvent::CanBeRead)?;
        }
    }
}

impl Writable for PipeEndpoint {
    fn write(&self, buffer: &[u8]) -> ObjectResult<usize> {
        if self.kind != PipeEndpointKind::Write {
            return Err(ObjectError::InvalidArguments);
        }
        if buffer.is_empty() {
            return Ok(0);
        }

        loop {
            let mut state = self.inner.state.lock();
            if state.readers == 0 {
                return Err(SocketError::BrokenPipe.into());
            }

            let available = state.capacity.saturating_sub(state.buffer.len());
            if available > 0 {
                let write_len = buffer.len().min(available);
                state.buffer.extend(buffer[..write_len].iter().copied());
                drop(state);
                self.wake_readers();
                if let Some(read) = self
                    .inner
                    .read_endpoint
                    .lock()
                    .as_ref()
                    .and_then(Weak::upgrade)
                {
                    read.notify_readable();
                }
                return Ok(write_len);
            }

            if self.is_nonblocking() {
                return Err(ObjectError::TryAgain);
            }
            drop(state);
            let write = self
                .inner
                .write_endpoint
                .lock()
                .as_ref()
                .and_then(Weak::upgrade)
                .expect("pipe write endpoint must exist while writing");
            wait_for_object_event_interruptible(write as ObjectRef, PollableEvent::PipeWriteSpace)?;
        }
    }
}

impl Pollable for PipeEndpoint {
    fn is_event_ready(&self, event: PollableEvent) -> bool {
        let state = self.inner.state.lock();
        match (self.kind, event) {
            (PipeEndpointKind::Read, PollableEvent::CanBeRead) => {
                !state.buffer.is_empty() || state.writers == 0
            }
            (PipeEndpointKind::Read, PollableEvent::Closed | PollableEvent::ReadClosed) => {
                state.writers == 0
            }
            (PipeEndpointKind::Write, PollableEvent::CanBeWritten) => {
                state.readers == 0 || state.capacity.saturating_sub(state.buffer.len()) >= PIPE_BUF
            }
            (PipeEndpointKind::Write, PollableEvent::PipeWriteSpace) => {
                state.readers == 0 || state.buffer.len() < state.capacity
            }
            (PipeEndpointKind::Write, PollableEvent::Closed) => state.readers == 0,
            _ => false,
        }
    }
}
