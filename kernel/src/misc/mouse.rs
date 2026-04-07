use alloc::collections::vec_deque::VecDeque;
use heapless::{Deque, mpmc::Queue};
use ps2_mouse::Mouse;
use seele_sys::abi::object::ObjectFlags;
use spin::Mutex;
use x86_64::{
    instructions::port::Port,
    structures::{self, idt::InterruptStackFrame},
};

use crate::{
    impl_cast_function,
    interrupts::hardware_interrupt::send_eoi,
    object::{
        Object, device::get_device, error::ObjectError, misc::ObjectResult, traits::Readable,
    },
    polling::{event::PollableEvent, object::Pollable},
    println, s_print,
    thread::{
        THREAD_MANAGER,
        yielding::{BlockType, WakeType, block_current},
    },
};

lazy_static::lazy_static! {
    pub static ref MOUSE_PACKETS: Mutex<Deque<u8, 1024>> = Mutex::new(Deque::new());
}

pub fn init() {
    Mouse::new().init().unwrap();
}

pub extern "x86-interrupt" fn mouse_interrupt_handler(_stack_frame: InterruptStackFrame) {
    unsafe {
        MOUSE_PACKETS
            .lock()
            .push_back(Port::new(0x60).read())
            .unwrap();
    }

    if let Ok(mouse_obj) = get_device("ps2mouse".into()) {
        THREAD_MANAGER
            .get()
            .unwrap()
            .lock()
            .wake_poller(mouse_obj, PollableEvent::CanBeRead);
    }

    send_eoi();
}

#[derive(Debug, Default)]
pub struct PS2MouseObject {
    flags: Mutex<ObjectFlags>,
}

impl Object for PS2MouseObject {
    fn get_flags(self: alloc::sync::Arc<Self>) -> ObjectResult<ObjectFlags> {
        Ok(*self.flags.lock())
    }

    fn set_flags(self: alloc::sync::Arc<Self>, flags: ObjectFlags) -> ObjectResult<()> {
        *self.flags.lock() = flags;

        Ok(())
    }

    impl_cast_function!("readable", Readable);
    impl_cast_function!("pollable", Pollable);
}

impl Pollable for PS2MouseObject {
    fn is_event_ready(&self, event: PollableEvent) -> bool {
        match event {
            PollableEvent::CanBeRead => !MOUSE_PACKETS.lock().is_empty(),
            _ => false,
        }
    }
}

impl Readable for PS2MouseObject {
    fn read(&self, buffer: &mut [u8]) -> ObjectResult<usize> {
        loop {
            let mut queue = MOUSE_PACKETS.lock();

            if queue.is_empty() {
                if self.flags.lock().contains(ObjectFlags::NONBLOCK) {
                    return Err(ObjectError::TryAgain);
                }

                drop(queue);
                block_current(BlockType::WakeRequired {
                    wake_type: WakeType::Mouse,
                    deadline: None,
                });
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
