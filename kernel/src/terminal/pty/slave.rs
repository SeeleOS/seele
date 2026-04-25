use core::ptr::{read_volatile, write_volatile};

use alloc::sync::Arc;
use spin::Mutex;

use crate::{
    filesystem::info::LinuxStat,
    impl_cast_function,
    object::{
        FileFlags, Object,
        config::{ConfigurateRequest, LinuxWinsize},
        misc::ObjectResult,
        queue_helpers::{copy_from_queue, read_or_block},
        traits::{Configuratable, Readable, Statable, Writable},
    },
    polling::{event::PollableEvent, object::Pollable},
    process::group::ProcessGroupID,
    process::manager::get_current_process,
    terminal::{
        line_discipline::process_output_bytes,
        linux_kd::{LinuxConsoleState, handle_kd_request},
        linux_vt::handle_vt_request,
        pty::shared::PtyShared,
    },
    thread::{THREAD_MANAGER, yielding::WakeType},
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
    shared: Arc<Mutex<PtyShared>>,
    linux_console: Mutex<LinuxConsoleState>,
    pub flags: Mutex<FileFlags>,
}

impl PtySlave {
    pub fn new(number: u32, shared: Arc<Mutex<PtyShared>>) -> Self {
        Self {
            number,
            shared,
            linux_console: Mutex::new(LinuxConsoleState::default()),
            flags: Mutex::new(FileFlags::default()),
        }
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
}

impl Writable for PtySlave {
    fn write(&self, buffer: &[u8]) -> ObjectResult<usize> {
        let master = {
            let mut shared = self.shared.lock();
            let info = shared.info;
            process_output_bytes(&info, buffer, |byte| {
                shared.from_slave.push_back(byte);
            });
            shared.get_master()
        };

        let mut manager = THREAD_MANAGER.get().unwrap().lock();
        manager.wake_pty();
        manager.wake_poller(master, PollableEvent::CanBeRead);
        Ok(buffer.len())
    }
}

impl Readable for PtySlave {
    fn read(&self, buffer: &mut [u8]) -> ObjectResult<usize> {
        read_or_block(buffer, &self.flags, WakeType::Pty, |buffer| {
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
                let group_id = get_current_process().lock().group_id;
                self.shared.lock().active_group = Some(group_id);
                Ok(0)
            }
            ConfigurateRequest::LinuxTiocgPgrp(ptr) => unsafe {
                let tty_group = self
                    .shared
                    .lock()
                    .active_group
                    .map(|group| group.0 as i32)
                    .unwrap_or(0);
                *ptr = tty_group;
                Ok(0)
            },
            ConfigurateRequest::LinuxTiocnotty => Ok(0),
            ConfigurateRequest::LinuxTiocspgrp(ptr) => unsafe {
                let requested_group = *ptr as u64;
                self.shared.lock().active_group = Some(ProcessGroupID(requested_group));
                Ok(0)
            },
            ConfigurateRequest::LinuxTcGets(_) | ConfigurateRequest::LinuxTcGets2(_) => Ok(0),
            ConfigurateRequest::LinuxTcSets(termios) => unsafe {
                let _ = read_volatile(termios);
                Ok(0)
            },
            ConfigurateRequest::LinuxTcSets2(termios) => unsafe {
                let _ = read_volatile(termios);
                Ok(0)
            },
            ConfigurateRequest::LinuxTiocgwinsz(winsize) => unsafe {
                let info = self.shared.lock().info;
                write_volatile(
                    winsize,
                    LinuxWinsize {
                        ws_row: info.rows as u16,
                        ws_col: info.cols as u16,
                        ws_xpixel: 0,
                        ws_ypixel: 0,
                    },
                );
                Ok(0)
            },
            ConfigurateRequest::LinuxTiocswinsz(winsize) => unsafe {
                let winsize = read_volatile(winsize);
                let mut shared = self.shared.lock();
                if winsize.ws_row != 0 {
                    shared.info.rows = winsize.ws_row as u64;
                }
                if winsize.ws_col != 0 {
                    shared.info.cols = winsize.ws_col as u64;
                }
                Ok(0)
            },
            ConfigurateRequest::LinuxTiocvhangup => {
                let mut shared = self.shared.lock();
                shared.line_buffer.clear();
                shared.from_master.clear();
                Ok(0)
            }
            _ => Ok(0),
        }
    }
}

impl Statable for PtySlave {
    fn stat(&self) -> LinuxStat {
        LinuxStat::char_device_with_rdev(0o620, (136u64 << 8) | self.number as u64)
    }
}
