use alloc::{
    alloc::{Layout, alloc_zeroed, dealloc},
    format,
};
use conquer_once::spin::OnceCell;
use core::{ffi::c_void, ptr::NonNull};
use flanterm::sys;
use spin::Mutex;

use crate::{
    misc::framebuffer::{Canvas, FramebufferPixelFormat, framebuffer_user_controlled},
    terminal::term_trait::PtyWriter,
};

static PTY_WRITER: OnceCell<Mutex<Option<PtyWriter>>> = OnceCell::uninit();

pub struct KernelTerminal(pub FlantermTerminal);

pub struct FlantermTerminal {
    context: NonNull<sys::flanterm_fb_context>,
}

impl KernelTerminal {
    pub fn new(canvas: &Mutex<Canvas>) -> Self {
        Self(FlantermTerminal::new(canvas))
    }
}

impl FlantermTerminal {
    pub fn new(canvas: &Mutex<Canvas>) -> Self {
        PTY_WRITER.get_or_init(|| Mutex::new(None));

        let (framebuffer, width, height, pitch, red_mask_shift, green_mask_shift, blue_mask_shift) = {
            let canvas = canvas.lock();
            let info = canvas.info;
            assert_eq!(
                info.bytes_per_pixel, 4,
                "flanterm terminal requires a 32-bit framebuffer"
            );
            let (red_mask_shift, green_mask_shift, blue_mask_shift) = match info.pixel_format {
                FramebufferPixelFormat::Rgb => (0, 8, 16),
                FramebufferPixelFormat::Bgr => (16, 8, 0),
            };
            (
                canvas.fb.as_ptr() as *mut u32,
                info.width,
                info.height,
                info.stride * info.bytes_per_pixel,
                red_mask_shift,
                green_mask_shift,
                blue_mask_shift,
            )
        };

        let context = unsafe {
            sys::flanterm_fb_init(
                Some(flanterm_alloc),
                Some(flanterm_free),
                framebuffer,
                width,
                height,
                pitch,
                8,
                red_mask_shift,
                8,
                green_mask_shift,
                8,
                blue_mask_shift,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                0,
                0,
                1,
                1,
                1,
                0,
            )
        };
        let context =
            NonNull::new(context.cast::<sys::flanterm_fb_context>()).expect("flanterm init failed");

        unsafe {
            context.as_mut().term.autoflush = false;
            context.as_mut().term.callback = Some(flanterm_callback);
        }

        Self { context }
    }

    pub fn write_str(&mut self, text: &str) {
        unsafe {
            sys::flanterm_write(self.term_ptr(), text.as_ptr().cast(), text.len());
            if !framebuffer_user_controlled() {
                self.flush();
            }
        }
    }

    pub fn flush(&mut self) {
        unsafe {
            if let Some(flush) = self.context.as_ref().term.double_buffer_flush {
                flush(self.term_ptr());
            }
        }
    }

    pub fn rows(&self) -> usize {
        unsafe { self.context.as_ref().term.rows }
    }

    pub fn columns(&self) -> usize {
        unsafe { self.context.as_ref().term.cols }
    }

    pub fn cursor_position(&self) -> (usize, usize) {
        unsafe {
            let context = self.context.as_ref();
            (context.cursor_y, context.cursor_x)
        }
    }

    pub fn set_pty_writer(&mut self, writer: PtyWriter) {
        *PTY_WRITER.get().unwrap().lock() = Some(writer);
    }

    pub fn clear(&mut self) {
        unsafe {
            if let Some(clear) = self.context.as_ref().term.clear {
                clear(self.term_ptr(), true);
            }
            if !framebuffer_user_controlled() {
                self.flush();
            }
        }
    }

    fn term_ptr(&mut self) -> *mut sys::flanterm_context {
        unsafe { &mut self.context.as_mut().term }
    }
}

impl core::fmt::Debug for FlantermTerminal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("FlantermTerminal")
    }
}

unsafe impl Send for FlantermTerminal {}
unsafe impl Sync for FlantermTerminal {}

unsafe extern "C" fn flanterm_alloc(size: usize) -> *mut c_void {
    let layout = Layout::from_size_align(size.max(1), 16).unwrap();
    unsafe { alloc_zeroed(layout).cast() }
}

unsafe extern "C" fn flanterm_free(ptr: *mut c_void, size: usize) {
    if ptr.is_null() {
        return;
    }

    let layout = Layout::from_size_align(size.max(1), 16).unwrap();
    unsafe { dealloc(ptr.cast(), layout) };
}

unsafe extern "C" fn flanterm_callback(
    _context: *mut sys::flanterm_context,
    callback: u64,
    arg1: u64,
    arg2: u64,
    _arg3: u64,
) {
    match callback as u32 {
        sys::FLANTERM_CB_PRIVATE_ID => write_pty_response("\x1b[?6c"),
        sys::FLANTERM_CB_STATUS_REPORT => write_pty_response("\x1b[0n"),
        sys::FLANTERM_CB_POS_REPORT => {
            let response = format!("\x1b[{};{}R", arg2, arg1);
            write_pty_response(&response);
        }
        _ => {}
    }
}

fn write_pty_response(response: &str) {
    if let Some(writer) = PTY_WRITER.get() {
        if let Some(writer) = writer.lock().as_mut() {
            writer(response);
        }
    }
}
