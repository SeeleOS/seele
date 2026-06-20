use anyhow::{Context, Result};
use std::{
    env, fs,
    path::{Path, PathBuf},
};
use xshell::{Shell, cmd};

pub fn create_boot_iso(kernel_path: &Path) -> Result<PathBuf> {
    let sh = Shell::new()?;
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
    let limine_config = limine_config_contents()?;
    fs::write(iso_limine_dir.join("limine.conf"), &limine_config)
        .context("failed to stage limine.conf")?;

    let limine_dir = limine_support_dir()?;
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
    create_efi_boot_image(&sh, &efi_image, kernel_path, &limine_dir)?;

    let sh = sh.with_current_dir(&build_root);
    cmd!(
        sh,
        "xorriso -as mkisofs -e efiboot.img -no-emul-boot -isohybrid-gpt-basdat -efi-boot-part --efi-boot-image --protective-msdos-label . -o {image_path}"
    )
    .run()?;

    fs::remove_dir_all(&build_root)
        .with_context(|| format!("failed to remove {}", build_root.display()))?;
    Ok(image_path)
}

fn create_efi_boot_image(
    sh: &Shell,
    image: &Path,
    kernel_path: &Path,
    limine_dir: &Path,
) -> Result<()> {
    cmd!(sh, "truncate -s 128M {image}").run()?;

    let boot_efi = limine_dir.join("BOOTX64.EFI");
    let limine_conf = image.with_file_name("limine.conf");
    fs::write(&limine_conf, limine_config_contents()?)
        .with_context(|| format!("failed to write {}", limine_conf.display()))?;
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

fn limine_config_contents() -> Result<String> {
    let mut contents =
        fs::read_to_string(limine_config_path()).context("failed to read limine.conf")?;
    if let Ok(init) = env::var("SEELE_INIT") {
        contents = contents
            .lines()
            .map(|line| {
                if line.trim_start().starts_with("cmdline:") {
                    let indent = &line[..line.len() - line.trim_start().len()];
                    format!("{indent}cmdline: init={init}")
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

fn limine_config_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("limine.conf")
}

fn limine_support_dir() -> Result<PathBuf> {
    let sh = Shell::new()?;
    let path = cmd!(sh, "limine --print-datadir").read()?;
    Ok(PathBuf::from(path.trim()))
}
