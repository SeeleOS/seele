use alloc::{collections::vec_deque::VecDeque, sync::Arc};
use conquer_once::spin::OnceCell;
use spin::Mutex;

use crate::{
    filesystem::info::LinuxStat,
    impl_cast_function,
    object::{
        FileFlags, Object,
        config::ConfigurateRequest,
        misc::ObjectRef,
        queue_helpers::{copy_from_queue, read_or_block_with_flags},
        traits::{Configuratable, Readable, Statable, Writable},
    },
    polling::{event::PollableEvent, object::Pollable},
    process::group::ProcessGroupID,
    process::manager::get_current_process,
    terminal::{
        linux_kd::{DisplayMode, KeyboardMode, LinuxConsoleState, handle_kd_request},
        linux_vt::handle_vt_request,
        object::TerminalObject,
    },
    thread::{THREAD_MANAGER, yielding::WakeType},
};

pub static CONSOLE_TTY: OnceCell<Arc<TtyDevice>> = OnceCell::uninit();
pub static DEFAULT_TTY: OnceCell<Arc<TtyDevice>> = OnceCell::uninit();

pub fn get_console_tty() -> Arc<TtyDevice> {
    CONSOLE_TTY.get().unwrap().clone()
}

pub fn get_default_tty() -> Arc<TtyDevice> {
    DEFAULT_TTY.get().unwrap().clone()
}

pub fn get_active_tty() -> Arc<TtyDevice> {
    get_default_tty()
}

pub fn wake_tty_poller_readable() {
    let tty: ObjectRef = get_active_tty();
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
    interactive: bool,
    keyboard_queue: Mutex<VecDeque<u8>>,
    terminal_response_queue: Mutex<VecDeque<u8>>,
    raw_queue: Mutex<VecDeque<u8>>,
    medium_raw_queue: Mutex<VecDeque<u8>>,
    line_buffer: Mutex<VecDeque<u8>>,
    /// The foreground process group currently attached to this tty.
    /// Line-discipline generated signals such as Ctrl+C should be sent here.
    pub active_group: Mutex<Option<ProcessGroupID>>,
    pub flags: Mutex<FileFlags>,
}

impl TtyDevice {
    pub fn new(terminal: Arc<Mutex<TerminalObject>>, interactive: bool) -> Self {
        Self {
            terminal,
            linux_console: Mutex::new(LinuxConsoleState::default()),
            interactive,
            keyboard_queue: Mutex::new(VecDeque::new()),
            terminal_response_queue: Mutex::new(VecDeque::new()),
            raw_queue: Mutex::new(VecDeque::new()),
            medium_raw_queue: Mutex::new(VecDeque::new()),
            line_buffer: Mutex::new(VecDeque::new()),
            active_group: Mutex::new(None),
            flags: Mutex::new(FileFlags::empty()),
        }
    }

    pub fn keyboard_mode(&self) -> KeyboardMode {
        self.linux_console.lock().keyboard_mode
    }

    pub fn push_raw_byte(&self, byte: u8) {
        self.raw_queue.lock().push_back(byte);
    }

    pub fn push_medium_raw_bytes(&self, bytes: &[u8]) {
        self.medium_raw_queue.lock().extend(bytes.iter().copied());
    }

    pub fn push_keyboard_byte(&self, byte: u8) {
        self.keyboard_queue.lock().push_back(byte);
    }

    pub fn push_keyboard_bytes(&self, bytes: &[u8]) {
        self.keyboard_queue.lock().extend(bytes.iter().copied());
    }

    pub fn line_buffer(&self) -> &Mutex<VecDeque<u8>> {
        &self.line_buffer
    }

    pub fn clear_input_state(&self) {
        self.keyboard_queue.lock().clear();
        self.terminal_response_queue.lock().clear();
        self.raw_queue.lock().clear();
        self.medium_raw_queue.lock().clear();
        self.line_buffer.lock().clear();
    }

    pub fn clear_line_buffer(&self) {
        self.line_buffer.lock().clear();
    }

    pub fn flush_line_buffer(&self) {
        let mut line_buffer = self.line_buffer.lock();
        let mut keyboard_queue = self.keyboard_queue.lock();
        keyboard_queue.extend(line_buffer.drain(..));
    }

    fn should_route_terminal_responses(&self) -> bool {
        let console = self.linux_console.lock();
        matches!(
            console.keyboard_mode,
            KeyboardMode::Raw | KeyboardMode::MediumRaw | KeyboardMode::Off
        ) || console.display_mode == DisplayMode::Graphics
    }

    fn clear_terminal_response_queue(&self) {
        self.terminal_response_queue.lock().clear();
    }

    fn set_active_group(&self, group: Option<ProcessGroupID>) {
        let changed = {
            let mut active_group = self.active_group.lock();
            let changed = *active_group != group;
            *active_group = group;
            changed
        };

        if changed {
            self.clear_terminal_response_queue();
        }
    }

    fn push_terminal_query_responses(&self, bytes: &[u8]) {
        if !self.interactive || bytes.is_empty() {
            return;
        }

        // Canonical tty readers expect human text input, not terminal query
        // responses. In text-console mode we emulate Linux console behavior
        // and drop xterm-style replies entirely instead of feeding them back
        // into tty input.
        if !self.should_route_terminal_responses() {
            return;
        }

        self.terminal_response_queue
            .lock()
            .extend(bytes.iter().copied());

        THREAD_MANAGER.get().unwrap().lock().wake_keyboard();
    }

    pub fn push_terminal_response_bytes(&self, bytes: &[u8]) {
        self.push_terminal_query_responses(bytes);
        if !bytes.is_empty() {
            wake_tty_poller_readable();
        }
    }
}

impl Pollable for TtyDevice {
    fn is_event_ready(&self, event: PollableEvent) -> bool {
        match event {
            PollableEvent::CanBeRead => {
                if !self.terminal_response_queue.lock().is_empty() {
                    return true;
                }

                match self.keyboard_mode() {
                    KeyboardMode::Raw | KeyboardMode::Off => !self.raw_queue.lock().is_empty(),
                    KeyboardMode::MediumRaw => !self.medium_raw_queue.lock().is_empty(),
                    KeyboardMode::Xlate | KeyboardMode::Unicode => {
                        !self.keyboard_queue.lock().is_empty()
                    }
                }
            }
            PollableEvent::CanBeWritten => true,
            _ => false,
        }
    }
}

impl Object for TtyDevice {
    impl_cast_function!("writable", Writable);
    impl_cast_function!("readable", Readable);
    impl_cast_function!("configuratable", Configuratable);
    impl_cast_function!("pollable", Pollable);
    impl_cast_function!("statable", Statable);
    crate::impl_cast_function_non_trait!("tty_device", TtyDevice);
}

impl Writable for TtyDevice {
    fn write(&self, buffer: &[u8]) -> super::ObjectResult<usize> {
        self.terminal.lock().write(buffer)
    }
}

impl Readable for TtyDevice {
    fn read(&self, buffer: &mut [u8]) -> super::ObjectResult<usize> {
        self.read_with_flags(buffer, *self.flags.lock())
    }

    fn read_with_flags(&self, buffer: &mut [u8], flags: FileFlags) -> super::ObjectResult<usize> {
        read_or_block_with_flags(buffer, flags, WakeType::Keyboard, |buffer| {
            let mut response_queue = self.terminal_response_queue.lock();
            if !response_queue.is_empty() {
                return Some(copy_from_queue(&mut response_queue, buffer));
            }
            drop(response_queue);

            match self.keyboard_mode() {
                KeyboardMode::Raw | KeyboardMode::Off => {
                    let mut queue = self.raw_queue.lock();
                    (!queue.is_empty()).then(|| copy_from_queue(&mut queue, buffer))
                }
                KeyboardMode::MediumRaw => {
                    let mut queue = self.medium_raw_queue.lock();
                    (!queue.is_empty()).then(|| copy_from_queue(&mut queue, buffer))
                }
                KeyboardMode::Xlate | KeyboardMode::Unicode => {
                    let mut queue = self.keyboard_queue.lock();
                    (!queue.is_empty()).then(|| copy_from_queue(&mut queue, buffer))
                }
            }
        })
    }
}

impl Configuratable for TtyDevice {
    fn configure(
        &self,
        request: super::config::ConfigurateRequest,
    ) -> super::misc::ObjectResult<isize> {
        if matches!(
            request,
            ConfigurateRequest::LinuxKdSetKeyboardMode(_)
                | ConfigurateRequest::LinuxKdSetDisplayMode(_)
        ) && let Some(result) = handle_kd_request(&self.linux_console, &request)?
        {
            self.clear_input_state();
            return Ok(result);
        }

        if let Some(result) = handle_kd_request(&self.linux_console, &request)? {
            return Ok(result);
        }

        if let Some(result) = handle_vt_request(&self.linux_console, &request)? {
            return Ok(result);
        }

        match request {
            ConfigurateRequest::LinuxTcFlush(_) => {
                self.clear_input_state();
                Ok(0)
            }
            ConfigurateRequest::LinuxTiocnxcl => Ok(0),
            ConfigurateRequest::LinuxTiocsctty(_) => {
                let group_id = get_current_process().lock().group_id;
                self.set_active_group(Some(group_id));
                Ok(0)
            }
            ConfigurateRequest::LinuxTiocgPgrp(ptr) => unsafe {
                *ptr = self
                    .active_group
                    .lock()
                    .map(|group| group.0 as i32)
                    .unwrap_or(0);
                Ok(0)
            },
            ConfigurateRequest::LinuxTiocnotty => Ok(0),
            ConfigurateRequest::LinuxTiocspgrp(ptr) => unsafe {
                self.set_active_group(Some(ProcessGroupID((*ptr) as u64)));
                Ok(0)
            },
            ConfigurateRequest::LinuxTiocvhangup => {
                self.clear_input_state();
                Ok(0)
            }
            ConfigurateRequest::LinuxTcSets(_) | ConfigurateRequest::LinuxTcSets2(_) => {
                self.terminal.lock().configure(request)
            }
            _ => self.terminal.lock().configure(request),
        }
    }
}

impl Statable for TtyDevice {
    fn stat(&self) -> LinuxStat {
        LinuxStat::char_device(0o666)
    }
}
