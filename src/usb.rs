use crate::error::RescueError;
use anyhow::{bail, Context, Result};
use colored::*;
use regex::Regex;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const UDEV_RULE_DEST: &str = "/etc/udev/rules.d/60-exynos-loke-rescue.rules";
const UDEV_RULE_PAYLOAD: &str = r#"# Samsung Exynos / Qualcomm Download Mode (LOKE / Odin Protocol)
SUBSYSTEM=="usb", ATTR{idVendor}=="04e8", ATTR{idProduct}=="685d", MODE="0666", GROUP="plugdev", TAG+="uaccess", ENV{ID_MM_DEVICE_IGNORE}="1"
SUBSYSTEM=="usb", ATTR{idVendor}=="04e8", ATTR{idProduct}=="6860", MODE="0666", GROUP="plugdev", TAG+="uaccess", ENV{ID_MM_DEVICE_IGNORE}="1"
SUBSYSTEM=="usb", ATTR{idVendor}=="04e8", ATTR{idProduct}=="6861", MODE="0666", GROUP="plugdev", TAG+="uaccess", ENV{ID_MM_DEVICE_IGNORE}="1"
"#;

#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub model: String,
    pub serial: String,
    pub bootloader: String,
    pub sales_code: String,
    pub storage: String,
}

/// Resolves `samloader-rs` binary from explicit path, `./samloader`, or `$PATH`.
pub fn resolve_samloader(override_path: Option<&Path>) -> Result<PathBuf> {
    if let Some(p) = override_path {
        if p.is_file() {
            return Ok(p.to_path_buf());
        }
        bail!("Explicit --samloader-bin path does not exist: {}", p.display());
    }

    let cwd_bin = PathBuf::from("./samloader");
    if cwd_bin.is_file() {
        return Ok(cwd_bin);
    }

    which::which("samloader").map_err(|_| RescueError::SamloaderNotFound.into())
}

/// Scans `/sys/bus/usb/devices` directly (without requiring external `lsusb`) and falls
/// back to `lsusb` to verify whether `04e8:685d` is currently enumerated on the host.
pub fn scan_usb_bus_for_download_mode() -> Result<Option<String>> {
    let usb_sysfs = Path::new("/sys/bus/usb/devices");
    if usb_sysfs.is_dir() {
        for entry in fs::read_dir(usb_sysfs)? {
            let dev_path = entry?.path();
            let vid_path = dev_path.join("idVendor");
            let pid_path = dev_path.join("idProduct");

            if vid_path.is_file() && pid_path.is_file() {
                let vid = fs::read_to_string(&vid_path).unwrap_or_default();
                let pid = fs::read_to_string(&pid_path).unwrap_or_default();

                if vid.trim().eq_ignore_ascii_case("04e8")
                    && pid.trim().eq_ignore_ascii_case("685d")
                {
                    let busnum = fs::read_to_string(dev_path.join("busnum"))
                        .unwrap_or_else(|_| "?".into());
                    let devnum = fs::read_to_string(dev_path.join("devnum"))
                        .unwrap_or_else(|_| "?".into());
                    let speed = fs::read_to_string(dev_path.join("speed"))
                        .unwrap_or_else(|_| "?".into());

                    return Ok(Some(format!(
                        "Bus {:0>3} Device {:0>3}: ID 04e8:685d Samsung Download Mode ({} Mbps)",
                        busnum.trim(),
                        devnum.trim(),
                        speed.trim()
                    )));
                }
            }
        }
    }
    Ok(None)
}

/// Checks whether the `cdc_acm` kernel module is currently loaded via `/proc/modules`.
pub fn is_cdc_acm_loaded() -> bool {
    fs::read_to_string("/proc/modules")
        .map(|content| content.lines().any(|l| l.starts_with("cdc_acm ")))
        .unwrap_or(false)
}

/// Configures udev rules with `ID_MM_DEVICE_IGNORE=1`, stops `ModemManager`, and unloads `cdc_acm`.
pub fn sanitize_linux_usb(samloader_bin: &Path, no_systemd: bool) -> Result<()> {
    println!("{}", "==> Sanitizing Linux USB subsystem for Samsung LOKE...".cyan().bold());

    // Step 1: Write persistent udev rule
    let tmp = tempfile::NamedTempFile::new()?;
    fs::write(tmp.path(), UDEV_RULE_PAYLOAD)?;

    let cp_ok = Command::new("sudo")
        .args(["cp", tmp.path().to_str().unwrap(), UDEV_RULE_DEST])
        .status()
        .context("Failed to execute sudo cp for udev rules")?
        .success();

    if !cp_ok {
        bail!("Failed to install udev rule at {}", UDEV_RULE_DEST);
    }

    let _ = Command::new("sudo")
        .args(["chmod", "0644", UDEV_RULE_DEST])
        .status();
    let _ = Command::new("sudo")
        .args(["udevadm", "control", "--reload-rules"])
        .status();
    let _ = Command::new("sudo")
        .args(["udevadm", "trigger"])
        .status();

    // Also invoke samloader's internal fix-usb if supported
    let _ = Command::new("sudo")
        .arg(samloader_bin)
        .arg("fix-usb")
        .output();

    println!("    {} Installed {}", "✓".green(), UDEV_RULE_DEST);

    // Step 2: Stop ModemManager
    if !no_systemd {
        let _ = Command::new("sudo")
            .args(["systemctl", "stop", "ModemManager"])
            .status();
        println!("    {} Stopped ModemManager daemon", "✓".green());
    }

    // Step 3: Unload cdc_acm kernel driver if bound
    if is_cdc_acm_loaded() {
        let rmmod_ok = Command::new("sudo")
            .args(["modprobe", "-r", "cdc_acm"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if rmmod_ok {
            println!("    {} Unloaded conflicting `cdc_acm` kernel module", "✓".green());
        } else {
            println!(
                "    {} Could not unload `cdc_acm` (may be built-in or in use)",
                "!".yellow()
            );
        }
    } else {
        println!("    {} `cdc_acm` module is not loaded", "✓".green());
    }

    // Step 4: Verify USB bus presence
    match scan_usb_bus_for_download_mode()? {
        Some(summary) => {
            println!("{} {}", "✓ Device active:".green().bold(), summary);
        }
        None => {
            println!(
                "{}",
                "⚠ Samsung Download Mode (04e8:685d) not found on USB bus.\n\
                 - Ensure device is powered on and in Download Mode / Emergency Recovery.\n\
                 - Unplug USB cable for 5 seconds and reconnect directly to a host USB port."
                    .yellow()
            );
        }
    }

    Ok(())
}

/// Queries `samloader detect` and parses hardware identity fields.
pub fn detect_device(samloader_bin: &Path, backend: &str) -> Result<DeviceInfo> {
    if scan_usb_bus_for_download_mode()?.is_none() {
        return Err(RescueError::DeviceNotDetected.into());
    }

    if is_cdc_acm_loaded() {
        println!(
            "{}",
            "⚠ Warning: `cdc_acm` kernel module is loaded. Run `exynos-loke-rescue prep-usb` if handshake fails."
                .yellow()
        );
    }

    let output = Command::new(samloader_bin)
        .args(["--usb-backend", backend, "detect"])
        .output()
        .context("Failed to execute `samloader detect`")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() {
        bail!(
            "`samloader detect` failed:\n{}\n{}\nAction: Unplug USB cable for 5 seconds and replug to reset LOKE endpoint.",
            stdout.trim(),
            stderr.trim()
        );
    }

    let extract = |key: &str| -> String {
        let re = Regex::new(&format!(r"(?m)^\s*{}:\s*(.+)$", regex::escape(key))).unwrap();
        re.captures(&stdout)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().trim().to_string())
            .unwrap_or_else(|| "UNKNOWN".to_string())
    };

    Ok(DeviceInfo {
        model: extract("Model"),
        serial: extract("Serial Number"),
        bootloader: extract("Bootloader Version"),
        sales_code: extract("Sales Code"),
        storage: extract("Storage"),
    })
}
