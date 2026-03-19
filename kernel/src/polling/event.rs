#[derive(Debug, PartialEq, Eq)]
pub enum PollableEvent {
    Keypress,
    Other(u64),
}

impl From<u64> for PollableEvent {
    fn from(value: u64) -> Self {
        match value {
            0 => Self::Keypress,
            _ => Self::Other(value),
        }
    }
}
