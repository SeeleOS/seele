use crate::memory::utils::Mut;
use alloc::sync::Arc;
use conquer_once::spin::OnceCell;

use crate::terminal::object::TerminalObject;

pub static DEFAULT_TERMINAL: OnceCell<Arc<Mut<TerminalObject>>> = OnceCell::uninit();
