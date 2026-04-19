use alloc::{collections::btree_map::BTreeMap, sync::Arc};
use core::sync::atomic::Ordering;
use crossbeam_queue::ArrayQueue;
use spin::Mutex;

use crate::task::task::{Task, TaskID};

pub struct TaskSpawner {
    tasks: Arc<Mutex<BTreeMap<TaskID, Task>>>,
    task_queue: Arc<ArrayQueue<TaskID>>,
}

impl TaskSpawner {
    pub fn new(
        tasks: Arc<Mutex<BTreeMap<TaskID, Task>>>,
        task_queue: Arc<ArrayQueue<TaskID>>,
    ) -> Self {
        Self { tasks, task_queue }
    }

    pub fn spawn(&mut self, task: Task) {
        let task_id = task.id;
        if self.tasks.lock().insert(task.id, task).is_some() {
            panic!("task with same ID already in tasks");
        }
        self.task_queue.push(task_id).expect("queue full");
    }

    pub fn wake_existing(&mut self, task_id: TaskID) {
        let queued = {
            let tasks = self.tasks.lock();
            let Some(task) = tasks.get(&task_id) else {
                return;
            };
            task.wake_handle()
        };

        if !queued.swap(true, Ordering::AcqRel) {
            self.task_queue.push(task_id).expect("queue full");
        }
    }
}
