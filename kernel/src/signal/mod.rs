use num_enum::TryFromPrimitive;

#[derive(TryFromPrimitive, Debug)]
#[repr(u64)]
pub enum Signal {
    Terminate = 0,
    Kill,
    Interrupt,
}

pub mod action;

pub type SignalHandlerFn = extern "C" fn(i32);
