#[cfg(not(feature = "profiling"))]
mod disabled;
#[cfg(feature = "profiling")]
mod enabled;

#[cfg(not(feature = "profiling"))]
pub use disabled::*;
#[cfg(feature = "profiling")]
pub use enabled::*;
