use crate::object::{ObjectResult, error::ObjectError};

use crate::misc::framebuffer_ioctl::{FbCmap, FbFixScreeninfo, FbVarScreeninfo};
use crate::terminal::linux_kd::{LinuxKbEntry, LinuxVtMode, LinuxVtStat};

pub enum ConfigurateRequest {
    FbGetVariableScreenInfo(*mut FbVarScreeninfo),
    FbPutVariableScreenInfo(*mut FbVarScreeninfo),
    FbGetFixedScreenInfo(*mut FbFixScreeninfo),
    FbGetColorMap(*mut FbCmap),
    FbPutColorMap(*mut FbCmap),
    FbPanDisplay(*mut FbVarScreeninfo),
    FbBlank(u32),
    LinuxTcGets(*mut LinuxTermios),
    LinuxTcSets(*const LinuxTermios),
    LinuxTcGets2(*mut LinuxTermios2),
    LinuxTcSets2(*const LinuxTermios2),
    LinuxTiocgPgrp(*mut i32),
    LinuxTiocspgrp(*const i32),
    LinuxTiocgwinsz(*mut LinuxWinsize),
    LinuxKdGetKeyboardMode(*mut u32),
    LinuxKdSetKeyboardMode(u32),
    LinuxKdGetKeyboardType(*mut u8),
    LinuxKdGetKeyboardEntry(*mut LinuxKbEntry),
    LinuxKdGetDisplayMode(*mut u32),
    LinuxKdSetDisplayMode(u32),
    LinuxVtOpenQuery(*mut u32),
    LinuxVtGetMode(*mut LinuxVtMode),
    LinuxVtGetState(*mut LinuxVtStat),
    LinuxVtSetMode(*const LinuxVtMode),
    LinuxVtActivate(u32),
    LinuxVtWaitActive(u32),
    LinuxVtRelDisp(u32),
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct LinuxTermios {
    pub c_iflag: u32,
    pub c_oflag: u32,
    pub c_cflag: u32,
    pub c_lflag: u32,
    pub c_line: u8,
    pub c_cc: [u8; 19],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct LinuxTermios2 {
    pub c_iflag: u32,
    pub c_oflag: u32,
    pub c_cflag: u32,
    pub c_lflag: u32,
    pub c_line: u8,
    pub c_cc: [u8; 19],
    pub c_ispeed: u32,
    pub c_ospeed: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct LinuxWinsize {
    pub ws_row: u16,
    pub ws_col: u16,
    pub ws_xpixel: u16,
    pub ws_ypixel: u16,
}

impl ConfigurateRequest {
    pub fn new(request: u64, ptr: u64) -> ObjectResult<Self> {
        Ok(match request {
            0x4600 => Self::FbGetVariableScreenInfo(ptr as *mut FbVarScreeninfo),
            0x4601 => Self::FbPutVariableScreenInfo(ptr as *mut FbVarScreeninfo),
            0x4602 => Self::FbGetFixedScreenInfo(ptr as *mut FbFixScreeninfo),
            0x4604 => Self::FbGetColorMap(ptr as *mut FbCmap),
            0x4605 => Self::FbPutColorMap(ptr as *mut FbCmap),
            0x4606 => Self::FbPanDisplay(ptr as *mut FbVarScreeninfo),
            0x4611 => Self::FbBlank(ptr as u32),
            0x5401 => Self::LinuxTcGets(ptr as *mut LinuxTermios),
            0x5402 | 0x5403 | 0x5404 => Self::LinuxTcSets(ptr as *const LinuxTermios),
            0x802C542A => Self::LinuxTcGets2(ptr as *mut LinuxTermios2),
            0x402C542B | 0x402C542C | 0x402C542D => Self::LinuxTcSets2(ptr as *const LinuxTermios2),
            0x540F => Self::LinuxTiocgPgrp(ptr as *mut i32),
            0x5410 => Self::LinuxTiocspgrp(ptr as *const i32),
            0x5413 => Self::LinuxTiocgwinsz(ptr as *mut LinuxWinsize),
            0x4B44 => Self::LinuxKdGetKeyboardMode(ptr as *mut u32),
            0x4B45 => Self::LinuxKdSetKeyboardMode(ptr as u32),
            0x4B33 => Self::LinuxKdGetKeyboardType(ptr as *mut u8),
            0x4B46 => Self::LinuxKdGetKeyboardEntry(ptr as *mut LinuxKbEntry),
            0x4B3B => Self::LinuxKdGetDisplayMode(ptr as *mut u32),
            0x4B3A => Self::LinuxKdSetDisplayMode(ptr as u32),
            0x5600 => Self::LinuxVtOpenQuery(ptr as *mut u32),
            0x5601 => Self::LinuxVtGetMode(ptr as *mut LinuxVtMode),
            0x5603 => Self::LinuxVtGetState(ptr as *mut LinuxVtStat),
            0x5602 => Self::LinuxVtSetMode(ptr as *const LinuxVtMode),
            0x5606 => Self::LinuxVtActivate(ptr as u32),
            0x5607 => Self::LinuxVtWaitActive(ptr as u32),
            0x5605 => Self::LinuxVtRelDisp(ptr as u32),
            _ => return Err(ObjectError::InvalidRequest),
        })
    }
}
