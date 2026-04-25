use alloc::collections::vec_deque::VecDeque;
use core::mem;

use spin::Mutex;
use x86_64::instructions::port::Port;

use crate::keyboard::char_processing::process_char;

const AGENT_TTY_COM_PORT: u16 = 0x2F8;
const DATA_OFFSET: u16 = 0;
const LSR_OFFSET: u16 = 5;
const LSR_DATA_READY: u8 = 1 << 0;

struct AgentTtyInput {
    pending_bytes: VecDeque<u8>,
}

impl AgentTtyInput {
    fn fill_pending_bytes(&mut self) {
        while uart_has_pending_byte() {
            self.pending_bytes.push_back(read_uart_byte());
        }
    }
}

static AGENT_TTY_INPUT: Mutex<AgentTtyInput> = Mutex::new(AgentTtyInput {
    pending_bytes: VecDeque::new(),
});

pub fn init() -> bool {
    true
}

pub fn process_pending_input() {
    let pending_bytes = {
        let mut input = AGENT_TTY_INPUT.lock();
        input.fill_pending_bytes();
        mem::take(&mut input.pending_bytes)
    };

    for byte in pending_bytes {
        process_char(char::from(byte));
    }
}

pub fn has_pending_input() -> bool {
    let mut input = AGENT_TTY_INPUT.lock();
    input.fill_pending_bytes();
    !input.pending_bytes.is_empty()
}

fn uart_has_pending_byte() -> bool {
    read_lsr() & LSR_DATA_READY != 0
}

fn read_lsr() -> u8 {
    unsafe { Port::new(AGENT_TTY_COM_PORT + LSR_OFFSET).read() }
}

fn read_uart_byte() -> u8 {
    unsafe { Port::new(AGENT_TTY_COM_PORT + DATA_OFFSET).read() }
}
