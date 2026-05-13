use crate::memory::utils::Mut;
use alloc::format;
use conquer_once::spin::OnceCell;
use flanterm::sys;

use crate::terminal::term_trait::PtyWriter;

static PTY_WRITER: OnceCell<Mut<Option<PtyWriter>>> = OnceCell::uninit();

pub(super) fn init_pty_writer() {
    PTY_WRITER.get_or_init(|| Mut::new(None));
}

pub(super) fn set_pty_writer(writer: PtyWriter) {
    *PTY_WRITER.get().unwrap().lock() = Some(writer);
}

pub(super) unsafe extern "C" fn flanterm_callback(
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
    if let Some(writer) = PTY_WRITER.get()
        && let Some(writer) = writer.lock().as_mut()
    {
        writer(response);
    }
}
