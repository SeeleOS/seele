use crate::memory::utils::Mut;
use alloc::{
    collections::{btree_map::BTreeMap, vec_deque::VecDeque},
    sync::Arc,
};
use conquer_once::spin::OnceCell;

use crate::{
    filesystem::info::LinuxStat,
    impl_cast_function,
    memory::user_safe,
    misc::framebuffer::{FRAME_BUFFER, framebuffer_set_user_controlled},
    object::{
        FileFlags, Object,
        config::ConfigurateRequest,
        error::ObjectError,
        misc::ObjectRef,
        queue_helpers::{copy_from_queue, read_or_block_with_flags},
        traits::{Configuratable, Readable, Statable, Writable},
    },
    polling::{event::PollableEvent, object::Pollable},
    process::group::ProcessGroupID,
    process::{ControllingTerminal, manager::get_current_process},
    s_print,
    terminal::{
        linux_kd::{DisplayMode, KeyboardMode, LinuxConsoleState, handle_kd_request},
        linux_vt::handle_vt_request,
        object::TerminalObject,
        output_filter::OutputFilter,
    },
    thread::yielding::{WakeType, wake_pollers_for_object},
};

pub static CONSOLE_TTY: OnceCell<Arc<TtyDevice>> = OnceCell::uninit();
pub static DEFAULT_TTY: OnceCell<Arc<TtyDevice>> = OnceCell::uninit();
pub static ACTIVE_VT: OnceCell<Mut<u32>> = OnceCell::uninit();
pub static VIRTUAL_TTYS: OnceCell<Mut<BTreeMap<u32, Arc<TtyDevice>>>> = OnceCell::uninit();
pub const MAX_VIRTUAL_TTYS: u32 = 6;

pub fn init_virtual_ttys() {
    ACTIVE_VT.get_or_init(|| Mut::new(1));
    VIRTUAL_TTYS.get_or_init(|| Mut::new(BTreeMap::new()));
}

pub fn get_console_tty() -> Arc<TtyDevice> {
    CONSOLE_TTY.get().unwrap().clone()
}

pub fn get_default_tty() -> Arc<TtyDevice> {
    DEFAULT_TTY.get().unwrap().clone()
}

pub fn register_virtual_tty(vt: u32, tty: Arc<TtyDevice>) {
    VIRTUAL_TTYS.get().unwrap().lock().insert(vt, tty);
}

pub fn get_virtual_tty(vt: u32) -> Option<Arc<TtyDevice>> {
    VIRTUAL_TTYS.get().unwrap().lock().get(&vt).cloned()
}

pub fn get_active_vt() -> u32 {
    *ACTIVE_VT.get().unwrap().lock()
}

pub fn set_active_vt(vt: u32) -> bool {
    let Some(tty) = get_virtual_tty(vt) else {
        return false;
    };

    *ACTIVE_VT.get().unwrap().lock() = vt;
    tty.apply_display_mode_to_framebuffer();
    true
}

pub fn get_active_tty() -> Arc<TtyDevice> {
    get_virtual_tty(get_active_vt()).expect("active tty is not registered")
}

pub fn find_unused_virtual_tty() -> Option<u32> {
    let active_vt = get_active_vt();
    let ttys = VIRTUAL_TTYS.get().unwrap().lock().clone();

    ttys.iter()
        .find(|(vt, tty)| **vt != active_vt && tty.active_group.lock().is_none())
        .map(|(vt, _)| *vt)
        .or_else(|| ttys.keys().copied().find(|vt| *vt != active_vt))
}

pub fn wake_tty_poller_readable() {
    let tty: ObjectRef = get_active_tty();
    wake_pollers_for_object(tty, PollableEvent::CanBeRead);
}

#[derive(Debug)]
pub struct TtyDevice {
    terminal: Arc<Mut<TerminalObject>>,
    linux_console: Arc<Mut<LinuxConsoleState>>,
    virtual_terminal: Option<u32>,
    interactive: bool,
    output_filter: Mut<OutputFilter>,
    keyboard_queue: Mut<VecDeque<u8>>,
    terminal_response_queue: Mut<VecDeque<u8>>,
    raw_queue: Mut<VecDeque<u8>>,
    medium_raw_queue: Mut<VecDeque<u8>>,
    line_buffer: Mut<VecDeque<u8>>,
    /// The foreground process group currently attached to this tty.
    /// Line-discipline generated signals such as Ctrl+C should be sent here.
    pub active_group: Mut<Option<ProcessGroupID>>,
    pub flags: Mut<FileFlags>,
}

impl TtyDevice {
    pub fn new(
        terminal: Arc<Mut<TerminalObject>>,
        interactive: bool,
        virtual_terminal: Option<u32>,
    ) -> Self {
        Self {
            terminal,
            linux_console: Arc::new(Mut::new(LinuxConsoleState::default())),
            virtual_terminal,
            interactive,
            output_filter: Mut::new(OutputFilter::default()),
            keyboard_queue: Mut::new(VecDeque::new()),
            terminal_response_queue: Mut::new(VecDeque::new()),
            raw_queue: Mut::new(VecDeque::new()),
            medium_raw_queue: Mut::new(VecDeque::new()),
            line_buffer: Mut::new(VecDeque::new()),
            active_group: Mut::new(None),
            flags: Mut::new(FileFlags::empty()),
        }
    }

    pub fn keyboard_mode(&self) -> KeyboardMode {
        self.linux_console.lock().keyboard_mode
    }

    fn is_active_virtual_terminal(&self) -> bool {
        self.virtual_terminal == Some(get_active_vt())
    }

    fn apply_display_mode_to_framebuffer(&self) {
        match self.linux_console.lock().display_mode {
            DisplayMode::Graphics => framebuffer_set_user_controlled(true),
            DisplayMode::Text | DisplayMode::Text0 | DisplayMode::Text1 => {
                framebuffer_set_user_controlled(false);
                FRAME_BUFFER.get().unwrap().lock().flush();
            }
        }
    }

    pub fn receives_hardware_keyboard_input(&self) -> bool {
        self.linux_console.lock().display_mode != DisplayMode::Graphics
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

    pub fn line_buffer(&self) -> &Mut<VecDeque<u8>> {
        &self.line_buffer
    }

    pub fn foreground_process_group(&self) -> Option<ProcessGroupID> {
        *self.active_group.lock()
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

        self.terminal_response_queue
            .lock()
            .extend(bytes.iter().copied());

        crate::thread::with_thread_manager(|manager| manager.wake_keyboard());
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
    fn get_flags(self: Arc<Self>) -> super::ObjectResult<FileFlags> {
        Ok(*self.flags.lock())
    }

    fn set_flags(self: Arc<Self>, flags: FileFlags) -> super::ObjectResult<()> {
        *self.flags.lock() = flags;
        Ok(())
    }

    impl_cast_function!("writable", Writable);
    impl_cast_function!("readable", Readable);
    impl_cast_function!("configuratable", Configuratable);
    impl_cast_function!("pollable", Pollable);
    impl_cast_function!("statable", Statable);
    crate::impl_cast_function_non_trait!("tty_device", TtyDevice);
}

impl Writable for TtyDevice {
    fn write(&self, buffer: &[u8]) -> super::ObjectResult<usize> {
        let string = core::str::from_utf8(buffer).unwrap_or("Unsupported charcter");
        let filtered = self.output_filter.lock().filter(string);

        for response in &filtered.responses {
            self.push_terminal_response_bytes(response.as_bytes());
        }

        if !filtered.display_text.is_empty() {
            s_print!("{}", filtered.display_text);
            self.terminal
                .lock()
                .write_screen_text(filtered.display_text.as_str());
        }

        Ok(buffer.len())
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
        ) && let Some(result) = handle_kd_request(self.linux_console.as_ref(), &request)?
        {
            if matches!(request, ConfigurateRequest::LinuxKdSetDisplayMode(_))
                && self.is_active_virtual_terminal()
            {
                self.apply_display_mode_to_framebuffer();
            }
            self.clear_input_state();
            return Ok(result);
        }

        if let Some(result) = handle_kd_request(self.linux_console.as_ref(), &request)? {
            return Ok(result);
        }

        if let Some(result) = handle_vt_request(self.linux_console.as_ref(), &request)? {
            return Ok(result);
        }

        match request {
            ConfigurateRequest::LinuxTcFlush(_) => {
                self.clear_input_state();
                Ok(0)
            }
            ConfigurateRequest::LinuxTiocnxcl => Ok(0),
            ConfigurateRequest::LinuxTiocsctty(_) => {
                let process = get_current_process();
                let (group_id, controlling_terminal) = {
                    let process = process.lock();
                    (
                        process.group_id,
                        process.stdin_terminal_rdev().map(ControllingTerminal),
                    )
                };
                self.set_active_group(Some(group_id));
                process.lock().controlling_terminal = controlling_terminal;
                Ok(0)
            }
            ConfigurateRequest::LinuxTiocgPgrp(ptr) => {
                user_safe::write(
                    ptr,
                    &self
                        .active_group
                        .lock()
                        .map(|group| group.0 as i32)
                        .unwrap_or(0),
                )
                .map_err(|_| ObjectError::BadAddress)?;
                Ok(0)
            }
            ConfigurateRequest::LinuxTiocnotty => {
                get_current_process().lock().controlling_terminal = None;
                Ok(0)
            }
            ConfigurateRequest::LinuxTiocspgrp(ptr) => {
                let group = user_safe::read(ptr).map_err(|_| ObjectError::BadAddress)?;
                self.set_active_group(Some(ProcessGroupID(group as u64)));
                Ok(0)
            }
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
