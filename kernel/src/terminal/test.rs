use crate::memory::utils::Mut;
use alloc::{collections::VecDeque, string::String, sync::Arc, vec::Vec};

use crate::{
    object::{
        FileFlags,
        config::{
            ConfigurateRequest, LinuxTermios, LinuxTermios2, LinuxWinsize, PtyPeerAccessMode,
            PtyPeerOpenFlags, PtyPeerOpenRequest,
        },
        error::ObjectError,
        misc::ObjectRef,
        traits::{Configuratable, Readable, Writable},
        tty_device::{ACTIVE_VT, VIRTUAL_TTYS},
    },
    process::{FdFlags, manager::get_current_process},
    terminal::{
        line_discipline::{process_input_byte, process_output_bytes},
        linux_kd::{DisplayMode, KeyboardMode, LinuxConsoleState, LinuxKbEntry, LinuxVtMode},
        linux_vt::handle_vt_request,
        object::TerminalObject,
        output_filter::OutputFilter,
        pty::{
            get_pty_slave, master::PtyMaster, open_ptmx, set_pty_lock, shared::PtyShared,
            slave::PtySlave,
        },
        term_trait::{AbstractTerminal, PtyWriter, TerminalCursorPosition, TerminalSize},
        termios::{VEOF_INDEX, VERASE_INDEX, VINTR_INDEX},
    },
};

crate::test!(
    canonical_input_buffering,
    "canonical input buffers until newline and handles erase",
    canonical_input_buffers_until_newline_and_handles_erase
);
crate::test!(
    noncanonical_input_and_interrupts,
    "noncanonical input queues immediately and interrupts signal",
    noncanonical_input_queues_immediately_and_interrupts_signal
);
crate::test!(
    output_newline_filtering,
    "output filter maps lone newline to crlf",
    output_filter_maps_lone_newline_to_crlf
);
crate::test!(
    xtgettcap_sequence_buffering,
    "xtgettcap filter buffers incomplete sequences",
    xtgettcap_filter_buffers_incomplete_sequences
);
crate::test!(
    default_termios_basics,
    "default termios matches linux tty basics",
    default_termios_matches_linux_tty_basics
);
crate::test!(
    pty_ioctl_semantics,
    "pty ioctls follow linux rules",
    pty_ioctls_follow_linux_rules
);
crate::test!(
    terminal_and_tty_ioctl_semantics,
    "terminal and tty ioctls follow linux rules",
    terminal_and_tty_ioctls_follow_linux_rules
);

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

#[derive(Debug)]
struct TestTerminal {
    rows: usize,
    cols: usize,
}

impl AbstractTerminal for TestTerminal {
    fn push_str(&mut self, _str: &str) {}

    fn size(&self) -> TerminalSize {
        TerminalSize::new(self.rows, self.cols)
    }

    fn cursor_position(&self) -> TerminalCursorPosition {
        TerminalCursorPosition::from_zero_based(0, 0)
    }

    fn set_pty_writer(&mut self, _writer: PtyWriter) {}

    fn clear(&mut self) {}
}

fn make_terminal_object(rows: usize, cols: usize) -> Arc<Mut<TerminalObject>> {
    Arc::new(Mut::new(TerminalObject::new(Arc::new(Mut::new(
        TestTerminal { rows, cols },
    )))))
}

fn make_pty_pair() -> (Arc<PtyMaster>, Arc<PtySlave>) {
    let shared = Arc::new(Mut::new(PtyShared::default()));
    let master = Arc::new(PtyMaster::new(7, shared.clone()));
    let slave = Arc::new(PtySlave::new(7, shared.clone()));
    let master_ref: ObjectRef = master.clone();
    let slave_ref: ObjectRef = slave.clone();
    let mut locked = shared.lock();
    locked.master = Some(Arc::downgrade(&master_ref));
    locked.slave = Some(Arc::downgrade(&slave_ref));
    drop(locked);
    (master, slave)
}

fn pty_ioctls_follow_linux_rules() {
    let (master, slave) = make_pty_pair();
    let mut pty_number = -1i32;
    assert_eq!(
        master
            .configure(ConfigurateRequest::LinuxTiocgptn(&mut pty_number))
            .unwrap(),
        0
    );
    assert_eq!(pty_number, 7);

    let mut winsize = LinuxWinsize::default_terminal_size();
    assert_eq!(
        slave
            .configure(ConfigurateRequest::LinuxTiocgwinsz(&mut winsize))
            .unwrap(),
        0
    );
    assert_eq!(winsize.ws_row, 25);
    assert_eq!(winsize.ws_col, 80);

    let new_winsize = LinuxWinsize::from_rows_cols(40, 120);
    assert_eq!(
        slave
            .configure(ConfigurateRequest::LinuxTiocswinsz(&new_winsize))
            .unwrap(),
        0
    );
    let mut updated_winsize = LinuxWinsize::default();
    assert_eq!(
        master
            .configure(ConfigurateRequest::LinuxTiocgwinsz(&mut updated_winsize))
            .unwrap(),
        0
    );
    assert_eq!(updated_winsize.ws_row, 40);
    assert_eq!(updated_winsize.ws_col, 120);

    let partial_winsize = LinuxWinsize::from_rows_cols(0, 90);
    assert_eq!(
        slave
            .configure(ConfigurateRequest::LinuxTiocswinsz(&partial_winsize))
            .unwrap(),
        0
    );
    let mut partially_updated = LinuxWinsize::default();
    assert_eq!(
        master
            .configure(ConfigurateRequest::LinuxTiocgwinsz(&mut partially_updated))
            .unwrap(),
        0
    );
    assert_eq!(partially_updated.ws_row, 40);
    assert_eq!(partially_updated.ws_col, 90);

    let mut termios = LinuxTermios::default();
    assert_eq!(
        slave
            .configure(ConfigurateRequest::LinuxTcGets(&mut termios))
            .unwrap(),
        0
    );
    assert_eq!(termios.c_cc[VINTR_INDEX], 3);
    let mut termios2 = LinuxTermios2::default();
    assert_eq!(
        slave
            .configure(ConfigurateRequest::LinuxTcGets2(&mut termios2))
            .unwrap(),
        0
    );
    assert_eq!(termios2.c_cc[VINTR_INDEX], 3);

    let pgrp = 4242i32;
    assert_eq!(
        slave
            .configure(ConfigurateRequest::LinuxTiocspgrp(&pgrp))
            .unwrap(),
        0
    );
    let mut reported_pgrp = 0i32;
    assert_eq!(
        master
            .configure(ConfigurateRequest::LinuxTiocgPgrp(&mut reported_pgrp))
            .unwrap(),
        0
    );
    assert_eq!(reported_pgrp, pgrp);

    let registry_master = open_ptmx();
    let registry_master = registry_master.as_configuratable().unwrap();
    let mut registry_number = -1i32;
    assert_eq!(
        registry_master
            .configure(ConfigurateRequest::LinuxTiocgptn(&mut registry_number))
            .unwrap(),
        0
    );
    assert!(get_pty_slave(registry_number as u32).is_none());

    let unlocked = 0i32;
    assert_eq!(
        registry_master
            .configure(ConfigurateRequest::LinuxTiocsptlck(&unlocked))
            .unwrap(),
        0
    );
    assert!(get_pty_slave(registry_number as u32).is_some());

    let locked = 1i32;
    assert_eq!(
        registry_master
            .configure(ConfigurateRequest::LinuxTiocsptlck(&locked))
            .unwrap(),
        0
    );
    assert!(get_pty_slave(registry_number as u32).is_none());
    assert_eq!(
        registry_master
            .configure(ConfigurateRequest::LinuxTiocsptlck(&unlocked))
            .unwrap(),
        0
    );

    let peer_fd = registry_master
        .configure(ConfigurateRequest::LinuxTiocgptpeer(PtyPeerOpenRequest {
            access_mode: PtyPeerAccessMode::ReadWrite,
            flags: PtyPeerOpenFlags::O_NONBLOCK | PtyPeerOpenFlags::O_CLOEXEC,
        }))
        .unwrap() as usize;
    let process = get_current_process();
    let fd_flags = process.lock().get_fd_flags(peer_fd).unwrap();
    assert_eq!(fd_flags, FdFlags::CLOEXEC);
    let peer = process.lock().get_object(peer_fd as u64).unwrap();
    assert!(peer.as_pty_slave().is_ok());
    process.lock().clear_fd_slot(peer_fd).unwrap();

    *slave.flags.lock() = FileFlags::NONBLOCK;
    master.write(b"queued\n").unwrap();
    assert_eq!(
        slave
            .configure(ConfigurateRequest::LinuxTiocvhangup)
            .unwrap(),
        0
    );
    let mut buffer = [0u8; 16];
    assert!(matches!(
        slave.read(&mut buffer),
        Err(ObjectError::TryAgain)
    ));

    *slave.flags.lock() = FileFlags::empty();
    master.write(b"a").unwrap();
    assert_eq!(
        slave
            .configure(ConfigurateRequest::LinuxTiocvhangup)
            .unwrap(),
        0
    );
    master.write(b"\n").unwrap();
    *slave.flags.lock() = FileFlags::NONBLOCK;
    let read = slave.read(&mut buffer).unwrap();
    assert_eq!(&buffer[..read], b"\n");

    let invalid = ConfigurateRequest::LinuxTiocgptn(&mut pty_number);
    assert!(matches!(
        slave.configure(invalid),
        Err(ObjectError::InvalidRequest)
    ));

    set_pty_lock(registry_number as u32, false);
}

fn terminal_and_tty_ioctls_follow_linux_rules() {
    let terminal = make_terminal_object(30, 100);

    let mut termios = LinuxTermios::default();
    assert_eq!(
        terminal
            .lock()
            .configure(ConfigurateRequest::LinuxTcGets(&mut termios))
            .unwrap(),
        0
    );
    assert_eq!(termios.c_cc[VERASE_INDEX], 127);

    let next_termios = LinuxTermios {
        c_lflag: 0,
        ..termios
    };
    assert_eq!(
        terminal
            .lock()
            .configure(ConfigurateRequest::LinuxTcSets(&next_termios))
            .unwrap(),
        0
    );
    let mut roundtrip = LinuxTermios::default();
    terminal
        .lock()
        .configure(ConfigurateRequest::LinuxTcGets(&mut roundtrip))
        .unwrap();
    assert_eq!(roundtrip.c_lflag, 0);

    let state = Mut::new(LinuxConsoleState::default());
    let mut mode = LinuxVtMode::default();
    assert_eq!(
        handle_vt_request(&state, &ConfigurateRequest::LinuxVtGetMode(&mut mode))
            .unwrap()
            .unwrap(),
        0
    );
    assert_eq!(mode.mode, 0);

    let saved_active = *ACTIVE_VT.get().unwrap().lock();
    let saved_vts = VIRTUAL_TTYS.get().unwrap().lock().clone();
    let tty = Arc::new(crate::object::tty_device::TtyDevice::new(
        terminal.clone(),
        true,
        Some(2),
    ));
    VIRTUAL_TTYS.get().unwrap().lock().insert(2, tty.clone());
    *ACTIVE_VT.get().unwrap().lock() = 2;
    assert_eq!(
        tty.configure(ConfigurateRequest::LinuxKdSetKeyboardMode(
            KeyboardMode::Raw as u32
        ))
        .unwrap(),
        0
    );
    assert_eq!(tty.keyboard_mode(), KeyboardMode::Raw);

    let mut display_mode = 99u32;
    assert_eq!(
        tty.configure(ConfigurateRequest::LinuxKdGetDisplayMode(&mut display_mode))
            .unwrap(),
        0
    );
    assert_eq!(display_mode, DisplayMode::Text as u32);

    assert_eq!(
        tty.configure(ConfigurateRequest::LinuxKdSetDisplayMode(
            DisplayMode::Graphics as u32
        ))
        .unwrap(),
        0
    );
    assert_eq!(
        tty.configure(ConfigurateRequest::LinuxKdGetDisplayMode(&mut display_mode))
            .unwrap(),
        0
    );
    assert_eq!(display_mode, DisplayMode::Graphics as u32);

    let mut kb_mode = 0u32;
    assert_eq!(
        tty.configure(ConfigurateRequest::LinuxKdGetKeyboardMode(&mut kb_mode))
            .unwrap(),
        0
    );
    assert_eq!(kb_mode, KeyboardMode::Raw as u32);

    let mut kb_type = 0u8;
    assert_eq!(
        tty.configure(ConfigurateRequest::LinuxKdGetKeyboardType(&mut kb_type))
            .unwrap(),
        0
    );
    assert_eq!(kb_type, 0x02);

    let mut kb_entry = LinuxKbEntry {
        kb_table: 0,
        kb_index: 16,
        kb_value: 0,
    };
    assert_eq!(
        tty.configure(ConfigurateRequest::LinuxKdGetKeyboardEntry(&mut kb_entry))
            .unwrap(),
        0
    );
    assert_ne!(kb_entry.kb_value, 0);

    assert_eq!(
        tty.configure(ConfigurateRequest::LinuxKdSignalAccept(9))
            .unwrap(),
        0
    );

    let mut tty_pgrp = 0i32;
    assert_eq!(
        tty.configure(ConfigurateRequest::LinuxTiocgPgrp(&mut tty_pgrp))
            .unwrap(),
        0
    );
    assert_eq!(tty_pgrp, 0);

    let tty_new_pgrp = 123i32;
    assert_eq!(
        tty.configure(ConfigurateRequest::LinuxTiocspgrp(&tty_new_pgrp))
            .unwrap(),
        0
    );
    assert_eq!(
        tty.configure(ConfigurateRequest::LinuxTiocgPgrp(&mut tty_pgrp))
            .unwrap(),
        0
    );
    assert_eq!(tty_pgrp, tty_new_pgrp);

    let mut tty_winsize = LinuxWinsize::default();
    assert_eq!(
        tty.configure(ConfigurateRequest::LinuxTiocgwinsz(&mut tty_winsize))
            .unwrap(),
        0
    );
    assert_eq!(tty_winsize.ws_row, 30);
    assert_eq!(tty_winsize.ws_col, 100);

    let tty_new_winsize = LinuxWinsize::from_rows_cols(50, 140);
    assert_eq!(
        tty.configure(ConfigurateRequest::LinuxTiocswinsz(&tty_new_winsize))
            .unwrap(),
        0
    );
    assert_eq!(
        tty.configure(ConfigurateRequest::LinuxTiocgwinsz(&mut tty_winsize))
            .unwrap(),
        0
    );
    assert_eq!(tty_winsize.ws_row, 50);
    assert_eq!(tty_winsize.ws_col, 140);

    let mut vt_state = crate::terminal::linux_kd::LinuxVtStat::default();
    assert_eq!(
        tty.configure(ConfigurateRequest::LinuxVtGetState(&mut vt_state))
            .unwrap(),
        0
    );
    assert_eq!(vt_state.v_active, 2);
    assert_eq!(vt_state.v_state, 1u16 << 2);

    let mut vt_query = 0u32;
    assert_eq!(
        tty.configure(ConfigurateRequest::LinuxVtOpenQuery(&mut vt_query))
            .unwrap(),
        0
    );
    assert_ne!(vt_query, 0);

    let new_mode = LinuxVtMode {
        mode: 1,
        relsig: 2,
        acqsig: 3,
        frsig: 4,
        waitv: 0,
    };
    assert_eq!(
        tty.configure(ConfigurateRequest::LinuxVtSetMode(&new_mode))
            .unwrap(),
        0
    );
    let mut roundtrip_mode = LinuxVtMode::default();
    assert_eq!(
        tty.configure(ConfigurateRequest::LinuxVtGetMode(&mut roundtrip_mode))
            .unwrap(),
        0
    );
    assert_eq!(roundtrip_mode.mode, 1);
    assert_eq!(roundtrip_mode.relsig, 2);

    assert_eq!(
        tty.configure(ConfigurateRequest::LinuxVtRelDisp(1))
            .unwrap(),
        0
    );
    assert!(matches!(
        tty.configure(ConfigurateRequest::LinuxVtRelDisp(0)),
        Err(ObjectError::InvalidArguments)
    ));
    assert!(matches!(
        tty.configure(ConfigurateRequest::LinuxVtActivate(99)),
        Err(ObjectError::InvalidArguments)
    ));

    let unsupported = ConfigurateRequest::LinuxTiocgptn(&mut -1i32);
    assert!(matches!(
        tty.configure(unsupported),
        Err(ObjectError::InvalidRequest)
    ));

    *ACTIVE_VT.get().unwrap().lock() = saved_active;
    *VIRTUAL_TTYS.get().unwrap().lock() = saved_vts;
}
