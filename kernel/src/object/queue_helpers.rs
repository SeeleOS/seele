use crate::memory::utils::Mut;
use alloc::collections::vec_deque::VecDeque;

use crate::{
    misc::profile::{self, HotSyscallPhase},
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
    flags: &Mut<FileFlags>,
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
        let try_read_start = profile::scope_start();
        if let Some(read_chars) = try_read(buffer) {
            profile::record_hot_syscall_phase(
                HotSyscallPhase::ReadTryRead,
                profile::scope_start().saturating_sub(try_read_start),
            );
            return Ok(read_chars);
        }
        profile::record_hot_syscall_phase(
            HotSyscallPhase::ReadTryRead,
            profile::scope_start().saturating_sub(try_read_start),
        );

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

        let prepare_start = profile::scope_start();
        let current = prepare_block_current(BlockType::WakeRequired {
            wake_type: wake_type.clone(),
            deadline: None,
        });
        profile::record_hot_syscall_phase(
            HotSyscallPhase::ReadBlockPrepare,
            profile::scope_start().saturating_sub(prepare_start),
        );

        let retry_start = profile::scope_start();
        if let Some(read_chars) = try_read(buffer) {
            profile::record_hot_syscall_phase(
                HotSyscallPhase::ReadBlockRetry,
                profile::scope_start().saturating_sub(retry_start),
            );
            cancel_block(&current);
            return Ok(read_chars);
        }
        profile::record_hot_syscall_phase(
            HotSyscallPhase::ReadBlockRetry,
            profile::scope_start().saturating_sub(retry_start),
        );

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
    use alloc::vec::Vec;

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
        assert_eq!(queue.into_iter().collect::<Vec<_>>(), [4]);
    }

    fn queue_helpers_append_bytes_in_order() {
        let mut queue = VecDeque::from([7u8]);
        push_to_queue(&mut queue, &[8, 9, 10]);
        assert_eq!(queue.into_iter().collect::<Vec<_>>(), [7, 8, 9, 10]);
    }
}
