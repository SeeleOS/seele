use alloc::boxed::Box;
use core::fmt::Debug;

pub struct TerminalSize {
    pub rows: u64,
    pub cols: u64,
}

impl TerminalSize {
    pub fn new(rows: usize, cols: usize) -> Self {
        Self {
            rows: rows as u64,
            cols: cols as u64,
        }
    }
}

pub struct TerminalCursorPosition {
    pub row: u64,
    pub col: u64,
}

impl TerminalCursorPosition {
    pub fn from_zero_based(row: usize, col: usize) -> Self {
        Self {
            row: row as u64 + 1,
            col: col as u64 + 1,
        }
    }
}

pub type PtyWriter = Box<dyn FnMut(&str) + Send>;

pub trait AbstractTerminal: Debug + Sync + Send {
    fn push_str(&mut self, str: &str);
    fn size(&self) -> TerminalSize;
    fn cursor_position(&self) -> TerminalCursorPosition;
    fn set_pty_writer(&mut self, writer: PtyWriter);
    fn clear(&mut self);
}
