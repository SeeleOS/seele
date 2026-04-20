use core::task::{Context, Poll, Waker};

use alloc::{collections::btree_map::BTreeMap, sync::Arc};
use crossbeam_queue::ArrayQueue;
use spin::Mutex;
use x86_64::instructions::interrupts::{self, enable_and_hlt};

use crate::task::{
    TASK_SPAWNER,
    spawner::TaskSpawner,
    task::{Task, TaskID, TaskWaker},
};

// When a task was awoken, the taskid will be pushed to the
// task queue to be executed.
pub struct Executor {
    tasks: Arc<Mutex<BTreeMap<TaskID, Task>>>,
    task_queue: Arc<ArrayQueue<TaskID>>,
    wakers: BTreeMap<TaskID, Waker>,
}

impl Default for Executor {
    fn default() -> Self {
        let tasks = Arc::new(Mutex::new(BTreeMap::new()));
        let task_queue = Arc::new(ArrayQueue::new(128));

        TASK_SPAWNER
            .get_or_init(|| Mutex::new(TaskSpawner::new(tasks.clone(), task_queue.clone())));

        Self {
            tasks,
            task_queue,
            wakers: BTreeMap::new(),
        }
    }
}

impl Executor {
    pub fn run_queued_tasks(&mut self) {
        let Self {
            tasks,
            task_queue,
            wakers,
        } = self;

        while let Some(taskid) = task_queue.pop() {
            let mut task = {
                let mut task_guard = tasks.lock();
                match task_guard.remove(&taskid) {
                    Some(task) => task,
                    None => continue,
                }
            };
            task.mark_dequeued();
            let waker = wakers
                .entry(taskid)
                // inserts a new waker if there is no waker assigned to the task
                .or_insert_with(|| {
                    TaskWaker::into_waker(taskid, task_queue.clone(), task.wake_handle())
                });
            let mut context = Context::from_waker(waker);

            match task.poll(&mut context) {
                Poll::Ready(()) => {
                    // remove the task and waker if completed
                    tasks.lock().remove(&taskid);
                    wakers.remove(&taskid);
                }
                Poll::Pending => {
                    tasks.lock().insert(taskid, task);
                }
            }
        }
    }

    pub fn run(&mut self) -> ! {
        loop {
            log::trace!("started running queued tasks");
            self.run_queued_tasks();
            log::trace!("finished running queued tasks");
            self.sleep_on_idle();
        }
    }

    fn sleep_on_idle(&self) {
        interrupts::disable();
        if self.task_queue.is_empty() {
            enable_and_hlt();
        } else {
            interrupts::enable();
        }
    }
}
