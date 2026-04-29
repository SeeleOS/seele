use core::fmt::{Debug, Formatter, Result as FmtResult, Write};

use crate::{
    terminal::{
        KernelTerminal,
        term_trait::{AbstractTerminal, PtyWriter, TerminalCursorPosition, TerminalSize},
    },
};

impl AbstractTerminal for KernelTerminal {
    fn push_str(&mut self, str: &str) {
        self.terminal.write_str(str).unwrap();
        self.pending_bytes = self.pending_bytes.saturating_add(str.len());

        let should_flush =
            self.pending_bytes >= 512 || (self.pending_bytes >= 128 && str.contains('\n'));
        if should_flush {
            self.terminal.flush();
            self.pending_bytes = 0;
        }
    }

    fn size(&self) -> TerminalSize {
        TerminalSize::new(self.terminal.rows(), self.terminal.columns())
    }

    fn cursor_position(&self) -> TerminalCursorPosition {
        let position = self.terminal.cursor_position();
        TerminalCursorPosition::from_zero_based(position.row, position.column)
    }

    fn set_pty_writer(&mut self, writer: PtyWriter) {
        self.terminal.set_pty_writer(writer);
    }

    fn clear(&mut self) {
        self.terminal.clear();
        self.pending_bytes = 0;
    }
}

unsafe impl Send for KernelTerminal {}
unsafe impl Sync for KernelTerminal {}

impl Debug for KernelTerminal {
    fn fmt(&self, _f: &mut Formatter<'_>) -> FmtResult {
        Ok(())
    }
}
