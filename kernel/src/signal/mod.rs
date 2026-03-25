pub enum Signal {
    Terminate,
    Kill,
    Interrupt,
}

pub type SignalHandler = *const fn(i32);
