use crate::reporter::{WorkflowReporter, log_event};
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::{
    env,
    io::{BufRead, BufReader},
    path::PathBuf,
    process::Stdio,
    thread,
};
use xshell::{Shell, cmd};

pub enum BuildMode {
    Run,
    UnitTest,
    IntegrationTests(&'static [&'static str]),
}

#[derive(Clone, Copy, Debug, Default)]
pub struct BuildOptions {
    pub enable_profiling: bool,
}

pub fn build_kernel(reporter: &dyn WorkflowReporter) -> Result<Vec<PathBuf>> {
    build_kernel_with_options(BuildMode::Run, reporter, BuildOptions::default())
}

pub fn build_kernel_with_options(
    mode: BuildMode,
    reporter: &dyn WorkflowReporter,
    options: BuildOptions,
) -> Result<Vec<PathBuf>> {
    build_kernel_with_mode(mode, reporter, options)
}

pub fn build_kernel_tests(reporter: &dyn WorkflowReporter) -> Result<Vec<PathBuf>> {
    build_kernel_with_mode(BuildMode::UnitTest, reporter, BuildOptions::default())
}

pub fn build_kernel_with_mode(
    mode: BuildMode,
    reporter: &dyn WorkflowReporter,
    options: BuildOptions,
) -> Result<Vec<PathBuf>> {
    let sh = Shell::new()?;
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let cargo_args = cargo_args(&mode, options);
    let mut command = cmd!(sh, "{cargo} {cargo_args...}").to_command();

    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    let mut child = command.spawn().context("failed to start cargo")?;
    let stdout = child.stdout.take().context("missing cargo stdout")?;
    let stderr = child.stderr.take().context("missing cargo stderr")?;
    let reader = BufReader::new(stdout);
    let mut executables = Vec::new();
    let status = thread::scope(|scope| {
        let stderr_thread = scope.spawn(|| {
            let stderr_reader = BufReader::new(stderr);
            for line in stderr_reader.lines() {
                match line {
                    Ok(line) => {
                        let _ = log_event(reporter, "build", "stderr", &line);
                        let _ = log_event(reporter, "build", "stderr", "\n");
                    }
                    Err(err) => {
                        let _ = log_event(
                            reporter,
                            "build",
                            "stderr",
                            &format!("failed to read cargo stderr: {err}\n"),
                        );
                        break;
                    }
                }
            }
        });

        for line in reader.lines() {
            let line: String = line.context("failed to read cargo output")?;
            if let Some(path) = handle_cargo_message(&line, &mode, reporter) {
                executables.push(path);
            }
        }

        let status = child.wait().context("failed to wait on cargo")?;
        let _ = stderr_thread.join();
        Ok::<_, anyhow::Error>(status)
    })?;
    if !status.success() {
        bail!("cargo command failed with status {}", status);
    }
    if executables.is_empty() {
        bail!("kernel executable missing from cargo output");
    }
    Ok(executables)
}

fn cargo_args(mode: &BuildMode, options: BuildOptions) -> Vec<String> {
    let mut args = Vec::new();
    args.push(
        match mode {
            BuildMode::Run => "build",
            BuildMode::UnitTest | BuildMode::IntegrationTests(_) => "test",
        }
        .to_string(),
    );
    args.extend(["-p", "kernel", "--target", "x86_64-unknown-none"].map(str::to_string));

    if !cfg!(debug_assertions) {
        args.push("--release".to_string());
    }
    if options.enable_profiling {
        args.extend(["--features", "profiling"].map(str::to_string));
    }

    match mode {
        BuildMode::Run => {
            args.extend(["--bin", "kernel"].map(str::to_string));
        }
        BuildMode::UnitTest => {
            args.extend(
                [
                    "--lib",
                    "-Z",
                    "build-std=core,alloc",
                    "-Z",
                    "panic-abort-tests",
                    "--no-run",
                ]
                .map(str::to_string),
            );
        }
        BuildMode::IntegrationTests(tests) => {
            for test in *tests {
                args.extend(["--test".to_string(), test.to_string()]);
            }
            args.extend(
                [
                    "-Z",
                    "build-std=core,alloc",
                    "-Z",
                    "panic-abort-tests",
                    "--no-run",
                ]
                .map(str::to_string),
            );
        }
    }

    args.push("--message-format=json-render-diagnostics".to_string());
    args
}

fn handle_cargo_message(
    line: &str,
    mode: &BuildMode,
    reporter: &dyn WorkflowReporter,
) -> Option<PathBuf> {
    let value: Value = match serde_json::from_str(line) {
        Ok(value) => value,
        Err(_) => {
            let _ = log_event(reporter, "build", "stdout", &format!("{line}\n"));
            return None;
        }
    };

    match value.get("reason").and_then(Value::as_str) {
        Some("compiler-message") => {
            if let Some(rendered) = value["message"]["rendered"].as_str() {
                let _ = log_event(reporter, "build", "stderr", rendered);
            }
            None
        }
        Some("compiler-artifact") => {
            let kind = value["target"]["kind"].as_array()?;
            let keep = match mode {
                BuildMode::Run => kind.iter().any(|item| item.as_str() == Some("bin")),
                BuildMode::UnitTest => {
                    kind.iter().any(|item| item.as_str() == Some("lib"))
                        && value["profile"]["test"].as_bool() == Some(true)
                }
                BuildMode::IntegrationTests(_) => {
                    kind.iter().any(|item| item.as_str() == Some("test"))
                        && value["profile"]["test"].as_bool() == Some(true)
                }
            };

            if !keep {
                return None;
            }

            value
                .get("executable")
                .and_then(Value::as_str)
                .map(PathBuf::from)
        }
        _ => None,
    }
}
