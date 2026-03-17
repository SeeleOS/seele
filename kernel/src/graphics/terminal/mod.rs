pub mod color;
pub mod impls;
pub mod macros;
pub mod misc;
pub mod renderer;
pub mod state;
pub mod term_trait;

pub use color::{COLOR_SCHEME, Color};
pub use macros::term_print;
use os_terminal::Terminal;
pub use renderer::TermRenderer;

pub struct KernelTerminal(pub Terminal<TermRenderer<'static>>);
