use colored::*;

pub fn print_diagnostic_guidance(err_msg: &str, is_custom_binary: bool) {
    println!(
        "\n{}",
        "==================== LOKE DIAGNOSTIC ENGINE ===================="
            .red()
            .bold()
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
        "================================================================"
            .red()
            .bold()
    );
}

pub fn analyze_loke_failure(code: i32, part_name: &str) {
    println!(
        "\n{}",
        "==================== LOKE FAILURE ANALYSIS ===================="
            .red()
            .bold()
    );
    match code {
        -1 => {
            println!(
                "{}",
                format!(
                    "PARTITION: {}\n\
                     ROOT CAUSE: Bootloader rejected binary signature (VaultKeeper / KG State: Prenormal)\n\
                     Device bootloader enforces OEM signature verification and rejected unsigned image.\n\
                     REMEDIATION:\n\
                     1. Boot into stock One UI and complete setup wizard.\n\
                     2. Connect to Wi-Fi, open Developer Options, and verify OEM unlocking is ENABLED.\n\
                     3. Ensure VaultKeeper status in Download Mode reads 'KG STATE: CHECKING' or 'COMPLETED'.",
                    part_name
                )
                .yellow()
            );
        }
        -7 => {
            println!(
                "{}",
                format!(
                    "PARTITION: {}\n\
                     ROOT CAUSE: USB Bulk Read/Write Timeout (-7)\n\
                     Device bootloader hung or dropped USB communication.\n\
                     REMEDIATION:\n\
                     1. Reconnect USB cable directly without USB hubs.\n\
                     2. Use Odin v3 transport tier without v4 subcommands.",
                    part_name
                )
                .yellow()
            );
        }
        other => {
            println!(
                "{}",
                format!(
                    "PARTITION: {}\n\
                     ERROR CODE: {}\n\
                     Inspect device screen for bootloader error message.",
                    part_name, other
                )
                .yellow()
            );
        }
    }
    println!(
        "{}\n",
        "================================================================"
            .red()
            .bold()
    );
}
