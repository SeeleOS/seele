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

pub trait AbstractTerminal: Debug + Sync + Send {
    fn push_str(&mut self, str: &str);
    fn size(&self) -> TerminalSize;
}
