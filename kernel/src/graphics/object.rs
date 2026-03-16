use core::{fmt::Write, str::from_utf8};

use alloc::sync::Arc;
use spin::{Mutex, mutex::Mutex};

use crate::{
    graphics::{
        framebuffer::FRAME_BUFFER,
        object_config::{TerminalInfo, WindowSizeInfo},
        terminal::term_trait::AbstractTerminal,
    },
    impl_cast_function,
    object::{
        Object,
        misc::ObjectResult,
        traits::{Configuratable, Writable},
    },
    print,
};

#[derive(Debug)]
pub struct TerminalObject {
    pub inner: Arc<Mutex<dyn AbstractTerminal>>,
    pub window_size: WindowSizeInfo,
    pub terminal_info: TerminalInfo,
}

impl TerminalObject {
    pub fn new(term: impl AbstractTerminal) -> Self {
        Self {
            window_size: WindowSizeInfo {
                rows: 
            },
            terminal_info: TerminalInfo::new_default(),
            inner: Arc::new(Mutex::new(term)),
        }
    }
}

impl Object for TerminalObject {
    impl_cast_function!(configuratable, Configuratable);
    impl_cast_function!(writable, Writable);
}

impl Writable for TerminalObject {
    fn write(&self, buffer: &[u8]) -> ObjectResult<usize> {
        self.inner
            .lock()
            .push_str(from_utf8(buffer).unwrap_or("Unsupported charcter"));
        Ok(buffer.len())
    }
}
