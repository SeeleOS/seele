use core::ptr::{read_volatile, write_volatile};

use alloc::{collections::vec_deque::VecDeque, sync::Arc};
use spin::Mutex;

use crate::{
    filesystem::info::LinuxStat,
    impl_cast_function,
    object::{
        FileFlags, Object,
        config::ConfigurateRequest,
        error::ObjectError,
        misc::ObjectResult,
        queue_helpers::{copy_from_queue, read_or_block_with_flags},
        traits::{Configuratable, Readable, Statable, Writable},
    },
    polling::{event::PollableEvent, object::Pollable},
    process::manager::get_current_process,
    signal::{Signal, send_signal_to_process},
    terminal::line_discipline::{process_input_byte, process_output_bytes},
    terminal::pty::{set_pty_lock, shared::PtyShared},
    thread::{THREAD_MANAGER, yielding::WakeType},
};

impl Pollable for PtyMaster {
    fn is_event_ready(&self, event: PollableEvent) -> bool {
        match event {
            PollableEvent::CanBeRead => !self.shared.lock().from_slave.is_empty(),
            PollableEvent::CanBeWritten => true,
            _ => false,
        }
    }
}

#[derive(Debug)]
pub struct PtyMaster {
    number: u32,
    shared: Arc<Mutex<PtyShared>>,
    pub flags: Mutex<FileFlags>,
}

impl PtyMaster {
    pub fn new(number: u32, shared: Arc<Mutex<PtyShared>>) -> Self {
        Self {
            number,
            shared,
            flags: Mutex::new(FileFlags::default()),
        }
    }
}

impl Object for PtyMaster {
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

impl Writable for PtyMaster {
    fn write(&self, buffer: &[u8]) -> ObjectResult<usize> {
        let (master, slave, wrote_input, wrote_echo, interrupt_group) = {
            let mut shared = self.shared.lock();
            let master = shared.get_master();
            let slave = shared.get_slave();
            let termios = shared.termios;
            let mut wrote_input = false;
            let mut wrote_echo = false;
            let mut interrupt_group = None;

            for byte in buffer.iter().copied() {
                let mut queued_input = VecDeque::new();
                let mut queued_echo = VecDeque::new();
                let mut wants_interrupt = false;
                process_input_byte(
                    &termios,
                    &mut shared.line_buffer,
                    byte,
                    |byte| {
                        queued_input.push_back(byte);
                    },
                    |bytes| {
                        process_output_bytes(&termios, bytes, |byte| {
                            queued_echo.push_back(byte);
                        });
                    },
                    || {
                        wants_interrupt = true;
                    },
                );

                if wants_interrupt {
                    shared.line_buffer.clear();
                    interrupt_group = shared.active_group;
                }

                if !queued_input.is_empty() {
                    shared.from_master.append(&mut queued_input);
                    wrote_input = true;
                }

                if !queued_echo.is_empty() {
                    shared.from_slave.append(&mut queued_echo);
                    wrote_echo = true;
                }
            }

            (master, slave, wrote_input, wrote_echo, interrupt_group)
        };

        if let Some(group_id) = interrupt_group {
            group_id
                .get_processes()
                .iter()
                .for_each(|process| send_signal_to_process(process, Signal::SIGINT));
        }

        let mut manager = THREAD_MANAGER.get().unwrap().lock();
        manager.wake_pty();
        if wrote_input {
            manager.wake_poller(slave, PollableEvent::CanBeRead);
        }
        if wrote_echo {
            manager.wake_poller(master, PollableEvent::CanBeRead);
        }
        Ok(buffer.len())
    }
}

impl Readable for PtyMaster {
    fn read(&self, buffer: &mut [u8]) -> ObjectResult<usize> {
        self.read_with_flags(buffer, *self.flags.lock())
    }

    fn read_with_flags(&self, buffer: &mut [u8], flags: FileFlags) -> ObjectResult<usize> {
        read_or_block_with_flags(buffer, flags, WakeType::Pty, |buffer| {
            let mut shared = self.shared.lock();
            if shared.from_slave.is_empty() {
                None
            } else {
                Some(copy_from_queue(&mut shared.from_slave, buffer))
            }
        })
    }
}

impl Configuratable for PtyMaster {
    fn configure(&self, request: ConfigurateRequest) -> ObjectResult<isize> {
        match request {
            ConfigurateRequest::LinuxTiocgptn(number_ptr) => unsafe {
                write_volatile(number_ptr, self.number as i32);
                return Ok(0);
            },
            ConfigurateRequest::LinuxTiocsptlck(lock_ptr) => unsafe {
                let locked = read_volatile(lock_ptr) != 0;
                set_pty_lock(self.number, locked);
                return Ok(0);
            },
            ConfigurateRequest::LinuxTiocgptpeer(open_request) => {
                let slave = {
                    let shared = self.shared.lock();
                    shared.get_slave()
                };
                slave.clone().set_flags(open_request.file_flags())?;
                let fd = get_current_process()
                    .lock()
                    .push_object_with_flags(slave, open_request.fd_flags());
                return isize::try_from(fd).map_err(|_| ObjectError::Other);
            }
            _ => {}
        }

        let slave = {
            let shared = self.shared.lock();
            shared.get_slave()
        };

        slave
            .as_configuratable()
            .map_err(|_| ObjectError::InvalidRequest)?
            .configure(request)
    }
}

impl Statable for PtyMaster {
    fn stat(&self) -> LinuxStat {
        LinuxStat::char_device_with_rdev(0o666, (5u64 << 8) | 2)
    }
}
