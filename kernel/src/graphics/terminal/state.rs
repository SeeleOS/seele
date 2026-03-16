use conquer_once::spin::OnceCell;
use os_terminal::Terminal;
use spin::Mutex;

use crate::graphics::terminal::renderer::TermRenderer;

pub static TERMINAL: OnceCell<Mutex<Terminal<TermRenderer>>> = OnceCell::uninit();

