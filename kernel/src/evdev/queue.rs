use alloc::{
    collections::vec_deque::VecDeque,
    sync::{Arc, Weak},
    vec::Vec,
};
use core::{mem::size_of, slice};

use crate::{misc::time::Time, object::FileFlags};

use super::{
    device_info::EventDeviceKind,
    event_bits::{
        KEY_BITMAP_BYTES, btn_left, btn_middle, btn_right, ev_key, ev_rel, ev_syn, rel_x, rel_y,
        syn_report,
    },
    object::{EventDeviceClientObject, EventDeviceHub},
    ps2::DecodedMousePacket,
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
const EVENT_QUEUE_CAPACITY: usize = 4096;

#[derive(Clone, Copy, Debug)]
pub(super) struct EventDeviceHubState {
    pub(super) key_state: [u8; KEY_BITMAP_BYTES],
    pub(super) mouse_buttons: MouseButtons,
}

#[derive(Debug)]
pub(super) struct EventDeviceState {
    pub(super) queue: VecDeque<u8>,
    pub(super) key_state: [u8; KEY_BITMAP_BYTES],
    pub(super) clock_id: i32,
    pub(super) mouse_buttons: MouseButtons,
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct MouseButtons {
    pub(super) left: bool,
    pub(super) right: bool,
    pub(super) middle: bool,
}

impl EventDeviceHub {
    pub(super) fn new(kind: EventDeviceKind) -> Self {
        Self {
            kind,
            state: spin::Mutex::new(EventDeviceHubState {
                key_state: [0; KEY_BITMAP_BYTES],
                mouse_buttons: MouseButtons::default(),
            }),
            clients: spin::Mutex::new(Vec::new()),
        }
    }

    pub(crate) fn open(self: &Arc<Self>) -> Arc<EventDeviceClientObject> {
        let snapshot = *self.state.lock();
        let client = Arc::new(EventDeviceClientObject::new(self.kind, snapshot));
        self.clients.lock().push(Arc::downgrade(&client));
        client
    }

    fn live_clients(&self) -> Vec<Arc<EventDeviceClientObject>> {
        let mut strong = Vec::new();
        self.clients
            .lock()
            .retain(|client: &Weak<EventDeviceClientObject>| {
                if let Some(client) = client.upgrade() {
                    strong.push(client);
                    true
                } else {
                    false
                }
            });
        strong
    }

    pub(crate) fn push_key_event(&self, code: u16, pressed: bool) {
        set_key_state(&mut self.state.lock().key_state, code as usize, pressed);

        for client in self.live_clients() {
            client.push_key_event(code, pressed);
        }
    }

    pub(crate) fn push_mouse_packet(&self, mouse_state: DecodedMousePacket) {
        {
            let mut state = self.state.lock();
            state.mouse_buttons.left = mouse_state.left;
            state.mouse_buttons.right = mouse_state.right;
            state.mouse_buttons.middle = mouse_state.middle;
            set_key_state(&mut state.key_state, btn_left() as usize, mouse_state.left);
            set_key_state(
                &mut state.key_state,
                btn_right() as usize,
                mouse_state.right,
            );
            set_key_state(
                &mut state.key_state,
                btn_middle() as usize,
                mouse_state.middle,
            );
        }

        for client in self.live_clients() {
            client.push_mouse_packet(mouse_state);
        }
    }
}

impl EventDeviceClientObject {
    pub(super) fn new(kind: EventDeviceKind, snapshot: EventDeviceHubState) -> Self {
        Self {
            kind,
            flags: spin::Mutex::new(FileFlags::empty()),
            state: spin::Mutex::new(EventDeviceState {
                queue: VecDeque::with_capacity(EVENT_QUEUE_CAPACITY),
                key_state: snapshot.key_state,
                clock_id: 1,
                mouse_buttons: snapshot.mouse_buttons,
            }),
        }
    }

    fn enqueue_input_event(&self, queue: &mut VecDeque<u8>, type_: u16, code: u16, value: i32) {
        while queue.len().saturating_add(INPUT_EVENT_SIZE) > EVENT_QUEUE_CAPACITY {
            for _ in 0..INPUT_EVENT_SIZE {
                if queue.pop_front().is_none() {
                    break;
                }
            }
        }

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

    pub(crate) fn push_key_event(self: &Arc<Self>, code: u16, pressed: bool) {
        let mut state = self.state.lock();
        set_key_state(&mut state.key_state, code as usize, pressed);
        self.enqueue_input_event(&mut state.queue, ev_key(), code, pressed as i32);
        self.enqueue_input_event(&mut state.queue, ev_syn(), syn_report(), 0);
        drop(state);
        self.wake_readers();
    }

    pub(crate) fn push_mouse_packet(self: &Arc<Self>, mouse_state: DecodedMousePacket) {
        let mut state = self.state.lock();
        let mut changed = false;

        if mouse_state.left != state.mouse_buttons.left {
            state.mouse_buttons.left = mouse_state.left;
            set_key_state(&mut state.key_state, btn_left() as usize, mouse_state.left);
            self.enqueue_input_event(
                &mut state.queue,
                ev_key(),
                btn_left(),
                mouse_state.left as i32,
            );
            changed = true;
        }

        if mouse_state.right != state.mouse_buttons.right {
            state.mouse_buttons.right = mouse_state.right;
            set_key_state(
                &mut state.key_state,
                btn_right() as usize,
                mouse_state.right,
            );
            self.enqueue_input_event(
                &mut state.queue,
                ev_key(),
                btn_right(),
                mouse_state.right as i32,
            );
            changed = true;
        }

        if mouse_state.middle != state.mouse_buttons.middle {
            state.mouse_buttons.middle = mouse_state.middle;
            set_key_state(
                &mut state.key_state,
                btn_middle() as usize,
                mouse_state.middle,
            );
            self.enqueue_input_event(
                &mut state.queue,
                ev_key(),
                btn_middle(),
                mouse_state.middle as i32,
            );
            changed = true;
        }

        if mouse_state.dx != 0 {
            self.enqueue_input_event(&mut state.queue, ev_rel(), rel_x(), mouse_state.dx as i32);
            changed = true;
        }

        if mouse_state.dy != 0 {
            self.enqueue_input_event(
                &mut state.queue,
                ev_rel(),
                rel_y(),
                -(mouse_state.dy as i32),
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
