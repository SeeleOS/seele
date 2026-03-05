use alloc::vec::Vec;
use conquer_once::spin::OnceCell;
use spin::Mutex;
use spleen_font::PSF2Font;
use vte::Parser;

use crate::graphics::{
    framebuffer::{Canvas, FRAME_BUFFER},
    tty::rendering::{PADDING, TextCell},
};

pub mod ansi_color;
pub mod misc;
pub mod processing;
pub mod rendering;
pub mod wallpaper;

pub const DEFAULT_FOREGROUND: Color = (255, 255, 255);
pub const EMPTY_BACKGROUND: Color = (0, 0, 0);

pub type Color = (u8, u8, u8);

pub static TTY: OnceCell<Mutex<Tty>> = OnceCell::uninit();

pub struct Tty<'a> {
    font: PSF2Font<'a>,
    canvas: &'a Mutex<Canvas>,

    text_buf: Vec<TextCell>,
    cursor_y: u32,
    cursor_x: u32,

    pub max_rows: u16,
    pub max_cols: u16,

    parser: Parser,

    current_background: Color,
    current_foreground: Color,
    bold: bool,
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
            cursor_y: 0,
            text_buf,
            cursor_x: 0,

            max_rows: height as u16,
            max_cols: width as u16,

            parser: Parser::new(),

            current_foreground: DEFAULT_FOREGROUND,
            current_background: EMPTY_BACKGROUND,
            bold: false,
        }
    }
}
