use alloc::collections::vec_deque::VecDeque;
use core::{mem::size_of, slice};

use ps2_mouse::MouseState;

use crate::misc::time::Time;

use super::{
    event_bits::{
        KEY_BITMAP_BYTES, btn_left, btn_right, ev_key, ev_rel, ev_syn, rel_x, rel_y, syn_report,
    },
    object::EventDeviceObject,
};

#[repr(C)]
#[derive(Clone, Copy)]
struct LinuxInputEvent {
    tv_sec: i64,
    tv_usec: i64,
    type_: u16,
    code: u16,
    value: i32,
}

pub(super) const INPUT_EVENT_SIZE: usize = size_of::<LinuxInputEvent>();

#[derive(Debug)]
pub(super) struct EventDeviceState {
    pub(super) queue: VecDeque<u8>,
    pub(super) key_state: [u8; KEY_BITMAP_BYTES],
    pub(super) clock_id: i32,
    pub(super) mouse_buttons: MouseButtons,
}

#[derive(Debug, Default)]
pub(super) struct MouseButtons {
    pub(super) left: bool,
    pub(super) right: bool,
}

impl EventDeviceObject {
    pub(super) fn new(kind: super::device_info::EventDeviceKind) -> Self {
        Self {
            kind,
            flags: spin::Mutex::new(crate::object::FileFlags::empty()),
            state: spin::Mutex::new(EventDeviceState {
                queue: VecDeque::new(),
                key_state: [0; KEY_BITMAP_BYTES],
                clock_id: 1,
                mouse_buttons: Default::default(),
            }),
        }
    }

    fn enqueue_input_event(&self, queue: &mut VecDeque<u8>, type_: u16, code: u16, value: i32) {
        let timestamp = Time::since_boot();
        let event = LinuxInputEvent {
            tv_sec: timestamp.as_seconds() as i64,
            tv_usec: timestamp.subsec_microseconds() as i64,
            type_,
            code,
            value,
        };
        let bytes = unsafe {
            slice::from_raw_parts(
                (&event as *const LinuxInputEvent).cast::<u8>(),
                size_of::<LinuxInputEvent>(),
            )
        };
        queue.extend(bytes.iter().copied());
    }

    pub(crate) fn push_key_event(&self, code: u16, pressed: bool) {
        let mut state = self.state.lock();
        set_key_state(&mut state.key_state, code as usize, pressed);
        self.enqueue_input_event(&mut state.queue, ev_key(), code, pressed as i32);
        self.enqueue_input_event(&mut state.queue, ev_syn(), syn_report(), 0);
        drop(state);
        self.wake_readers();
    }

    pub(crate) fn push_mouse_state(&self, mouse_state: MouseState) {
        let mut state = self.state.lock();
        let mut changed = false;

        let left = mouse_state.left_button_down();
        if left != state.mouse_buttons.left {
            state.mouse_buttons.left = left;
            set_key_state(&mut state.key_state, btn_left() as usize, left);
            self.enqueue_input_event(&mut state.queue, ev_key(), btn_left(), left as i32);
            changed = true;
        }

        let right = mouse_state.right_button_down();
        if right != state.mouse_buttons.right {
            state.mouse_buttons.right = right;
            set_key_state(&mut state.key_state, btn_right() as usize, right);
            self.enqueue_input_event(&mut state.queue, ev_key(), btn_right(), right as i32);
            changed = true;
        }

        if mouse_state.get_x() != 0 {
            self.enqueue_input_event(&mut state.queue, ev_rel(), rel_x(), mouse_state.get_x() as i32);
            changed = true;
        }

        if mouse_state.get_y() != 0 {
            self.enqueue_input_event(
                &mut state.queue,
                ev_rel(),
                rel_y(),
                -(mouse_state.get_y() as i32),
            );
            changed = true;
        }

        if changed {
            self.enqueue_input_event(&mut state.queue, ev_syn(), syn_report(), 0);
        }

        drop(state);
        if changed {
            self.wake_readers();
        }
    }
}

fn set_key_state(bits: &mut [u8; KEY_BITMAP_BYTES], bit: usize, pressed: bool) {
    let index = bit / 8;
    if index >= bits.len() {
        return;
    }

    if pressed {
        bits[index] |= 1 << (bit % 8);
    } else {
        bits[index] &= !(1 << (bit % 8));
    }
}
