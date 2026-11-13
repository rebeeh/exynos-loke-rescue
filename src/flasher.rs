use crate::diagnostics::diagnose_flash_failure;
use anyhow::{bail, Context, Result};
use colored::*;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Clone)]
pub struct PartitionMapping {
    pub partition_name: String, // e.g., "BOOT", "VBMETA", "RECOVERY", "SUPER", "DTBO"
    pub image_path: PathBuf,
}

/// Executes `samloader flash -p <PART> <FILE> ...` while streaming output live and
/// running post-mortem diagnostics if LOKE or USB errors occur.
pub fn flash_raw_partitions(
    samloader_bin: &Path,
    backend: &str,
    partitions: &[PartitionMapping],
    no_reboot: bool,
    is_custom_binary: bool,
) -> Result<()> {
    let mut cmd = Command::new(samloader_bin);
    cmd.args(["--usb-backend", backend, "flash"]);

    if no_reboot {
        cmd.arg("--no-reboot");
    }

    for part in partitions {
        cmd.arg("-p")
            .arg(&part.partition_name)
            .arg(&part.image_path);
    }

    println!("{}", "==> Executing LOKE partition flash:".cyan().bold());
    for part in partitions {
        println!(
            "    - {:<10} -> {}",
            part.partition_name.green(),
            part.image_path.display()
        );
    }

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .context("Failed to spawn `samloader flash` process")?;

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let mut captured_log = String::new();

    let out_reader = BufReader::new(stdout);
    for line in out_reader.lines().map_while(Result::ok) {
        println!("    {}", line);
        captured_log.push_str(&line);
        captured_log.push('\n');
    }

    let err_reader = BufReader::new(stderr);
    for line in err_reader.lines().map_while(Result::ok) {
        eprintln!("    {}", line.red());
        captured_log.push_str(&line);
        captured_log.push('\n');
    }

    let status = child.wait()?;
    if !status.success() {
        diagnose_flash_failure(&captured_log, is_custom_binary);
        bail!("`samloader flash` exited with status {}", status);
    }

    println!(
        "{}",
        "✓ LOKE session closed cleanly (PASS). Emergency Recovery flag cleared."
            .green()
            .bold()
    );
    Ok(())
}

/// Downloads and unzips official stock firmware using `samloader download`.
pub fn download_stock_firmware(
    samloader_bin: &Path,
    model: &str,
    region: &str,
    out_dir: &Path,
) -> Result<()> {
    std::fs::create_dir_all(out_dir)?;
    let zip_path = out_dir.join(format!("{}_{}_stock.zip", model, region));

    println!(
        "{} {} ({})",
        "==> Fetching official signed firmware for".cyan().bold(),
        model.green(),
        region.green()
    );

    let status = Command::new(samloader_bin)
        .args([
            "download",
            "-m",
            model,
            "-r",
            region,
            "-o",
            zip_path.to_str().unwrap(),
        ])
        .status()
        .context("Failed to execute `samloader download`")?;

    if !status.success() {
        bail!("Firmware download failed for {} ({})", model, region);
    }

    println!("{}", "==> Extracting firmware archive...".cyan().bold());
    let unzip_status = Command::new("unzip")
        .args(["-o", zip_path.to_str().unwrap(), "-d", out_dir.to_str().unwrap()])
        .status()
        .context("Failed to run `unzip`. Install `unzip` (sudo dnf install unzip).")?;

    if !unzip_status.success() {
        bail!("Failed to unzip {}", zip_path.display());
    }

    println!(
        "{} {}",
        "✓ Stock firmware extracted to:".green().bold(),
        out_dir.display()
    );
    Ok(())
}
