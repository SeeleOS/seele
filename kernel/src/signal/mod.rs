pub enum Signal {
    Terminate,
    Kill,
    Interrupt,
}

pub mod action;

pub type SignalHandlerFn = extern "C" fn(i32);
