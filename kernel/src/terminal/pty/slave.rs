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
            _ => Ok(0),
        }
    }
}

impl Statable for PtySlave {
    fn stat(&self) -> LinuxStat {
        LinuxStat::char_device_with_rdev(0o620, (136u64 << 8) | self.number as u64)
    }
}
