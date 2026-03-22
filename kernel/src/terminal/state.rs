use alloc::sync::Arc;
use conquer_once::spin::OnceCell;
use spin::Mutex;

use crate::terminal::object::TerminalObject;

pub static DEFAULT_TERMINAL: OnceCell<Arc<Mutex<TerminalObject>>> = OnceCell::uninit();
