use core::fmt::{Debug, Write};

use os_terminal::Terminal;

use crate::{
    misc::framebuffer::FRAME_BUFFER,
    terminal::{
        KernelTerminal,
        term_trait::{AbstractTerminal, TerminalSize},
    },
};

impl AbstractTerminal for KernelTerminal {
    fn push_str(&mut self, str: &str) {
        self.0.write_str(str).unwrap();
        self.0.flush();
        FRAME_BUFFER.get().unwrap().lock().flush();
    }

    fn size(&self) -> TerminalSize {
        TerminalSize::new(self.0.rows(), self.0.columns())
    }
}

unsafe impl Send for KernelTerminal {}
unsafe impl Sync for KernelTerminal {}

impl Debug for KernelTerminal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        Ok(())
    }
}
