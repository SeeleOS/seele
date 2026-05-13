use crate::memory::utils::Mut;
use alloc::sync::Arc;

use crate::{
    filesystem::info::LinuxStat,
    impl_cast_function,
    memory::user_safe,
    object::{
        FileFlags, Object,
        config::ConfigurateRequest,
        error::ObjectError,
        misc::ObjectResult,
        queue_helpers::{copy_from_queue, read_or_block_with_flags},
        traits::{Configuratable, Readable, Statable, Writable},
    },
    polling::{event::PollableEvent, object::Pollable},
    process::group::ProcessGroupID,
    process::{ControllingTerminal, manager::get_current_process},
    terminal::{
        line_discipline::process_output_bytes,
        linux_kd::{LinuxConsoleState, handle_kd_request},
        linux_vt::handle_vt_request,
        pty::shared::PtyShared,
    },
    thread::yielding::{WakeType, wake_pollers_for_object},
};

impl Pollable for PtySlave {
    fn is_event_ready(&self, event: PollableEvent) -> bool {
        match event {
            PollableEvent::CanBeRead => !self.shared.lock().from_master.is_empty(),
            PollableEvent::CanBeWritten => true,
            _ => false,
        }
    }
}

#[derive(Debug)]
pub struct PtySlave {
    number: u32,
    shared: Arc<Mut<PtyShared>>,
    linux_console: Mut<LinuxConsoleState>,
    pub flags: Mut<FileFlags>,
}

impl PtySlave {
    pub fn new(number: u32, shared: Arc<Mut<PtyShared>>) -> Self {
        Self {
            number,
            shared,
            linux_console: Mut::new(LinuxConsoleState::default()),
            flags: Mut::new(FileFlags::default()),
        }
    }

    pub fn foreground_process_group(&self) -> Option<ProcessGroupID> {
        self.shared.lock().active_group
    }
}

impl Object for PtySlave {
    fn get_flags(self: Arc<Self>) -> ObjectResult<FileFlags> {
        Ok(*self.flags.lock())
    }

    fn set_flags(self: Arc<Self>, flags: FileFlags) -> ObjectResult<()> {
        *self.flags.lock() = flags;
        Ok(())
    }

    impl_cast_function!("writable", Writable);
    impl_cast_function!("readable", Readable);
    impl_cast_function!("configuratable", Configuratable);
    impl_cast_function!("pollable", Pollable);
    impl_cast_function!("statable", Statable);
    crate::impl_cast_function_non_trait!("pty_slave", PtySlave);
}

impl Writable for PtySlave {
    fn write(&self, buffer: &[u8]) -> ObjectResult<usize> {
        let master = {
            let mut shared = self.shared.lock();
            let termios = shared.termios;
            process_output_bytes(&termios, buffer, |byte| {
                shared.from_slave.push_back(byte);
            });
            shared.get_master()
        };

        crate::thread::with_thread_manager(|manager| {
            manager.wake_pty();
        });
        wake_pollers_for_object(master, PollableEvent::CanBeRead);
        Ok(buffer.len())
    }
}

impl Readable for PtySlave {
    fn read(&self, buffer: &mut [u8]) -> ObjectResult<usize> {
        self.read_with_flags(buffer, *self.flags.lock())
    }

    fn read_with_flags(&self, buffer: &mut [u8], flags: FileFlags) -> ObjectResult<usize> {
        read_or_block_with_flags(buffer, flags, WakeType::Pty, |buffer| {
            let mut shared = self.shared.lock();
            if shared.from_master.is_empty() {
                None
            } else {
                Some(copy_from_queue(&mut shared.from_master, buffer))
            }
        })
    }
}

impl Configuratable for PtySlave {
    fn configure(&self, request: ConfigurateRequest) -> ObjectResult<isize> {
        if let Some(result) = handle_kd_request(&self.linux_console, &request)? {
            return Ok(result);
        }

        if let Some(result) = handle_vt_request(&self.linux_console, &request)? {
            return Ok(result);
        }

        match request {
            ConfigurateRequest::LinuxTiocsctty(_) => {
                let process = get_current_process();
                let (group_id, controlling_terminal) = {
                    let process = process.lock();
                    (
                        process.group_id,
                        process.stdin_terminal_rdev().map(ControllingTerminal),
                    )
                };
                self.shared.lock().active_group = Some(group_id);
                process.lock().controlling_terminal = controlling_terminal;
                Ok(0)
            }
            ConfigurateRequest::LinuxTiocgPgrp(ptr) => {
                let tty_group = self
                    .shared
                    .lock()
                    .active_group
                    .map(|group| group.0 as i32)
                    .unwrap_or(0);
                user_safe::write(ptr, &tty_group).map_err(|_| ObjectError::BadAddress)?;
                Ok(0)
            }
            ConfigurateRequest::LinuxTiocnotty => {
                get_current_process().lock().controlling_terminal = None;
                Ok(0)
            }
            ConfigurateRequest::LinuxTiocspgrp(ptr) => {
                let requested_group =
                    user_safe::read(ptr).map_err(|_| ObjectError::BadAddress)? as u64;
                self.shared.lock().active_group = Some(ProcessGroupID(requested_group));
                Ok(0)
            }
            ConfigurateRequest::LinuxTcGets(termios) => {
                let termios_state = self.shared.lock().termios;
                user_safe::write(termios, &termios_state.as_linux_termios())
                    .map_err(|_| ObjectError::BadAddress)?;
                Ok(0)
            }
            ConfigurateRequest::LinuxTcSets(termios) => {
                let termios = user_safe::read(termios).map_err(|_| ObjectError::BadAddress)?;
                let mut shared = self.shared.lock();
                shared.termios.apply_linux_termios(&termios);
                Ok(0)
            }
            ConfigurateRequest::LinuxTcGets2(termios) => {
                user_safe::write(termios, &self.shared.lock().termios)
                    .map_err(|_| ObjectError::BadAddress)?;
                Ok(0)
            }
            ConfigurateRequest::LinuxTcSets2(termios) => {
                let termios = user_safe::read(termios).map_err(|_| ObjectError::BadAddress)?;
                let mut shared = self.shared.lock();
                shared.termios.apply_linux_termios2(&termios);
                Ok(0)
            }
            ConfigurateRequest::LinuxTiocgwinsz(winsize) => {
                user_safe::write(winsize, &self.shared.lock().winsize)
                    .map_err(|_| ObjectError::BadAddress)?;
                Ok(0)
            }
            ConfigurateRequest::LinuxTiocswinsz(winsize) => {
                let winsize = user_safe::read(winsize).map_err(|_| ObjectError::BadAddress)?;
                let mut shared = self.shared.lock();
                if winsize.ws_row != 0 {
                    shared.winsize.ws_row = winsize.ws_row;
                }
                if winsize.ws_col != 0 {
                    shared.winsize.ws_col = winsize.ws_col;
                }
                Ok(0)
            }
            ConfigurateRequest::LinuxTiocvhangup => {
                let mut shared = self.shared.lock();
                shared.line_buffer.clear();
                shared.from_master.clear();
                Ok(0)
            }
            _ => Err(ObjectError::InvalidRequest),
        }
    }
}

impl Statable for PtySlave {
    fn stat(&self) -> LinuxStat {
        LinuxStat::char_device_with_rdev(0o620, (136u64 << 8) | self.number as u64)
    }
}
