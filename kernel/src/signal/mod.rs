use num_enum::TryFromPrimitive;

#[derive(Clone, Copy, TryFromPrimitive, Debug)]
#[repr(u64)]
pub enum Signal {
    Terminate = 0,
    Kill,
    Interrupt,
}

pub const SIGNAL_AMOUNT: usize = 3;

pub mod action;
pub mod misc;

pub type SignalHandlerFn = extern "C" fn(i32);
