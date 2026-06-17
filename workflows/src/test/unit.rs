use crate::run::{
    build::build_kernel_tests, build_iso::create_boot_iso, qemu::run_qemu_test_capture,
};
use anyhow::{Context, Result};
use std::fs;

use crate::reporter::{TestStatus, WorkflowReporter, log_event, remove_file, test_event};

pub fn run(reporter: &dyn WorkflowReporter) -> Result<i32> {
    let mut exit_code = 0;

    for kernel_test in build_kernel_tests(reporter)? {
        let test_name = kernel_test
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("kernel unit tests");
        test_event(
            reporter,
            "test",
            test_name,
            TestStatus::Running,
            "kernel unit test image started",
        )?;
        let iso_path = create_boot_iso(&kernel_test)?;
        let result = run_qemu_test_capture(&iso_path, reporter)?;
        exit_code = result.exit_code;
        if reporter.capture_subprocess_output() {
            remove_file(&iso_path, reporter)?;
        } else {
            fs::remove_file(&iso_path)
                .with_context(|| format!("failed to remove ISO image {}", iso_path.display()))?;
        }

        if exit_code != 0 {
            if let Some(failure) = &result.failure {
                log_event(reporter, "test", "stderr", failure)?;
            }
            if !result.serial_output.is_empty() {
                log_event(reporter, "test", "serial", &result.serial_output)?;
            }
            test_event(
                reporter,
                "test",
                test_name,
                TestStatus::Failed,
                result
                    .failure
                    .as_deref()
                    .unwrap_or("kernel unit test failed"),
            )?;
            if !reporter.capture_subprocess_output() {
                if let Some(failure) = result.failure {
                    eprintln!("{failure}");
                }
                if !result.serial_output.is_empty() {
                    eprint!("{}", result.serial_output);
                    if !result.serial_output.ends_with('\n') {
                        eprintln!();
                    }
                }
            }
            break;
        } else {
            let output = unit_output(&result.serial_output);
            if !output.is_empty() {
                log_event(reporter, "test", "serial", output)?;
            }
            test_event(
                reporter,
                "test",
                test_name,
                TestStatus::Ok,
                "kernel unit test image passed",
            )?;
        }
    }

    Ok(exit_code)
}

fn unit_output(serial_output: &str) -> &str {
    let Some((_, test_output)) = serial_output.split_once("Running ") else {
        return "";
    };

    &serial_output[serial_output.len() - test_output.len() - "Running ".len()..]
}
