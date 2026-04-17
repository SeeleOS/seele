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
    mut try_read: F,
) -> ObjectResult<usize>
where
    F: FnMut(&mut [u8]) -> Option<usize>,
{
    loop {
        if let Some(read_chars) = try_read(buffer) {
            return Ok(read_chars);
        }

        if flags.lock().contains(FileFlags::NONBLOCK) {
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
