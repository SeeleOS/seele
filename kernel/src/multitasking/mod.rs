pub mod context;
pub mod kernel_task;
pub mod manager;
pub mod memory;
pub mod process;
pub mod scheduling;
pub mod switch;
pub mod yielding;

use lazy_static::lazy_static;
use spin::Mutex;
use x86_64::instructions::interrupts::without_interrupts;

use crate::multitasking::manager::Manager;

lazy_static! {
    pub static ref MANAGER: Mutex<Manager> = Mutex::new(Manager::default());
}

pub fn init() {
    without_interrupts(|| MANAGER.lock().init())
}
