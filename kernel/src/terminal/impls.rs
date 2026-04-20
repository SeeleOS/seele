use core::fmt::{Debug, Formatter, Result as FmtResult, Write};

use crate::{
    misc::framebuffer::{FRAME_BUFFER, framebuffer_user_controlled},
    terminal::{
        KernelTerminal,
        term_trait::{AbstractTerminal, TerminalSize},
    },
};

impl AbstractTerminal for KernelTerminal {
    fn push_str(&mut self, str: &str) {
        self.0.write_str(str).unwrap();
        self.0.flush();
        if !framebuffer_user_controlled() {
            FRAME_BUFFER.get().unwrap().lock().flush();
        }
    }

    fn size(&self) -> TerminalSize {
        TerminalSize::new(self.0.rows(), self.0.columns())
    }

    fn clear(&mut self) {
        self.0.clear();
    }
}

unsafe impl Send for KernelTerminal {}
unsafe impl Sync for KernelTerminal {}

impl Debug for KernelTerminal {
    fn fmt(&self, _f: &mut Formatter<'_>) -> FmtResult {
        Ok(())
    }
}
