use alloc::sync::Arc;
use spin::Mutex;

use crate::{
    filesystem::info::LinuxStat,
    impl_cast_function,
    memory::user_safe,
    object::{
        FileFlags, Object,
        error::ObjectError,
        config::ConfigurateRequest,
        misc::ObjectResult,
        queue_helpers::{copy_from_queue, read_or_block_with_flags},
        traits::{Configuratable, Readable, Statable, Writable},
    },
    polling::{event::PollableEvent, object::Pollable},
    process::group::ProcessGroupID,
    process::{ControllingTerminal, manager::get_current_process},
    terminal::{
        line_discipline::process_output_bytes,
        linux_kd::{LinuxConsoleState, handle_kd_request},
        linux_vt::handle_vt_request,
        pty::shared::PtyShared,
    },
    thread::{THREAD_MANAGER, yielding::WakeType},
};

impl Pollable for PtySlave {
    fn is_event_ready(&self, event: PollableEvent) -> bool {
        match event {
            PollableEvent::CanBeRead => !self.shared.lock().from_master.is_empty(),
            PollableEvent::CanBeWritten => true,
            _ => false,
        }
    }
}

#[derive(Debug)]
pub struct PtySlave {
    number: u32,
    shared: Arc<Mutex<PtyShared>>,
    linux_console: Mutex<LinuxConsoleState>,
    pub flags: Mutex<FileFlags>,
}

impl PtySlave {
    pub fn new(number: u32, shared: Arc<Mutex<PtyShared>>) -> Self {
        Self {
            number,
            shared,
            linux_console: Mutex::new(LinuxConsoleState::default()),
            flags: Mutex::new(FileFlags::default()),
        }
    }

    pub fn foreground_process_group(&self) -> Option<ProcessGroupID> {
        self.shared.lock().active_group
    }
}

impl Object for PtySlave {
    fn get_flags(self: Arc<Self>) -> ObjectResult<FileFlags> {
        Ok(*self.flags.lock())
    }

    fn set_flags(self: Arc<Self>, flags: FileFlags) -> ObjectResult<()> {
        *self.flags.lock() = flags;
        Ok(())
    }

    impl_cast_function!("writable", Writable);
    impl_cast_function!("readable", Readable);
    impl_cast_function!("configuratable", Configuratable);
    impl_cast_function!("pollable", Pollable);
    impl_cast_function!("statable", Statable);
    crate::impl_cast_function_non_trait!("pty_slave", PtySlave);
}

impl Writable for PtySlave {
    fn write(&self, buffer: &[u8]) -> ObjectResult<usize> {
        let master = {
            let mut shared = self.shared.lock();
            let termios = shared.termios;
            process_output_bytes(&termios, buffer, |byte| {
                shared.from_slave.push_back(byte);
            });
            shared.get_master()
        };

        let mut manager = THREAD_MANAGER.get().unwrap().lock();
        manager.wake_pty();
        manager.wake_poller(master, PollableEvent::CanBeRead);
        Ok(buffer.len())
    }
}

impl Readable for PtySlave {
    fn read(&self, buffer: &mut [u8]) -> ObjectResult<usize> {
        self.read_with_flags(buffer, *self.flags.lock())
    }

    fn read_with_flags(&self, buffer: &mut [u8], flags: FileFlags) -> ObjectResult<usize> {
        read_or_block_with_flags(buffer, flags, WakeType::Pty, |buffer| {
            let mut shared = self.shared.lock();
            if shared.from_master.is_empty() {
                None
            } else {
                Some(copy_from_queue(&mut shared.from_master, buffer))
            }
        })
    }
}

impl Configuratable for PtySlave {
    fn configure(&self, request: ConfigurateRequest) -> ObjectResult<isize> {
        if let Some(result) = handle_kd_request(&self.linux_console, &request)? {
            return Ok(result);
        }

        if let Some(result) = handle_vt_request(&self.linux_console, &request)? {
            return Ok(result);
        }

        match request {
            ConfigurateRequest::LinuxTiocsctty(_) => {
                let process = get_current_process();
                let (group_id, controlling_terminal) = {
                    let process = process.lock();
                    (
                        process.group_id,
                        process.stdin_terminal_rdev().map(ControllingTerminal),
                    )
                };
                self.shared.lock().active_group = Some(group_id);
                process.lock().controlling_terminal = controlling_terminal;
                Ok(0)
            }
            ConfigurateRequest::LinuxTiocgPgrp(ptr) => unsafe {
                let tty_group = self
                    .shared
                    .lock()
                    .active_group
                    .map(|group| group.0 as i32)
                    .unwrap_or(0);
                *ptr = tty_group;
                Ok(0)
            },
            ConfigurateRequest::LinuxTiocnotty => {
                get_current_process().lock().controlling_terminal = None;
                Ok(0)
            }
            ConfigurateRequest::LinuxTiocspgrp(ptr) => {
                let requested_group =
                    user_safe::read(ptr).map_err(|_| ObjectError::BadAddress)? as u64;
                self.shared.lock().active_group = Some(ProcessGroupID(requested_group));
                Ok(0)
            }
            ConfigurateRequest::LinuxTcGets(termios) => {
                let termios_state = self.shared.lock().termios;
                user_safe::write(termios, &termios_state.as_linux_termios())
                    .map_err(|_| ObjectError::BadAddress)?;
                Ok(0)
            }
            ConfigurateRequest::LinuxTcSets(termios) => {
                let termios = user_safe::read(termios).map_err(|_| ObjectError::BadAddress)?;
                let mut shared = self.shared.lock();
                shared.termios.apply_linux_termios(&termios);
                Ok(0)
            }
            ConfigurateRequest::LinuxTcGets2(termios) => {
                user_safe::write(termios, &self.shared.lock().termios)
                    .map_err(|_| ObjectError::BadAddress)?;
                Ok(0)
            }
            ConfigurateRequest::LinuxTcSets2(termios) => {
                let termios = user_safe::read(termios).map_err(|_| ObjectError::BadAddress)?;
                let mut shared = self.shared.lock();
                shared.termios.apply_linux_termios2(&termios);
                Ok(0)
            }
            ConfigurateRequest::LinuxTiocgwinsz(winsize) => {
                user_safe::write(winsize, &self.shared.lock().winsize)
                    .map_err(|_| ObjectError::BadAddress)?;
                Ok(0)
            }
            ConfigurateRequest::LinuxTiocswinsz(winsize) => {
                let winsize = user_safe::read(winsize).map_err(|_| ObjectError::BadAddress)?;
                let mut shared = self.shared.lock();
                if winsize.ws_row != 0 {
                    shared.winsize.ws_row = winsize.ws_row;
                }
                if winsize.ws_col != 0 {
                    shared.winsize.ws_col = winsize.ws_col;
                }
                Ok(0)
            },
            ConfigurateRequest::LinuxTiocvhangup => {
                let mut shared = self.shared.lock();
                shared.line_buffer.clear();
                shared.from_master.clear();
                Ok(0)
            }
            _ => {
                crate::s_println!(
                    "dangerous noop success pty slave ioctl request={}",
                    request_name(&request)
                );
                Ok(0)
            }
        }
    }
}

impl Statable for PtySlave {
    fn stat(&self) -> LinuxStat {
        LinuxStat::char_device_with_rdev(0o620, (136u64 << 8) | self.number as u64)
    }
}

fn request_name(request: &ConfigurateRequest) -> &'static str {
    match request {
        ConfigurateRequest::FbGetVariableScreenInfo(_) => "FbGetVariableScreenInfo",
        ConfigurateRequest::FbPutVariableScreenInfo(_) => "FbPutVariableScreenInfo",
        ConfigurateRequest::FbGetFixedScreenInfo(_) => "FbGetFixedScreenInfo",
        ConfigurateRequest::FbGetColorMap(_) => "FbGetColorMap",
        ConfigurateRequest::FbPutColorMap(_) => "FbPutColorMap",
        ConfigurateRequest::FbPanDisplay(_) => "FbPanDisplay",
        ConfigurateRequest::FbBlank(_) => "FbBlank",
        ConfigurateRequest::LinuxTcGets(_) => "LinuxTcGets",
        ConfigurateRequest::LinuxTcSets(_) => "LinuxTcSets",
        ConfigurateRequest::LinuxTcFlush(_) => "LinuxTcFlush",
        ConfigurateRequest::LinuxTcGets2(_) => "LinuxTcGets2",
        ConfigurateRequest::LinuxTcSets2(_) => "LinuxTcSets2",
        ConfigurateRequest::LinuxTiocnxcl => "LinuxTiocnxcl",
        ConfigurateRequest::LinuxTiocsctty(_) => "LinuxTiocsctty",
        ConfigurateRequest::LinuxTiocgPgrp(_) => "LinuxTiocgPgrp",
        ConfigurateRequest::LinuxTiocnotty => "LinuxTiocnotty",
        ConfigurateRequest::LinuxTiocspgrp(_) => "LinuxTiocspgrp",
        ConfigurateRequest::LinuxTiocoutq(_) => "LinuxTiocoutq",
        ConfigurateRequest::LinuxTiocgwinsz(_) => "LinuxTiocgwinsz",
        ConfigurateRequest::LinuxTiocswinsz(_) => "LinuxTiocswinsz",
        ConfigurateRequest::LinuxTiocgptn(_) => "LinuxTiocgptn",
        ConfigurateRequest::LinuxTiocsptlck(_) => "LinuxTiocsptlck",
        ConfigurateRequest::LinuxTiocgptpeer(_) => "LinuxTiocgptpeer",
        ConfigurateRequest::LinuxTiocvhangup => "LinuxTiocvhangup",
        ConfigurateRequest::LinuxKdGetKeyboardMode(_) => "LinuxKdGetKeyboardMode",
        ConfigurateRequest::LinuxKdSetKeyboardMode(_) => "LinuxKdSetKeyboardMode",
        ConfigurateRequest::LinuxKdGetKeyboardType(_) => "LinuxKdGetKeyboardType",
        ConfigurateRequest::LinuxKdGetKeyboardEntry(_) => "LinuxKdGetKeyboardEntry",
        ConfigurateRequest::LinuxKdGetDisplayMode(_) => "LinuxKdGetDisplayMode",
        ConfigurateRequest::LinuxKdSetDisplayMode(_) => "LinuxKdSetDisplayMode",
        ConfigurateRequest::LinuxKdSignalAccept(_) => "LinuxKdSignalAccept",
        ConfigurateRequest::LinuxVtOpenQuery(_) => "LinuxVtOpenQuery",
        ConfigurateRequest::LinuxVtGetMode(_) => "LinuxVtGetMode",
        ConfigurateRequest::LinuxVtGetState(_) => "LinuxVtGetState",
        ConfigurateRequest::LinuxVtSetMode(_) => "LinuxVtSetMode",
        ConfigurateRequest::LinuxVtActivate(_) => "LinuxVtActivate",
        ConfigurateRequest::LinuxVtWaitActive(_) => "LinuxVtWaitActive",
        ConfigurateRequest::LinuxVtRelDisp(_) => "LinuxVtRelDisp",
        ConfigurateRequest::DrmVersion(_) => "DrmVersion",
        ConfigurateRequest::DrmGetUnique(_) => "DrmGetUnique",
        ConfigurateRequest::DrmGetMagic(_) => "DrmGetMagic",
        ConfigurateRequest::DrmGetCap(_) => "DrmGetCap",
        ConfigurateRequest::DrmWaitVblank(_) => "DrmWaitVblank",
        ConfigurateRequest::DrmSetUnique(_) => "DrmSetUnique",
        ConfigurateRequest::DrmAuthMagic(_) => "DrmAuthMagic",
        ConfigurateRequest::DrmSetClientCap(_) => "DrmSetClientCap",
        ConfigurateRequest::DrmSetMaster => "DrmSetMaster",
        ConfigurateRequest::DrmDropMaster => "DrmDropMaster",
        ConfigurateRequest::DrmModeGetResources(_) => "DrmModeGetResources",
        ConfigurateRequest::DrmModeGetCrtc(_) => "DrmModeGetCrtc",
        ConfigurateRequest::DrmModeSetCrtc(_) => "DrmModeSetCrtc",
        ConfigurateRequest::DrmModeCursor(_) => "DrmModeCursor",
        ConfigurateRequest::DrmModeCursor2(_) => "DrmModeCursor2",
        ConfigurateRequest::DrmModeGetGamma(_) => "DrmModeGetGamma",
        ConfigurateRequest::DrmModeSetGamma(_) => "DrmModeSetGamma",
        ConfigurateRequest::DrmModeGetEncoder(_) => "DrmModeGetEncoder",
        ConfigurateRequest::DrmModeGetConnector(_) => "DrmModeGetConnector",
        ConfigurateRequest::DrmModeGetProperty(_) => "DrmModeGetProperty",
        ConfigurateRequest::DrmModeObjGetProperties(_) => "DrmModeObjGetProperties",
        ConfigurateRequest::DrmModeGetPlaneResources(_) => "DrmModeGetPlaneResources",
        ConfigurateRequest::DrmModeGetPlane(_) => "DrmModeGetPlane",
        ConfigurateRequest::DrmModeListLessees(_) => "DrmModeListLessees",
        ConfigurateRequest::DrmModeAddFb(_) => "DrmModeAddFb",
        ConfigurateRequest::DrmModeAddFb2(_) => "DrmModeAddFb2",
        ConfigurateRequest::DrmModeRemoveFb(_) => "DrmModeRemoveFb",
        ConfigurateRequest::DrmModePageFlip(_) => "DrmModePageFlip",
        ConfigurateRequest::DrmModeDirtyFb(_) => "DrmModeDirtyFb",
        ConfigurateRequest::DrmModeCreateDumb(_) => "DrmModeCreateDumb",
        ConfigurateRequest::DrmModeMapDumb(_) => "DrmModeMapDumb",
        ConfigurateRequest::DrmModeDestroyDumb(_) => "DrmModeDestroyDumb",
        ConfigurateRequest::DrmGemClose(_) => "DrmGemClose",
        ConfigurateRequest::DrmPrimeHandleToFd(_) => "DrmPrimeHandleToFd",
        ConfigurateRequest::DrmPrimeFdToHandle(_) => "DrmPrimeFdToHandle",
        ConfigurateRequest::RawIoctl { .. } => "RawIoctl",
    }
}
