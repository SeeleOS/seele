use bootloader_api::info::PixelFormat;
use conquer_once::spin::OnceCell;
use spin::Mutex;

use crate::graphics::terminal::Color;

pub fn init(boot_info: &'static mut bootloader_api::info::FrameBuffer) {
    log::info!("graphics: init start");
    FRAME_BUFFER.init_once(|| Mutex::new(Canvas::new(boot_info)));
    log::debug!("graphics: terminal configured");
}

pub static FRAME_BUFFER: OnceCell<Mutex<Canvas>> = OnceCell::uninit();

pub struct Canvas {
    fb: &'static mut [u8],
    buffer: alloc::vec::Vec<u8>,
    info: bootloader_api::info::FrameBufferInfo,

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
        self.fb.copy_from_slice(&self.buffer);
    }

    pub fn clear(&mut self) {
        self.fb.fill(0);
    }
}
