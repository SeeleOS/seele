use super::{IntegrationTest, IntegrationTestResult};
use crate::{
    reporter::{TestStatus, WorkflowReporter, log_event, metadata_event, test_event},
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

    fn run(&self, reporter: &dyn WorkflowReporter) -> Result<IntegrationTestResult> {
        let kernel_paths = build_kernel(reporter)?;
        let kernel_path = kernel_paths
            .first()
            .map(Path::new)
            .context("kernel executable missing")?;
        let iso_path = create_boot_iso(kernel_path)?;
        let mut login_state = LoginState::AwaitLogin;
        let options = RunOptions::for_agent_run_without_timeout();
        let timeout = ltp_timeout();
        let result = run_qemu_interactive_capture(&iso_path, &options, timeout, |output| {
            if let Err(err) = advance_login(&mut login_state, output) {
                eprintln!("failed to drive LTP login through QMP: {err:?}");
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
            emit_ltp_json_events(reporter, &report)?;
        }

        Ok(IntegrationTestResult {
            exit_code: if failure.is_some() { 1 } else { 0 },
            failure,
            output: ltp_failure_output(&result.serial_output),
        })
    }
}

fn qmp_socket_path() -> std::path::PathBuf {
    env::var_os("SEELE_QMP_SOCKET")
        .map(Into::into)
        .unwrap_or_else(|| "/tmp/seele-agent-qmp.sock".into())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoginState {
    AwaitLogin,
    AwaitPasswordOrShell,
    AwaitShell,
    CommandSent,
}

fn advance_login(state: &mut LoginState, output: &str) -> Result<()> {
    match *state {
        LoginState::AwaitLogin if login_prompt_observed(output) => {
            qmp_type_text(&qmp_socket_path(), "root\n")?;
            *state = LoginState::AwaitPasswordOrShell;
        }
        LoginState::AwaitPasswordOrShell if password_prompt_observed(output) => {
            qmp_type_text(&qmp_socket_path(), "\n")?;
            *state = LoginState::AwaitShell;
        }
        LoginState::AwaitPasswordOrShell | LoginState::AwaitShell
            if shell_prompt_observed(output) =>
        {
            qmp_type_text(&qmp_socket_path(), "seele-run-ltp\n")?;
            *state = LoginState::CommandSent;
        }
        _ => {}
    }
    Ok(())
}

fn login_prompt_observed(output: &str) -> bool {
    output.lines().any(|line| {
        let line = line.trim_end_matches('\r').trim_end();
        line.ends_with("Seele login:")
    })
}

fn password_prompt_observed(output: &str) -> bool {
    output.lines().any(|line| {
        let line = line.trim_end_matches('\r').trim_end();
        line.ends_with("Password:")
    })
}

fn shell_prompt_observed(output: &str) -> bool {
    output.lines().any(|line| {
        let line = line.trim_end_matches('\r');
        (line.contains("bash-") || line.contains("root@")) && line.contains("# ")
    })
}

fn ltp_timeout() -> Duration {
    env::var("SEELE_LTP_TIMEOUT")
        .ok()
        .and_then(|value| parse_duration(&value))
        .unwrap_or_else(|| Duration::from_secs(45 * 60))
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
    let json = strip_ansi_escape_sequences(output[start..end].trim());
    serde_json::from_str(&json).ok()
}

fn ltp_failure_output(output: &str) -> String {
    let Some(begin) = output.find(REPORT_BEGIN) else {
        return output.to_string();
    };
    let Some(exit_start) = output[begin..]
        .find(EXIT_PREFIX)
        .map(|offset| begin + offset)
    else {
        return output.to_string();
    };

    let mut trimmed = String::new();
    let before_report = output[..begin].trim();
    if !before_report.is_empty() {
        trimmed.push_str(before_report);
        trimmed.push('\n');
    }

    let exit_line = output[exit_start..]
        .lines()
        .next()
        .unwrap_or("")
        .trim_end_matches('\r');
    if !exit_line.is_empty() {
        trimmed.push_str(exit_line);
        trimmed.push('\n');
    }

    trimmed
}

fn strip_ansi_escape_sequences(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch != '\x1b' {
            output.push(ch);
            continue;
        }

        skip_ansi_sequence(&mut chars);
    }

    output
}

fn skip_ansi_sequence(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
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

fn emit_ltp_json_events(reporter: &dyn WorkflowReporter, report: &LtpReport) -> Result<()> {
    let summary = report.summary();
    for result in &report.results {
        let name = result.name().unwrap_or("ltp::unknown");
        let status = result_status(result);
        test_event(
            reporter,
            "test",
            &format!("ltp::{name}"),
            status,
            status.as_str(),
        )?;
        if matches!(status, TestStatus::Failed | TestStatus::Broken)
            && let Some(log) = result.log()
            && !log.is_empty()
        {
            log_event(reporter, "test", "ltp", log)?;
        }
    }

    metadata_event(
        reporter,
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
    )?;
    Ok(())
}

fn result_status(result: &LtpTestResult) -> TestStatus {
    if result.failed() > 0 {
        TestStatus::Failed
    } else if result.broken() > 0 {
        TestStatus::Broken
    } else if result.skipped() > 0 {
        TestStatus::Skipped
    } else {
        TestStatus::Ok
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
struct LtpSummary {
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
}

impl LtpReport {
    fn summary(&self) -> LtpSummary {
        if let Some(stats) = self.stats {
            return stats;
        }

        let mut summary = LtpSummary::default();
        for result in &self.results {
            summary.passed += result.passed();
            summary.failed += result.failed();
            summary.broken += result.broken();
            summary.skipped += result.skipped();
            summary.warnings += result.warnings();
        }
        summary
    }
}

#[derive(Debug, Deserialize)]
struct LtpReport {
    #[serde(default)]
    results: Vec<LtpTestResult>,
    #[serde(default)]
    stats: Option<LtpSummary>,
}

#[derive(Debug, Default, Deserialize)]
struct LtpTestResult {
    #[serde(default)]
    test_fqn: Option<String>,
    #[serde(default)]
    test: LtpTest,
    #[serde(default)]
    passed: Option<u64>,
    #[serde(default)]
    failed: Option<u64>,
    #[serde(default)]
    broken: Option<u64>,
    #[serde(default)]
    skipped: Option<u64>,
    #[serde(default)]
    warnings: Option<u64>,
    #[serde(default)]
    stdout: Option<String>,
    #[serde(flatten)]
    _extra: Value,
}

impl LtpTestResult {
    fn name(&self) -> Option<&str> {
        self.test_fqn
            .as_deref()
            .or(self.test.name.as_deref())
            .or(self.test.command.as_deref())
    }

    fn passed(&self) -> u64 {
        self.passed.or(self.test.passed).unwrap_or_default()
    }

    fn failed(&self) -> u64 {
        self.failed.or(self.test.failed).unwrap_or_default()
    }

    fn broken(&self) -> u64 {
        self.broken.or(self.test.broken).unwrap_or_default()
    }

    fn skipped(&self) -> u64 {
        self.skipped.or(self.test.skipped).unwrap_or_default()
    }

    fn warnings(&self) -> u64 {
        self.warnings.or(self.test.warnings).unwrap_or_default()
    }

    fn log(&self) -> Option<&str> {
        self.stdout.as_deref().or(self.test.log.as_deref())
    }
}

#[derive(Debug, Default, Deserialize)]
struct LtpTest {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    log: Option<String>,
    #[serde(default)]
    passed: Option<u64>,
    #[serde(default)]
    failed: Option<u64>,
    #[serde(default)]
    broken: Option<u64>,
    #[serde(default)]
    skipped: Option<u64>,
    #[serde(default)]
    warnings: Option<u64>,
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
        assert_eq!(result_status(&report.results[0]), TestStatus::Ok);
    }

    #[test]
    fn extracts_report_with_ansi_colored_json() {
        let output = format!(
            "noise\n{REPORT_BEGIN}\n{{\n    \x1b[1m\"results\"\x1b[0m: [{{\x1b[1m\"test\"\x1b[0m: {{\x1b[1m\"name\"\x1b[0m: \x1b[32m\"getpid02\"\x1b[0m}}, \x1b[1m\"failed\"\x1b[0m: \x1b[33m1\x1b[0m}}]\n}}\n{REPORT_END}\n"
        );

        let report = extract_ltp_report(&output).unwrap();

        assert_eq!(report.results.len(), 1);
        assert_eq!(report.results[0].test.name.as_deref(), Some("getpid02"));
        assert_eq!(report.results[0].failed(), 1);
    }

    #[test]
    fn reads_kirk_counts_from_nested_test_object() {
        let output = format!(
            "noise\n{REPORT_BEGIN}\n{{\"results\":[{{\"test_fqn\":\"brk01\",\"test\":{{\"command\":\"brk01\",\"passed\":2,\"broken\":1,\"warnings\":3}}}}]}}\n{REPORT_END}\n"
        );

        let report = extract_ltp_report(&output).unwrap();
        let summary = report.summary();

        assert_eq!(report.results[0].name(), Some("brk01"));
        assert_eq!(result_status(&report.results[0]), TestStatus::Broken);
        assert_eq!(summary.passed, 2);
        assert_eq!(summary.broken, 1);
        assert_eq!(summary.warnings, 3);
    }

    #[test]
    fn prefers_kirk_top_level_stats() {
        let output = format!(
            "noise\n{REPORT_BEGIN}\n{{\"results\":[{{\"test_fqn\":\"waitpid01\",\"status\":\"fail\",\"test\":{{\"command\":\"waitpid01\",\"log\":\"failure details\"}}}}],\"stats\":{{\"passed\":10,\"failed\":1,\"broken\":2,\"skipped\":3,\"warnings\":4}}}}\n{REPORT_END}\n"
        );

        let report = extract_ltp_report(&output).unwrap();
        let summary = report.summary();

        assert_eq!(summary.passed, 10);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.broken, 2);
        assert_eq!(summary.skipped, 3);
        assert_eq!(summary.warnings, 4);
        assert_eq!(report.results[0].log(), Some("failure details"));
    }

    #[test]
    fn strips_non_csi_ansi_sequences_from_report_json() {
        let output = format!(
            "noise\n{REPORT_BEGIN}\n\x1b]0;title\x07{{\x1b(B\x1b[1m\"results\"\x1b[0m:[]}}\n{REPORT_END}\n"
        );

        let report = extract_ltp_report(&output).unwrap();

        assert!(report.results.is_empty());
    }

    #[test]
    fn maps_ltp_result_status_by_linux_outcome_counts() {
        let failed = LtpTestResult {
            failed: Some(1),
            ..Default::default()
        };
        let broken = LtpTestResult {
            broken: Some(1),
            ..Default::default()
        };
        let skipped = LtpTestResult {
            skipped: Some(1),
            ..Default::default()
        };

        assert_eq!(result_status(&failed), TestStatus::Failed);
        assert_eq!(result_status(&broken), TestStatus::Broken);
        assert_eq!(result_status(&skipped), TestStatus::Skipped);
    }

    #[test]
    fn observes_login_prompt_without_treating_it_as_shell() {
        let output = "\r\nArch Linux 0.0.1 (tty1)\r\n\r\nSeele login: ";

        assert!(login_prompt_observed(output));
        assert!(!shell_prompt_observed(output));
    }

    #[test]
    fn observes_shell_prompt_after_login() {
        assert!(shell_prompt_observed("root@Seele ~ # "));
        assert!(shell_prompt_observed("bash-5.2# "));
        assert!(!login_prompt_observed("root@Seele ~ # "));
    }

    #[test]
    fn trims_ltp_failure_output_to_non_json_serial_context() {
        let output = format!(
            "login\n{REPORT_BEGIN}\n{{\"results\":[{{\"test\":{{\"log\":\"big\"}}}}]}}\n{REPORT_END}\n{EXIT_PREFIX}1\n"
        );

        let trimmed = ltp_failure_output(&output);

        assert!(trimmed.contains("login"));
        assert!(trimmed.contains(EXIT_PREFIX));
        assert!(!trimmed.contains("\"results\""));
    }
}
