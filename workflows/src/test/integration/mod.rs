mod kernel_images;
mod ltp;
mod panic_handler_smoke;
mod userspace_boot;

use anyhow::Result;
use owo_colors::OwoColorize;

use crate::reporter::{TestStatus, WorkflowReporter, log_event, progress, test_event};

const FULL_TEST_FILTER: &str = "full";
type IntegrationRun = fn(&dyn WorkflowReporter) -> Result<i32>;

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

    for (name, run) in tests {
        test_event(reporter, "test", name, TestStatus::Running, "started")?;
        let exit_code = run(reporter)?;
        if exit_code == 0 {
            test_event(reporter, "test", name, TestStatus::Ok, "passed")?;
            if !reporter.capture_subprocess_output() {
                eprint!("test {name} ... ");
                eprintln!("{}", "ok".green().bold());
            }
        } else {
            test_event(
                reporter,
                "test",
                name,
                TestStatus::Failed,
                "integration test failed",
            )?;
            if !reporter.capture_subprocess_output() {
                eprint!("test {name} ... ");
                eprintln!("{}", "FAILED".red().bold());
            }
            return Ok(exit_code);
        }
    }

    Ok(0)
}

fn integration_tests(test_filter: Option<&str>) -> Option<Vec<(&'static str, IntegrationRun)>> {
    let tests = [
        (kernel_images::NAME, kernel_images::run as IntegrationRun),
        (userspace_boot::NAME, userspace_boot::run as IntegrationRun),
        (ltp::NAME, ltp::run as IntegrationRun),
        (
            panic_handler_smoke::NAME,
            panic_handler_smoke::run as IntegrationRun,
        ),
    ];
    let filtered = match test_filter {
        Some(FULL_TEST_FILTER) => tests.into_iter().collect::<Vec<_>>(),
        Some(filter) => tests
            .into_iter()
            .filter(|(name, _)| name.contains(filter))
            .collect::<Vec<_>>(),
        None => vec![(ltp::NAME, ltp::run as IntegrationRun)],
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
            .map(|(name, _)| name)
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
