mod kernel_images;
mod ltp;
mod panic_handler_smoke;
mod userspace_boot;

use anyhow::Result;
use owo_colors::OwoColorize;

use crate::json_output::{JsonEvent, OutputMode, emit};

trait IntegrationTest {
    fn name(&self) -> &'static str;
    fn run(&self, output_mode: OutputMode) -> Result<IntegrationTestResult>;
}

pub struct IntegrationTestResult {
    pub exit_code: i32,
    pub failure: Option<String>,
    pub output: String,
}

pub fn run(output_mode: OutputMode, test_filter: Option<&str>) -> Result<i32> {
    let tests = integration_tests(test_filter);
    let Some(tests) = tests else {
        let message = format!(
            "no integration test matched filter {:?}",
            test_filter.unwrap_or("")
        );
        if output_mode.is_json() {
            emit(&JsonEvent::log("test", "stderr", &message))?;
        } else {
            eprintln!("{message}");
        }
        return Ok(1);
    };
    if output_mode.is_json() {
        emit(&JsonEvent::progress(
            "test",
            "integration",
            "running integration tests",
        ))?;
    } else {
        eprintln!();
        eprintln!("running {} integration tests", tests.len());
    }

    for test in tests {
        if output_mode.is_json() {
            emit(&JsonEvent::test(
                "test",
                test.name(),
                "running",
                "integration test started",
            ))?;
        }
        let result = test.run(output_mode)?;
        if result.exit_code == 0 {
            if output_mode.is_json() {
                emit(&JsonEvent::test(
                    "test",
                    test.name(),
                    "ok",
                    "integration test passed",
                ))?;
            } else {
                eprint!("test {} ... ", test.name());
                eprintln!("{}", "ok".green().bold());
            }
        } else {
            if output_mode.is_json() {
                if let Some(failure) = &result.failure {
                    emit(&JsonEvent::log("test", "stderr", failure))?;
                }
                if !result.output.is_empty() {
                    emit(&JsonEvent::log("test", "serial", &result.output))?;
                }
                emit(&JsonEvent::test(
                    "test",
                    test.name(),
                    "failed",
                    result
                        .failure
                        .as_deref()
                        .unwrap_or("integration test failed"),
                ))?;
            } else {
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
        Some(filter) => tests
            .into_iter()
            .filter(|test| test.name().contains(filter))
            .collect::<Vec<_>>(),
        None => tests.into_iter().collect::<Vec<_>>(),
    };
    if filtered.is_empty() {
        None
    } else {
        Some(filtered)
    }
}
