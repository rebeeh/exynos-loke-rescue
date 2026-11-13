use colored::*;

/// Analyzes stderr/stdout from `samloader-rs` and prints concrete, root-cause recovery advice.
pub fn diagnose_flash_failure(combined_log: &str, is_custom_binary: bool) {
    println!("\n{}", "==================== DIAGNOSTIC REPORT ====================".red().bold());

    if combined_log.contains("LOKE returned general failure (FAIL!) [code: -1]") {
        if is_custom_binary {
            println!(
                "{}",
                "ROOT CAUSE: VaultKeeper / KG State Lock (LOKE FAIL! [code: -1])\n\
                 Samsung's bootloader rejected your unsigned custom binary at session close.\n\
                 FIX:\n\
                 1. Run `exynos-loke-rescue full-raw-restore` to boot into stock Samsung One UI.\n\
                 2. Connect to Wi-Fi, open Settings > Developer options, and verify `OEM unlocking` is ON.\n\
                 3. Re-enter Download Mode and run `flash-custom-recovery` again."
                    .yellow()
            );
        } else {
            println!(
                "{}",
                "ROOT CAUSE: Bootloader Binary Revision or Region Mismatch (LOKE FAIL! [code: -1])\n\
                 Check the top-left corner of your phone's Download Mode screen for red text:\n\
                 - `SW REV. CHECK FAIL`: Your stock firmware binary revision is older than the phone's fuse.\n\
                   Run `exynos-loke-rescue fetch-stock` to pull the latest matching bootloader revision.\n\
                 - `Only official released binaries`: Ensure you are pointing `--ap` to an official stock AP file."
                    .yellow()
            );
        }
    } else if combined_log.contains("Dynamic partition pre-flight check failed") {
        println!(
            "{}",
            "ROOT CAUSE: samloader-rs Dynamic Partition Parser Bug\n\
             Leftover custom ROM (LineageOS) partition metadata caused samloader-rs to miscalculate `super` size.\n\
             FIX: Do NOT pass `-a AP_*.tar.md5` directly. Use `exynos-loke-rescue rescue-boot` or\n\
             `exynos-loke-rescue full-raw-restore` to flash raw extracted partitions via `-p`."
                .yellow()
        );
    } else if combined_log.contains("Failed to send handshake")
        || combined_log.contains("USB bulk read failed")
        || combined_log.contains("Device not found")
    {
        println!(
            "{}",
            "ROOT CAUSE: Hung LOKE USB Endpoint\n\
             An aborted flash left the phone's USB stack unresponsive.\n\
             FIX:\n\
             1. Unplug the USB cable from the phone for 5 seconds.\n\
             2. Replug into a direct USB 2.0 port (avoid hubs/USB-C adapters).\n\
             3. Run `exynos-loke-rescue prep-usb` and retry."
                .yellow()
        );
    } else {
        println!(
            "{}",
            "Unknown LOKE/USB error encountered. Review the raw output above and ensure:\n\
             - Battery has >15% charge.\n\
             - ModemManager is stopped (`sudo systemctl stop ModemManager`).\n\
             - Cable was replugged after any previous failed flash."
                .yellow()
        );
    }

    println!("{}", "===========================================================\n".red().bold());
}
