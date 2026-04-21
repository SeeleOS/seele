use core::{
    pin::Pin,
    task::{Context, Poll},
};

use alloc::sync::Arc;
use x86_64::instructions::interrupts::without_interrupts;

use crate::{
    misc::snapshot::Snapshot,
    process::manager::MANAGER,
    smp::{current_thread, set_current_kernel_stack, set_current_process, set_current_thread},
    thread::{THREAD_MANAGER, ThreadRef, misc::State, snapshot::ThreadSnapshot},
};

pub struct ThreadFuture(pub ThreadRef);

/*
How my "process as a task" system would work:
Running a process = process.poll()
when polling, the process will switch to the user stack
when a timer interrupt or something occurs,
i will do some magic stuff and then it will come back
to the polling function, and then the poll() returns pending
Just like a regular task would when it finished working

Process initilized -> Executor polls the process ->
switches from executor context to the process context ->
runs the actrual process -> timer interrupt -> switch from
the user process context to the executor context ->
back to poll() -> poll returns
*/
impl Future for ThreadFuture {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let (thread_snapshot, executor_snapshot) = {
            without_interrupts(|| {
                let _manager = THREAD_MANAGER.get().unwrap().lock();
                let mut thread = self.0.lock();
                let previous_thread_ref = current_thread();

                if Arc::ptr_eq(&self.0, &previous_thread_ref) {
                    thread.state = State::Running;
                } else {
                    let previous_thread = previous_thread_ref.lock();

                    let thread_pid = {
                        let p = thread.parent.lock();
                        p.pid
                    };
                    let previous_thread_pid = {
                        let p = previous_thread.parent.lock();
                        p.pid
                    };

                    thread.state = State::Running;
                    set_current_thread(Some(self.0.clone()));
                    set_current_kernel_stack(thread.kernel_stack_top);

                    if previous_thread_pid != thread_pid {
                        MANAGER.lock().load_process(thread.parent.clone());
                    } else {
                        set_current_process(Some(thread.parent.clone()));
                    }
                };
                (
                    thread.get_appropriate_snapshot() as *mut ThreadSnapshot,
                    &mut thread.executor_snapshot as *mut ThreadSnapshot,
                )
            })
        };

        unsafe {
            (*thread_snapshot).switch_from(
                Some(&mut *executor_snapshot),
                Some(&mut Snapshot::from_current()),
            )
        };

        let process = {
            let thread = self.0.lock();
            thread.parent.clone()
        };
        let process_pid = process.lock().pid.0;
        let should_cleanup = process.lock().process_signals();
        if should_cleanup {
            if process_pid == 32 {
                crate::s_println!("signal cleanup path=thread-poll pid={}", process_pid);
            }
            THREAD_MANAGER
                .get()
                .unwrap()
                .lock()
                .cleanup_exited_threads();
        }

        let state = self.0.lock().state.clone();
        match state {
            State::Zombie => {
                log::debug!("thread poll: zombie");
                self.0.lock().task_id = None;
                THREAD_MANAGER
                    .get()
                    .unwrap()
                    .lock()
                    .cleanup_exited_threads();
                Poll::Ready(())
            }
            State::Running => {
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            State::Blocked(_) => Poll::Pending,
            State::Ready => panic!("wat"),
        }
    }
}
