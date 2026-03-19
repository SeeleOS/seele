use core::task::Poll;

use alloc::sync::Arc;
use x86_64::{VirtAddr, instructions::interrupts::without_interrupts};

use crate::{
    misc::snapshot::Snapshot,
    multitasking::{
        MANAGER,
        thread::{THREAD_MANAGER, ThreadRef, misc::State, snapshot::ThreadSnapshot},
    },
    println,
    tss::TSS,
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

    fn poll(
        self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Self::Output> {
        let (thread_snapshot, executor_snapshot) = {
            without_interrupts(|| {
                let mut manager = THREAD_MANAGER.get().unwrap().lock();
                let mut thread = self.0.lock();
                let previous_thread_ref = manager.current.clone().unwrap();

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
                    manager.current = Some(self.0.clone());
                    unsafe {
                        TSS.privilege_stack_table[0] = VirtAddr::new(thread.kernel_stack_top);
                    }

                    if previous_thread_pid != thread_pid {
                        MANAGER.lock().load_process(thread.parent.clone());
                    }
                };

                (
                    &mut thread.snapshot as *mut ThreadSnapshot,
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

        let state = self.0.lock().state.clone();

        match state {
            State::Zombie => {
                log::debug!("thread poll: zombie");
                THREAD_MANAGER.get().unwrap().lock().clean_zombies();
                Poll::Ready(())
            }
            State::Running => {
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            State::Blocked(_) => Poll::Ready(()),
            State::Ready => panic!("wat"),
        }
    }
}
