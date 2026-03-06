use core::ops::Deref;

use alloc::boxed::Box;
use os_terminal::{
    Terminal,
    font::{BitmapFont, FontManager, TrueTypeFont},
};
use spin::Mutex;
use spleen_font::PSF2Font;

use crate::{
    graphics::{
        framebuffer::{Canvas, FRAME_BUFFER},
        terminal::{TERMINAL, TermRenderer},
    },
    println,
};

pub mod framebuffer;
pub mod object;
pub mod object_config;
pub mod terminal;

pub static FONT: &[u8] = include_bytes!("../../../MapleMono-Regular.ttf");

pub fn init(boot_info: &'static mut bootloader_api::info::FrameBuffer) {
    let canvas = FRAME_BUFFER.get_or_init(|| Mutex::new(Canvas::new(boot_info)));
    let mut terminal = TERMINAL
        .get_or_init(|| Mutex::new(Terminal::new(TermRenderer::new(canvas))))
        .lock();

    let font_manager = TrueTypeFont::new(13.0, FONT);

    terminal.set_font_manager(Box::new(font_manager));
    terminal.set_crnl_mapping(true);
}
