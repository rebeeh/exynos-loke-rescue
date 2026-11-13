use anyhow::{bail, Context, Result};
use colored::*;
use regex::Regex;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const UDEV_RULE_PATH: &str = "/etc/udev/rules.d/60-exynos-loke-rescue.rules";
const UDEV_RULE_CONTENT: &str = r#"# Samsung Download Mode (LOKE / Odin)
SUBSYSTEM=="usb", ATTR{idVendor}=="04e8", ATTR{idProduct}=="685d", MODE="0666", GROUP="plugdev", TAG+="uaccess"
"#;

#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub model: String,
    pub serial: String,
    pub bootloader: String,
    pub sales_code: String,
}

/// Resolves the samloader-rs executable path from CLI override, ./samloader, or $PATH.
pub fn resolve_samloader(override_path: Option<&Path>) -> Result<PathBuf> {
    if let Some(p) = override_path {
        if p.exists() {
            return Ok(p.to_path_buf());
        }
        bail!("Specified --samloader-bin does not exist: {}", p.display());
    }

    let local = PathBuf::from("./samloader");
    if local.exists() {
        return Ok(local);
    }

    which::which("samloader").context(
        "Could not find `samloader` (topjohnwu/samloader-rs) in ./samloader or $PATH.\n\
         Download the Linux x86_64 release from https://github.com/topjohnwu/samloader-rs/releases",
    )
}

/// Checks lsusb for Samsung Download Mode (04e8:685d) and warns if missing or hung.
pub fn check_lsusb_samsung() -> Result<Option<String>> {
    let output = Command::new("lsusb")
        .output()
        .context("Failed to execute `lsusb`. Install `usbutils` (e.g., sudo dnf install usbutils).")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if line.contains("04e8:685d") {
            return Ok(Some(line.trim().to_string()));
        }
    }
    Ok(None)
}

/// Sanitizes the Linux USB stack: writes udev rules, pauses ModemManager, unloads cdc_acm.
pub fn sanitize_linux_usb(samloader_bin: &Path, skip_systemd: bool) -> Result<()> {
    println!("{}", "==> Configuring Linux USB environment for Samsung LOKE...".cyan().bold());

    // 1. Run samloader's built-in fix-usb if available, plus write explicit 04e8:685d rule
    let status = Command::new("sudo")
        .arg(samloader_bin)
        .arg("fix-usb")
        .status();

    if status.map(|s| !s.success()).unwrap_or(true) {
        println!("{}", "    Writing udev rule manually via sudo tee...".yellow());
        let tmp = tempfile::NamedTempFile::new()?;
        fs::write(tmp.path(), UDEV_RULE_CONTENT)?;
        let cp_status = Command::new("sudo")
            .args(["cp", tmp.path().to_str().unwrap(), UDEV_RULE_PATH])
            .status()
            .context("Failed to copy udev rules")?;
        if !cp_status.success() {
            bail!("Failed to install udev rule at {}", UDEV_RULE_PATH);
        }
        let _ = Command::new("sudo")
            .args(["udevadm", "control", "--reload-rules"])
            .status();
        let _ = Command::new("sudo")
            .args(["udevadm", "trigger"])
            .status();
    }

    // 2. Stop ModemManager so it doesn't grab 04e8:685d serial endpoints
    if !skip_systemd {
        println!("    Stopping ModemManager service...");
        let _ = Command::new("sudo")
            .args(["systemctl", "stop", "ModemManager"])
            .status();
    }

    // 3. Unload cdc_acm kernel driver
    println!("    Unloading cdc_acm kernel module...");
    let _ = Command::new("sudo")
        .args(["modprobe", "-r", "cdc_acm"])
        .status();

    // 4. Verify lsusb visibility
    match check_lsusb_samsung()? {
        Some(line) => {
            println!("{} {}", "✓ Detected on USB bus:".green().bold(), line);
            Ok(())
        }
        None => {
            println!(
                "{}",
                "⚠ Samsung Download Mode (04e8:685d) is NOT visible in `lsusb`.\n\
                 - Check if battery drained to 0% (charge on wall adapter for 15 mins).\n\
                 - Unplug the USB cable for 5 seconds and reconnect to a direct USB-A 2.0 port."
                    .yellow()
            );
            Ok(())
        }
    }
}

/// Runs `samloader detect` and parses Model, Serial, Bootloader, and Sales Code.
pub fn detect_device(samloader_bin: &Path, backend: &str) -> Result<DeviceInfo> {
    if check_lsusb_samsung()?.is_none() {
        bail!(
            "No Samsung device (04e8:685d) found in `lsusb`.\n\
             If a previous flash aborted, unplug the USB cable for 5 seconds and replug it to reset the LOKE USB endpoint."
        );
    }

    let output = Command::new(samloader_bin)
        .args(["--usb-backend", backend, "detect"])
        .output()
        .context("Failed to run `samloader detect`")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() {
        bail!(
            "`samloader detect` failed:\n{}\n{}\nHint: Unplug and replug the USB cable to clear any hung LOKE session.",
            stdout.trim(),
            stderr.trim()
        );
    }

    let extract_field = |label: &str| -> String {
        let pattern = format!(r"(?m)^\s*{}:\s*(.+)$", regex::escape(label));
        let re = Regex::new(&pattern).unwrap();
        re.captures(&stdout)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().trim().to_string())
            .unwrap_or_else(|| "UNKNOWN".to_string())
    };

    Ok(DeviceInfo {
        model: extract_field("Model"),
        serial: extract_field("Serial Number"),
        bootloader: extract_field("Bootloader Version"),
        sales_code: extract_field("Sales Code"),
    })
}
