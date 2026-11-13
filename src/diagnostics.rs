use colored::*;

/// Evaluates captured stdout/stderr from `samloader-rs` and prints deterministic
/// root-cause diagnosis and recovery steps.
pub fn diagnose_flash_failure(log: &str, is_custom_binary: bool) {
    println!(
        "\n{}",
        "==================== LOKE FAILURE DIAGNOSTICS ====================".red().bold()
    );

    if log.contains("LOKE returned general failure (FAIL!) [code: -1]") {
        if is_custom_binary {
            println!(
                "{}",
                "DIAGNOSIS: VaultKeeper (`KG State: Prenormal`) or Bootloader Signature Gate\n\
                 Samsung `sboot` rejected your unsigned custom binary during `CMD_SESSION_END`.\n\
                 REMEDIATION:\n\
                 1. Run `exynos-loke-rescue full-raw-restore --stock-dir <DIR>` to boot official One UI.\n\
                 2. Complete setup, connect to Wi-Fi, and open Settings > Developer options.\n\
                 3. Confirm `OEM unlocking` is ON and greyed out (syncs VaultKeeper to `Checking`).\n\
                 4. Re-enter Download Mode and re-run `flash-custom-recovery`."
                    .yellow()
            );
        } else {
            println!(
                "{}",
                "DIAGNOSIS: Bootloader Anti-Rollback (`SW REV. CHECK FAIL`) or Incomplete Session\n\
                 Inspect the top-left corner of your phone's Download Mode display for red text:\n\
                 - `SW REV. CHECK FAIL (BOOT/RECOVERY)`: Your stock archive has an older binary\n\
                   fuse bit than the bootloader currently flashed on the device. Run\n\
                   `exynos-loke-rescue fetch-stock` to download the latest matching revision.\n\
                 - `Only official released binaries are allowed`: Verify `--ap` points to an\n\
                   unmodified Samsung stock AP archive."
                    .yellow()
            );
        }
    } else if log.contains("Dynamic partition pre-flight check failed") {
        println!(
            "{}",
            "DIAGNOSIS: `samloader-rs` Dynamic Partition Metadata Pre-Flight Bug\n\
             Leftover custom ROM logical partition extents inside `super` caused `samloader-rs`\n\
             to abort before honoring `--skip-size-check`.\n\
             REMEDIATION:\n\
             Never pass `-a AP_*.tar.md5` directly when reverting from a custom ROM.\n\
             Use `exynos-loke-rescue rescue-boot` or `exynos-loke-rescue full-raw-restore`\n\
             to flash extracted `.img` payloads via explicit `-p` partition flags."
                .yellow()
        );
    } else if log.contains("Failed to send handshake")
        || log.contains("USB bulk read failed")
        || log.contains("Device not found")
    {
        println!(
            "{}",
            "DIAGNOSIS: Stalled LOKE USB Bulk Endpoint (`04e8:685d`)\n\
             A previous aborted session left the phone's USB OTG controller unresponsive.\n\
             REMEDIATION:\n\
             1. Physically unplug the USB cable from the device for 5 seconds.\n\
             2. Reconnect to a direct motherboard USB-A 2.0 port (avoid hubs).\n\
             3. Run `exynos-loke-rescue prep-usb` and re-execute the flash command."
                .yellow()
        );
    } else {
        println!(
            "{}",
            "DIAGNOSIS: Unclassified LOKE / Transport Fault\n\
             Verify:\n\
             - Battery charge is above 15%.\n\
             - `ModemManager` is stopped (`sudo systemctl stop ModemManager`).\n\
             - `cdc_acm` is unloaded (`sudo modprobe -r cdc_acm`)."
                .yellow()
        );
    }

    println!(
        "{}\n",
        "==================================================================".red().bold()
    );
}
