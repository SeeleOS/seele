use crate::{
    Artifact, ArtifactKind, Event, JobContext, KernelUnitReport, LtpCase, LtpReport, Report,
    TestEvent, TestSelector, process::ProcessRunner, qemu, target_dir,
};
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug, Clone, Default)]
pub struct RunTestsConfig {
    pub selector: Option<String>,
    pub ltp_suite: Option<String>,
    pub ltp_pattern: Option<String>,
}

pub fn run_tests(repo: &Path, config: &RunTestsConfig, context: &JobContext) -> Result<i32> {
    let selector = TestSelector::parse(config.selector.as_deref());
    context.event(Event::Test(TestEvent::Started {
        selector: selector.clone(),
    }));

    let mut failed = 0;
    let mut passed = 0;
    let mut skipped = 0;

    if matches!(
        selector,
        TestSelector::Default | TestSelector::Full | TestSelector::KernelUnit
    ) {
        let report = run_kernel_unit(repo, context)?;
        if report.passed {
            passed += 1;
        } else {
            failed += 1;
        }
        context.report(Report::KernelUnit(report));
    }

    if matches!(
        selector,
        TestSelector::Default | TestSelector::Full | TestSelector::Ltp
    ) {
        let report = run_ltp(repo, config, context)?;
        failed += report.failed;
        passed += report.passed;
        skipped += report.skipped;
        context.report(Report::Ltp(report));
    }

    if let TestSelector::Integration(name) = selector {
        let report = run_integration(repo, &name, context)?;
        if report.booted && report.qmp_connectable {
            passed += 1;
        } else {
            failed += 1;
        }
        context.report(Report::VmSmoke(report));
    }

    context.event(Event::Test(TestEvent::Finished {
        passed,
        failed,
        skipped,
    }));
    Ok(if failed == 0 { 0 } else { 1 })
}

fn run_kernel_unit(repo: &Path, context: &JobContext) -> Result<KernelUnitReport> {
    let artifact_dir = target_dir(repo)
        .join("control-artifacts")
        .join("tests")
        .join("kernel-unit");
    let runner = ProcessRunner::new(&artifact_dir)?;
    let result = runner.run(
        context,
        "kernel_unit_cargo_test",
        Command::new("cargo").current_dir(repo).args([
            "test",
            "-p",
            "kernel",
            "--target",
            "x86_64-unknown-none",
            "--lib",
            "-Z",
            "build-std=core,alloc",
            "-Z",
            "panic-abort-tests",
            "--no-run",
            "--message-format=json-render-diagnostics",
        ]),
    )?;
    context.artifact(Artifact {
        kind: ArtifactKind::CargoJson,
        path: result.stdout_artifact.clone(),
        description: "kernel unit cargo JSON messages".to_string(),
    });
    let executable = find_last_executable(&result.stdout_artifact)?;
    Ok(KernelUnitReport {
        executable,
        iso: None,
        passed: result.exit_code == 0,
        serial_log: None,
        stdout: String::new(),
        stderr: String::new(),
    })
}

fn run_ltp(repo: &Path, config: &RunTestsConfig, context: &JobContext) -> Result<LtpReport> {
    let artifact_dir = target_dir(repo)
        .join("control-artifacts")
        .join("tests")
        .join("ltp");
    fs::create_dir_all(&artifact_dir)
        .with_context(|| format!("failed to create {}", artifact_dir.display()))?;
    let kirk_json = artifact_dir.join("kirk-results.json");
    context.artifact(Artifact {
        kind: ArtifactKind::KirkJson,
        path: kirk_json.clone(),
        description: "kirk JSON report".to_string(),
    });

    if !kirk_json.exists() {
        bail!(
            "LTP execution is not implemented in the new control plane yet; expected kirk JSON artifact at {}",
            kirk_json.display()
        );
    }

    parse_ltp_report(&kirk_json, config)
}

fn run_integration(
    repo: &Path,
    _name: &str,
    _context: &JobContext,
) -> Result<crate::VmSmokeReport> {
    Ok(qemu::vm_smoke_report(repo))
}

fn find_last_executable(path: &Path) -> Result<PathBuf> {
    let content =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let mut executable = None;
    for line in content.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if value.get("reason").and_then(Value::as_str) == Some("compiler-artifact")
            && let Some(path) = value.get("executable").and_then(Value::as_str)
        {
            executable = Some(PathBuf::from(path));
        }
    }
    Ok(executable.unwrap_or_default())
}

fn parse_ltp_report(path: &Path, config: &RunTestsConfig) -> Result<LtpReport> {
    let value: Value = serde_json::from_slice(
        &fs::read(path).with_context(|| format!("failed to read {}", path.display()))?,
    )
    .with_context(|| format!("failed to parse {}", path.display()))?;

    let cases = value
        .get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|case| LtpCase {
            name: case
                .get("test")
                .or_else(|| case.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string(),
            status: case
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string(),
            duration_ms: case.get("duration_ms").and_then(Value::as_u64),
        })
        .collect::<Vec<_>>();

    let summary = value.get("summary");
    Ok(LtpReport {
        suite: config.ltp_suite.clone(),
        pattern: config.ltp_pattern.clone(),
        passed: summary
            .and_then(|summary| summary.get("passed"))
            .and_then(Value::as_u64)
            .unwrap_or_else(|| count_status(&cases, "pass")),
        failed: summary
            .and_then(|summary| summary.get("failed"))
            .and_then(Value::as_u64)
            .unwrap_or_else(|| count_status(&cases, "fail")),
        skipped: summary
            .and_then(|summary| summary.get("skipped"))
            .and_then(Value::as_u64)
            .unwrap_or_else(|| count_status(&cases, "skip")),
        cases,
        artifact: Some(path.to_path_buf()),
        stdout: String::new(),
        stderr: String::new(),
    })
}

fn count_status(cases: &[LtpCase], needle: &str) -> u64 {
    cases
        .iter()
        .filter(|case| case.status.to_ascii_lowercase().contains(needle))
        .count() as u64
}
