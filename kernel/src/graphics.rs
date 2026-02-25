use bootloader_api::BootInfo;
use spin::Mutex;
use spleen_font::{FONT_6X12, FONT_8X16, FONT_12X24, FONT_16X32, FONT_32X64, PSF2Font};

use crate::graphics::{
    framebuffer::{Canvas, FRAME_BUFFER},
    tty::{TTY, Tty},
};

pub mod framebuffer;
pub mod tty;

pub static FONT: &[u8] = include_bytes!("../../maplemono.psf");

pub fn init(boot_info: &'static mut bootloader_api::info::FrameBuffer) {
    FRAME_BUFFER.get_or_init(|| Mutex::new(Canvas::new(boot_info)));
    let tty = TTY.get_or_init(|| Mutex::new(Tty::new(PSF2Font::new(FONT).unwrap())));

    tty.lock().draw_wallpaper();
}
