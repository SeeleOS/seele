use core::ptr::write_volatile;

use num_enum::TryFromPrimitive;
use spin::Mutex;

use crate::{
    keyboard::decoding_task::KEYBOARD_QUEUE,
    object::{config::ConfigurateRequest, error::ObjectError, misc::ObjectResult},
    terminal::misc::LINE_BUFFER,
};

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
    pub vt_mode: LinuxVtMode,
    pub active_vt: u32,
}

impl Default for LinuxConsoleState {
    fn default() -> Self {
        Self {
            keyboard_mode: KeyboardMode::Unicode,
            display_mode: DisplayMode::Text,
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
            match mode {
                KeyboardMode::Raw
                | KeyboardMode::Xlate
                | KeyboardMode::Unicode
                | KeyboardMode::Off => {
                    state.lock().keyboard_mode = mode;
                    KEYBOARD_QUEUE
                        .get_or_init(|| Mutex::new(Default::default()))
                        .lock()
                        .clear();
                    LINE_BUFFER.lock().clear();
                    Ok(Some(0))
                }
                KeyboardMode::MediumRaw => Err(ObjectError::InvalidArguments),
            }
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
        _ => Ok(None),
    }
}
