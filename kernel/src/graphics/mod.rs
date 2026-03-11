use core::ops::Deref;

use alloc::{boxed::Box, vec};
use os_terminal::{
    Terminal,
    font::{BitmapFont, FontManager, TrueTypeFont},
};
use spin::Mutex;
use spleen_font::PSF2Font;
use vte::Parser;
use x86_64::VirtAddr;

use crate::{
    filesystem::{
        path::{Path, PathPart},
        vfs::VirtualFS,
        vfs_operations::read_all,
    },
    graphics::{
        framebuffer::{Canvas, FRAME_BUFFER},
        terminal::{COLOR_SCHEME, TERMINAL, TermRenderer},
    },
};

pub mod framebuffer;
pub mod object;
pub mod object_config;
pub mod terminal;

pub fn init(boot_info: &'static mut bootloader_api::info::FrameBuffer) {
    log::info!("graphics: init start");
    let canvas = FRAME_BUFFER.get_or_init(|| Mutex::new(Canvas::new(boot_info)));
    let mut terminal = TERMINAL
        .get_or_init(|| Mutex::new(Terminal::new(TermRenderer::new(canvas))))
        .lock();

    log::debug!("graphics: terminal ready");
    terminal.set_font_manager(Box::new(BitmapFont));
    terminal.set_crnl_mapping(true);
    terminal.set_custom_color_scheme(&COLOR_SCHEME);
    log::debug!("graphics: terminal configured");
}
