use core::str::from_utf8;

use alloc::sync::Arc;
use spin::Mutex;

use crate::{
    impl_cast_function,
    object::{
        Object,
        config::{LinuxTermios2, LinuxWinsize},
        misc::ObjectResult,
        traits::{Configuratable, Writable},
    },
    s_print,
    terminal::term_trait::AbstractTerminal,
};

use super::linux_kd::LinuxConsoleState;

#[derive(Debug)]
pub struct TerminalObject {
    pub inner: Arc<Mutex<dyn AbstractTerminal>>,
    pub termios: Mutex<LinuxTermios2>,
    pub winsize: Mutex<LinuxWinsize>,
    pub linux_console: Arc<Mutex<LinuxConsoleState>>,
}

impl TerminalObject {
    pub fn new(term: Arc<Mutex<dyn AbstractTerminal>>) -> Self {
        let window_size = term.lock().size();
        Self {
            termios: Mutex::new(LinuxTermios2::new_default()),
            winsize: Mutex::new(LinuxWinsize::from_rows_cols(
                window_size.rows,
                window_size.cols,
            )),
            inner: term,
            linux_console: Arc::new(Mutex::new(LinuxConsoleState::default())),
        }
    }
}

impl Object for TerminalObject {
    impl_cast_function!("configuratable", Configuratable);
    impl_cast_function!("writable", Writable);
}

impl Writable for TerminalObject {
    fn write(&self, buffer: &[u8]) -> ObjectResult<usize> {
        let string = from_utf8(buffer).unwrap_or("Unsupported charcter");
        s_print!("{string}");
        self.inner.lock().push_str(string);
        Ok(buffer.len())
    }
}
