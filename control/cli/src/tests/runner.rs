use super::{config::RunTestsConfig, kernel_unit, ltp};
use anyhow::{Result, bail};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TestSelector {
    Default,
    Full,
    KernelUnit,
    Ltp,
}

impl TestSelector {
    fn parse(selector: Option<&str>) -> Result<Self> {
        match selector.unwrap_or("default") {
            "default" => Ok(Self::Default),
            "full" => Ok(Self::Full),
            "kernel_unit" | "kernel-unit" | "unit" => Ok(Self::KernelUnit),
            "ltp" => Ok(Self::Ltp),
            other => {
                bail!("unknown test selector {other}; expected default, full, kernel_unit, or ltp")
            }
        }
    }

    fn includes_kernel_unit(self) -> bool {
        matches!(self, Self::Default | Self::Full | Self::KernelUnit)
    }

    fn includes_ltp(self) -> bool {
        matches!(self, Self::Default | Self::Full | Self::Ltp)
    }
}

pub fn run_tests(repo: &Path, config: &RunTestsConfig) -> Result<i32> {
    let selector = TestSelector::parse(config.selector.as_deref())?;
    let mut passed = 0;
    let mut failed = 0;
    let mut skipped = 0;

    if selector.includes_kernel_unit() {
        if kernel_unit::run(repo, config)? {
            passed += 1;
        } else {
            failed += 1;
        }
        if failed != 0 {
            eprintln!("test summary: {passed} passed, {failed} failed, {skipped} skipped");
            return Ok(1);
        }
    }

    if selector.includes_ltp() {
        let summary = ltp::run(repo, config)?;
        passed += summary.passed;
        failed += summary.failed;
        skipped += summary.skipped;
    }

    eprintln!("test summary: {passed} passed, {failed} failed, {skipped} skipped");
    Ok(if failed == 0 { 0 } else { 1 })
}
