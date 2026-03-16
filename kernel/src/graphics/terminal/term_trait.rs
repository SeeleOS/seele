use core::fmt::Debug;

pub trait AbstractTerminal: Debug + Sync + Send {
    fn push_str(&mut self, str: &str);
}
