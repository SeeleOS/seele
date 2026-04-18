use pc_keyboard::{KeyEvent, KeyState};
use spin::Mutex;

use crate::terminal::linux_kd::linux_keycode_from_keycode;

use super::object::{KEYBOARD_EVENT_DEVICE, MOUSE_EVENT_DEVICE};

lazy_static::lazy_static! {
    static ref PS2_PACKET_DECODER: Mutex<Ps2MouseDecoder> = Mutex::new(Ps2MouseDecoder::default());
}

pub fn init_mouse_packet_decoder() {
    *PS2_PACKET_DECODER.lock() = Ps2MouseDecoder::default();
}

pub fn process_ps2_mouse_packet(packet: u8) {
    if let Some(state) = PS2_PACKET_DECODER.lock().push_byte(packet) {
        MOUSE_EVENT_DEVICE.push_mouse_packet(state);
    }
}

pub fn push_keyboard_event(event: &KeyEvent) {
    let Some(code) = linux_keycode_from_keycode(event.code) else {
        return;
    };

    KEYBOARD_EVENT_DEVICE.push_key_event(code as u16, !matches!(event.state, KeyState::Up));
}

const PACKET_ALWAYS_ONE: u8 = 1 << 3;
const PACKET_LEFT_BUTTON: u8 = 1 << 0;
const PACKET_RIGHT_BUTTON: u8 = 1 << 1;
const PACKET_MIDDLE_BUTTON: u8 = 1 << 2;
const PACKET_X_SIGN: u8 = 1 << 4;
const PACKET_Y_SIGN: u8 = 1 << 5;
const PACKET_X_OVERFLOW: u8 = 1 << 6;
const PACKET_Y_OVERFLOW: u8 = 1 << 7;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct DecodedMousePacket {
    pub(crate) left: bool,
    pub(crate) right: bool,
    pub(crate) middle: bool,
    pub(crate) dx: i16,
    pub(crate) dy: i16,
}

#[derive(Debug, Default)]
struct Ps2MouseDecoder {
    bytes: [u8; 3],
    index: usize,
}

impl Ps2MouseDecoder {
    fn push_byte(&mut self, byte: u8) -> Option<DecodedMousePacket> {
        if self.index == 0 && (byte & PACKET_ALWAYS_ONE) == 0 {
            return None;
        }

        self.bytes[self.index] = byte;
        self.index += 1;

        if self.index < self.bytes.len() {
            return None;
        }

        self.index = 0;
        Some(self.decode_packet())
    }

    fn decode_packet(&self) -> DecodedMousePacket {
        let flags = self.bytes[0];
        DecodedMousePacket {
            left: (flags & PACKET_LEFT_BUTTON) != 0,
            right: (flags & PACKET_RIGHT_BUTTON) != 0,
            middle: (flags & PACKET_MIDDLE_BUTTON) != 0,
            dx: decode_axis(flags, self.bytes[1], PACKET_X_SIGN, PACKET_X_OVERFLOW),
            dy: decode_axis(flags, self.bytes[2], PACKET_Y_SIGN, PACKET_Y_OVERFLOW),
        }
    }
}

fn decode_axis(flags: u8, byte: u8, sign_bit: u8, overflow_bit: u8) -> i16 {
    if (flags & overflow_bit) != 0 {
        return 0;
    }

    if (flags & sign_bit) != 0 {
        i16::from(byte as i8)
    } else {
        byte as i16
    }
}
