#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    Keypress,
    Other(u64),
}

impl From<u64> for Event {
    fn from(value: u64) -> Self {
        match value {
            0 => Self::Keypress,
            _ => Self::Other(value),
        }
    }
}
