use core::ops::Deref;

use alloc::{boxed::Box, sync::Arc, vec};
use os_terminal::{
    Terminal,
    font::{BitmapFont, FontManager, TrueTypeFont},
};
use spin::Mutex;

use crate::{
    filesystem::path::{Path, PathPart},
    graphics::{
        object::TerminalObject,
        terminal::{COLOR_SCHEME, KernelTerminal, TermRenderer, state::DEFAULT_TERMINAL},
    },
    misc::framebuffer::FRAME_BUFFER,
    object::tty_device::{DEFAULT_TTY, TtyDevice},
};

pub mod object;
pub mod object_config;
pub mod terminal;

pub static FONT: &[u8] = include_bytes!("../../../misc/maplemono.ttf");

pub fn init() {
    log::info!("graphics: init start");
    let mut terminal = Terminal::new(TermRenderer::new(FRAME_BUFFER.get().unwrap()));

    log::debug!("graphics: terminal ready");

    terminal.set_font_manager(Box::new(BitmapFont));
    terminal.set_crnl_mapping(true);
    terminal.set_custom_color_scheme(&COLOR_SCHEME);
    terminal.set_auto_flush(false);
    terminal.set_font_manager(Box::new(TrueTypeFont::new(12.0, FONT)));

    let default_terminal = DEFAULT_TERMINAL.get_or_init(|| {
        Arc::new(Mutex::new(TerminalObject::new(Arc::new(Mutex::new(
            KernelTerminal(terminal),
        )))))
    });

    DEFAULT_TTY.get_or_init(|| Arc::new(TtyDevice::new(default_terminal.clone())));

    log::debug!("graphics: terminal configured");
}
