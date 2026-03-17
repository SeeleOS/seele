use core::fmt::{Debug, Write};

use os_terminal::Terminal;

use crate::{
    graphics::{
        object_config::WindowSizeInfo,
        terminal::{KernelTerminal, TermRenderer, term_trait::AbstractTerminal},
    },
    misc::framebuffer::FRAME_BUFFER,
};

impl<'a> AbstractTerminal for KernelTerminal {
    fn push_str(&mut self, str: &str) {
        self.0.write_str(str).unwrap();
        self.0.flush();
        FRAME_BUFFER.get().unwrap().lock().flush();
    }

    fn size(&self) -> crate::graphics::object_config::WindowSizeInfo {
        WindowSizeInfo {
            rows: self.0.rows() as u16,
            cols: self.0.columns() as u16,
            ..Default::default()
        }
    }
}

unsafe impl Send for KernelTerminal {}
unsafe impl Sync for KernelTerminal {}

impl Debug for KernelTerminal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        Ok(())
    }
}
