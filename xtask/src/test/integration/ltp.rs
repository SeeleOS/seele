use super::{IntegrationTest, IntegrationTestResult};
use crate::{
    json_output::{JsonEvent, OutputMode, emit},
    run::{
        build::build_kernel,
        build_iso::create_boot_iso,
        interaction::{qmp_type_text, run_qemu_interactive_capture},
        qemu::RunOptions,
    },
};
use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{env, fs, path::Path, time::Duration};

pub const LTP: Ltp = Ltp;

const REPORT_BEGIN: &str = "__SEELE_LTP_JSON_BEGIN__";
const REPORT_END: &str = "__SEELE_LTP_JSON_END__";
const EXIT_PREFIX: &str = "__SEELE_LTP_EXIT__:";

pub struct Ltp;

impl IntegrationTest for Ltp {
    fn name(&self) -> &'static str {
        "integration::ltp"
    }

    fn run(&self, output_mode: OutputMode) -> Result<IntegrationTestResult> {
        let kernel_paths = build_kernel(output_mode)?;
        let kernel_path = kernel_paths
            .first()
            .map(Path::new)
            .context("kernel executable missing")?;
        let iso_path = create_boot_iso(kernel_path)?;
        let mut command_sent = false;
        let options = RunOptions::for_agent_run_without_timeout();
        let timeout = ltp_timeout();
        let result = run_qemu_interactive_capture(&iso_path, &options, timeout, |output| {
            if !command_sent && guest_ready(output) {
                command_sent = true;
                if let Err(err) = qmp_type_text(&qmp_socket_path(), "root\nseele-run-ltp\n") {
                    eprintln!("failed to send LTP command through QMP: {err:?}");
                }
            }
            output.contains(REPORT_END) && output.contains(EXIT_PREFIX)
        })?;
        fs::remove_file(&iso_path)
            .with_context(|| format!("failed to remove ISO image {}", iso_path.display()))?;

        if result.exit_code != 0 {
            return Ok(IntegrationTestResult {
                exit_code: result.exit_code,
                failure: result.failure,
                output: result.serial_output,
            });
        }

        let exit_code = parse_ltp_exit_code(&result.serial_output).unwrap_or(1);
        let report = extract_ltp_report(&result.serial_output);
        let failure = if let Some(report) = &report {
            let summary = report.summary();
            (summary.failed > 0 || summary.broken > 0).then(|| {
                format!(
                    "LTP reported {} failed and {} broken results",
                    summary.failed, summary.broken
                )
            })
        } else {
            Some("LTP JSON report was not observed on serial output".to_string())
        };
        let failure = failure.or_else(|| {
            if exit_code != 0 {
                Some(format!("kirk exited with status {exit_code}"))
            } else {
                None
            }
        });
        if let Some(report) = report {
            emit_ltp_json_events(output_mode, &report)?;
        }

        Ok(IntegrationTestResult {
            exit_code: if failure.is_some() { 1 } else { 0 },
            failure,
            output: result.serial_output,
        })
    }
}

fn qmp_socket_path() -> std::path::PathBuf {
    env::var_os("SEELE_QMP_SOCKET")
        .map(Into::into)
        .unwrap_or_else(|| "/tmp/seele-agent-qmp.sock".into())
}

fn guest_ready(output: &str) -> bool {
    output.lines().any(|line| {
        let line = line.trim_end_matches('\r').trim_end();
        line.ends_with("Seele login:")
            || ((line.contains("bash-") || line.contains("root@")) && line.contains("# "))
    })
}

fn ltp_timeout() -> Duration {
    env::var("SEELE_LTP_TIMEOUT")
        .ok()
        .and_then(|value| parse_duration(&value))
        .unwrap_or_else(|| Duration::from_secs(180))
}

fn parse_duration(value: &str) -> Option<Duration> {
    let value = value.trim();
    if let Some(seconds) = value.strip_suffix('s') {
        return seconds.parse::<u64>().ok().map(Duration::from_secs);
    }
    if let Some(minutes) = value.strip_suffix('m') {
        return minutes
            .parse::<u64>()
            .ok()
            .map(|minutes| Duration::from_secs(minutes.saturating_mul(60)));
    }
    value.parse::<u64>().ok().map(Duration::from_secs)
}

fn parse_ltp_exit_code(output: &str) -> Option<i32> {
    output
        .lines()
        .find_map(|line| line.trim().strip_prefix(EXIT_PREFIX)?.parse().ok())
}

fn extract_ltp_report(output: &str) -> Option<LtpReport> {
    let start = output.find(REPORT_BEGIN)? + REPORT_BEGIN.len();
    let end = output[start..].find(REPORT_END)? + start;
    let json = output[start..end].trim();
    serde_json::from_str(json).ok()
}

fn emit_ltp_json_events(output_mode: OutputMode, report: &LtpReport) -> Result<()> {
    if !output_mode.is_json() {
        return Ok(());
    }

    let summary = report.summary();
    for result in &report.results {
        let name = result.test.name.as_deref().unwrap_or("ltp::unknown");
        let status = result_status(result);
        emit(&JsonEvent::test(
            "test",
            &format!("ltp::{name}"),
            status,
            status,
        ))?;
        if matches!(status, "failed" | "broken")
            && let Some(stdout) = &result.stdout
            && !stdout.is_empty()
        {
            emit(&JsonEvent::log("test", "ltp", stdout))?;
        }
    }

    emit(&JsonEvent::metadata(
        "test",
        json!({
            "ltp": {
                "results": report.results.len(),
                "passed": summary.passed,
                "failed": summary.failed,
                "broken": summary.broken,
                "skipped": summary.skipped,
                "warnings": summary.warnings,
            }
        }),
    ))?;
    Ok(())
}

fn result_status(result: &LtpTestResult) -> &'static str {
    if result.failed > 0 {
        "failed"
    } else if result.broken > 0 {
        "broken"
    } else if result.skipped > 0 {
        "skipped"
    } else {
        "ok"
    }
}

#[derive(Debug, Default)]
struct LtpSummary {
    passed: u64,
    failed: u64,
    broken: u64,
    skipped: u64,
    warnings: u64,
}

impl LtpReport {
    fn summary(&self) -> LtpSummary {
        let mut summary = LtpSummary::default();
        for result in &self.results {
            summary.passed += result.passed;
            summary.failed += result.failed;
            summary.broken += result.broken;
            summary.skipped += result.skipped;
            summary.warnings += result.warnings;
        }
        summary
    }
}

#[derive(Debug, Deserialize)]
struct LtpReport {
    #[serde(default)]
    results: Vec<LtpTestResult>,
}

#[derive(Debug, Default, Deserialize)]
struct LtpTestResult {
    #[serde(default)]
    test: LtpTest,
    #[serde(default)]
    passed: u64,
    #[serde(default)]
    failed: u64,
    #[serde(default)]
    broken: u64,
    #[serde(default)]
    skipped: u64,
    #[serde(default)]
    warnings: u64,
    #[serde(default)]
    stdout: Option<String>,
    #[serde(flatten)]
    _extra: Value,
}

#[derive(Debug, Default, Deserialize)]
struct LtpTest {
    #[serde(default)]
    name: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_report_between_serial_markers() {
        let output = format!(
            "noise\n{REPORT_BEGIN}\n{{\"results\":[{{\"test\":{{\"name\":\"getpid01\"}},\"passed\":1}}]}}\n{REPORT_END}\n"
        );

        let report = extract_ltp_report(&output).unwrap();

        assert_eq!(report.results.len(), 1);
        assert_eq!(report.results[0].test.name.as_deref(), Some("getpid01"));
        assert_eq!(result_status(&report.results[0]), "ok");
    }

    #[test]
    fn maps_ltp_result_status_by_linux_outcome_counts() {
        let failed = LtpTestResult {
            failed: 1,
            ..Default::default()
        };
        let broken = LtpTestResult {
            broken: 1,
            ..Default::default()
        };
        let skipped = LtpTestResult {
            skipped: 1,
            ..Default::default()
        };

        assert_eq!(result_status(&failed), "failed");
        assert_eq!(result_status(&broken), "broken");
        assert_eq!(result_status(&skipped), "skipped");
    }
}
