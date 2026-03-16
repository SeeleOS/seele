pub mod color;
pub mod font;
pub mod impls;
pub mod macros;
pub mod renderer;
pub mod state;
pub mod term_trait;

pub use color::{COLOR_SCHEME, Color};
pub use font::{FONT_PATH, init_font};
pub use macros::term_print;
use os_terminal::Terminal;
pub use renderer::TermRenderer;
pub use state::TERMINAL;

pub struct KernelTerminal(Terminal<TermRenderer<'static>>);
