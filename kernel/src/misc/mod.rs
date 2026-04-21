use x86_64::instructions::hlt;

pub mod auxv;
pub mod c_types;
pub mod debug_exit;

pub mod cpu_core_context;
pub mod devices;
pub mod error;
pub mod fb_object;
pub mod framebuffer;
pub mod framebuffer_ioctl;
pub mod gdt;
pub mod logging;
pub mod mouse;
pub mod others;
pub mod panic;
pub mod reboot;
pub mod serial_print;
pub mod signal;
pub mod snapshot;
pub mod stack_builder;
pub mod systemd_perf;
pub mod testing;
pub mod time;
pub mod timer;
pub mod tss;
pub mod utsname;

pub use cpu_core_context::*;

pub fn hlt_loop() -> ! {
    loop {
        hlt();
    }
}

#[cfg(target_arch = "x86_64")]
pub fn get_cycles() -> u64 {
    let low: u32;
    let high: u32;
    unsafe {
        core::arch::asm!(
            "rdtsc",
            out("eax") low,
            out("edx") high,
            options(nomem, nostack, preserves_flags)
        );
    }
    ((high as u64) << 32) | low as u64
}

#[macro_export]
macro_rules! read_addr {
    ($addr: expr, $type: ty) => {
        *($addr as *mut $type)
    };
}

#[macro_export]
macro_rules! write_addr {
    ($addr: expr, $type: ty, $value: expr) => {
        read_addr!($addr, $type) = $value
    };
}

#[macro_export]
macro_rules! read_port {
    ($port: expr) => {
        Port::new($port).read()
    };
}

#[macro_export]
macro_rules! write_port {
    ($port: expr,$value: expr) => {
        Port::new($port).write($value)
    };
}
