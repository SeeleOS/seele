use core::ptr::{read_volatile, write_volatile};

use spin::Mutex;

use crate::{
    object::{config::ConfigurateRequest, error::ObjectError, misc::ObjectResult},
    terminal::linux_kd::{LinuxConsoleState, LinuxVtMode, LinuxVtStat},
};

pub fn handle_vt_request(
    state: &Mutex<LinuxConsoleState>,
    request: &ConfigurateRequest,
) -> ObjectResult<Option<isize>> {
    match request {
        ConfigurateRequest::LinuxVtOpenQuery(ptr) => {
            if ptr.is_null() {
                return Err(ObjectError::InvalidArguments);
            }

            let vt = state.lock().active_vt as u16;
            unsafe { write_volatile(*ptr, u32::from(vt)) };
            Ok(Some(0))
        }
        ConfigurateRequest::LinuxVtGetMode(ptr) => {
            if ptr.is_null() {
                return Err(ObjectError::InvalidArguments);
            }

            let mode = state.lock().vt_mode;
            unsafe { write_volatile(*ptr, mode) };
            Ok(Some(0))
        }
        ConfigurateRequest::LinuxVtGetState(ptr) => {
            if ptr.is_null() {
                return Err(ObjectError::InvalidArguments);
            }

            let active = state.lock().active_vt as u16;
            let vt_state = LinuxVtStat {
                v_active: active,
                v_signal: 0,
                v_state: 1u16 << active,
            };
            unsafe { write_volatile(*ptr, vt_state) };
            Ok(Some(0))
        }
        ConfigurateRequest::LinuxVtSetMode(ptr) => {
            if ptr.is_null() {
                return Err(ObjectError::InvalidArguments);
            }

            let new_mode: LinuxVtMode = unsafe { read_volatile(*ptr) };
            state.lock().vt_mode = new_mode;
            Ok(Some(0))
        }
        ConfigurateRequest::LinuxVtActivate(vt)
        | ConfigurateRequest::LinuxVtWaitActive(vt) => {
            if *vt == 0 {
                return Err(ObjectError::InvalidArguments);
            }

            state.lock().active_vt = *vt;
            Ok(Some(0))
        }
        ConfigurateRequest::LinuxVtRelDisp(ack) => {
            let mut state = state.lock();

            if *ack == 0 {
                return Err(ObjectError::InvalidArguments);
            }

            // Minimal VT emulation: record that the current VT remains active.
            if *ack == 2 {
                state.active_vt = 1;
            }

            Ok(Some(0))
        }
        _ => Ok(None),
    }
}
