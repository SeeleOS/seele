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
        framebuffer::{Canvas, FRAME_BUFFER},
        object::TerminalObject,
        terminal::{COLOR_SCHEME, KernelTerminal, TermRenderer, state::DEFAULT_TERMINAL},
    },
};

pub mod framebuffer;
pub mod object;
pub mod object_config;
pub mod terminal;

pub fn init(boot_info: &'static mut bootloader_api::info::FrameBuffer) {
    log::info!("graphics: init start");
    let canvas = FRAME_BUFFER.get_or_init(|| Mutex::new(Canvas::new(boot_info)));
    let mut terminal = Terminal::new(TermRenderer::new(canvas));

    log::debug!("graphics: terminal ready");

    terminal.set_font_manager(Box::new(BitmapFont));
    terminal.set_crnl_mapping(true);
    terminal.set_custom_color_scheme(&COLOR_SCHEME);
    terminal.set_auto_flush(false);

    DEFAULT_TERMINAL.get_or_init(|| {
        Arc::new(Mutex::new(TerminalObject {
            inner: Arc::new(Mutex::new(KernelTerminal(terminal))),
        }))
    });

    log::debug!("graphics: terminal configured");
}
