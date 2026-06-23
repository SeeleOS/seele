use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use xshell::{Shell, cmd};

#[derive(Debug, Clone, Default)]
pub struct BootConfig {
    pub init: Option<String>,
    pub ltp_suite: Option<String>,
    pub ltp_pattern: Option<String>,
}

pub fn create_boot_iso(
    sh: &Shell,
    repo: &Path,
    kernel_path: &Path,
    config: &BootConfig,
) -> Result<PathBuf> {
    eprintln!("==> creating boot ISO");
    let image_path = kernel_path.with_extension("iso");
    let build_root = kernel_path
        .parent()
        .context("kernel path missing parent directory")?
        .join("limine-image");
    let iso_boot_dir = build_root.join("boot");
    let iso_limine_dir = iso_boot_dir.join("limine");
    let iso_efi_dir = build_root.join("EFI").join("BOOT");
    let efi_image = build_root.join("efiboot.img");

    sh.remove_path(&image_path)?;
    sh.remove_path(&build_root)?;
    sh.create_dir(&iso_limine_dir)?;
    sh.create_dir(&iso_efi_dir)?;

    sh.copy_file(kernel_path, iso_boot_dir.join("kernel"))?;
    sh.write_file(
        iso_limine_dir.join("limine.conf"),
        limine_config_contents(sh, repo, config)?,
    )?;

    let limine_dir = limine_support_dir(sh)?;
    sh.copy_file(
        limine_dir.join("BOOTX64.EFI"),
        iso_efi_dir.join("BOOTX64.EFI"),
    )?;
    sh.copy_file(
        limine_dir.join("limine-bios.sys"),
        iso_limine_dir.join("limine-bios.sys"),
    )?;
    create_efi_boot_image(sh, repo, &efi_image, kernel_path, &limine_dir, config)?;

    let _dir = sh.push_dir(&build_root);
    cmd!(
        sh,
        "xorriso -as mkisofs -e efiboot.img -no-emul-boot -isohybrid-gpt-basdat -efi-boot-part --efi-boot-image --protective-msdos-label . -o {image_path}"
    )
    .run()?;
    drop(_dir);

    sh.remove_path(&build_root)?;
    eprintln!("    ISO: {}", image_path.display());
    Ok(image_path)
}

fn create_efi_boot_image(
    sh: &Shell,
    repo: &Path,
    image: &Path,
    kernel_path: &Path,
    limine_dir: &Path,
    config: &BootConfig,
) -> Result<()> {
    let size = "128M";
    cmd!(sh, "truncate -s {size} {image}").run()?;

    let boot_efi = limine_dir.join("BOOTX64.EFI");
    let limine_conf = image.with_file_name("limine.conf");
    sh.write_file(&limine_conf, limine_config_contents(sh, repo, config)?)?;
    cmd!(sh, "mformat -i {image} -F ::").run()?;
    cmd!(
        sh,
        "mmd -i {image} ::/EFI ::/EFI/BOOT ::/boot ::/boot/limine"
    )
    .run()?;
    cmd!(sh, "mcopy -i {image} {boot_efi} ::/EFI/BOOT/BOOTX64.EFI").run()?;
    cmd!(
        sh,
        "mcopy -i {image} {limine_conf} ::/boot/limine/limine.conf"
    )
    .run()?;
    cmd!(sh, "mcopy -i {image} {kernel_path} ::/boot/kernel").run()?;
    Ok(())
}

fn limine_config_contents(sh: &Shell, repo: &Path, config: &BootConfig) -> Result<String> {
    let mut contents = sh
        .read_file(repo.join("limine.conf"))
        .context("failed to read limine.conf")?;
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

fn limine_support_dir(sh: &Shell) -> Result<PathBuf> {
    Ok(PathBuf::from(
        cmd!(sh, "limine --print-datadir").read()?.trim(),
    ))
}
