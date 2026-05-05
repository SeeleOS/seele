use alloc::{collections::VecDeque, string::String, vec::Vec};

use crate::{
    object::config::LinuxTermios2,
    terminal::{
        line_discipline::{process_input_byte, process_output_bytes},
        output_filter::OutputFilter,
        termios::{VEOF_INDEX, VERASE_INDEX, VINTR_INDEX},
    },
};

crate::test!("terminal line discipline and termios helpers", || {
    canonical_input_buffers_until_newline_and_handles_erase();
    noncanonical_input_queues_immediately_and_interrupts_signal();
    output_filter_maps_lone_newline_to_crlf();
    xtgettcap_filter_buffers_incomplete_sequences();
    default_termios_matches_linux_tty_basics();
});

fn canonical_input_buffers_until_newline_and_handles_erase() {
    let termios = LinuxTermios2::new_default();
    let mut line = VecDeque::new();
    let mut queued = Vec::new();
    let mut echoed = Vec::new();
    let mut interrupts = 0usize;

    for byte in [b'a', b'b', termios.erase_char(), b'c', b'\n'] {
        process_input_byte(
            &termios,
            &mut line,
            byte,
            |byte| queued.push(byte),
            |bytes| echoed.extend_from_slice(bytes),
            || interrupts += 1,
        );
    }

    assert_eq!(queued, b"ac\n");
    assert_eq!(echoed, b"ab\x08 \x08c\n");
    assert_eq!(interrupts, 0);
}

fn noncanonical_input_queues_immediately_and_interrupts_signal() {
    let mut termios = LinuxTermios2::new_default();
    termios.c_lflag &= !0x2;
    let mut line = VecDeque::new();
    let mut queued = Vec::new();
    let mut echoed = Vec::new();
    let mut interrupts = 0usize;

    process_input_byte(
        &termios,
        &mut line,
        b'x',
        |byte| queued.push(byte),
        |bytes| echoed.extend_from_slice(bytes),
        || interrupts += 1,
    );
    process_input_byte(
        &termios,
        &mut line,
        termios.interrupt_char(),
        |byte| queued.push(byte),
        |bytes| echoed.extend_from_slice(bytes),
        || interrupts += 1,
    );

    assert_eq!(queued, b"x");
    assert_eq!(echoed, b"x");
    assert_eq!(interrupts, 1);
}

fn output_filter_maps_lone_newline_to_crlf() {
    let termios = LinuxTermios2::new_default();
    let mut emitted = Vec::new();

    process_output_bytes(&termios, b"a\nb\r\n", |byte| emitted.push(byte));

    assert_eq!(emitted, b"a\r\nb\r\n");
}

fn xtgettcap_filter_buffers_incomplete_sequences() {
    let mut filter = OutputFilter::default();

    let first = filter.filter("pre\x1bP+q6e616d");
    assert_eq!(first.display_text, "pre");
    assert!(first.responses.is_empty());

    let second = filter.filter("65\x1b\\post");
    assert_eq!(second.display_text, "post");
    assert_eq!(
        second.responses,
        [String::from("\x1bP1+r6e616d65=6C696E7578\x1b\\")]
    );
}

fn default_termios_matches_linux_tty_basics() {
    let termios = LinuxTermios2::new_default();

    assert!(termios.is_canonical());
    assert!(termios.should_echo());
    assert!(termios.should_echo_erase());
    assert!(termios.should_echo_newline());
    assert!(termios.should_signal_on_special_chars());
    assert!(termios.map_input_cr_to_nl());
    assert!(termios.map_output_newline_to_crlf());
    assert_eq!(termios.c_cc[VINTR_INDEX], 3);
    assert_eq!(termios.c_cc[VERASE_INDEX], 127);
    assert_eq!(termios.c_cc[VEOF_INDEX], 4);
}
