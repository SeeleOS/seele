use pc_keyboard::{KeyEvent, KeyState};
use ps2_mouse::Mouse;
use spin::Mutex;

use crate::terminal::linux_kd::linux_keycode_from_keycode;

use super::object::{KEYBOARD_EVENT_DEVICE, MOUSE_EVENT_DEVICE};

lazy_static::lazy_static! {
    static ref PS2_PACKET_DECODER: Mutex<Mouse> = Mutex::new(Mouse::new());
}

pub fn init_mouse_packet_decoder() {
    PS2_PACKET_DECODER
        .lock()
        .set_on_complete(handle_mouse_packet_complete);
}

pub fn process_ps2_mouse_packet(packet: u8) {
    PS2_PACKET_DECODER.lock().process_packet(packet);
}

pub fn push_keyboard_event(event: &KeyEvent) {
    let Some(code) = linux_keycode_from_keycode(event.code) else {
        return;
    };

    KEYBOARD_EVENT_DEVICE.push_key_event(code as u16, !matches!(event.state, KeyState::Up));
}

fn handle_mouse_packet_complete(state: ps2_mouse::MouseState) {
    MOUSE_EVENT_DEVICE.push_mouse_state(state);
}
