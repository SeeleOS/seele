use alloc::{collections::vec_deque::VecDeque, sync::Arc};
use conquer_once::spin::OnceCell;
use spin::Mutex;

use crate::{
    filesystem::info::LinuxStat,
    impl_cast_function,
    keyboard::decoding_task::{KEYBOARD_QUEUE, MEDIUM_RAW_QUEUE, RAW_QUEUE},
    object::{
        FileFlags, Object,
        config::ConfigurateRequest,
        misc::ObjectRef,
        queue_helpers::{copy_from_queue, read_or_block},
        traits::{Configuratable, Readable, Statable, Writable},
    },
    polling::{event::PollableEvent, object::Pollable},
    process::group::ProcessGroupID,
    terminal::{
        linux_kd::{KeyboardMode, LinuxConsoleState, handle_kd_request},
        linux_vt::handle_vt_request,
        object::TerminalObject,
    },
    thread::{THREAD_MANAGER, yielding::WakeType},
};

pub static DEFAULT_TTY: OnceCell<Arc<TtyDevice>> = OnceCell::uninit();

pub fn get_default_tty() -> Arc<TtyDevice> {
    DEFAULT_TTY.get().unwrap().clone()
}

fn get_appropriate_keyboard_queue(mode: KeyboardMode) -> &'static Mutex<VecDeque<u8>> {
    // Linux raw/off expose scan codes, mediumraw exposes Linux keycodes,
    // cooked modes expose decoded bytes.
    match mode {
        KeyboardMode::Raw | KeyboardMode::Off => {
            RAW_QUEUE.get_or_init(|| Mutex::new(VecDeque::new()))
        }
        KeyboardMode::MediumRaw => MEDIUM_RAW_QUEUE.get_or_init(|| Mutex::new(VecDeque::new())),
        KeyboardMode::Xlate | KeyboardMode::Unicode => {
            KEYBOARD_QUEUE.get_or_init(|| Mutex::new(VecDeque::new()))
        }
    }
}

impl Pollable for TtyDevice {
    fn is_event_ready(&self, event: PollableEvent) -> bool {
        match event {
            PollableEvent::CanBeRead => {
                let queue = get_appropriate_keyboard_queue(self.keyboard_mode());
                !queue.lock().is_empty()
            }
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
    linux_console: Mutex<LinuxConsoleState>,
    /// The foreground process group currently attached to this tty.
    /// Line-discipline generated signals such as Ctrl+C should be sent here.
    pub active_group: Mutex<Option<ProcessGroupID>>,
    pub flags: Mutex<FileFlags>,
}

impl TtyDevice {
    pub fn new(terminal: Arc<Mutex<TerminalObject>>) -> Self {
        Self {
            terminal,
            linux_console: Mutex::new(LinuxConsoleState::default()),
            active_group: Mutex::new(None),
            flags: Mutex::new(FileFlags::empty()),
        }
    }

    pub fn keyboard_mode(&self) -> KeyboardMode {
        self.linux_console.lock().keyboard_mode
    }
}

impl Object for TtyDevice {
    impl_cast_function!("writable", Writable);
    impl_cast_function!("readable", Readable);
    impl_cast_function!("configuratable", Configuratable);
    impl_cast_function!("pollable", Pollable);
    impl_cast_function!("statable", Statable);
}

impl Writable for TtyDevice {
    fn write(&self, buffer: &[u8]) -> super::ObjectResult<usize> {
        self.terminal.lock().write(buffer)
    }
}

impl Readable for TtyDevice {
    fn read(&self, buffer: &mut [u8]) -> super::ObjectResult<usize> {
        read_or_block(buffer, &self.flags, WakeType::Keyboard, |buffer| {
            let queue = get_appropriate_keyboard_queue(self.keyboard_mode());
            let mut queue = queue.lock();

            if queue.is_empty() {
                None
            } else {
                Some(copy_from_queue(&mut queue, buffer))
            }
        })
    }
}

impl Configuratable for TtyDevice {
    fn configure(
        &self,
        request: super::config::ConfigurateRequest,
    ) -> super::misc::ObjectResult<isize> {
        if let Some(result) = handle_kd_request(&self.linux_console, &request)? {
            return Ok(result);
        }

        if let Some(result) = handle_vt_request(&self.linux_console, &request)? {
            return Ok(result);
        }

        match request {
            ConfigurateRequest::LinuxTiocgPgrp(ptr) => unsafe {
                *ptr = self.active_group.lock().unwrap().0 as i32;
                Ok(0)
            },
            ConfigurateRequest::LinuxTiocspgrp(ptr) => unsafe {
                *self.active_group.lock() = Some(ProcessGroupID((*ptr) as u64));
                Ok(0)
            },
            _ => self.terminal.lock().configure(request),
        }
    }
}

impl Statable for TtyDevice {
    fn stat(&self) -> LinuxStat {
        LinuxStat::char_device(0o666)
    }
}
