pub mod color;
pub mod flanterm;
pub mod impls;
pub mod line_discipline;
pub mod linux_kd;
pub mod linux_vt;
pub mod macros;
pub mod misc;
pub mod pty;
pub mod state;
pub mod term_trait;
pub mod termios;

use alloc::{boxed::Box, sync::Arc};
pub use macros::term_print;
pub use color::Color;
pub use flanterm::KernelTerminal;
use spin::mutex::Mutex;

use crate::{
    misc::framebuffer::FRAME_BUFFER,
    object::tty_device::{
        CONSOLE_TTY, DEFAULT_TTY, MAX_VIRTUAL_TTYS, TtyDevice, get_active_tty, init_virtual_ttys,
        register_virtual_tty,
    },
    terminal::object::TerminalObject,
    terminal::state::DEFAULT_TERMINAL,
};

pub mod object;
pub mod object_config;

pub fn init() {
    log::info!("graphics: init start");
    let terminal = KernelTerminal::new(FRAME_BUFFER.get().unwrap());

    log::debug!("graphics: terminal ready");

    let default_terminal = DEFAULT_TERMINAL.get_or_init(|| {
        Arc::new(Mutex::new(TerminalObject::new(Arc::new(Mutex::new(
            KernelTerminal(terminal),
        )))))
    });

    init_virtual_ttys();

    CONSOLE_TTY.get_or_init(|| Arc::new(TtyDevice::new(default_terminal.clone(), false)));
    let default_tty = DEFAULT_TTY
        .get_or_init(|| Arc::new(TtyDevice::new(default_terminal.clone(), true)))
        .clone();
    register_virtual_tty(1, default_tty);
    for vt in 2..=MAX_VIRTUAL_TTYS {
        register_virtual_tty(vt, Arc::new(TtyDevice::new(default_terminal.clone(), true)));
    }

    default_terminal
        .lock()
        .inner
        .lock()
        .set_pty_writer(Box::new(|data| {
            get_active_tty().push_terminal_response_bytes(data.as_bytes());
        }));

    log::debug!("graphics: terminal configured");
}
