use crate::memory::utils::Mut;
use alloc::{
    sync::{Arc, Weak},
    vec::Vec,
};
use core::fmt;

use crate::{
    filesystem::info::LinuxStat,
    impl_cast_function,
    object::{
        FileFlags, Object,
        config::ConfigurateRequest,
        error::ObjectError,
        misc::{ObjectRef, ObjectResult},
        queue_helpers::copy_from_queue,
        traits::{Configuratable, Readable, Statable},
    },
    polling::{event::PollableEvent, object::Pollable},
    process::manager::get_current_process,
    thread::{
        with_thread_manager,
        yielding::{
            BlockType, WakeType, cancel_block, finish_block_current, prepare_block_current,
            wake_pollers_for_object,
        },
    },
};

use super::{
    device_info::EventDeviceKind,
    ioctl::handle_ioctl,
    queue::{EventDeviceHubState, EventDeviceState, INPUT_EVENT_SIZE},
};

pub struct EventDeviceHub {
    pub(super) kind: EventDeviceKind,
    pub(super) state: Mut<EventDeviceHubState>,
    pub(super) clients: Mut<Vec<Weak<EventDeviceClientObject>>>,
}

pub struct EventDeviceClientObject {
    pub(super) hub: Weak<EventDeviceHub>,
    pub(super) client_id: u64,
    pub(super) kind: EventDeviceKind,
    pub(super) flags: Mut<FileFlags>,
    pub(super) state: Mut<EventDeviceState>,
}

pub type EventDeviceObject = EventDeviceClientObject;

lazy_static::lazy_static! {
    pub static ref KEYBOARD_EVENT_DEVICE: Arc<EventDeviceHub> =
        Arc::new(EventDeviceHub::new(EventDeviceKind::Keyboard));
    pub static ref MOUSE_EVENT_DEVICE: Arc<EventDeviceHub> =
        Arc::new(EventDeviceHub::new(EventDeviceKind::Mouse));
}

pub fn open_event_device(name: &str) -> Option<ObjectRef> {
    match name {
        "event-kbd" => Some(KEYBOARD_EVENT_DEVICE.open() as ObjectRef),
        "event-mouse" => Some(MOUSE_EVENT_DEVICE.open() as ObjectRef),
        _ => None,
    }
}

impl EventDeviceClientObject {
    fn hub(&self) -> Option<Arc<EventDeviceHub>> {
        self.hub.upgrade()
    }

    pub(super) fn is_revoked(&self) -> bool {
        self.state.lock().revoked
    }

    pub(super) fn revoke(self: &Arc<Self>) {
        {
            let mut state = self.state.lock();
            if state.revoked {
                return;
            }
            state.revoked = true;
            state.queue.clear();
        }

        if let Some(hub) = self.hub() {
            hub.ungrab(self.client_id);
        }

        with_thread_manager(|manager| match self.kind {
            EventDeviceKind::Keyboard => manager.wake_keyboard(),
            EventDeviceKind::Mouse => manager.wake_mouse(),
        });
        let object: ObjectRef = self.clone();
        wake_pollers_for_object(object.clone(), PollableEvent::Error);
        wake_pollers_for_object(object, PollableEvent::Closed);
    }

    pub(super) fn wake_type(&self) -> WakeType {
        match self.kind {
            EventDeviceKind::Keyboard => WakeType::Keyboard,
            EventDeviceKind::Mouse => WakeType::Mouse,
        }
    }

    pub(super) fn wake_readers(self: &Arc<Self>) {
        with_thread_manager(|manager| match self.kind {
            EventDeviceKind::Keyboard => manager.wake_keyboard(),
            EventDeviceKind::Mouse => manager.wake_mouse(),
        });
        let object: ObjectRef = self.clone();
        wake_pollers_for_object(object, PollableEvent::CanBeRead);
    }
}

impl fmt::Debug for EventDeviceHub {
    fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
        Ok(())
    }
}

impl fmt::Debug for EventDeviceClientObject {
    fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
        Ok(())
    }
}

impl Drop for EventDeviceClientObject {
    fn drop(&mut self) {
        if let Some(hub) = self.hub() {
            hub.ungrab(self.client_id);
        }
    }
}

impl Object for EventDeviceClientObject {
    fn get_flags(self: Arc<Self>) -> ObjectResult<FileFlags> {
        Ok(*self.flags.lock())
    }

    fn set_flags(self: Arc<Self>, flags: FileFlags) -> ObjectResult<()> {
        *self.flags.lock() = flags;
        Ok(())
    }

    impl_cast_function!("readable", Readable);
    impl_cast_function!("configuratable", Configuratable);
    impl_cast_function!("pollable", Pollable);
    impl_cast_function!("statable", Statable);
}

impl Readable for EventDeviceClientObject {
    fn read(&self, buffer: &mut [u8]) -> ObjectResult<usize> {
        self.read_with_flags(buffer, *self.flags.lock())
    }

    fn read_with_flags(&self, buffer: &mut [u8], flags: FileFlags) -> ObjectResult<usize> {
        if buffer.len() < INPUT_EVENT_SIZE {
            return Err(ObjectError::InvalidArguments);
        }

        let max_len = buffer.len() - (buffer.len() % INPUT_EVENT_SIZE);
        let buffer = &mut buffer[..max_len];
        loop {
            let maybe_read = {
                let mut state = self.state.lock();
                if state.revoked {
                    return Err(ObjectError::DeviceRevoked);
                }
                let readable = state.queue.len() - (state.queue.len() % INPUT_EVENT_SIZE);
                if readable == 0 {
                    None
                } else {
                    let copy_len = buffer.len().min(readable);
                    Some(copy_from_queue(&mut state.queue, &mut buffer[..copy_len]))
                }
            };
            if let Some(read) = maybe_read {
                return Ok(read);
            }

            if flags.contains(FileFlags::NONBLOCK) {
                return Err(ObjectError::TryAgain);
            }

            if !get_current_process().lock().pending_signals.is_empty() {
                return Err(ObjectError::Interrupted);
            }

            let current = prepare_block_current(BlockType::WakeRequired {
                wake_type: self.wake_type(),
                deadline: None,
            });

            let maybe_read = {
                let mut state = self.state.lock();
                if state.revoked {
                    cancel_block(&current);
                    return Err(ObjectError::DeviceRevoked);
                }
                let readable = state.queue.len() - (state.queue.len() % INPUT_EVENT_SIZE);
                if readable == 0 {
                    None
                } else {
                    let copy_len = buffer.len().min(readable);
                    Some(copy_from_queue(&mut state.queue, &mut buffer[..copy_len]))
                }
            };
            if let Some(read) = maybe_read {
                cancel_block(&current);
                return Ok(read);
            }

            finish_block_current();

            if !get_current_process().lock().pending_signals.is_empty() {
                return Err(ObjectError::Interrupted);
            }
        }
    }
}

impl Configuratable for EventDeviceClientObject {
    fn configure(&self, request: ConfigurateRequest) -> ObjectResult<isize> {
        if self.is_revoked() {
            return Err(ObjectError::DeviceRevoked);
        }

        match request {
            ConfigurateRequest::RawIoctl { request, arg } => handle_ioctl(self, request, arg),
            _ => Err(ObjectError::InvalidRequest),
        }
    }
}

impl Pollable for EventDeviceClientObject {
    fn is_event_ready(&self, event: PollableEvent) -> bool {
        let state = self.state.lock();
        match event {
            PollableEvent::CanBeRead => !state.revoked && state.queue.len() >= INPUT_EVENT_SIZE,
            PollableEvent::Error | PollableEvent::Closed => state.revoked,
            _ => false,
        }
    }
}

impl Statable for EventDeviceClientObject {
    fn stat(&self) -> LinuxStat {
        let rdev = (13u64 << 8) | self.kind.minor();
        LinuxStat::char_device_with_rdev(0o660, rdev)
    }
}
