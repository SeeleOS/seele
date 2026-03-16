use core::fmt::{Debug, Write};

use os_terminal::Terminal;

use crate::graphics::terminal::{KernelTerminal, TermRenderer, term_trait::AbstractTerminal};

impl<'a> AbstractTerminal for KernelTerminal {
    fn push_str(&mut self, str: &str) {
        self.0.write_str(str).unwrap();
    }
}

unsafe impl Send for KernelTerminal {}
unsafe impl Sync for KernelTerminal {}

impl Debug for KernelTerminal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        Ok(())
    }
}
