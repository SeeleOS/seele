use core::str::from_utf8;

use alloc::sync::Arc;
use spin::Mutex;

use crate::{
    impl_cast_function,
    object::{
        Object,
        misc::ObjectResult,
        traits::{Configuratable, Writable},
    },
    s_print,
    terminal::term_trait::AbstractTerminal,
};

use super::linux_kd::LinuxConsoleState;

#[derive(Debug, Clone, Copy)]
pub struct TerminalSettings {
    pub rows: u64,
    pub cols: u64,
    pub echo: bool,
    pub canonical: bool,
    pub send_sig_on_special_chars: bool,
    pub echo_newline: bool,
    pub echo_delete: bool,
    pub map_output_newline_to_crlf: bool,
}

impl TerminalSettings {
    pub const fn new(rows: u64, cols: u64) -> Self {
        Self {
            rows,
            cols,
            echo: true,
            canonical: true,
            send_sig_on_special_chars: true,
            echo_newline: true,
            echo_delete: true,
            map_output_newline_to_crlf: true,
        }
    }
}

impl Default for TerminalSettings {
    fn default() -> Self {
        Self::new(0, 0)
    }
}

#[derive(Debug)]
pub struct TerminalObject {
    pub inner: Arc<Mutex<dyn AbstractTerminal>>,
    pub info: Mutex<TerminalSettings>,
    pub linux_console: Mutex<LinuxConsoleState>,
}

impl TerminalObject {
    pub fn new(term: Arc<Mutex<dyn AbstractTerminal>>) -> Self {
        let window_size = term.lock().size();
        Self {
            info: Mutex::new(TerminalSettings::new(window_size.rows, window_size.cols)),
            inner: term,
            linux_console: Mutex::new(LinuxConsoleState::default()),
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
