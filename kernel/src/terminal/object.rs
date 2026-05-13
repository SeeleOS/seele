use core::str::from_utf8;

use crate::memory::utils::Mut;
use alloc::{string::String, sync::Arc};
use x86_64::instructions::interrupts::without_interrupts;

use crate::{
    impl_cast_function,
    object::{
        FileFlags, Object,
        config::{LinuxTermios2, LinuxWinsize},
        misc::ObjectResult,
        open_state::OpenState,
        traits::{Configuratable, Writable},
    },
    s_print,
    terminal::term_trait::AbstractTerminal,
};

use super::linux_kd::LinuxConsoleState;

#[derive(Debug)]
pub struct TerminalObject {
    pub inner: Arc<Mut<dyn AbstractTerminal>>,
    pub termios: Mut<LinuxTermios2>,
    pub winsize: Mut<LinuxWinsize>,
    pub linux_console: Arc<Mut<LinuxConsoleState>>,
    open_state: OpenState,
}

impl TerminalObject {
    pub fn new(term: Arc<Mut<dyn AbstractTerminal>>) -> Self {
        let window_size = term.lock().size();
        Self {
            termios: Mut::new(LinuxTermios2::new_default()),
            winsize: Mut::new(LinuxWinsize::from_rows_cols(
                window_size.rows,
                window_size.cols,
            )),
            inner: term,
            linux_console: Arc::new(Mut::new(LinuxConsoleState::default())),
            open_state: OpenState::default(),
        }
    }

    pub fn write_screen_text(&self, text: &str) {
        let filtered = filter_terminal_output(text);
        if !filtered.is_empty() {
            without_interrupts(|| {
                self.inner.lock().push_str(&filtered);
            });
        }
    }

    pub fn clear_screen(&self) {
        without_interrupts(|| {
            self.inner.lock().clear();
        });
    }
}

fn filter_terminal_output(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut index = 0usize;

    while index < bytes.len() {
        if bytes[index] == 0x1b && bytes.get(index + 1) == Some(&b']') {
            index += 2;
            while let Some(&byte) = bytes.get(index) {
                index += 1;
                if byte == 0x07 {
                    break;
                }
                if byte == 0x1b && bytes.get(index) == Some(&b'\\') {
                    index += 1;
                    break;
                }
            }
            continue;
        }

        output.push(bytes[index] as char);
        index += 1;
    }

    output
}
impl Object for TerminalObject {
    fn get_flags(self: Arc<Self>) -> ObjectResult<FileFlags> {
        Ok(self.open_state.get_flags())
    }

    fn set_flags(self: Arc<Self>, flags: FileFlags) -> ObjectResult<()> {
        self.open_state.set_flags(flags);
        Ok(())
    }

    impl_cast_function!("configuratable", Configuratable);
    impl_cast_function!("writable", Writable);
}

impl Writable for TerminalObject {
    fn write(&self, buffer: &[u8]) -> ObjectResult<usize> {
        let string = from_utf8(buffer).unwrap_or("Unsupported charcter");
        let filtered = filter_terminal_output(string);
        if !filtered.is_empty() {
            s_print!("{filtered}");
            without_interrupts(|| {
                self.inner.lock().push_str(&filtered);
            });
        }
        Ok(buffer.len())
    }
}
