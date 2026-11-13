use crate::error::RescueError;
use anyhow::{bail, Context, Result};
use colored::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const UDEV_RULE_DEST: &str = "/etc/udev/rules.d/60-exynos-loke-rescue.rules";
const UDEV_RULE_PAYLOAD: &str = r#"# Samsung Exynos / Qualcomm Download Mode (LOKE / Odin Wire Protocol)
SUBSYSTEM=="usb", ATTR{idVendor}=="04e8", ATTR{idProduct}=="685d", MODE="0666", GROUP="plugdev", TAG+="uaccess", ENV{ID_MM_DEVICE_IGNORE}="1"
SUBSYSTEM=="usb", ATTR{idVendor}=="04e8", ATTR{idProduct}=="6860", MODE="0666", GROUP="plugdev", TAG+="uaccess", ENV{ID_MM_DEVICE_IGNORE}="1"
SUBSYSTEM=="usb", ATTR{idVendor}=="04e8", ATTR{idProduct}=="6861", MODE="0666", GROUP="plugdev", TAG+="uaccess", ENV{ID_MM_DEVICE_IGNORE}="1"
"#;

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

pub fn scan_sysfs_for_samsung_download() -> Result<Option<String>> {
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
                        "Bus {:0>3} Device {:0>3}: ID 04e8:685d Samsung LOKE Download Mode ({} Mbps)",
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

pub fn is_cdc_acm_loaded() -> bool {
    fs::read_to_string("/proc/modules")
        .map(|content| content.lines().any(|l| l.starts_with("cdc_acm ")))
        .unwrap_or(false)
}

pub fn sanitize_linux_usb(no_systemd: bool) -> Result<()> {
    println!("{}", "==> Sanitizing Linux USB subsystem for Samsung LOKE...".cyan().bold());

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

    println!("    {} Installed {}", "✓".green(), UDEV_RULE_DEST);

    if !no_systemd {
        let _ = Command::new("sudo")
            .args(["systemctl", "stop", "ModemManager"])
            .status();
        println!("    {} Stopped ModemManager daemon", "✓".green());
    }

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
                "    {} Could not unload `cdc_acm` (may be built-in or bound)",
                "!".yellow()
            );
        }
    } else {
        println!("    {} `cdc_acm` kernel module is not loaded", "✓".green());
    }

    match scan_sysfs_for_samsung_download()? {
        Some(summary) => {
            println!("{} {}", "✓ Device active:".green().bold(), summary);
        }
        None => {
            println!(
                "{}",
                "⚠ Samsung Download Mode (04e8:685d) not found in /sys/bus/usb/devices.\n\
                 - Verify battery is charged (>15%) and device is in Download/Emergency Recovery Mode.\n\
                 - Unplug USB cable for 5 seconds and reconnect directly to a host USB port."
                    .yellow()
            );
        }
    }

    Ok(())
}
