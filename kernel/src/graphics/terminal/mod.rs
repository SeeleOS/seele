pub mod color;
pub mod font;
pub mod renderer;
pub mod state;
pub mod macros;

pub use color::{COLOR_SCHEME, Color};
pub use font::{init_font, FONT_PATH};
pub use renderer::TermRenderer;
pub use state::TERMINAL;
pub use macros::term_print;

