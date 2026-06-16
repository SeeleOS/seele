use anyhow::{Context, Result, bail};
use std::process::Command;

pub fn run(command: &mut Command) -> Result<()> {
    println!("running: {command:?}");
    let status = command
        .status()
        .with_context(|| format!("failed to spawn {command:?}"))?;
    if !status.success() {
        bail!("{command:?} exited with {status}");
    }
    Ok(())
}
