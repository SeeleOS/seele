use super::config::RunTestsConfig;
use crate::{
    build::{KernelBuildMode, KernelBuildOptions, build_kernel, shell_for_repo},
    vm::{BootConfig, SerialConfig, VmConfig, create_boot_iso, run_iso_capture},
};
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::{path::Path, time::Duration};

const REPORT_BEGIN: &str = "__SEELE_LTP_JSON_BEGIN__";
const REPORT_END: &str = "__SEELE_LTP_JSON_END__";
const EXIT_PREFIX: &str = "__SEELE_LTP_EXIT__:";

#[derive(Debug, Clone, Copy)]
pub struct LtpSummary {
    pub passed: u64,
    pub failed: u64,
    pub skipped: u64,
}

pub fn run(repo: &Path, config: &RunTestsConfig) -> Result<LtpSummary> {
    eprintln!("==> running LTP");
    let sh = shell_for_repo(repo)?;
    let kernels = build_kernel(&sh, KernelBuildMode::Run, KernelBuildOptions::default())?;
    let iso = create_boot_iso(
        &sh,
        repo,
        &kernels[0],
        &BootConfig {
            init: Some("/usr/local/bin/seele-run-ltp".to_string()),
            ltp_suite: config.ltp_suite.clone(),
            ltp_pattern: config.ltp_pattern.clone(),
        },
    )?;
    let result = run_iso_capture(
        &sh,
        repo,
        &iso,
        test_vm_config(repo),
        Some(Duration::from_secs(45 * 60)),
        Some(ltp_report_observed),
    )?;

    let report_json = extract_ltp_report(&result.serial_output).with_context(|| {
        format!(
            "LTP JSON report was not observed in {}",
            result.serial_log.display()
        )
    })?;
    let summary = parse_report(&report_json)?;
    let exit_code = parse_ltp_exit_code(&result.serial_output).unwrap_or(result.exit_code);
    eprintln!(
        "LTP summary: {} passed, {} failed, {} skipped",
        summary.passed, summary.failed, summary.skipped
    );
    eprintln!("LTP serial log: {}", result.serial_log.display());
    if exit_code != 0 || summary.failed > 0 {
        bail!(
            "LTP failed: kirk exit {exit_code}, failed cases {}",
            summary.failed
        );
    }
    Ok(summary)
}

fn test_vm_config(repo: &Path) -> VmConfig {
    let mut config = VmConfig::for_repo(repo);
    config.display = false;
    config.serial = SerialConfig::File;
    config
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

fn parse_report(report_json: &str) -> Result<LtpSummary> {
    let value: Value = serde_json::from_str(report_json).context("failed to parse LTP JSON")?;
    let cases = value
        .get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(case_status)
        .collect::<Vec<_>>();
    let summary = value.get("summary").or_else(|| value.get("stats"));
    Ok(LtpSummary {
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
    })
}

fn case_status(case: &Value) -> String {
    case.get("status")
        .or_else(|| case.pointer("/test/result"))
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string()
}

fn count_status(cases: &[String], needle: &str) -> u64 {
    cases
        .iter()
        .filter(|status| status.to_ascii_lowercase().contains(needle))
        .count() as u64
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
