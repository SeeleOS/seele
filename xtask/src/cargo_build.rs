use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::{
    env,
    io::{BufRead, BufReader},
    path::PathBuf,
    process::{Command, Stdio},
};

pub enum BuildMode {
    Run,
    UnitTest,
    IntegrationTests(&'static [&'static str]),
}

pub fn build_kernel() -> Result<Vec<PathBuf>> {
    build_kernel_with_mode(BuildMode::Run)
}

pub fn build_kernel_tests() -> Result<Vec<PathBuf>> {
    build_kernel_with_mode(BuildMode::UnitTest)
}

pub fn build_kernel_with_mode(mode: BuildMode) -> Result<Vec<PathBuf>> {
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let mut command = Command::new(cargo);
    command.arg(match mode {
        BuildMode::Run => "build",
        BuildMode::UnitTest | BuildMode::IntegrationTests(_) => "test",
    });
    command.args(["-p", "kernel", "--target", "x86_64-unknown-none"]);

    if !cfg!(debug_assertions) {
        command.arg("--release");
    }

    match mode {
        BuildMode::Run => {
            command.args(["--bin", "kernel"]);
        }
        BuildMode::UnitTest => {
            command.args([
                "--lib",
                "-Z",
                "build-std=core,alloc",
                "-Z",
                "panic-abort-tests",
                "--no-run",
            ]);
            command.env("RUSTFLAGS", append_rustflags());
        }
        BuildMode::IntegrationTests(tests) => {
            for test in tests {
                command.args(["--test", test]);
            }
            command.args([
                "-Z",
                "build-std=core,alloc",
                "-Z",
                "panic-abort-tests",
                "--no-run",
            ]);
            command.env("RUSTFLAGS", append_rustflags());
        }
    }

    command.arg("--message-format=json-render-diagnostics");
    command.stdout(Stdio::piped());
    command.stderr(Stdio::inherit());

    let mut child = command.spawn().context("failed to start cargo")?;
    let stdout = child.stdout.take().context("missing cargo stdout")?;
    let reader = BufReader::new(stdout);
    let mut executables = Vec::new();

    for line in reader.lines() {
        let line = line.context("failed to read cargo output")?;
        if let Some(path) = handle_cargo_message(&line, &mode) {
            executables.push(path);
        }
    }

    let status = child.wait().context("failed to wait on cargo")?;
    if !status.success() {
        bail!("cargo command failed with status {}", status);
    }
    if executables.is_empty() {
        bail!("kernel executable missing from cargo output");
    }
    Ok(executables)
}

fn handle_cargo_message(line: &str, mode: &BuildMode) -> Option<PathBuf> {
    let value: Value = match serde_json::from_str(line) {
        Ok(value) => value,
        Err(_) => {
            println!("{line}");
            return None;
        }
    };

    match value.get("reason").and_then(Value::as_str) {
        Some("compiler-message") => {
            if let Some(rendered) = value["message"]["rendered"].as_str() {
                print!("{rendered}");
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

fn append_rustflags() -> String {
    let extra = "-Zunstable-options";
    match env::var("RUSTFLAGS") {
        Ok(existing) if !existing.trim().is_empty() => format!("{existing} {extra}"),
        _ => extra.to_string(),
    }
}
