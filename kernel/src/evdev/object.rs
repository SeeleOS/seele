use alloc::sync::Arc;
use spin::Mutex;

use crate::{
    filesystem::info::LinuxStat,
    impl_cast_function,
    object::{
        FileFlags, Object,
        config::ConfigurateRequest,
        error::ObjectError,
        misc::ObjectResult,
        queue_helpers::{copy_from_queue, read_or_block},
        traits::{Configuratable, Readable, Statable},
    },
    polling::{event::PollableEvent, object::Pollable},
    thread::{THREAD_MANAGER, yielding::WakeType},
};

use super::{
    device_info::EventDeviceKind,
    ioctl::handle_ioctl,
    queue::{EventDeviceState, INPUT_EVENT_SIZE},
};

#[derive(Debug)]
pub struct EventDeviceObject {
    pub(super) kind: EventDeviceKind,
    pub(super) flags: Mutex<FileFlags>,
    pub(super) state: Mutex<EventDeviceState>,
}

lazy_static::lazy_static! {
    pub static ref KEYBOARD_EVENT_DEVICE: Arc<EventDeviceObject> =
        Arc::new(EventDeviceObject::new(EventDeviceKind::Keyboard));
    pub static ref MOUSE_EVENT_DEVICE: Arc<EventDeviceObject> =
        Arc::new(EventDeviceObject::new(EventDeviceKind::Mouse));
}

impl EventDeviceObject {
    pub(super) fn wake_type(&self) -> WakeType {
        match self.kind {
            EventDeviceKind::Keyboard => WakeType::Keyboard,
            EventDeviceKind::Mouse => WakeType::Mouse,
        }
    }

    pub(super) fn wake_readers(&self) {
        let mut manager = THREAD_MANAGER.get().unwrap().lock();
        match self.kind {
            EventDeviceKind::Keyboard => manager.wake_keyboard(),
            EventDeviceKind::Mouse => manager.wake_mouse(),
        }
        let object: crate::object::misc::ObjectRef = match self.kind {
            EventDeviceKind::Keyboard => KEYBOARD_EVENT_DEVICE.clone(),
            EventDeviceKind::Mouse => MOUSE_EVENT_DEVICE.clone(),
        };
        manager.wake_poller(object, PollableEvent::CanBeRead);
    }
}

impl Object for EventDeviceObject {
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

impl Readable for EventDeviceObject {
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

impl Configuratable for EventDeviceObject {
    fn configure(&self, request: ConfigurateRequest) -> ObjectResult<isize> {
        match request {
            ConfigurateRequest::RawIoctl { request, arg } => {
                handle_ioctl(self.kind, &self.state, request, arg)
            }
            _ => Err(ObjectError::InvalidRequest),
        }
    }
}

impl Pollable for EventDeviceObject {
    fn is_event_ready(&self, event: PollableEvent) -> bool {
        matches!(event, PollableEvent::CanBeRead) && self.state.lock().queue.len() >= INPUT_EVENT_SIZE
    }
}

impl Statable for EventDeviceObject {
    fn stat(&self) -> LinuxStat {
        let rdev = (13u64 << 8) | self.kind.minor();
        LinuxStat::char_device_with_rdev(0o660, rdev)
    }
}
