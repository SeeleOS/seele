use crate::memory::utils::Mut;

use crate::{
    memory::user_safe,
    object::{
        config::ConfigurateRequest,
        error::ObjectError,
        misc::ObjectResult,
        tty_device::{find_unused_virtual_tty, get_active_vt, set_active_vt},
    },
    terminal::linux_kd::{LinuxConsoleState, LinuxVtMode, LinuxVtStat},
};

pub fn handle_vt_request(
    state: &Mut<LinuxConsoleState>,
    request: &ConfigurateRequest,
) -> ObjectResult<Option<isize>> {
    match request {
        ConfigurateRequest::LinuxVtOpenQuery(ptr) => {
            let vt = find_unused_virtual_tty().ok_or(ObjectError::InvalidArguments)? as u16;
            user_safe::write(*ptr, &u32::from(vt)).map_err(|_| ObjectError::BadAddress)?;
            Ok(Some(0))
        }
        ConfigurateRequest::LinuxVtGetMode(ptr) => {
            let mode = state.lock().vt_mode;
            user_safe::write(*ptr, &mode).map_err(|_| ObjectError::BadAddress)?;
            Ok(Some(0))
        }
        ConfigurateRequest::LinuxVtGetState(ptr) => {
            let active = get_active_vt() as u16;
            let vt_state = LinuxVtStat {
                v_active: active,
                v_signal: 0,
                v_state: 1u16 << active,
            };
            user_safe::write(*ptr, &vt_state).map_err(|_| ObjectError::BadAddress)?;
            Ok(Some(0))
        }
        ConfigurateRequest::LinuxVtSetMode(ptr) => {
            let new_mode: LinuxVtMode =
                user_safe::read(*ptr).map_err(|_| ObjectError::BadAddress)?;
            state.lock().vt_mode = new_mode;
            Ok(Some(0))
        }
        ConfigurateRequest::LinuxVtActivate(vt) | ConfigurateRequest::LinuxVtWaitActive(vt) => {
            if *vt == 0 || !set_active_vt(*vt) {
                return Err(ObjectError::InvalidArguments);
            }
            Ok(Some(0))
        }
        ConfigurateRequest::LinuxVtRelDisp(ack) => {
            if *ack == 0 {
                return Err(ObjectError::InvalidArguments);
            }

            // VT_RELDISP only acknowledges a release/acquire handshake. The
            // active VT itself is selected by VT_ACTIVATE/VT_WAITACTIVE, so do
            // not silently jump back to tty1 here.
            Ok(Some(0))
        }
        _ => Ok(None),
    }
}
