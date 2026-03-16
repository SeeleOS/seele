use alloc::boxed::Box;

use os_terminal::font::TrueTypeFont;

use crate::{
    filesystem::{path::Path, vfs_operations::read_all},
    graphics::terminal::state::TERMINAL,
};

pub const FONT_PATH: &str = "/misc/fonts/maplem~1.ttf";

pub fn init_font() {
    let font_path = Path::new(FONT_PATH);
    let font: &'static mut [u8] =
        Box::leak(Box::new(read_all(font_path).unwrap()).into_boxed_slice());

    let font_manager = TrueTypeFont::new(13.0, font);

    TERMINAL
        .get()
        .unwrap()
        .lock()
        .set_font_manager(Box::new(font_manager));
}

