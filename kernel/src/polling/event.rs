/// Events made by a specific object to a poller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PollableEvent {
    CanBeRead,
    CanBeWritten,
    Priority,
    Error,
    Closed,
    ReadClosed,
    PipeWriteSpace,
    Other(u64),
}

impl From<u64> for PollableEvent {
    fn from(value: u64) -> Self {
        match value {
            0 => Self::CanBeRead,
            1 => Self::CanBeWritten,
            2 => Self::Priority,
            3 => Self::Error,
            4 => Self::Closed,
            5 => Self::ReadClosed,
            _ => Self::Other(value),
        }
    }
}
