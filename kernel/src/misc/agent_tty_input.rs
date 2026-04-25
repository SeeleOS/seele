use alloc::collections::vec_deque::VecDeque;
use core::mem;

use conquer_once::spin::OnceCell;
use spin::Mutex;
use uart_16550::{Config, Uart16550, backend::PioBackend, spec::registers::IER};

use crate::keyboard::char_processing::process_char;

const AGENT_TTY_COM_PORT: u16 = 0x2F8;

struct AgentTtyInput {
    uart: Uart16550<PioBackend>,
    pending_bytes: VecDeque<u8>,
}

impl AgentTtyInput {
    fn fill_pending_bytes(&mut self) {
        while let Ok(byte) = self.uart.try_receive_byte() {
            self.pending_bytes.push_back(byte);
        }
    }
}

static AGENT_TTY_INPUT: OnceCell<Mutex<AgentTtyInput>> = OnceCell::uninit();

pub fn init() {
    AGENT_TTY_INPUT.get_or_init(|| {
        let config = Config {
            interrupts: IER::empty(),
            ..Config::default()
        };
        let mut uart = unsafe {
            Uart16550::new_port(AGENT_TTY_COM_PORT).expect("invalid agent tty input port")
        };
        uart.init(config)
            .expect("failed to initialize agent tty input");
        Mutex::new(AgentTtyInput {
            uart,
            pending_bytes: VecDeque::new(),
        })
    });
}

pub fn process_pending_input() {
    let Some(input) = AGENT_TTY_INPUT.get() else {
        return;
    };

    let pending_bytes = {
        let mut input = input.lock();
        input.fill_pending_bytes();
        mem::take(&mut input.pending_bytes)
    };

    for byte in pending_bytes {
        process_char(char::from(byte));
    }
}

pub fn has_pending_input() -> bool {
    let Some(input) = AGENT_TTY_INPUT.get() else {
        return false;
    };

    let mut input = input.lock();
    input.fill_pending_bytes();
    !input.pending_bytes.is_empty()
}
