use core::ptr::write_volatile;

use spin::Mutex;

use crate::{
    keyboard::decoding_task::KEYBOARD_QUEUE,
    object::{config::ConfigurateRequest, error::ObjectError, misc::ObjectResult},
    terminal::misc::LINE_BUFFER,
};

pub const K_RAW: u32 = 0x00;
pub const K_XLATE: u32 = 0x01;
pub const K_MEDIUMRAW: u32 = 0x02;
pub const K_UNICODE: u32 = 0x03;
pub const K_OFF: u32 = 0x04;

pub const KD_TEXT: u32 = 0x00;
pub const KD_GRAPHICS: u32 = 0x01;
pub const KD_TEXT0: u32 = 0x02;
pub const KD_TEXT1: u32 = 0x03;

const KT_LATIN: u16 = 0;
const KT_FN: u16 = 1;
const KT_SPEC: u16 = 2;
const KT_PAD: u16 = 3;
const KT_CUR: u16 = 6;
const KT_SHIFT: u16 = 7;
const KT_LETTER: u16 = 11;

const fn k(ty: u16, value: u16) -> u16 {
    (ty << 8) | value
}

const K_ENTER: u16 = k(KT_SPEC, 1);
const K_BREAK: u16 = k(KT_SPEC, 2);
const K_CAPS: u16 = k(KT_SPEC, 8);
const K_NUM: u16 = k(KT_SPEC, 9);
const K_HOLD: u16 = k(KT_SPEC, 10);
const K_ALT: u16 = k(KT_SPEC, 12);
const K_ALTGR: u16 = k(KT_SPEC, 13);
const K_CTRL: u16 = k(KT_SPEC, 14);
const K_CTRLL: u16 = k(KT_SPEC, 15);
const K_CTRLR: u16 = k(KT_SPEC, 16);
const K_SHIFT: u16 = k(KT_SPEC, 17);
const K_SHIFTL: u16 = k(KT_SPEC, 18);
const K_SHIFTR: u16 = k(KT_SPEC, 19);
const K_COMPOSE: u16 = k(KT_SPEC, 127);

const K_F1: u16 = k(KT_FN, 0);
const K_F2: u16 = k(KT_FN, 1);
const K_F3: u16 = k(KT_FN, 2);
const K_F4: u16 = k(KT_FN, 3);
const K_F5: u16 = k(KT_FN, 4);
const K_F6: u16 = k(KT_FN, 5);
const K_F7: u16 = k(KT_FN, 6);
const K_F8: u16 = k(KT_FN, 7);
const K_F9: u16 = k(KT_FN, 8);
const K_F10: u16 = k(KT_FN, 9);
const K_F11: u16 = k(KT_FN, 10);
const K_F12: u16 = k(KT_FN, 11);
const K_FIND: u16 = k(KT_FN, 20);
const K_INSERT: u16 = k(KT_FN, 21);
const K_REMOVE: u16 = k(KT_FN, 22);
const K_SELECT: u16 = k(KT_FN, 23);
const K_PGUP: u16 = k(KT_FN, 24);
const K_PGDN: u16 = k(KT_FN, 25);
const K_MACRO: u16 = k(KT_FN, 26);
const K_PAUSE: u16 = k(KT_FN, 29);

const K_PPLUS: u16 = k(KT_PAD, 0);
const K_PMINUS: u16 = k(KT_PAD, 1);
const K_PSTAR: u16 = k(KT_PAD, 2);
const K_PSLASH: u16 = k(KT_PAD, 3);
const K_PENTER: u16 = k(KT_PAD, 4);
const K_PCOMMA: u16 = k(KT_PAD, 5);
const K_PDOT: u16 = k(KT_PAD, 6);
const K_PPLUSMINUS: u16 = k(KT_PAD, 7);
const K_P0: u16 = k(KT_PAD, 0);
const K_P1: u16 = k(KT_PAD, 1);
const K_P2: u16 = k(KT_PAD, 2);
const K_P3: u16 = k(KT_PAD, 3);
const K_P4: u16 = k(KT_PAD, 4);
const K_P5: u16 = k(KT_PAD, 5);
const K_P6: u16 = k(KT_PAD, 6);
const K_P7: u16 = k(KT_PAD, 7);
const K_P8: u16 = k(KT_PAD, 8);
const K_P9: u16 = k(KT_PAD, 9);

const K_DOWN: u16 = k(KT_CUR, 0);
const K_LEFT: u16 = k(KT_CUR, 1);
const K_RIGHT: u16 = k(KT_CUR, 2);
const K_UP: u16 = k(KT_CUR, 3);

#[derive(Debug, Clone, Copy)]
pub struct LinuxConsoleState {
    pub keyboard_mode: u32,
    pub display_mode: u32,
    pub vt_mode: LinuxVtMode,
    pub active_vt: u32,
}

impl Default for LinuxConsoleState {
    fn default() -> Self {
        Self {
            keyboard_mode: K_UNICODE,
            display_mode: KD_TEXT,
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
        1 => k(KT_SPEC, 27),
        2 => k(KT_LATIN, if shifted { b'!' } else { b'1' } as u16),
        3 => k(KT_LATIN, if shifted { b'@' } else { b'2' } as u16),
        4 => k(KT_LATIN, if shifted { b'#' } else { b'3' } as u16),
        5 => k(KT_LATIN, if shifted { b'$' } else { b'4' } as u16),
        6 => k(KT_LATIN, if shifted { b'%' } else { b'5' } as u16),
        7 => k(KT_LATIN, if shifted { b'^' } else { b'6' } as u16),
        8 => k(KT_LATIN, if shifted { b'&' } else { b'7' } as u16),
        9 => k(KT_LATIN, if shifted { b'*' } else { b'8' } as u16),
        10 => k(KT_LATIN, if shifted { b'(' } else { b'9' } as u16),
        11 => k(KT_LATIN, if shifted { b')' } else { b'0' } as u16),
        12 => k(KT_LATIN, if shifted { b'_' } else { b'-' } as u16),
        13 => k(KT_LATIN, if shifted { b'+' } else { b'=' } as u16),
        14 => k(KT_LATIN, 127),
        15 => k(KT_LATIN, 9),
        16..=25 => {
            let base = b"qwertyuiop"[(index - 16) as usize];
            let ch = if shifted {
                base.to_ascii_uppercase()
            } else {
                base
            };
            k(KT_LETTER, ch as u16)
        }
        26 => k(KT_LATIN, if shifted { b'{' } else { b'[' } as u16),
        27 => k(KT_LATIN, if shifted { b'}' } else { b']' } as u16),
        28 => K_ENTER,
        29 => K_CTRLL,
        30..=38 => {
            let base = b"asdfghjkl"[(index - 30) as usize];
            let ch = if shifted {
                base.to_ascii_uppercase()
            } else {
                base
            };
            k(KT_LETTER, ch as u16)
        }
        39 => k(KT_LATIN, if shifted { b':' } else { b';' } as u16),
        40 => k(KT_LATIN, if shifted { b'"' } else { b'\'' } as u16),
        41 => k(KT_LATIN, if shifted { b'~' } else { b'`' } as u16),
        42 => K_SHIFTL,
        43 => k(KT_LATIN, if shifted { b'|' } else { b'\\' } as u16),
        44..=50 => {
            let base = b"zxcvbnm"[(index - 44) as usize];
            let ch = if shifted {
                base.to_ascii_uppercase()
            } else {
                base
            };
            k(KT_LETTER, ch as u16)
        }
        51 => k(KT_LATIN, if shifted { b'<' } else { b',' } as u16),
        52 => k(KT_LATIN, if shifted { b'>' } else { b'.' } as u16),
        53 => k(KT_LATIN, if shifted { b'?' } else { b'/' } as u16),
        54 => K_SHIFTR,
        55 => K_PSTAR,
        56 => K_ALT,
        57 => k(KT_LATIN, b' ' as u16),
        58 => K_CAPS,
        59 => K_F1,
        60 => K_F2,
        61 => K_F3,
        62 => K_F4,
        63 => K_F5,
        64 => K_F6,
        65 => K_F7,
        66 => K_F8,
        67 => K_F9,
        68 => K_F10,
        69 => K_NUM,
        70 => K_HOLD,
        71 => if shifted { K_FIND } else { K_P7 },
        72 => if shifted { K_UP } else { K_P8 },
        73 => if shifted { K_PGUP } else { K_P9 },
        74 => K_PMINUS,
        75 => if shifted { K_LEFT } else { K_P4 },
        76 => K_P5,
        77 => if shifted { K_RIGHT } else { K_P6 },
        78 => K_PPLUS,
        79 => if shifted { K_SELECT } else { K_P1 },
        80 => if shifted { K_DOWN } else { K_P2 },
        81 => if shifted { K_PGDN } else { K_P3 },
        82 => if shifted { K_INSERT } else { K_P0 },
        83 => if shifted { K_REMOVE } else { K_PDOT },
        87 => K_F11,
        88 => K_F12,
        96 => K_PENTER,
        97 => K_CTRLR,
        98 => K_PSLASH,
        99 => K_BREAK,
        100 => K_ALTGR,
        102 => K_FIND,
        103 => K_UP,
        104 => K_PGUP,
        105 => K_LEFT,
        106 => K_RIGHT,
        107 => K_SELECT,
        108 => K_DOWN,
        109 => K_PGDN,
        110 => K_INSERT,
        111 => K_REMOVE,
        119 => K_PAUSE,
        125 => K_ALT,
        126 => K_ALTGR,
        127 => K_MACRO,
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
            unsafe { write_volatile(*ptr, mode) };
            Ok(Some(0))
        }
        ConfigurateRequest::LinuxKdSetKeyboardMode(mode) => {
            match *mode {
                K_RAW | K_XLATE | K_UNICODE | K_OFF => {
                    state.lock().keyboard_mode = *mode;
                    KEYBOARD_QUEUE
                        .get_or_init(|| Mutex::new(Default::default()))
                        .lock()
                        .clear();
                    LINE_BUFFER.lock().clear();
                    Ok(Some(0))
                }
                _ => Err(ObjectError::InvalidArguments),
            }
        }
        ConfigurateRequest::LinuxKdGetKeyboardType(ptr) => {
            if ptr.is_null() {
                return Err(ObjectError::InvalidArguments);
            }

            const KB_101: u8 = 0x02;
            unsafe { write_volatile(*ptr, KB_101) };
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
            unsafe { write_volatile(*ptr, mode) };
            Ok(Some(0))
        }
        ConfigurateRequest::LinuxKdSetDisplayMode(mode) => {
            match *mode {
                KD_TEXT | KD_GRAPHICS | KD_TEXT0 | KD_TEXT1 => {
                    state.lock().display_mode = *mode;
                    Ok(Some(0))
                }
                _ => Err(ObjectError::InvalidArguments),
            }
        }
        _ => Ok(None),
    }
}
