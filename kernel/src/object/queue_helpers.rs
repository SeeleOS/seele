use alloc::collections::vec_deque::VecDeque;
use spin::Mutex;

use crate::{
    object::{FileFlags, error::ObjectError, misc::ObjectResult},
    thread::yielding::{
        BlockType, WakeType, cancel_block, finish_block_current, prepare_block_current,
    },
};

pub fn copy_from_queue(queue: &mut VecDeque<u8>, buffer: &mut [u8]) -> usize {
    let mut read_chars = 0;
    while read_chars < buffer.len() {
        match queue.pop_front() {
            Some(val) => {
                buffer[read_chars] = val;
                read_chars += 1;
            }
            None => break,
        }
    }

    read_chars
}

pub fn push_to_queue(queue: &mut VecDeque<u8>, buffer: &[u8]) {
    queue.extend(buffer.iter().copied());
}

pub fn read_or_block<F>(
    buffer: &mut [u8],
    flags: &Mutex<FileFlags>,
    wake_type: WakeType,
    try_read: F,
) -> ObjectResult<usize>
where
    F: FnMut(&mut [u8]) -> Option<usize>,
{
    read_or_block_with_flags(buffer, *flags.lock(), wake_type, try_read)
}

pub fn read_or_block_with_flags<F>(
    buffer: &mut [u8],
    flags: FileFlags,
    wake_type: WakeType,
    mut try_read: F,
) -> ObjectResult<usize>
where
    F: FnMut(&mut [u8]) -> Option<usize>,
{
    loop {
        if let Some(read_chars) = try_read(buffer) {
            return Ok(read_chars);
        }

        if flags.contains(FileFlags::NONBLOCK) {
            return Err(ObjectError::TryAgain);
        }

        if !crate::process::manager::get_current_process()
            .lock()
            .pending_signals
            .is_empty()
        {
            return Err(ObjectError::Interrupted);
        }

        let current = prepare_block_current(BlockType::WakeRequired {
            wake_type: wake_type.clone(),
            deadline: None,
        });

        if let Some(read_chars) = try_read(buffer) {
            cancel_block(&current);
            return Ok(read_chars);
        }

        finish_block_current();

        if !crate::process::manager::get_current_process()
            .lock()
            .pending_signals
            .is_empty()
        {
            return Err(ObjectError::Interrupted);
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::collections::vec_deque::VecDeque;

    use super::{copy_from_queue, push_to_queue};

    crate::test!(
        queue_helpers_copy_semantics,
        "queue helpers copy up to buffer length and drain consumed bytes",
        queue_helpers_copy_up_to_buffer_length_and_drain_consumed_bytes
    );
    crate::test!(
        queue_helpers_push_semantics,
        "queue helpers append bytes in order",
        queue_helpers_append_bytes_in_order
    );

    fn queue_helpers_copy_up_to_buffer_length_and_drain_consumed_bytes() {
        let mut queue = VecDeque::from([1u8, 2, 3, 4]);
        let mut buffer = [0u8; 3];
        let read = copy_from_queue(&mut queue, &mut buffer);

        assert_eq!(read, 3);
        assert_eq!(buffer, [1, 2, 3]);
        assert_eq!(queue.into_iter().collect::<alloc::vec::Vec<_>>(), [4]);
    }

    fn queue_helpers_append_bytes_in_order() {
        let mut queue = VecDeque::from([7u8]);
        push_to_queue(&mut queue, &[8, 9, 10]);
        assert_eq!(
            queue.into_iter().collect::<alloc::vec::Vec<_>>(),
            [7, 8, 9, 10]
        );
    }
}
