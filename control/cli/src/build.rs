use anyhow::{Result, bail};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    process::Stdio,
};
use xshell::{Shell, cmd};

#[derive(Clone, Copy, Debug)]
pub enum KernelBuildMode {
    Run,
    UnitTest,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct KernelBuildOptions {
    pub enable_profiling: bool,
}

pub fn build_kernel(
    sh: &Shell,
    mode: KernelBuildMode,
    options: KernelBuildOptions,
) -> Result<Vec<PathBuf>> {
    let args = cargo_args(mode, options);
    eprintln!("==> building kernel");
    let mut command: std::process::Command = cmd!(
        sh,
        "cargo {args...} --message-format=json-render-diagnostics"
    )
    .into();
    let output = command.stderr(Stdio::inherit()).output()?;
    if !output.status.success() {
        bail!("cargo kernel build failed: {}", output.status);
    }
    let executables = parse_cargo_json(&output.stdout, mode)?;
    if executables.is_empty() {
        bail!("kernel executable missing from cargo JSON output");
    }
    Ok(executables)
}

fn cargo_args(mode: KernelBuildMode, options: KernelBuildOptions) -> Vec<String> {
    let mut args = vec![
        match mode {
            KernelBuildMode::Run => "build",
            KernelBuildMode::UnitTest => "test",
        }
        .to_string(),
        "-p".to_string(),
        "kernel".to_string(),
        "--target".to_string(),
        "x86_64-unknown-none".to_string(),
    ];

    if !cfg!(debug_assertions) {
        args.push("--release".to_string());
    }
    if options.enable_profiling {
        args.extend(["--features", "profiling"].map(str::to_string));
    }

    match mode {
        KernelBuildMode::Run => {
            args.extend(["--bin", "kernel"].map(str::to_string));
        }
        KernelBuildMode::UnitTest => {
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
    }

    args
}

fn parse_cargo_json(output: &[u8], mode: KernelBuildMode) -> Result<Vec<PathBuf>> {
    let content = String::from_utf8_lossy(output);
    let mut executables = Vec::new();
    for line in content.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if matches!(
            value.get("reason").and_then(Value::as_str),
            Some("compiler-artifact")
        ) && keep_artifact(&value, mode)
            && let Some(executable) = value.get("executable").and_then(Value::as_str)
        {
            executables.push(PathBuf::from(executable));
        }
    }
    Ok(executables)
}

fn keep_artifact(value: &Value, mode: KernelBuildMode) -> bool {
    let Some(kind) = value["target"]["kind"].as_array() else {
        return false;
    };
    match mode {
        KernelBuildMode::Run => kind.iter().any(|item| item.as_str() == Some("bin")),
        KernelBuildMode::UnitTest => {
            kind.iter().any(|item| item.as_str() == Some("lib"))
                && value["profile"]["test"].as_bool() == Some(true)
        }
    }
}

pub fn shell_for_repo(repo: &Path) -> Result<Shell> {
    let sh = Shell::new()?;
    sh.change_dir(repo);
    Ok(sh)
}
