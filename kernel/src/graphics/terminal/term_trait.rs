use core::fmt::Debug;

pub trait AbstractTerminal: Debug {
    fn push_char(&mut self, char: u8);
}
