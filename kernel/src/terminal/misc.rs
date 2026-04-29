use crate::terminal::state::DEFAULT_TERMINAL;

pub fn clear() {
    DEFAULT_TERMINAL.get().unwrap().lock().clear_screen();
}
