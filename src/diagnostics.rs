use colored::*;

pub fn print_diagnostic_guidance(err_msg: &str, is_custom_binary: bool) {
    println!(
        "\n{}",
        "==================== LOKE DIAGNOSTIC ENGINE ====================".red().bold()
    );

    if err_msg.contains("FAIL! [code: -1]") || err_msg.contains("LokeFailure") {
        if is_custom_binary {
            println!(
                "{}",
                "ROOT CAUSE: VaultKeeper (`KG State: Prenormal`) Signature Rejection\n\
                 Samsung `sboot` rejected your unsigned custom image during `CMD_FILE_TRANSFER` commit.\n\
                 REMEDIATION:\n\
                 1. Run `exynos-loke-rescue full-raw-restore --stock-dir <DIR>` to boot official One UI.\n\
                 2. Connect to Wi-Fi, open Settings > Developer options, and confirm `OEM unlocking` is ON.\n\
                 3. Re-enter Download Mode and re-run `flash-custom-recovery`."
                    .yellow()
            );
        } else {
            println!(
                "{}",
                "ROOT CAUSE: Bootloader Fuse Anti-Rollback (`SW REV. CHECK FAIL`) or Unsigned Stock Path\n\
                 Inspect the top-left corner of your device's Download Mode screen for red text:\n\
                 - `SW REV. CHECK FAIL`: Stock archive bootloader fuse revision is older than device eFuse.\n\
                   Run `exynos-loke-rescue fetch-stock` to fetch the newest matching firmware.\n\
                 - `Only official released binaries`: Verify `--ap` points to an unmodified Samsung AP archive."
                    .yellow()
            );
        }
    } else if err_msg.contains("timed out") || err_msg.contains("DeviceNotDetected") {
        println!(
            "{}",
            "ROOT CAUSE: Hung LOKE USB Endpoint (`04e8:685d`)\n\
             REMEDIATION:\n\
             1. Physically unplug USB cable from the phone for 5 seconds.\n\
             2. Reconnect directly to a motherboard USB-A 2.0 port.\n\
             3. Run `exynos-loke-rescue prep-usb` and retry."
                .yellow()
        );
    } else {
        println!("{}", err_msg.yellow());
    }

    println!(
        "{}\n",
        "================================================================".red().bold()
    );
}
