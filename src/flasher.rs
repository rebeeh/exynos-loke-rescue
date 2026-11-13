use crate::cli::TransportEngine;
use crate::diagnostics::print_diagnostic_guidance;
use crate::error::RescueError;
use crate::nusb_transport::NativeLokeSession;
use crate::usb;
use anyhow::{bail, Context, Result};
use colored::*;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone)]
pub struct PartitionTarget {
    pub name: String,
    pub path: PathBuf,
}

pub fn execute_partition_flash(
    engine: TransportEngine,
    samloader_override: Option<&Path>,
    targets: &[PartitionTarget],
    no_reboot: bool,
    is_custom_binary: bool,
    verbose: bool,
) -> Result<()> {
    let result = match engine {
        TransportEngine::Native => {
            flash_via_native_nusb(targets, no_reboot, verbose)
        }
        TransportEngine::Samloader => {
            let bin = usb::resolve_samloader(samloader_override)?;
            flash_via_samloader_subprocess(&bin, targets, no_reboot)
        }
    };

    if let Err(ref e) = result {
        print_diagnostic_guidance(&format!("{:#}", e), is_custom_binary);
    }
    result
}

fn flash_via_native_nusb(
    targets: &[PartitionTarget],
    no_reboot: bool,
    verbose: bool,
) -> Result<()> {
    println!(
        "{}",
        "==> Connecting via Native Rust LOKE Transport (nusb)...".cyan().bold()
    );
    let session = NativeLokeSession::connect(verbose)?;
    let pit = session.fetch_pit()?;

    let mut total_bytes: u64 = 0;
    let mut resolved: Vec<(&PartitionTarget, u32)> = Vec::new();

    for t in targets {
        let entry = pit
            .find_partition(&t.name)
            .ok_or_else(|| RescueError::PartitionNotInPit(t.name.clone()))?;
        let payload_len = std::fs::metadata(&t.path)?.len();
        if payload_len > entry.size_bytes() {
            return Err(RescueError::PartitionOverflow {
                partition: t.name.clone(),
                payload_bytes: payload_len,
                capacity_bytes: entry.size_bytes(),
            }
            .into());
        }
        total_bytes += payload_len;
        resolved.push((t, entry.partition_id));
    }

    session.set_total_bytes(total_bytes)?;

    for (target, part_id) in resolved {
        session.flash_partition_file(&target.name, part_id, &target.path)?;
    }

    session.end_session(no_reboot)?;
    println!(
        "{}",
        "✓ Native LOKE session closed cleanly (PASS). Emergency Recovery flag cleared."
            .green()
            .bold()
    );
    Ok(())
}

fn flash_via_samloader_subprocess(
    samloader_bin: &Path,
    targets: &[PartitionTarget],
    no_reboot: bool,
) -> Result<()> {
    let mut cmd = Command::new(samloader_bin);
    cmd.args(["--usb-backend", "nusb", "flash"]);
    if no_reboot {
        cmd.arg("--no-reboot");
    }
    for t in targets {
        cmd.arg("-p").arg(&t.name).arg(&t.path);
    }

    let output = cmd.output().context("Failed to execute samloader-rs")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    print!("{}", stdout);
    eprint!("{}", stderr);

    if !output.status.success() {
        bail!("{}\n{}", stdout, stderr);
    }
    Ok(())
}

pub fn download_stock_firmware(
    samloader_override: Option<&Path>,
    model: &str,
    region: &str,
    out_dir: &Path,
) -> Result<()> {
    let bin = usb::resolve_samloader(samloader_override)?;
    std::fs::create_dir_all(out_dir)?;
    let zip_path = out_dir.join(format!("{}_{}_official.zip", model, region));

    let dl = Command::new(&bin)
        .args([
            "download",
            "-m",
            model,
            "-r",
            region,
            "-o",
            zip_path.to_str().unwrap(),
        ])
        .status()?;

    if !dl.success() {
        bail!("Firmware download failed for {} ({})", model, region);
    }

    let unzip = Command::new("unzip")
        .args([
            "-o",
            zip_path.to_str().unwrap(),
            "-d",
            out_dir.to_str().unwrap(),
        ])
        .status()?;

    if !unzip.success() {
        bail!("Failed to unzip {}", zip_path.display());
    }
    Ok(())
}
