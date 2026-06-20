use crate::{
    Artifact, ArtifactKind, JobContext, KernelUnitReport, process::ProcessRunner, target_dir,
};
use anyhow::{Context, Result};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub fn run(repo: &Path, context: &JobContext) -> Result<KernelUnitReport> {
    let artifact_dir = target_dir(repo)
        .join("control-artifacts")
        .join("tests")
        .join("kernel-unit");
    let runner = ProcessRunner::new(&artifact_dir)?;
    let result = runner.run(
        context,
        "kernel_unit_cargo_test",
        Command::new("cargo").current_dir(repo).args([
            "test",
            "-p",
            "kernel",
            "--target",
            "x86_64-unknown-none",
            "--lib",
            "-Z",
            "build-std=core,alloc",
            "-Z",
            "panic-abort-tests",
            "--no-run",
            "--message-format=json-render-diagnostics",
        ]),
    )?;
    context.artifact(Artifact {
        kind: ArtifactKind::CargoJson,
        path: result.stdout_artifact.clone(),
        description: "kernel unit cargo JSON messages".to_string(),
    });
    let executable = find_last_executable(&result.stdout_artifact)?;
    Ok(KernelUnitReport {
        executable,
        iso: None,
        passed: result.exit_code == 0,
        serial_log: None,
        stdout: String::new(),
        stderr: String::new(),
    })
}

fn find_last_executable(path: &Path) -> Result<PathBuf> {
    let content =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let mut executable = None;
    for line in content.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if value.get("reason").and_then(Value::as_str) == Some("compiler-artifact")
            && let Some(path) = value.get("executable").and_then(Value::as_str)
        {
            executable = Some(PathBuf::from(path));
        }
    }
    Ok(executable.unwrap_or_default())
}
