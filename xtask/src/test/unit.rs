use crate::run::{
    build::build_kernel_tests, build_iso::create_boot_iso, qemu::run_qemu_test_capture,
};
use anyhow::{Context, Result};
use std::fs;

use crate::json_output::{JsonEvent, OutputMode, emit, remove_file};

pub fn run(output_mode: OutputMode) -> Result<i32> {
    let mut exit_code = 0;

    for kernel_test in build_kernel_tests(output_mode)? {
        let test_name = kernel_test
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("kernel unit tests");
        if output_mode.is_json() {
            emit(&JsonEvent::test(
                "test",
                test_name,
                "running",
                "kernel unit test image started",
            ))?;
        }
        let iso_path = create_boot_iso(&kernel_test)?;
        let result = run_qemu_test_capture(&iso_path, output_mode)?;
        exit_code = result.exit_code;
        if output_mode.is_json() {
            remove_file(&iso_path, output_mode)?;
        } else {
            fs::remove_file(&iso_path)
                .with_context(|| format!("failed to remove ISO image {}", iso_path.display()))?;
        }

        if exit_code != 0 {
            if output_mode.is_json() {
                if let Some(failure) = &result.failure {
                    emit(&JsonEvent::log("test", "stderr", failure))?;
                }
                if !result.serial_output.is_empty() {
                    emit(&JsonEvent::log("test", "serial", &result.serial_output))?;
                }
                emit(&JsonEvent::test(
                    "test",
                    test_name,
                    "failed",
                    result
                        .failure
                        .as_deref()
                        .unwrap_or("kernel unit test failed"),
                ))?;
            } else {
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
        } else if output_mode.is_json() {
            let output = unit_output(&result.serial_output);
            if !output.is_empty() {
                emit(&JsonEvent::log("test", "serial", output))?;
            }
            emit(&JsonEvent::test(
                "test",
                test_name,
                "ok",
                "kernel unit test image passed",
            ))?;
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
