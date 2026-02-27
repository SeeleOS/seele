use conquer_once::spin::OnceCell;
use spin::Mutex;

use crate::{
    keyboard::decoding_task::process_keypresses,
    multitasking::kernel_task::{executor::Executor, spawner::TaskSpawner, task::Task},
};

pub mod executor;
pub mod spawner;
pub mod task;
pub mod waker;

pub static TASK_SPAWNER: OnceCell<Mutex<TaskSpawner>> = OnceCell::uninit();

pub fn init() -> Executor {
    let mut executor = Executor::default();

    TASK_SPAWNER
        .get()
        .unwrap()
        .lock()
        .spawn(Task::new(process_keypresses()));

    executor
}
