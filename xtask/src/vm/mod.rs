use anyhow::{Result, bail};
use clap::Args;
use xshell::{Shell, cmd};

#[derive(Debug, Args)]
pub struct VmArgs {}

pub fn vm(_args: VmArgs) -> Result<i32> {
    let sh = Shell::new()?;
    let output = cmd!(sh, "ps -efww").read()?;
    if output.is_empty() {
        bail!("ps -efww produced no output");
    }

    for line in output.lines() {
        let is_xtask_runner = line.contains("cargo run -p xtask -- run --agent")
            || line.contains("target/debug/xtask run --agent");
        let is_qemu = line.contains("qemu-system-x86_64") && line.contains("seele-os-linux");
        if is_xtask_runner || is_qemu {
            println!("{line}");
        }
    }

    Ok(0)
}
