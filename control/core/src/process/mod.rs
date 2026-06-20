use crate::{Artifact, ArtifactKind, JobContext};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[derive(Debug, Clone)]
pub struct ProcessRunner {
    artifact_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessResult {
    pub exit_code: i32,
    pub stdout_artifact: PathBuf,
    pub stderr_artifact: PathBuf,
}

impl ProcessRunner {
    pub fn new(artifact_dir: impl Into<PathBuf>) -> Result<Self> {
        let artifact_dir = artifact_dir.into();
        fs::create_dir_all(&artifact_dir)
            .with_context(|| format!("failed to create {}", artifact_dir.display()))?;
        Ok(Self { artifact_dir })
    }

    pub fn run(
        &self,
        context: &JobContext,
        name: &str,
        command: &mut Command,
    ) -> Result<ProcessResult> {
        let output = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .with_context(|| format!("failed to run {name}"))?;
        let stdout_artifact = self.artifact_dir.join(format!("{name}.stdout.log"));
        let stderr_artifact = self.artifact_dir.join(format!("{name}.stderr.log"));
        write_bytes(&stdout_artifact, &output.stdout)?;
        write_bytes(&stderr_artifact, &output.stderr)?;
        context.artifact(Artifact {
            kind: ArtifactKind::StdoutLog,
            path: stdout_artifact.clone(),
            description: format!("{name} stdout"),
        });
        context.artifact(Artifact {
            kind: ArtifactKind::StderrLog,
            path: stderr_artifact.clone(),
            description: format!("{name} stderr"),
        });
        let exit_code = output.status.code().unwrap_or(1);
        Ok(ProcessResult {
            exit_code,
            stdout_artifact,
            stderr_artifact,
        })
    }

    pub fn run_success(
        &self,
        context: &JobContext,
        name: &str,
        command: &mut Command,
    ) -> Result<ProcessResult> {
        let result = self.run(context, name, command)?;
        if result.exit_code != 0 {
            bail!("{name} failed with exit code {}", result.exit_code);
        }
        Ok(result)
    }

    pub fn artifact_dir(&self) -> &Path {
        &self.artifact_dir
    }
}

fn write_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file =
        File::create(path).with_context(|| format!("failed to create {}", path.display()))?;
    file.write_all(bytes)
        .with_context(|| format!("failed to write {}", path.display()))
}
