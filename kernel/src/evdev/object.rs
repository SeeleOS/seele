use alloc::sync::Arc;
use core::fmt;
use spin::Mutex;

use crate::{
    filesystem::info::LinuxStat,
    impl_cast_function,
    object::{
        FileFlags, Object,
        config::ConfigurateRequest,
        error::ObjectError,
        misc::{ObjectRef, ObjectResult},
        queue_helpers::{copy_from_queue, read_or_block},
        traits::{Configuratable, Readable, Statable},
    },
    polling::{event::PollableEvent, object::Pollable},
    thread::{THREAD_MANAGER, yielding::WakeType},
};

use super::{
    device_info::EventDeviceKind,
    ioctl::handle_ioctl,
    queue::{EventDeviceHubState, EventDeviceState, INPUT_EVENT_SIZE},
};

pub struct EventDeviceHub {
    pub(super) kind: EventDeviceKind,
    pub(super) state: Mutex<EventDeviceHubState>,
    pub(super) clients: Mutex<alloc::vec::Vec<alloc::sync::Weak<EventDeviceClientObject>>>,
}

pub struct EventDeviceClientObject {
    pub(super) kind: EventDeviceKind,
    pub(super) flags: Mutex<FileFlags>,
    pub(super) state: Mutex<EventDeviceState>,
}

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
    pub(super) fn wake_type(&self) -> WakeType {
        match self.kind {
            EventDeviceKind::Keyboard => WakeType::Keyboard,
            EventDeviceKind::Mouse => WakeType::Mouse,
        }
    }

    pub(super) fn wake_readers(self: &Arc<Self>) {
        let mut manager = THREAD_MANAGER.get().unwrap().lock();
        match self.kind {
            EventDeviceKind::Keyboard => manager.wake_keyboard(),
            EventDeviceKind::Mouse => manager.wake_mouse(),
        }
        let object: ObjectRef = self.clone();
        manager.wake_poller(object, PollableEvent::CanBeRead);
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
        if buffer.len() < INPUT_EVENT_SIZE {
            return Err(ObjectError::InvalidArguments);
        }

        let max_len = buffer.len() - (buffer.len() % INPUT_EVENT_SIZE);
        read_or_block(
            &mut buffer[..max_len],
            &self.flags,
            self.wake_type(),
            |buffer| {
                let mut state = self.state.lock();
                let readable = state.queue.len() - (state.queue.len() % INPUT_EVENT_SIZE);
                if readable == 0 {
                    None
                } else {
                    let copy_len = buffer.len().min(readable);
                    Some(copy_from_queue(&mut state.queue, &mut buffer[..copy_len]))
                }
            },
        )
    }
}

impl Configuratable for EventDeviceClientObject {
    fn configure(&self, request: ConfigurateRequest) -> ObjectResult<isize> {
        match request {
            ConfigurateRequest::RawIoctl { request, arg } => {
                handle_ioctl(self.kind, &self.state, request, arg)
            }
            _ => Err(ObjectError::InvalidRequest),
        }
    }
}

impl Pollable for EventDeviceClientObject {
    fn is_event_ready(&self, event: PollableEvent) -> bool {
        matches!(event, PollableEvent::CanBeRead)
            && self.state.lock().queue.len() >= INPUT_EVENT_SIZE
    }
}

impl Statable for EventDeviceClientObject {
    fn stat(&self) -> LinuxStat {
        let rdev = (13u64 << 8) | self.kind.minor();
        LinuxStat::char_device_with_rdev(0o660, rdev)
    }
}
