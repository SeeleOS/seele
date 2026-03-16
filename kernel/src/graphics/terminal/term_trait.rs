use core::fmt::Debug;

pub trait AbstractTerminal: Debug + Sync + Send {
    fn push_char(&mut self, char: u8);
}
