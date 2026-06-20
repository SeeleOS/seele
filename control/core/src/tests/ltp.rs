use super::config::RunTestsConfig;
use crate::{Artifact, ArtifactKind, JobContext, LtpCase, LtpReport, target_dir};
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::{fs, path::Path};

pub fn run(repo: &Path, config: &RunTestsConfig, context: &JobContext) -> Result<LtpReport> {
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

    parse_report(&kirk_json, config)
}

fn parse_report(path: &Path, config: &RunTestsConfig) -> Result<LtpReport> {
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
