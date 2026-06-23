use crate::{
    Artifact, ArtifactKind, BuildEvent, ControlContext, Event, process::ProcessRunner, target_dir,
};
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

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
    repo: &Path,
    mode: KernelBuildMode,
    options: KernelBuildOptions,
    context: &dyn ControlContext,
) -> Result<Vec<PathBuf>> {
    let artifact_dir = target_dir(repo).join("control-artifacts").join("build");
    let runner = ProcessRunner::new(&artifact_dir)?;
    let args = cargo_args(mode, options);
    let result = runner.run_success(
        context,
        "kernel_cargo_json",
        Command::new("cargo").current_dir(repo).args(args),
    )?;
    context.artifact(Artifact {
        kind: ArtifactKind::CargoJson,
        path: result.stdout_artifact.clone(),
        description: "kernel cargo JSON messages".to_string(),
    });
    let executables = parse_cargo_json(&result.stdout_artifact, mode, context)?;
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

    args.push("--message-format=json-render-diagnostics".to_string());
    args
}

fn parse_cargo_json(
    path: &Path,
    mode: KernelBuildMode,
    context: &dyn ControlContext,
) -> Result<Vec<PathBuf>> {
    let content =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let mut executables = Vec::new();
    for line in content.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        match value.get("reason").and_then(Value::as_str) {
            Some("compiler-message") => {
                if let Some(rendered) = value["message"]["rendered"].as_str() {
                    context.event(Event::Build(BuildEvent::Diagnostic {
                        level: value["message"]["level"]
                            .as_str()
                            .unwrap_or("unknown")
                            .to_string(),
                        message: rendered.to_string(),
                    }));
                }
            }
            Some("compiler-artifact") if keep_artifact(&value, mode) => {
                let executable = value
                    .get("executable")
                    .and_then(Value::as_str)
                    .map(PathBuf::from);
                context.event(Event::Build(BuildEvent::CargoArtifact {
                    package: value["package_id"].as_str().unwrap_or("kernel").to_string(),
                    target: value["target"]["name"]
                        .as_str()
                        .unwrap_or("kernel")
                        .to_string(),
                    executable: executable.clone(),
                }));
                if let Some(executable) = executable {
                    executables.push(executable);
                }
            }
            _ => {}
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
