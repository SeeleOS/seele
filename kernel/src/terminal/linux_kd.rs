use core::ptr::write_volatile;

use num_enum::TryFromPrimitive;
use pc_keyboard::KeyCode;
use spin::Mutex;

use crate::object::{config::ConfigurateRequest, error::ObjectError, misc::ObjectResult};

#[derive(Debug, Clone, Copy, TryFromPrimitive, PartialEq, Eq)]
#[repr(u32)]
pub enum KeyboardMode {
    Raw = 0x00,
    Xlate = 0x01,
    MediumRaw = 0x02,
    Unicode = 0x03,
    Off = 0x04,
}

#[derive(Debug, Clone, Copy, TryFromPrimitive, PartialEq, Eq)]
#[repr(u32)]
pub enum DisplayMode {
    Text = 0x00,
    Graphics = 0x01,
    Text0 = 0x02,
    Text1 = 0x03,
}

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
enum KeyboardType {
    Kb101 = 0x02,
}

#[derive(Debug, Clone, Copy)]
#[repr(u16)]
#[allow(dead_code)]
enum KeyType {
    Latin = 0,
    Function = 1,
    Special = 2,
    Pad = 3,
    Cursor = 6,
    Shift = 7,
    Letter = 11,
}

const fn k(ty: KeyType, value: u16) -> u16 {
    ((ty as u16) << 8) | value
}

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
enum KeyValue {
    Enter,
    Break,
    Caps,
    Num,
    Hold,
    Alt,
    AltGr,
    Ctrl,
    CtrlL,
    CtrlR,
    Shift,
    ShiftL,
    ShiftR,
    Compose,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    Find,
    Insert,
    Remove,
    Select,
    PgUp,
    PgDn,
    Macro,
    Pause,
    PPlus,
    PMinus,
    PStar,
    PSlash,
    PEnter,
    PComma,
    PDot,
    PPlusMinus,
    P0,
    P1,
    P2,
    P3,
    P4,
    P5,
    P6,
    P7,
    P8,
    P9,
    Down,
    Left,
    Right,
    Up,
}

impl KeyValue {
    const fn code(self) -> u16 {
        match self {
            Self::Enter => k(KeyType::Special, 1),
            Self::Break => k(KeyType::Special, 2),
            Self::Caps => k(KeyType::Special, 8),
            Self::Num => k(KeyType::Special, 9),
            Self::Hold => k(KeyType::Special, 10),
            Self::Alt => k(KeyType::Special, 12),
            Self::AltGr => k(KeyType::Special, 13),
            Self::Ctrl => k(KeyType::Special, 14),
            Self::CtrlL => k(KeyType::Special, 15),
            Self::CtrlR => k(KeyType::Special, 16),
            Self::Shift => k(KeyType::Special, 17),
            Self::ShiftL => k(KeyType::Special, 18),
            Self::ShiftR => k(KeyType::Special, 19),
            Self::Compose => k(KeyType::Special, 127),
            Self::F1 => k(KeyType::Function, 0),
            Self::F2 => k(KeyType::Function, 1),
            Self::F3 => k(KeyType::Function, 2),
            Self::F4 => k(KeyType::Function, 3),
            Self::F5 => k(KeyType::Function, 4),
            Self::F6 => k(KeyType::Function, 5),
            Self::F7 => k(KeyType::Function, 6),
            Self::F8 => k(KeyType::Function, 7),
            Self::F9 => k(KeyType::Function, 8),
            Self::F10 => k(KeyType::Function, 9),
            Self::F11 => k(KeyType::Function, 10),
            Self::F12 => k(KeyType::Function, 11),
            Self::Find => k(KeyType::Function, 20),
            Self::Insert => k(KeyType::Function, 21),
            Self::Remove => k(KeyType::Function, 22),
            Self::Select => k(KeyType::Function, 23),
            Self::PgUp => k(KeyType::Function, 24),
            Self::PgDn => k(KeyType::Function, 25),
            Self::Macro => k(KeyType::Function, 26),
            Self::Pause => k(KeyType::Function, 29),
            Self::PPlus => k(KeyType::Pad, 0),
            Self::PMinus => k(KeyType::Pad, 1),
            Self::PStar => k(KeyType::Pad, 2),
            Self::PSlash => k(KeyType::Pad, 3),
            Self::PEnter => k(KeyType::Pad, 4),
            Self::PComma => k(KeyType::Pad, 5),
            Self::PDot => k(KeyType::Pad, 6),
            Self::PPlusMinus => k(KeyType::Pad, 7),
            Self::P0 => k(KeyType::Pad, 0),
            Self::P1 => k(KeyType::Pad, 1),
            Self::P2 => k(KeyType::Pad, 2),
            Self::P3 => k(KeyType::Pad, 3),
            Self::P4 => k(KeyType::Pad, 4),
            Self::P5 => k(KeyType::Pad, 5),
            Self::P6 => k(KeyType::Pad, 6),
            Self::P7 => k(KeyType::Pad, 7),
            Self::P8 => k(KeyType::Pad, 8),
            Self::P9 => k(KeyType::Pad, 9),
            Self::Down => k(KeyType::Cursor, 0),
            Self::Left => k(KeyType::Cursor, 1),
            Self::Right => k(KeyType::Cursor, 2),
            Self::Up => k(KeyType::Cursor, 3),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct LinuxConsoleState {
    pub keyboard_mode: KeyboardMode,
    pub display_mode: DisplayMode,
    pub kbrequest_signal: u32,
    pub vt_mode: LinuxVtMode,
    pub active_vt: u32,
}

impl Default for LinuxConsoleState {
    fn default() -> Self {
        Self {
            keyboard_mode: KeyboardMode::Unicode,
            display_mode: DisplayMode::Text,
            kbrequest_signal: 0,
            vt_mode: LinuxVtMode::default(),
            active_vt: 1,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct LinuxVtMode {
    pub mode: i8,
    pub waitv: i8,
    pub relsig: i16,
    pub acqsig: i16,
    pub frsig: i16,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct LinuxKbEntry {
    pub kb_table: u8,
    pub kb_index: u8,
    pub kb_value: u16,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct LinuxVtStat {
    pub v_active: u16,
    pub v_signal: u16,
    pub v_state: u16,
}

fn linux_kb_entry_value(table: u8, index: u8) -> Option<u16> {
    let shifted = matches!(table, 1 | 9);

    let value = match index {
        1 => k(KeyType::Special, 27),
        2 => k(KeyType::Latin, if shifted { b'!' } else { b'1' } as u16),
        3 => k(KeyType::Latin, if shifted { b'@' } else { b'2' } as u16),
        4 => k(KeyType::Latin, if shifted { b'#' } else { b'3' } as u16),
        5 => k(KeyType::Latin, if shifted { b'$' } else { b'4' } as u16),
        6 => k(KeyType::Latin, if shifted { b'%' } else { b'5' } as u16),
        7 => k(KeyType::Latin, if shifted { b'^' } else { b'6' } as u16),
        8 => k(KeyType::Latin, if shifted { b'&' } else { b'7' } as u16),
        9 => k(KeyType::Latin, if shifted { b'*' } else { b'8' } as u16),
        10 => k(KeyType::Latin, if shifted { b'(' } else { b'9' } as u16),
        11 => k(KeyType::Latin, if shifted { b')' } else { b'0' } as u16),
        12 => k(KeyType::Latin, if shifted { b'_' } else { b'-' } as u16),
        13 => k(KeyType::Latin, if shifted { b'+' } else { b'=' } as u16),
        14 => k(KeyType::Latin, 127),
        15 => k(KeyType::Latin, 9),
        16..=25 => {
            let base = b"qwertyuiop"[(index - 16) as usize];
            let ch = if shifted {
                base.to_ascii_uppercase()
            } else {
                base
            };
            k(KeyType::Letter, ch as u16)
        }
        26 => k(KeyType::Latin, if shifted { b'{' } else { b'[' } as u16),
        27 => k(KeyType::Latin, if shifted { b'}' } else { b']' } as u16),
        28 => KeyValue::Enter.code(),
        29 => KeyValue::CtrlL.code(),
        30..=38 => {
            let base = b"asdfghjkl"[(index - 30) as usize];
            let ch = if shifted {
                base.to_ascii_uppercase()
            } else {
                base
            };
            k(KeyType::Letter, ch as u16)
        }
        39 => k(KeyType::Latin, if shifted { b':' } else { b';' } as u16),
        40 => k(KeyType::Latin, if shifted { b'"' } else { b'\'' } as u16),
        41 => k(KeyType::Latin, if shifted { b'~' } else { b'`' } as u16),
        42 => KeyValue::ShiftL.code(),
        43 => k(KeyType::Latin, if shifted { b'|' } else { b'\\' } as u16),
        44..=50 => {
            let base = b"zxcvbnm"[(index - 44) as usize];
            let ch = if shifted {
                base.to_ascii_uppercase()
            } else {
                base
            };
            k(KeyType::Letter, ch as u16)
        }
        51 => k(KeyType::Latin, if shifted { b'<' } else { b',' } as u16),
        52 => k(KeyType::Latin, if shifted { b'>' } else { b'.' } as u16),
        53 => k(KeyType::Latin, if shifted { b'?' } else { b'/' } as u16),
        54 => KeyValue::ShiftR.code(),
        55 => KeyValue::PStar.code(),
        56 => KeyValue::Alt.code(),
        57 => k(KeyType::Latin, b' ' as u16),
        58 => KeyValue::Caps.code(),
        59 => KeyValue::F1.code(),
        60 => KeyValue::F2.code(),
        61 => KeyValue::F3.code(),
        62 => KeyValue::F4.code(),
        63 => KeyValue::F5.code(),
        64 => KeyValue::F6.code(),
        65 => KeyValue::F7.code(),
        66 => KeyValue::F8.code(),
        67 => KeyValue::F9.code(),
        68 => KeyValue::F10.code(),
        69 => KeyValue::Num.code(),
        70 => KeyValue::Hold.code(),
        71 => {
            if shifted {
                KeyValue::Find.code()
            } else {
                KeyValue::P7.code()
            }
        }
        72 => {
            if shifted {
                KeyValue::Up.code()
            } else {
                KeyValue::P8.code()
            }
        }
        73 => {
            if shifted {
                KeyValue::PgUp.code()
            } else {
                KeyValue::P9.code()
            }
        }
        74 => KeyValue::PMinus.code(),
        75 => {
            if shifted {
                KeyValue::Left.code()
            } else {
                KeyValue::P4.code()
            }
        }
        76 => KeyValue::P5.code(),
        77 => {
            if shifted {
                KeyValue::Right.code()
            } else {
                KeyValue::P6.code()
            }
        }
        78 => KeyValue::PPlus.code(),
        79 => {
            if shifted {
                KeyValue::Select.code()
            } else {
                KeyValue::P1.code()
            }
        }
        80 => {
            if shifted {
                KeyValue::Down.code()
            } else {
                KeyValue::P2.code()
            }
        }
        81 => {
            if shifted {
                KeyValue::PgDn.code()
            } else {
                KeyValue::P3.code()
            }
        }
        82 => {
            if shifted {
                KeyValue::Insert.code()
            } else {
                KeyValue::P0.code()
            }
        }
        83 => {
            if shifted {
                KeyValue::Remove.code()
            } else {
                KeyValue::PDot.code()
            }
        }
        87 => KeyValue::F11.code(),
        88 => KeyValue::F12.code(),
        96 => KeyValue::PEnter.code(),
        97 => KeyValue::CtrlR.code(),
        98 => KeyValue::PSlash.code(),
        99 => KeyValue::Break.code(),
        100 => KeyValue::AltGr.code(),
        102 => KeyValue::Find.code(),
        103 => KeyValue::Up.code(),
        104 => KeyValue::PgUp.code(),
        105 => KeyValue::Left.code(),
        106 => KeyValue::Right.code(),
        107 => KeyValue::Select.code(),
        108 => KeyValue::Down.code(),
        109 => KeyValue::PgDn.code(),
        110 => KeyValue::Insert.code(),
        111 => KeyValue::Remove.code(),
        119 => KeyValue::Pause.code(),
        125 => KeyValue::Alt.code(),
        126 => KeyValue::AltGr.code(),
        127 => KeyValue::Macro.code(),
        _ => return None,
    };

    Some(value)
}

pub fn linux_keycode_from_keycode(key: KeyCode) -> Option<u8> {
    Some(match key {
        KeyCode::Escape => 1,
        KeyCode::Key1 => 2,
        KeyCode::Key2 => 3,
        KeyCode::Key3 => 4,
        KeyCode::Key4 => 5,
        KeyCode::Key5 => 6,
        KeyCode::Key6 => 7,
        KeyCode::Key7 => 8,
        KeyCode::Key8 => 9,
        KeyCode::Key9 => 10,
        KeyCode::Key0 => 11,
        KeyCode::OemMinus => 12,
        KeyCode::OemPlus => 13,
        KeyCode::Backspace => 14,
        KeyCode::Tab => 15,
        KeyCode::Q => 16,
        KeyCode::W => 17,
        KeyCode::E => 18,
        KeyCode::R => 19,
        KeyCode::T => 20,
        KeyCode::Y => 21,
        KeyCode::U => 22,
        KeyCode::I => 23,
        KeyCode::O => 24,
        KeyCode::P => 25,
        KeyCode::Oem4 => 26,
        KeyCode::Oem6 => 27,
        KeyCode::Return => 28,
        KeyCode::LControl => 29,
        KeyCode::A => 30,
        KeyCode::S => 31,
        KeyCode::D => 32,
        KeyCode::F => 33,
        KeyCode::G => 34,
        KeyCode::H => 35,
        KeyCode::J => 36,
        KeyCode::K => 37,
        KeyCode::L => 38,
        KeyCode::Oem1 => 39,
        KeyCode::Oem3 => 40,
        KeyCode::Oem8 => 41,
        KeyCode::LShift => 42,
        KeyCode::Oem5 => 43,
        KeyCode::Z => 44,
        KeyCode::X => 45,
        KeyCode::C => 46,
        KeyCode::V => 47,
        KeyCode::B => 48,
        KeyCode::N => 49,
        KeyCode::M => 50,
        KeyCode::OemComma => 51,
        KeyCode::OemPeriod => 52,
        KeyCode::Oem2 => 53,
        KeyCode::RShift => 54,
        KeyCode::NumpadMultiply => 55,
        KeyCode::LAlt => 56,
        KeyCode::Spacebar => 57,
        KeyCode::CapsLock => 58,
        KeyCode::F1 => 59,
        KeyCode::F2 => 60,
        KeyCode::F3 => 61,
        KeyCode::F4 => 62,
        KeyCode::F5 => 63,
        KeyCode::F6 => 64,
        KeyCode::F7 => 65,
        KeyCode::F8 => 66,
        KeyCode::F9 => 67,
        KeyCode::F10 => 68,
        KeyCode::NumpadLock => 69,
        KeyCode::ScrollLock => 70,
        KeyCode::Numpad7 => 71,
        KeyCode::Numpad8 => 72,
        KeyCode::Numpad9 => 73,
        KeyCode::NumpadSubtract => 74,
        KeyCode::Numpad4 => 75,
        KeyCode::Numpad5 => 76,
        KeyCode::Numpad6 => 77,
        KeyCode::NumpadAdd => 78,
        KeyCode::Numpad1 => 79,
        KeyCode::Numpad2 => 80,
        KeyCode::Numpad3 => 81,
        KeyCode::Numpad0 => 82,
        KeyCode::NumpadPeriod => 83,
        KeyCode::Oem7 => 86,
        KeyCode::F11 => 87,
        KeyCode::F12 => 88,
        KeyCode::NumpadEnter => 96,
        KeyCode::RControl | KeyCode::RControl2 => 97,
        KeyCode::NumpadDivide => 98,
        KeyCode::PrintScreen | KeyCode::SysRq => 99,
        KeyCode::RAltGr | KeyCode::RAlt2 => 100,
        KeyCode::Home => 102,
        KeyCode::ArrowUp => 103,
        KeyCode::PageUp => 104,
        KeyCode::ArrowLeft => 105,
        KeyCode::ArrowRight => 106,
        KeyCode::End => 107,
        KeyCode::ArrowDown => 108,
        KeyCode::PageDown => 109,
        KeyCode::Insert => 110,
        KeyCode::Delete => 111,
        KeyCode::PauseBreak => 119,
        KeyCode::LWin => 125,
        KeyCode::RWin => 126,
        KeyCode::Apps => 127,
        _ => return None,
    })
}

pub fn handle_kd_request(
    state: &Mutex<LinuxConsoleState>,
    request: &ConfigurateRequest,
) -> ObjectResult<Option<isize>> {
    match request {
        ConfigurateRequest::LinuxKdGetKeyboardMode(ptr) => {
            if ptr.is_null() {
                return Err(ObjectError::InvalidArguments);
            }

            let mode = state.lock().keyboard_mode;
            unsafe { write_volatile(*ptr, mode as u32) };
            Ok(Some(0))
        }
        ConfigurateRequest::LinuxKdSetKeyboardMode(mode) => {
            let mode = KeyboardMode::try_from(*mode).map_err(|_| ObjectError::InvalidArguments)?;
            state.lock().keyboard_mode = mode;
            Ok(Some(0))
        }
        ConfigurateRequest::LinuxKdGetKeyboardType(ptr) => {
            if ptr.is_null() {
                return Err(ObjectError::InvalidArguments);
            }

            unsafe { write_volatile(*ptr, KeyboardType::Kb101 as u8) };
            Ok(Some(0))
        }
        ConfigurateRequest::LinuxKdGetKeyboardEntry(ptr) => {
            if ptr.is_null() {
                return Err(ObjectError::InvalidArguments);
            }

            let mut entry = unsafe { *(*ptr) };
            entry.kb_value = linux_kb_entry_value(entry.kb_table, entry.kb_index).unwrap_or(0);
            unsafe { write_volatile(*ptr, entry) };
            Ok(Some(0))
        }
        ConfigurateRequest::LinuxKdGetDisplayMode(ptr) => {
            if ptr.is_null() {
                return Err(ObjectError::InvalidArguments);
            }

            let mode = state.lock().display_mode;
            unsafe { write_volatile(*ptr, mode as u32) };
            Ok(Some(0))
        }
        ConfigurateRequest::LinuxKdSetDisplayMode(mode) => {
            let mode = DisplayMode::try_from(*mode).map_err(|_| ObjectError::InvalidArguments)?;
            state.lock().display_mode = mode;
            Ok(Some(0))
        }
        ConfigurateRequest::LinuxKdSignalAccept(signal) => {
            state.lock().kbrequest_signal = *signal;
            Ok(Some(0))
        }
        _ => Ok(None),
    }
}
