use super::config::RunTestsConfig;
use crate::{
    Artifact, ArtifactKind, JobContext, LtpCase, LtpReport,
    build::{KernelBuildMode, KernelBuildOptions, build_kernel},
    target_dir,
    vm::{BootConfig, VmConfig, create_boot_iso, run_iso_capture},
};
use anyhow::{Context, Result};
use serde_json::Value;
use std::{fs, path::Path, time::Duration};

const REPORT_BEGIN: &str = "__SEELE_LTP_JSON_BEGIN__";
const REPORT_END: &str = "__SEELE_LTP_JSON_END__";
const EXIT_PREFIX: &str = "__SEELE_LTP_EXIT__:";

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

    let kernels = build_kernel(
        repo,
        KernelBuildMode::Run,
        KernelBuildOptions {
            enable_profiling: config.enable_profiling,
        },
        context,
    )?;
    let iso = create_boot_iso(
        repo,
        &kernels[0],
        &BootConfig {
            init: Some("/usr/local/bin/seele-run-ltp".to_string()),
            ltp_suite: config.ltp_suite.clone(),
            ltp_pattern: config.ltp_pattern.clone(),
        },
        context,
    )?;
    let result = run_iso_capture(
        repo,
        &iso,
        VmConfig::for_repo(repo),
        Some(Duration::from_secs(45 * 60)),
        Some(ltp_report_observed),
        context,
    )?;

    let report_json = extract_ltp_report(&result.serial_output).with_context(|| {
        format!(
            "LTP JSON report was not observed in {}",
            result.serial_log.display()
        )
    })?;
    fs::write(&kirk_json, &report_json)
        .with_context(|| format!("failed to write {}", kirk_json.display()))?;
    let mut report = parse_report(&kirk_json, config)?;
    report.artifact = Some(kirk_json);
    let exit_code = parse_ltp_exit_code(&result.serial_output).unwrap_or(result.exit_code);
    if exit_code != 0 || report.failed > 0 {
        report.stderr = format!(
            "LTP failed: kirk exit {exit_code}, failed cases {}",
            report.failed
        );
    }
    Ok(report)
}

fn ltp_report_observed(output: &str) -> bool {
    output.contains(REPORT_END) && output.contains(EXIT_PREFIX)
}

fn parse_ltp_exit_code(output: &str) -> Option<i32> {
    output
        .lines()
        .find_map(|line| line.trim().strip_prefix(EXIT_PREFIX)?.parse().ok())
}

fn extract_ltp_report(output: &str) -> Option<String> {
    let start = output.find(REPORT_BEGIN)? + REPORT_BEGIN.len();
    let end = output[start..].find(REPORT_END)? + start;
    Some(strip_ansi_escape_sequences(output[start..end].trim()))
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
                .or_else(|| case.pointer("/test/result"))
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string(),
            duration_ms: case.get("duration_ms").and_then(Value::as_u64),
            log: case_log(case),
        })
        .collect::<Vec<_>>();

    let summary = value.get("summary").or_else(|| value.get("stats"));
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

fn case_log(case: &Value) -> String {
    case.get("log")
        .or_else(|| case.pointer("/test/log"))
        .or_else(|| case.get("stdout"))
        .or_else(|| case.pointer("/test/stdout"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn strip_ansi_escape_sequences(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\x1b' {
            output.push(ch);
            continue;
        }
        match chars.peek() {
            Some('[') => {
                chars.next();
                for ch in chars.by_ref() {
                    if ('@'..='~').contains(&ch) {
                        break;
                    }
                }
            }
            Some(']') => {
                chars.next();
                while let Some(ch) = chars.next() {
                    if ch == '\x07' {
                        break;
                    }
                    if ch == '\x1b' && matches!(chars.peek(), Some('\\')) {
                        chars.next();
                        break;
                    }
                }
            }
            Some('%' | '(' | ')' | '*' | '+' | '-' | '.' | '/') => {
                chars.next();
                chars.next();
            }
            Some(_) => {
                chars.next();
            }
            None => {}
        }
    }
    output
}
