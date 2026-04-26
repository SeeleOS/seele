use core::sync::atomic::{AtomicBool, Ordering};

use alloc::{format, string::String, sync::Arc};
use heapless::Deque;
use ps2_mouse::Mouse;
use spin::Mutex;
use x86_64::{
    instructions::interrupts::without_interrupts, instructions::port::Port,
    structures::idt::InterruptStackFrame,
};

use crate::{
    evdev::{
        MOUSE_EVENT_DEVICE, init_mouse_packet_decoder, process_ps2_mouse_packet_deferred_wake,
    },
    impl_cast_function,
    interrupts::hardware_interrupt::send_eoi,
    object::{
        FileFlags, Object, device::get_device_ref, error::ObjectError, misc::ObjectResult,
        traits::Readable,
    },
    polling::{event::PollableEvent, object::Pollable},
    thread::{
        THREAD_MANAGER,
        yielding::{
            BlockType, WakeType, cancel_block, finish_block_current, prepare_block_current,
        },
    },
};

lazy_static::lazy_static! {
    pub static ref MOUSE_PACKETS: Mutex<Deque<u8, 4096>> = Mutex::new(Deque::new());
}

const STATUS_OUTPUT_FULL: u8 = 1 << 0;

static MOUSE_PENDING: AtomicBool = AtomicBool::new(false);
static MOUSE_EVDEV_PENDING: AtomicBool = AtomicBool::new(false);

pub fn init() -> Result<(), String> {
    init_mouse_packet_decoder();

    let dropped = drain_output_buffer();
    if dropped != 0 {
        log::info!("ps2 mouse: dropped {dropped} stale byte(s) before init");
    }

    let mut mouse = Mouse::new();
    match mouse.init() {
        Ok(()) => Ok(()),
        Err(first_err) => {
            let dropped = drain_output_buffer();
            if dropped != 0 {
                log::warn!(
                    "ps2 mouse: init failed ({first_err}); dropped {dropped} stale byte(s) and retrying"
                );
            } else {
                log::warn!("ps2 mouse: init failed ({first_err}); retrying");
            }

            match mouse.init() {
                Ok(()) => {
                    log::info!("ps2 mouse: init succeeded on retry");
                    Ok(())
                }
                Err(second_err) => Err(format!(
                    "ps2 mouse initialization unavailable after retry: {second_err}"
                )),
            }
        }
    }
}

pub fn has_pending_events() -> bool {
    MOUSE_PENDING.load(Ordering::Acquire)
}

pub fn process_pending_mouse_events() {
    while MOUSE_PENDING.swap(false, Ordering::AcqRel) {
        if MOUSE_EVDEV_PENDING.swap(false, Ordering::AcqRel) {
            MOUSE_EVENT_DEVICE.wake_readers();
        }

        let mut thread_manager = THREAD_MANAGER.get().unwrap().lock();
        thread_manager.wake_mouse();

        if let Ok(mouse_obj) = get_device_ref("ps2mouse") {
            thread_manager.wake_poller(mouse_obj, PollableEvent::CanBeRead);
        }
    }
}

pub extern "x86-interrupt" fn mouse_interrupt_handler(_stack_frame: InterruptStackFrame) {
    let packet = unsafe { Port::new(0x60).read() };
    without_interrupts(|| {
        let mut packets = MOUSE_PACKETS.lock();
        if packets.is_full() {
            let _ = packets.pop_front();
        }
        let _ = packets.push_back(packet);
    });
    if process_ps2_mouse_packet_deferred_wake(packet) {
        MOUSE_EVDEV_PENDING.store(true, Ordering::Release);
    }
    MOUSE_PENDING.store(true, Ordering::Release);
    send_eoi();
}

fn drain_output_buffer() -> usize {
    let mut status_port: Port<u8> = Port::new(0x64);
    let mut data_port: Port<u8> = Port::new(0x60);
    let mut drained = 0;

    while drained < 256 {
        let status = unsafe { status_port.read() };
        if (status & STATUS_OUTPUT_FULL) == 0 {
            break;
        }

        let _ = unsafe { data_port.read() };
        drained += 1;
    }

    drained
}

#[derive(Debug, Default)]
pub struct PS2MouseObject {
    flags: Mutex<FileFlags>,
}

impl Object for PS2MouseObject {
    fn get_flags(self: Arc<Self>) -> ObjectResult<FileFlags> {
        Ok(*self.flags.lock())
    }

    fn set_flags(self: Arc<Self>, flags: FileFlags) -> ObjectResult<()> {
        *self.flags.lock() = flags;

        Ok(())
    }

    impl_cast_function!("readable", Readable);
    impl_cast_function!("pollable", Pollable);
}

impl Pollable for PS2MouseObject {
    fn is_event_ready(&self, event: PollableEvent) -> bool {
        match event {
            PollableEvent::CanBeRead => without_interrupts(|| !MOUSE_PACKETS.lock().is_empty()),
            _ => false,
        }
    }
}

impl Readable for PS2MouseObject {
    fn read(&self, buffer: &mut [u8]) -> ObjectResult<usize> {
        self.read_with_flags(buffer, *self.flags.lock())
    }

    fn read_with_flags(&self, buffer: &mut [u8], flags: FileFlags) -> ObjectResult<usize> {
        loop {
            let mut queue = without_interrupts(|| MOUSE_PACKETS.lock());

            if queue.is_empty() {
                if flags.contains(FileFlags::NONBLOCK) {
                    return Err(ObjectError::TryAgain);
                }

                drop(queue);
                let current = prepare_block_current(BlockType::WakeRequired {
                    wake_type: WakeType::Mouse,
                    deadline: None,
                });

                if without_interrupts(|| !MOUSE_PACKETS.lock().is_empty()) {
                    cancel_block(&current);
                } else {
                    finish_block_current();
                }
            } else {
                let mut read_chars = 0;
                while read_chars < buffer.len() {
                    match queue.pop_front() {
                        Some(val) => {
                            buffer[read_chars] = val;
                            read_chars += 1;
                        }
                        None => break,
                    }
                }

                return Ok(read_chars);
            }
        }
    }
}
