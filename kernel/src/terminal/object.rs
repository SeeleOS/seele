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
    terminal::{object_config::TerminalInfo, term_trait::AbstractTerminal},
};

#[derive(Debug)]
pub struct TerminalObject {
    pub inner: Arc<Mutex<dyn AbstractTerminal>>,
    pub info: Mutex<TerminalInfo>,
}

impl TerminalObject {
    pub fn new(term: Arc<Mutex<dyn AbstractTerminal>>) -> Self {
        let window_size = term.lock().size();
        Self {
            info: Mutex::new(TerminalInfo::new(window_size)),
            inner: term,
        }
    }
}

impl Object for TerminalObject {
    impl_cast_function!(configuratable, Configuratable);
    impl_cast_function!(writable, Writable);
}

impl Writable for TerminalObject {
    fn write(&self, buffer: &[u8]) -> ObjectResult<usize> {
        let string = from_utf8(buffer).unwrap_or("Unsupported charcter");
        self.inner.lock().push_str(string);
        Ok(buffer.len())
    }
}
