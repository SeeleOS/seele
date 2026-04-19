use core::{
    pin::Pin,
    sync::atomic::{AtomicBool, Ordering},
    task::{Context, Poll},
};

use futures_util::{Stream, StreamExt, task::AtomicWaker};
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

static MOUSE_PENDING: AtomicBool = AtomicBool::new(false);
static MOUSE_EVDEV_PENDING: AtomicBool = AtomicBool::new(false);
static MOUSE_WAKER: AtomicWaker = AtomicWaker::new();

struct MouseInterruptStream;

impl Stream for MouseInterruptStream {
    type Item = ();

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if MOUSE_PENDING.swap(false, Ordering::AcqRel) {
            return Poll::Ready(Some(()));
        }

        MOUSE_WAKER.register(cx.waker());

        if MOUSE_PENDING.swap(false, Ordering::AcqRel) {
            MOUSE_WAKER.take();
            Poll::Ready(Some(()))
        } else {
            Poll::Pending
        }
    }
}

pub fn init() {
    init_mouse_packet_decoder();
    Mouse::new()
        .init()
        .expect("ps2: failed to initialize mouse");
}

pub async fn process_mouse_events() {
    let mut interrupts = MouseInterruptStream;

    while interrupts.next().await.is_some() {
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
    MOUSE_WAKER.wake();
    send_eoi();
}

#[derive(Debug, Default)]
pub struct PS2MouseObject {
    flags: Mutex<FileFlags>,
}

impl Object for PS2MouseObject {
    fn get_flags(self: alloc::sync::Arc<Self>) -> ObjectResult<FileFlags> {
        Ok(*self.flags.lock())
    }

    fn set_flags(self: alloc::sync::Arc<Self>, flags: FileFlags) -> ObjectResult<()> {
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
        loop {
            let mut queue = without_interrupts(|| MOUSE_PACKETS.lock());

            if queue.is_empty() {
                if self.flags.lock().contains(FileFlags::NONBLOCK) {
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
