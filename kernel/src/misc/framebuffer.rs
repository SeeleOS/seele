use crate::memory::utils::Mut;
use alloc::{vec, vec::Vec};
use bootloader_api::info::{FrameBuffer, PixelFormat};
use conquer_once::spin::OnceCell;
use core::sync::atomic::{AtomicBool, Ordering};
use x86_64::{VirtAddr, structures::paging::Translate};

use crate::{memory::paging::MAPPER, terminal::Color};

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FramebufferPixelFormat {
    #[default]
    Rgb = 0,
    Bgr = 1,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct FramebufferInfo {
    pub phys_addr: usize,
    pub width: usize,
    pub height: usize,
    pub stride: usize,
    pub bytes_per_pixel: usize,
    pub byte_len: usize,
    pub pixel_format: FramebufferPixelFormat,
}

pub fn init(framebuffer: &'static mut FrameBuffer) {
    log::info!("graphics: init start");
    FRAME_BUFFER.init_once(|| Mut::new(Canvas::new(framebuffer)));
    log::debug!("graphics: terminal configured");
}

pub static FRAME_BUFFER: OnceCell<Mut<Canvas>> = OnceCell::uninit();
pub static FRAMEBUFFER_USER_CONTROLLED: AtomicBool = AtomicBool::new(false);

pub struct Canvas {
    pub fb: &'static mut [u8],
    buffer: Vec<u8>,
    pub info: FramebufferInfo,

    pub width: u32,
    pub height: u32,
}

impl Canvas {
    pub fn new(frame_buffer: &'static mut FrameBuffer) -> Self {
        let bootloader_info = frame_buffer.info();
        let info = FramebufferInfo {
            phys_addr: 0,
            width: bootloader_info.width,
            height: bootloader_info.height,
            stride: bootloader_info.stride,
            bytes_per_pixel: bootloader_info.bytes_per_pixel,
            byte_len: bootloader_info.byte_len,
            pixel_format: match bootloader_info.pixel_format {
                PixelFormat::Rgb => FramebufferPixelFormat::Rgb,
                PixelFormat::Bgr => FramebufferPixelFormat::Bgr,
                format => panic!("unsupported bootloader framebuffer format: {format:?}"),
            },
        };
        let fb = frame_buffer.buffer_mut();

        fb.fill(0);

        Self {
            info,
            fb,
            buffer: vec![0u8; info.byte_len],
            width: info.width as u32,
            height: info.height as u32,
        }
    }

    #[inline(always)]
    pub fn write_pixel(&mut self, x: usize, y: usize, color: Color) {
        if framebuffer_user_controlled() {
            return;
        }

        let pixels_offset = (y * self.info.stride) + x;
        let bytes_offset = pixels_offset * self.info.bytes_per_pixel;

        let (r, g, b) = color;

        match self.info.pixel_format {
            FramebufferPixelFormat::Rgb => {
                self.buffer[bytes_offset] = r;
                self.buffer[bytes_offset + 1] = g;
                self.buffer[bytes_offset + 2] = b;
            }
            FramebufferPixelFormat::Bgr => {
                self.buffer[bytes_offset] = b;
                self.buffer[bytes_offset + 1] = g;
                self.buffer[bytes_offset + 2] = r;
            }
        }
    }

    pub fn flush(&mut self) {
        if framebuffer_user_controlled() {
            return;
        }

        self.fb.copy_from_slice(&self.buffer);
    }

    pub fn clear(&mut self) {
        self.fb.fill(0);
    }

    pub fn user_controlled_buffer_mut(&mut self) -> &mut [u8] {
        &mut self.buffer
    }

    pub fn present_user_controlled(&mut self) {
        self.fb.copy_from_slice(&self.buffer);
    }

    pub fn fb_info(&self) -> FramebufferInfo {
        let phys_addr = MAPPER
            .get()
            .unwrap()
            .lock()
            .translate_addr(VirtAddr::new(self.fb.as_ptr() as u64))
            .expect("framebuffer must have a physical backing")
            .as_u64() as usize;

        FramebufferInfo {
            phys_addr,
            width: self.info.width,
            height: self.info.height,
            stride: self.info.stride,
            bytes_per_pixel: self.info.bytes_per_pixel,
            byte_len: self.info.byte_len,
            pixel_format: self.info.pixel_format,
        }
    }
}

pub fn framebuffer_set_user_controlled(controlled: bool) {
    FRAMEBUFFER_USER_CONTROLLED.store(controlled, Ordering::SeqCst);
}

pub fn framebuffer_user_controlled() -> bool {
    FRAMEBUFFER_USER_CONTROLLED.load(Ordering::SeqCst)
}
