pub mod color;
pub mod impls;
pub mod macros;
pub mod misc;
pub mod renderer;
pub mod state;
pub mod term_trait;

use alloc::{boxed::Box, sync::Arc};
pub use color::{COLOR_SCHEME, Color};
pub use macros::term_print;
use os_terminal::{Terminal, font::TrueTypeFont};
pub use renderer::TermRenderer;
use spin::mutex::Mutex;

use crate::{
    misc::framebuffer::FRAME_BUFFER,
    object::tty_device::{DEFAULT_TTY, TtyDevice},
    terminal::{object::TerminalObject, state::DEFAULT_TERMINAL},
};

pub struct KernelTerminal(pub Terminal<TermRenderer<'static>>);

pub mod object;
pub mod object_config;

pub static FONT: &[u8] = include_bytes!("../../../misc/maplemono.ttf");

pub fn init() {
    log::info!("graphics: init start");
    let mut terminal = Terminal::new(
        TermRenderer::new(FRAME_BUFFER.get().unwrap()),
        Box::new(TrueTypeFont::new(12.0, FONT)),
    );

    log::debug!("graphics: terminal ready");

    terminal.set_crnl_mapping(true);
    terminal.set_custom_color_scheme(&COLOR_SCHEME);
    terminal.set_auto_flush(false);

    let default_terminal = DEFAULT_TERMINAL.get_or_init(|| {
        Arc::new(Mutex::new(TerminalObject::new(Arc::new(Mutex::new(
            KernelTerminal(terminal),
        )))))
    });

    DEFAULT_TTY.get_or_init(|| Arc::new(TtyDevice::new(default_terminal.clone())));

    log::debug!("graphics: terminal configured");
}
