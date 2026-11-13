use crate::diagnostics::diagnose_flash_failure;
use crate::error::RescueError;
use anyhow::{bail, Context, Result};
use colored::*;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Clone)]
pub struct PartitionMapping {
    pub partition_name: String,
    pub image_path: PathBuf,
}

/// Invokes `samloader-rs flash -p <PART> <FILE> ...` while streaming live output
/// and running post-mortem LOKE diagnostics on failure.
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

    println!("{}", "==> Initiating LOKE raw partition transfer:".cyan().bold());
    for part in partitions {
        let size_mib = std::fs::metadata(&part.image_path)
            .map(|m| (m.len() as f64) / (1024.0 * 1024.0))
            .unwrap_or(0.0);
        println!(
            "    - {:<10} ({:>7.2} MiB) <- {}",
            part.partition_name.green(),
            size_mib,
            part.image_path.display()
        );
    }

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .context("Failed to spawn `samloader flash` subprocess")?;

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let mut captured_log = String::new();

    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        println!("    {}", line);
        captured_log.push_str(&line);
        captured_log.push('\n');
    }

    for line in BufReader::new(stderr).lines().map_while(Result::ok) {
        eprintln!("    {}", line.red());
        captured_log.push_str(&line);
        captured_log.push('\n');
    }

    let status = child.wait()?;
    if !status.success() {
        diagnose_flash_failure(&captured_log, is_custom_binary);
        return Err(RescueError::SubprocessFailed {
            cmd: "samloader flash".to_string(),
            code: status.code().unwrap_or(-1),
        }
        .into());
    }

    println!(
        "{}",
        "✓ LOKE session committed cleanly (PASS). Emergency Recovery state cleared."
            .green()
            .bold()
    );
    Ok(())
}

/// Dumps the live PIT from the connected device to a temporary file via `samloader dump-pit`.
pub fn dump_device_pit(samloader_bin: &Path, backend: &str, out_pit: &Path) -> Result<()> {
    let status = Command::new(samloader_bin)
        .args([
            "--usb-backend",
            backend,
            "dump-pit",
            out_pit.to_str().unwrap(),
        ])
        .status()
        .context("Failed to run `samloader dump-pit`")?;

    if !status.success() {
        bail!("`samloader dump-pit` failed");
    }
    Ok(())
}

/// Downloads and unpacks official signed firmware matching `model` and `region`.
pub fn download_stock_firmware(
    samloader_bin: &Path,
    model: &str,
    region: &str,
    out_dir: &Path,
) -> Result<()> {
    std::fs::create_dir_all(out_dir)?;
    let zip_path = out_dir.join(format!("{}_{}_official.zip", model, region));

    println!(
        "{} {} (CSC: {})",
        "==> Downloading official signed firmware for".cyan().bold(),
        model.green(),
        region.green()
    );

    let dl_status = Command::new(samloader_bin)
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

    if !dl_status.success() {
        bail!("Firmware download failed for {} ({})", model, region);
    }

    println!("{}", "==> Unpacking firmware zip archive...".cyan().bold());
    let unzip_status = Command::new("unzip")
        .args([
            "-o",
            zip_path.to_str().unwrap(),
            "-d",
            out_dir.to_str().unwrap(),
        ])
        .status()
        .context("Failed to run `unzip` (install via `sudo dnf install unzip`)")?;

    if !unzip_status.success() {
        bail!("Failed to unzip {}", zip_path.display());
    }

    println!(
        "{} {}",
        "✓ Official firmware ready in:".green().bold(),
        out_dir.display()
    );
    Ok(())
}
