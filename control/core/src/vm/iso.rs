use crate::{Artifact, ArtifactKind, JobContext, process::ProcessRunner};
use anyhow::{Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug, Clone, Default)]
pub struct BootConfig {
    pub init: Option<String>,
    pub ltp_suite: Option<String>,
    pub ltp_pattern: Option<String>,
}

pub fn create_boot_iso(
    repo: &Path,
    kernel_path: &Path,
    config: &BootConfig,
    context: &JobContext,
) -> Result<PathBuf> {
    let artifact_dir = crate::target_dir(repo)
        .join("control-artifacts")
        .join("iso");
    let runner = ProcessRunner::new(&artifact_dir)?;
    let image_path = kernel_path.with_extension("iso");
    let build_root = kernel_path
        .parent()
        .context("kernel path missing parent directory")?
        .join("limine-image");
    let iso_boot_dir = build_root.join("boot");
    let iso_limine_dir = iso_boot_dir.join("limine");
    let iso_efi_dir = build_root.join("EFI").join("BOOT");
    let efi_image = build_root.join("efiboot.img");

    let _ = fs::remove_file(&image_path);
    let _ = fs::remove_dir_all(&build_root);
    fs::create_dir_all(&iso_limine_dir)
        .with_context(|| format!("failed to create {}", iso_limine_dir.display()))?;
    fs::create_dir_all(&iso_efi_dir)
        .with_context(|| format!("failed to create {}", iso_efi_dir.display()))?;

    fs::copy(kernel_path, iso_boot_dir.join("kernel"))
        .with_context(|| format!("failed to stage kernel {}", kernel_path.display()))?;
    fs::write(
        iso_limine_dir.join("limine.conf"),
        limine_config_contents(repo, config)?,
    )
    .context("failed to stage limine.conf")?;

    let limine_dir = limine_support_dir(&runner, context)?;
    fs::copy(
        limine_dir.join("BOOTX64.EFI"),
        iso_efi_dir.join("BOOTX64.EFI"),
    )
    .context("failed to stage BOOTX64.EFI")?;
    fs::copy(
        limine_dir.join("limine-bios.sys"),
        iso_limine_dir.join("limine-bios.sys"),
    )
    .context("failed to stage limine-bios.sys")?;
    create_efi_boot_image(
        repo,
        &runner,
        &efi_image,
        kernel_path,
        &limine_dir,
        config,
        context,
    )?;

    runner.run_success(
        context,
        "xorriso_boot_iso",
        Command::new("xorriso")
            .current_dir(&build_root)
            .args([
                "-as",
                "mkisofs",
                "-e",
                "efiboot.img",
                "-no-emul-boot",
                "-isohybrid-gpt-basdat",
                "-efi-boot-part",
                "--efi-boot-image",
                "--protective-msdos-label",
                ".",
                "-o",
            ])
            .arg(&image_path),
    )?;

    fs::remove_dir_all(&build_root)
        .with_context(|| format!("failed to remove {}", build_root.display()))?;
    context.artifact(Artifact {
        kind: ArtifactKind::IsoImage,
        path: image_path.clone(),
        description: "Limine boot ISO".to_string(),
    });
    Ok(image_path)
}

fn create_efi_boot_image(
    repo: &Path,
    runner: &ProcessRunner,
    image: &Path,
    kernel_path: &Path,
    limine_dir: &Path,
    config: &BootConfig,
    context: &JobContext,
) -> Result<()> {
    runner.run_success(
        context,
        "efi_truncate",
        Command::new("truncate").args(["-s", "128M"]).arg(image),
    )?;

    let boot_efi = limine_dir.join("BOOTX64.EFI");
    let limine_conf = image.with_file_name("limine.conf");
    fs::write(&limine_conf, limine_config_contents(repo, config)?)
        .with_context(|| format!("failed to write {}", limine_conf.display()))?;
    runner.run_success(
        context,
        "efi_mformat",
        Command::new("mformat")
            .arg("-i")
            .arg(image)
            .args(["-F", "::"]),
    )?;
    runner.run_success(
        context,
        "efi_mmd",
        Command::new("mmd").arg("-i").arg(image).args([
            "::/EFI",
            "::/EFI/BOOT",
            "::/boot",
            "::/boot/limine",
        ]),
    )?;
    runner.run_success(
        context,
        "efi_mcopy_bootx64",
        Command::new("mcopy")
            .arg("-i")
            .arg(image)
            .arg(boot_efi)
            .arg("::/EFI/BOOT/BOOTX64.EFI"),
    )?;
    runner.run_success(
        context,
        "efi_mcopy_limine_conf",
        Command::new("mcopy")
            .arg("-i")
            .arg(image)
            .arg(limine_conf)
            .arg("::/boot/limine/limine.conf"),
    )?;
    runner.run_success(
        context,
        "efi_mcopy_kernel",
        Command::new("mcopy")
            .arg("-i")
            .arg(image)
            .arg(kernel_path)
            .arg("::/boot/kernel"),
    )?;
    Ok(())
}

fn limine_config_contents(repo: &Path, config: &BootConfig) -> Result<String> {
    let mut contents =
        fs::read_to_string(repo.join("limine.conf")).context("failed to read limine.conf")?;
    let mut cmdline_args = Vec::new();
    if let Some(init) = &config.init {
        cmdline_args.push(format!("init={init}"));
    }
    if let Some(suite) = &config.ltp_suite {
        cmdline_args.push(format!("seele.ltp_suite={suite}"));
    }
    if let Some(pattern) = &config.ltp_pattern {
        cmdline_args.push(format!("seele.ltp_pattern={pattern}"));
    }

    if !cmdline_args.is_empty() {
        let cmdline = cmdline_args.join(" ");
        contents = contents
            .lines()
            .map(|line| {
                if line.trim_start().starts_with("cmdline:") {
                    let indent = &line[..line.len() - line.trim_start().len()];
                    format!("{indent}cmdline: {cmdline}")
                } else {
                    line.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        contents.push('\n');
    }
    Ok(contents)
}

fn limine_support_dir(runner: &ProcessRunner, context: &JobContext) -> Result<PathBuf> {
    let result = runner.run_success(
        context,
        "limine_datadir",
        Command::new("limine").arg("--print-datadir"),
    )?;
    let path = fs::read_to_string(&result.stdout_artifact)
        .with_context(|| format!("failed to read {}", result.stdout_artifact.display()))?;
    Ok(PathBuf::from(path.trim()))
}
