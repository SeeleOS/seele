use alloc::{vec, vec::Vec};

use super::device_info::EventDeviceKind;

const EV_SYN: u16 = 0x00;
const EV_KEY: u16 = 0x01;
const EV_REL: u16 = 0x02;

const SYN_REPORT: u16 = 0;

const REL_X: u16 = 0x00;
const REL_Y: u16 = 0x01;

const BTN_LEFT: u16 = 0x110;
const BTN_RIGHT: u16 = 0x111;
const BTN_MIDDLE: u16 = 0x112;

const INPUT_PROP_POINTER: usize = 0x00;
pub(super) const KEY_BITMAP_BYTES: usize = 0x300 / 8;

impl EventDeviceKind {
    pub(super) fn supports_properties(self) -> Vec<u8> {
        let mut bytes = vec![0u8; 4];
        if matches!(self, Self::Mouse) {
            set_bit(&mut bytes, INPUT_PROP_POINTER);
        }
        bytes
    }

    pub(super) fn supported_event_bits(self, ev_type: u8) -> Vec<u8> {
        match (self, ev_type) {
            (Self::Keyboard, 0) => {
                let mut bits = vec![0u8; 1];
                set_bit(&mut bits, EV_SYN as usize);
                set_bit(&mut bits, EV_KEY as usize);
                bits
            }
            (Self::Mouse, 0) => {
                let mut bits = vec![0u8; 1];
                set_bit(&mut bits, EV_SYN as usize);
                set_bit(&mut bits, EV_KEY as usize);
                set_bit(&mut bits, EV_REL as usize);
                bits
            }
            (_, x) if x == EV_SYN as u8 => {
                let mut bits = vec![0u8; 1];
                set_bit(&mut bits, SYN_REPORT as usize);
                bits
            }
            (Self::Keyboard, x) if x == EV_KEY as u8 => {
                let mut bits = vec![0u8; KEY_BITMAP_BYTES];
                for key in 1..=127usize {
                    set_bit(&mut bits, key);
                }
                bits
            }
            (Self::Mouse, x) if x == EV_KEY as u8 => {
                let mut bits = vec![0u8; KEY_BITMAP_BYTES];
                set_bit(&mut bits, BTN_LEFT as usize);
                set_bit(&mut bits, BTN_RIGHT as usize);
                set_bit(&mut bits, BTN_MIDDLE as usize);
                bits
            }
            (Self::Mouse, x) if x == EV_REL as u8 => {
                let mut bits = vec![0u8; 1];
                set_bit(&mut bits, REL_X as usize);
                set_bit(&mut bits, REL_Y as usize);
                bits
            }
            _ => vec![],
        }
    }
}

pub(super) fn ev_key() -> u16 {
    EV_KEY
}

pub(super) fn ev_rel() -> u16 {
    EV_REL
}

pub(super) fn ev_syn() -> u16 {
    EV_SYN
}

pub(super) fn syn_report() -> u16 {
    SYN_REPORT
}

pub(super) fn rel_x() -> u16 {
    REL_X
}

pub(super) fn rel_y() -> u16 {
    REL_Y
}

pub(super) fn btn_left() -> u16 {
    BTN_LEFT
}

pub(super) fn btn_right() -> u16 {
    BTN_RIGHT
}

pub(super) fn btn_middle() -> u16 {
    BTN_MIDDLE
}

fn set_bit(bits: &mut [u8], bit: usize) {
    let index = bit / 8;
    if index < bits.len() {
        bits[index] |= 1 << (bit % 8);
    }
}
