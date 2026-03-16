use acpi::sdt::fadt::ArmBootArchFlags;
use alloc::sync::Arc;
use conquer_once::spin::OnceCell;
use os_terminal::Terminal;
use spin::Mutex;

use crate::graphics::{object::TerminalObject, terminal::renderer::TermRenderer};

pub static DEFAULT_TERMINAL: OnceCell<Arc<Mutex<TerminalObject>>> = OnceCell::uninit();
