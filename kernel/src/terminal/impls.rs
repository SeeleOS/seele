use core::fmt::{Debug, Formatter, Result as FmtResult, Write};

use crate::{
    misc::framebuffer::{FRAME_BUFFER, framebuffer_user_controlled},
    terminal::{
        KernelTerminal,
        term_trait::{AbstractTerminal, PtyWriter, TerminalCursorPosition, TerminalSize},
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

    fn cursor_position(&self) -> TerminalCursorPosition {
        let position = self.0.cursor_position();
        TerminalCursorPosition::from_zero_based(position.row, position.column)
    }

    fn set_pty_writer(&mut self, writer: PtyWriter) {
        self.0.set_pty_writer(writer);
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
