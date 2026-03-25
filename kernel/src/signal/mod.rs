pub use seele_sys::signal::{SIGNAL_AMOUNT, Signal, SignalHandlerFn};
pub mod misc;
pub mod action {
    pub use seele_sys::signal::{SignalAction, SignalHandlingType, Signals};
}
