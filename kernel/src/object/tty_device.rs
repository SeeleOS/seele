use alloc::sync::Arc;
use conquer_once::spin::OnceCell;
use spin::Mutex;

use crate::{
    impl_cast_function,
    keyboard::{decoding_task::KEYBOARD_QUEUE, object::KeyboardObject},
    object::{
        Object,
        misc::ObjectRef,
        traits::{Configuratable, Controllable, Readable, Writable},
    },
    polling::{event::PollableEvent, object::Pollable},
    process::group::ProcessGroupID,
    terminal::object::TerminalObject,
    thread::THREAD_MANAGER,
};

pub static DEFAULT_TTY: OnceCell<Arc<TtyDevice>> = OnceCell::uninit();

pub fn get_default_tty() -> Arc<TtyDevice> {
    DEFAULT_TTY.get().unwrap().clone()
}

impl Pollable for TtyDevice {
    fn is_event_ready(&self, event: PollableEvent) -> bool {
        match event {
            PollableEvent::CanBeRead => !KEYBOARD_QUEUE
                .get_or_init(|| Mutex::new(Default::default()))
                .lock()
                .is_empty(),
            PollableEvent::CanBeWritten => true,
            _ => false,
        }
    }
}

pub fn wake_tty_poller_readable() {
    let tty: ObjectRef = get_default_tty();
    THREAD_MANAGER
        .get()
        .unwrap()
        .lock()
        .wake_poller(tty, PollableEvent::CanBeRead);
}

#[derive(Debug)]
pub struct TtyDevice {
    terminal: Arc<Mutex<TerminalObject>>,
    /// The foreground process group currently attached to this tty.
    /// Line-discipline generated signals such as Ctrl+C should be sent here.
    pub active_group: Mutex<Option<ProcessGroupID>>,
}

impl TtyDevice {
    pub fn new(terminal: Arc<Mutex<TerminalObject>>) -> Self {
        Self {
            terminal,
            active_group: Mutex::new(None),
        }
    }
}

impl Object for TtyDevice {
    impl_cast_function!("writable", Writable);
    impl_cast_function!("readable", Readable);
    impl_cast_function!("configuratable", Configuratable);
    impl_cast_function!("controllable", Controllable);
    impl_cast_function!("pollable", Pollable);
}

impl Configuratable for TtyDevice {
    fn configure(&self, request: super::config::ConfigurateRequest) -> super::ObjectResult<isize> {
        log::trace!("tty: configure");
        self.terminal.lock().configure(request)
    }
}

impl Writable for TtyDevice {
    fn write(&self, buffer: &[u8]) -> super::ObjectResult<usize> {
        self.terminal.lock().write(buffer)
    }
}

impl Readable for TtyDevice {
    fn read(&self, buffer: &mut [u8]) -> super::ObjectResult<usize> {
        KeyboardObject.read(buffer)
    }
}

impl Controllable for TtyDevice {
    fn control(
        &self,
        _command: super::control::ControlCommand,
        _arg: u64,
    ) -> super::misc::ObjectResult<isize> {
        // Stub
        Ok(0)
    }
}
