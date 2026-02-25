use alloc::vec::Vec;
use conquer_once::spin::OnceCell;
use spin::Mutex;
use spleen_font::PSF2Font;

use crate::graphics::{
        framebuffer::{Canvas, FRAME_BUFFER},
        tty::text::{PADDING, TextCell},
    };

pub mod text;
pub mod wallpaper;

pub static TTY: OnceCell<Mutex<Tty>> = OnceCell::uninit();

pub struct Tty<'a> {
    font: PSF2Font<'a>,
    canvas: &'a Mutex<Canvas>,

    text_buf: Vec<TextCell>,
    row: u32,
    col: u32,
}

impl<'a> Tty<'a> {
    pub fn new(font: PSF2Font<'a>) -> Self {
        let width =
            ((FRAME_BUFFER.get().unwrap().lock().width - PADDING * 2) / font.width) as usize;
        let height =
            ((FRAME_BUFFER.get().unwrap().lock().height - PADDING * 2) / font.height) as usize;
        let size = width * height;

        let mut text_buf = Vec::with_capacity(size);
        text_buf.resize(size, TextCell::default());
        Self {
            font,
            canvas: FRAME_BUFFER.get().unwrap(),
            row: 0,
            text_buf,
            col: 0,
        }
    }
}
