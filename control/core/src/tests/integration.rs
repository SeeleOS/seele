use crate::{JobContext, VmSmokeReport, qemu};
use anyhow::Result;
use std::path::Path;

pub fn run(repo: &Path, _name: &str, _context: &JobContext) -> Result<VmSmokeReport> {
    Ok(qemu::vm_smoke_report(repo))
}
