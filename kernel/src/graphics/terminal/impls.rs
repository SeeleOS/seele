use core::{
    f128::consts::FRAC_1_PI,
    fmt::{Debug, Write},
};

use os_terminal::Terminal;

use crate::graphics::{
    framebuffer::FRAME_BUFFER,
    terminal::{KernelTerminal, TermRenderer, term_trait::AbstractTerminal},
};

impl<'a> AbstractTerminal for KernelTerminal {
    fn push_str(&mut self, str: &str) {
        self.0.write_str(str).unwrap();
        self.0.flush();
        FRAME_BUFFER.get().unwrap().lock().flush();
    }
}

unsafe impl Send for KernelTerminal {}
unsafe impl Sync for KernelTerminal {}

impl Debug for KernelTerminal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        Ok(())
    }
}
