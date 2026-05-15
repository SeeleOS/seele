use anyhow::{Result, bail};
use std::process::Command;

pub fn ps() -> Result<i32> {
    let output = Command::new("ps").args(["-efww"]).output()?;
    if !output.status.success() {
        bail!("ps -efww failed with status {}", output.status);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let is_xtask_runner = line.contains("cargo run -p xtask -- run --agent")
            || line.contains("target/debug/xtask run --agent");
        let is_qemu = line.contains("qemu-system-x86_64") && line.contains("seele-os-linux");
        if is_xtask_runner || is_qemu {
            println!("{line}");
        }
    }

    Ok(0)
}
