use alloc::{
    collections::VecDeque,
    sync::{Arc, Weak},
};

use crate::{
    filesystem::info::LinuxStat,
    impl_cast_function, impl_cast_function_non_trait,
    memory::utils::Mut,
    object::{
        FileFlags, Object,
        error::ObjectError,
        misc::{ObjectRef, ObjectResult},
        traits::{Readable, Statable, Writable},
    },
    polling::{event::PollableEvent, object::Pollable, wait_for_object_event},
    socket::SocketError,
    thread::yielding::wake_pollers_for_object,
};

const PIPE_CAPACITY: usize = 64 * 1024;

#[derive(Debug)]
struct PipeState {
    buffer: VecDeque<u8>,
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
            flags: Mut::new(flags),
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
}

impl Object for PipeEndpoint {
    fn get_flags(self: Arc<Self>) -> ObjectResult<FileFlags> {
        Ok(*self.flags.lock())
    }

    fn set_flags(self: Arc<Self>, flags: FileFlags) -> ObjectResult<()> {
        *self.flags.lock() = flags;
        Ok(())
    }

    impl_cast_function!("readable", Readable);
    impl_cast_function!("writable", Writable);
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
                let was_full = state.buffer.len() >= PIPE_CAPACITY;
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
            wait_for_object_event(read as ObjectRef, PollableEvent::CanBeRead);
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

            let available = PIPE_CAPACITY.saturating_sub(state.buffer.len());
            if available > 0 {
                let write_len = buffer.len().min(available);
                state.buffer.extend(buffer[..write_len].iter().copied());
                drop(state);
                self.wake_readers();
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
            wait_for_object_event(write as ObjectRef, PollableEvent::CanBeWritten);
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
                state.readers == 0 || state.buffer.len() < PIPE_CAPACITY
            }
            (PipeEndpointKind::Write, PollableEvent::Closed) => state.readers == 0,
            _ => false,
        }
    }
}
