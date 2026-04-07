use core::task::Poll;

use conquer_once::spin::OnceCell;
use crossbeam_queue::ArrayQueue;
use futures_util::{Stream, task::AtomicWaker};

pub static SCANCODE_QUEUE: OnceCell<ArrayQueue<u8>> = OnceCell::uninit();
pub static WAKER: AtomicWaker = AtomicWaker::new();

pub struct ScancodeStream;

impl Default for ScancodeStream {
    fn default() -> Self {
        SCANCODE_QUEUE.get_or_init(|| ArrayQueue::new(512));

        Self
    }
}

impl Stream for ScancodeStream {
    type Item = u8;

    fn poll_next(
        self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Option<Self::Item>> {
        let queue = SCANCODE_QUEUE.try_get().expect("Uninitialized");

        if let Some(scancode) = queue.pop() {
            // Skips registering a waker if there already is a scancode avalible
            Poll::Ready(Some(scancode))
        } else {
            // registers a waker to wakeup when a value is avalible
            // the queue might have the code again after registering the waker
            WAKER.register(cx.waker());
            match queue.pop() {
                Some(value) => {
                    // remove the waker because its nologner needed, we already have the value
                    WAKER.take();
                    Poll::Ready(Some(value))
                }
                None => Poll::Pending,
            }
        }
    }
}
