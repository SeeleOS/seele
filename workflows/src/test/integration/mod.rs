mod kernel_images;
mod ltp;
mod panic_handler_smoke;
mod userspace_boot;

use anyhow::Result;
use owo_colors::OwoColorize;

use crate::reporter::{TestStatus, WorkflowReporter, log_event, progress, test_event};

const FULL_TEST_FILTER: &str = "full";

trait IntegrationTest {
    fn name(&self) -> &'static str;
    fn run(&self, reporter: &dyn WorkflowReporter) -> Result<IntegrationTestResult>;
}

pub struct IntegrationTestResult {
    pub exit_code: i32,
    pub failure: Option<String>,
    pub output: String,
}

pub fn run(reporter: &dyn WorkflowReporter, test_filter: Option<&str>) -> Result<i32> {
    let tests = integration_tests(test_filter);
    let Some(tests) = tests else {
        let message = format!(
            "no integration test matched filter {:?}",
            test_filter.unwrap_or("")
        );
        log_event(reporter, "test", "stderr", &message)?;
        if !reporter.capture_subprocess_output() {
            eprintln!("{message}");
        }
        return Ok(1);
    };
    progress(reporter, "test", "integration", "running integration tests")?;
    if !reporter.capture_subprocess_output() {
        eprintln!();
        eprintln!("running {} integration tests", tests.len());
    }

    for test in tests {
        test_event(
            reporter,
            "test",
            test.name(),
            TestStatus::Running,
            "integration test started",
        )?;
        let result = test.run(reporter)?;
        if result.exit_code == 0 {
            test_event(
                reporter,
                "test",
                test.name(),
                TestStatus::Ok,
                "integration test passed",
            )?;
            if !reporter.capture_subprocess_output() {
                eprint!("test {} ... ", test.name());
                eprintln!("{}", "ok".green().bold());
            }
        } else {
            if let Some(failure) = &result.failure {
                log_event(reporter, "test", "stderr", failure)?;
            }
            if !result.output.is_empty() {
                log_event(reporter, "test", "serial", &result.output)?;
            }
            test_event(
                reporter,
                "test",
                test.name(),
                TestStatus::Failed,
                result
                    .failure
                    .as_deref()
                    .unwrap_or("integration test failed"),
            )?;
            if !reporter.capture_subprocess_output() {
                eprint!("test {} ... ", test.name());
                eprintln!("{}", "FAILED".red().bold());
                report_failure(test.name(), &result);
            }
            return Ok(result.exit_code);
        }
    }

    Ok(0)
}

fn report_failure(name: &str, result: &IntegrationTestResult) {
    eprintln!();
    eprintln!("{}", "failures:".red().bold());
    eprintln!();
    eprintln!("---- {name} stdout ----");
    if let Some(failure) = &result.failure {
        eprintln!("{failure}");
    }
    if !result.output.is_empty() {
        eprint!("{}", result.output);
        if !result.output.ends_with('\n') {
            eprintln!();
        }
    }
    eprintln!();
}

fn integration_tests(test_filter: Option<&str>) -> Option<Vec<&'static dyn IntegrationTest>> {
    let tests = [
        &kernel_images::KERNEL_IMAGES as &dyn IntegrationTest,
        &userspace_boot::USERSPACE_BOOT as &dyn IntegrationTest,
        &ltp::LTP as &dyn IntegrationTest,
        &panic_handler_smoke::PANIC_HANDLER_SMOKE as &dyn IntegrationTest,
    ];
    let filtered = match test_filter {
        Some(FULL_TEST_FILTER) => tests.into_iter().collect::<Vec<_>>(),
        Some(filter) => tests
            .into_iter()
            .filter(|test| test.name().contains(filter))
            .collect::<Vec<_>>(),
        None => vec![&ltp::LTP as &dyn IntegrationTest],
    };
    if filtered.is_empty() {
        None
    } else {
        Some(filtered)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selected_names(test_filter: Option<&str>) -> Vec<&'static str> {
        integration_tests(test_filter)
            .unwrap()
            .into_iter()
            .map(IntegrationTest::name)
            .collect()
    }

    #[test]
    fn default_integration_set_runs_only_ltp() {
        assert_eq!(selected_names(None), vec!["integration::ltp"]);
    }

    #[test]
    fn full_integration_set_runs_every_integration_test() {
        assert_eq!(
            selected_names(Some("full")),
            vec![
                "integration::kernel_images",
                "integration::userspace_boot",
                "integration::ltp",
                "integration::panic_handler_smoke",
            ]
        );
    }

    #[test]
    fn explicit_filter_still_selects_matching_test() {
        assert_eq!(
            selected_names(Some("panic_handler")),
            vec!["integration::panic_handler_smoke"]
        );
    }
}
