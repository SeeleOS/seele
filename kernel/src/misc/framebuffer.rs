use bootloader_api::info::PixelFormat;
use conquer_once::spin::OnceCell;
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;
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

pub fn init(boot_info: &'static mut bootloader_api::info::FrameBuffer) {
    log::info!("graphics: init start");
    FRAME_BUFFER.init_once(|| Mutex::new(Canvas::new(boot_info)));
    log::debug!("graphics: terminal configured");
}

pub static FRAME_BUFFER: OnceCell<Mutex<Canvas>> = OnceCell::uninit();
pub static FRAMEBUFFER_USER_CONTROLLED: AtomicBool = AtomicBool::new(false);

pub struct Canvas {
    pub fb: &'static mut [u8],
    buffer: alloc::vec::Vec<u8>,
    pub info: bootloader_api::info::FrameBufferInfo,

    pub width: u32,
    pub height: u32,
}

impl Canvas {
    pub fn new(frame_buffer: &'static mut bootloader_api::info::FrameBuffer) -> Self {
        let info = frame_buffer.info();
        let fb = frame_buffer.buffer_mut();

        // Clear screen
        fb.fill(0);

        Self {
            info,
            fb,
            buffer: alloc::vec![0u8; info.byte_len],

            width: info.width as u32,
            height: info.height as u32,
        }
    }

    #[inline(always)]
    pub fn write_pixel(&mut self, x: usize, y: usize, color: Color) {
        if framebuffer_user_controlled() {
            return;
        }

        // Offset of the pixel from the start
        // of the framebuffer (in pixels)
        let pixels_offset = (y * self.info.stride) + x;
        // Offset in bytes
        let bytes_offset = pixels_offset * self.info.bytes_per_pixel;

        let (r, g, b) = color;

        match self.info.pixel_format {
            PixelFormat::Rgb => {
                self.buffer[bytes_offset] = r;
                self.buffer[bytes_offset + 1] = g;
                self.buffer[bytes_offset + 2] = b;
            }
            PixelFormat::Bgr => {
                self.buffer[bytes_offset] = b;
                self.buffer[bytes_offset + 1] = g;
                self.buffer[bytes_offset + 2] = r;
            }
            _ => {
                panic!("Unsupported pixel format. Possible old hardware");
            }
        }
    }

    // Flushes the contents of the buffer into the real fb
    pub fn flush(&mut self) {
        if framebuffer_user_controlled() {
            return;
        }

        self.fb.copy_from_slice(&self.buffer);
    }

    pub fn clear(&mut self) {
        self.fb.fill(0);
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
            pixel_format: match self.info.pixel_format {
                PixelFormat::Rgb => FramebufferPixelFormat::Rgb,
                PixelFormat::Bgr => FramebufferPixelFormat::Bgr,
                _ => panic!("Unsupported pixel format"),
            },
        }
    }
}

pub fn framebuffer_set_user_controlled(controlled: bool) {
    FRAMEBUFFER_USER_CONTROLLED.store(controlled, Ordering::SeqCst);
}

pub fn framebuffer_user_controlled() -> bool {
    FRAMEBUFFER_USER_CONTROLLED.load(Ordering::SeqCst)
}
