use alloc::vec;
use alloc::vec::Vec;
use conquer_once::spin::OnceCell;
use core::{
    arch::x86_64::{__cpuid, __cpuid_count},
    fmt, ptr, slice,
};
use x86_64::registers::{
    control::{Cr4, Cr4Flags},
    xcontrol::{XCr0, XCr0Flags},
};

const XSAVE_CPUID_BIT: u32 = 1 << 26;
const AVX_CPUID_BIT: u32 = 1 << 28;
pub const EXTENDED_STATE_ALIGNMENT: usize = 64;
pub const FXSAVE_AREA_SIZE: usize = 512;

static EXTENDED_STATE_CONFIG: OnceCell<ExtendedStateConfig> = OnceCell::uninit();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExtendedStateConfig {
    uses_xsave: bool,
    xcr0_bits: u64,
    save_area_size: usize,
}

pub struct ExtendedState {
    buffer: Vec<u8>,
    save_area_size: usize,
    xcr0_bits: u64,
    uses_xsave: bool,
}

impl ExtendedStateConfig {
    pub const fn fxsave_only() -> Self {
        Self {
            uses_xsave: false,
            xcr0_bits: 0,
            save_area_size: FXSAVE_AREA_SIZE,
        }
    }

    const fn new(uses_xsave: bool, xcr0_bits: u64, save_area_size: usize) -> Self {
        Self {
            uses_xsave,
            xcr0_bits,
            save_area_size,
        }
    }

    pub const fn uses_xsave(self) -> bool {
        self.uses_xsave
    }

    pub const fn xcr0_bits(self) -> u64 {
        self.xcr0_bits
    }

    pub const fn save_area_size(self) -> usize {
        self.save_area_size
    }
}

impl ExtendedState {
    fn new_zeroed(config: ExtendedStateConfig) -> Self {
        let buffer = vec![0; config.save_area_size + EXTENDED_STATE_ALIGNMENT - 1];
        let state = Self {
            buffer,
            save_area_size: config.save_area_size,
            xcr0_bits: config.xcr0_bits,
            uses_xsave: config.uses_xsave,
        };
        debug_assert_eq!(state.as_ptr() as usize % EXTENDED_STATE_ALIGNMENT, 0);
        state
    }

    pub fn capture_current() -> Self {
        let mut state = Self::new_zeroed(current_extended_state_config());
        state.save_current();
        state
    }

    pub fn save_current(&mut self) {
        unsafe {
            if self.uses_xsave {
                let low = self.xcr0_bits as u32;
                let high = (self.xcr0_bits >> 32) as u32;
                core::arch::asm!(
                    "xsave64 [{ptr}]",
                    ptr = in(reg) self.as_mut_ptr(),
                    in("eax") low,
                    in("edx") high,
                    options(nostack)
                );
            } else {
                core::arch::asm!(
                    "fxsave64 [{ptr}]",
                    ptr = in(reg) self.as_mut_ptr(),
                    options(nostack)
                );
            }
        }
    }

    pub fn load_current(&self) {
        unsafe {
            if self.uses_xsave {
                let low = self.xcr0_bits as u32;
                let high = (self.xcr0_bits >> 32) as u32;
                core::arch::asm!(
                    "xrstor64 [{ptr}]",
                    ptr = in(reg) self.as_ptr(),
                    in("eax") low,
                    in("edx") high,
                    options(nostack)
                );
            } else {
                core::arch::asm!(
                    "fxrstor64 [{ptr}]",
                    ptr = in(reg) self.as_ptr(),
                    options(nostack)
                );
            }
        }
    }

    pub fn as_ptr(&self) -> *const u8 {
        unsafe { self.buffer.as_ptr().add(self.aligned_offset()) }
    }

    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        let aligned_offset = self.aligned_offset();
        unsafe { self.buffer.as_mut_ptr().add(aligned_offset) }
    }

    pub fn as_bytes(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.as_ptr(), self.save_area_size) }
    }

    pub const fn uses_xsave(&self) -> bool {
        self.uses_xsave
    }

    pub const fn xcr0_bits(&self) -> u64 {
        self.xcr0_bits
    }

    pub const fn save_area_size(&self) -> usize {
        self.save_area_size
    }

    fn aligned_offset(&self) -> usize {
        let base = self.buffer.as_ptr() as usize;
        let remainder = base % EXTENDED_STATE_ALIGNMENT;
        if remainder == 0 {
            0
        } else {
            EXTENDED_STATE_ALIGNMENT - remainder
        }
    }
}

impl Clone for ExtendedState {
    fn clone(&self) -> Self {
        let mut clone = Self::new_zeroed(ExtendedStateConfig::new(
            self.uses_xsave,
            self.xcr0_bits,
            self.save_area_size,
        ));
        unsafe {
            ptr::copy_nonoverlapping(self.as_ptr(), clone.as_mut_ptr(), self.save_area_size);
        }
        clone
    }
}

impl Default for ExtendedState {
    fn default() -> Self {
        Self::capture_current()
    }
}

impl fmt::Debug for ExtendedState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExtendedState")
            .field("uses_xsave", &self.uses_xsave)
            .field("xcr0_bits", &self.xcr0_bits)
            .field("save_area_size", &self.save_area_size)
            .finish()
    }
}

pub fn initialize_current_cpu_extended_state() {
    let xsave_candidate = detect_xsave_candidate();

    if let Some(xcr0_bits) = xsave_candidate {
        let mut cr4 = Cr4::read();
        cr4.insert(Cr4Flags::OSXSAVE);
        unsafe {
            Cr4::write(cr4);
            XCr0::write(XCr0Flags::from_bits_truncate(xcr0_bits));
        }
    }

    let config = build_current_cpu_config(xsave_candidate);
    if let Some(existing) = EXTENDED_STATE_CONFIG.get() {
        assert_eq!(
            *existing, config,
            "mismatched extended state configuration across CPUs"
        );
    } else {
        EXTENDED_STATE_CONFIG.get_or_init(|| config);
    }
}

pub fn current_extended_state_config() -> ExtendedStateConfig {
    EXTENDED_STATE_CONFIG
        .get()
        .copied()
        .unwrap_or_else(ExtendedStateConfig::fxsave_only)
}

fn build_current_cpu_config(xsave_candidate: Option<u64>) -> ExtendedStateConfig {
    let Some(xcr0_bits) = xsave_candidate else {
        return ExtendedStateConfig::fxsave_only();
    };

    let xsave_leaf = __cpuid_count(0xD, 0);
    let save_area_size = (xsave_leaf.ebx as usize).max(FXSAVE_AREA_SIZE);
    ExtendedStateConfig::new(true, xcr0_bits, save_area_size)
}

fn detect_xsave_candidate() -> Option<u64> {
    let max_leaf = __cpuid(0).eax;
    if max_leaf < 0xD {
        return None;
    }

    let feature_leaf = __cpuid(1);
    if (feature_leaf.ecx & XSAVE_CPUID_BIT) == 0 || (feature_leaf.ecx & AVX_CPUID_BIT) == 0 {
        return None;
    }

    let xsave_leaf = __cpuid_count(0xD, 0);
    let supported_xcr0_bits = ((xsave_leaf.edx as u64) << 32) | xsave_leaf.eax as u64;
    let required_xcr0_bits = required_xcr0_bits();
    if supported_xcr0_bits & required_xcr0_bits != required_xcr0_bits {
        return None;
    }

    Some(required_xcr0_bits)
}

fn required_xcr0_bits() -> u64 {
    (XCr0Flags::X87 | XCr0Flags::SSE | XCr0Flags::AVX).bits()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        memory::addrspace::AddrSpace,
        thread::snapshot::{ThreadSnapshot, ThreadSnapshotType},
    };

    fn assert_same_metadata(left: &ExtendedState, right: &ExtendedState) {
        assert_eq!(left.uses_xsave(), right.uses_xsave());
        assert_eq!(left.xcr0_bits(), right.xcr0_bits());
        assert_eq!(left.save_area_size(), right.save_area_size());
    }

    crate::test!(
        extended_state_runtime_capabilities,
        "extended state runtime capabilities stay consistent",
        || {
            let config = current_extended_state_config();
            assert!(config.save_area_size() >= FXSAVE_AREA_SIZE);
            let state = ExtendedState::default();
            assert_eq!(state.as_ptr() as usize % EXTENDED_STATE_ALIGNMENT, 0);

            if config.uses_xsave() {
                assert_eq!(config.xcr0_bits(), required_xcr0_bits());
            } else {
                assert_eq!(config.xcr0_bits(), 0);
            }
        }
    );

    crate::test!(
        extended_state_clone_preserves_contents,
        "extended state clone preserves metadata and bytes",
        || {
            let state = ExtendedState::default();
            let clone = state.clone();

            assert_same_metadata(&state, &clone);
            assert_eq!(state.as_bytes(), clone.as_bytes());
            assert_eq!(state.as_ptr() as usize % EXTENDED_STATE_ALIGNMENT, 0);
            assert_eq!(clone.as_ptr() as usize % EXTENDED_STATE_ALIGNMENT, 0);
        }
    );

    crate::test!(
        thread_snapshot_preserves_extended_state_metadata,
        "thread snapshot constructors preserve extended state metadata",
        || {
            let mut addrspace = AddrSpace::default();
            let extended_state = ExtendedState::default();
            let snapshot = ThreadSnapshot::new_with_extended_state(
                0x1234,
                &mut addrspace,
                0x5678,
                ThreadSnapshotType::Thread,
                extended_state.clone(),
            );

            assert_same_metadata(&snapshot.extended_state, &extended_state);
            assert_eq!(
                snapshot.extended_state.as_bytes(),
                extended_state.as_bytes()
            );
        }
    );
}
