use core::{
    pin::Pin,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    task::{Context, Poll, Waker},
};

use alloc::{boxed::Box, sync::Arc, task::Wake};
use crossbeam_queue::ArrayQueue;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TaskID(u64);

impl TaskID {
    fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);

        TaskID(NEXT_ID.fetch_add(1, core::sync::atomic::Ordering::Relaxed))
    }
}

// the reason that the future doesnt return anything
// is because this future is only supposed to be polled
// for the effect of polling, not the return value
pub struct Task {
    pub id: TaskID,
    future: Pin<Box<dyn Future<Output = ()> + Send>>,
    queued: Arc<AtomicBool>,
}

impl Task {
    pub fn new(future: impl Future<Output = ()> + 'static + Send) -> Self {
        Self {
            id: TaskID::new(),
            future: Box::pin(future),
            queued: Arc::new(AtomicBool::new(true)),
        }
    }

    pub fn poll(&mut self, context: &mut Context) -> Poll<()> {
        self.future.as_mut().poll(context)
    }

    pub fn mark_dequeued(&self) {
        self.queued.store(false, Ordering::Release);
    }

    pub fn wake_handle(&self) -> Arc<AtomicBool> {
        self.queued.clone()
    }
}

pub struct TaskWaker {
    taskid: TaskID,
    task_queue: Arc<ArrayQueue<TaskID>>,
    queued: Arc<AtomicBool>,
}

impl TaskWaker {
    pub fn into_waker(
        taskid: TaskID,
        task_queue: Arc<ArrayQueue<TaskID>>,
        queued: Arc<AtomicBool>,
    ) -> Waker {
        Waker::from(Arc::new(Self {
            taskid,
            task_queue,
            queued,
        }))
    }

    fn t_wake(&self) {
        if !self.queued.swap(true, Ordering::AcqRel) {
            self.task_queue.push(self.taskid).expect("Task queue full");
        }
    }
}

impl Wake for TaskWaker {
    fn wake(self: Arc<Self>) {
        self.t_wake();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.t_wake();
    }
}
