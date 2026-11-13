mod archive;
mod cli;
mod diagnostics;
mod error;
mod flasher;
mod pit;
mod usb;

use anyhow::{bail, Result};
use clap::Parser;
use cli::{Cli, Commands};
use colored::*;
use flasher::PartitionMapping;
use pit::PitTable;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

fn main() -> Result<()> {
    let cli = Cli::parse();
    let samloader_bin = usb::resolve_samloader(cli.samloader_bin.as_deref())?;
    let backend = cli.backend.as_str();

    match cli.command {
        Commands::PrepUsb { no_systemd } => {
            usb::sanitize_linux_usb(&samloader_bin, no_systemd)?;
        }

        Commands::Inspect => {
            let info = usb::detect_device(&samloader_bin, backend)?;
            println!("{}", "==> Connected Samsung Exynos Device:".green().bold());
            println!("    Model:              {}", info.model);
            println!("    Serial Number:      {}", info.serial);
            println!("    Bootloader Version: {}", info.bootloader);
            println!("    Sales Code (CSC):   {}", info.sales_code);
            println!("    Storage Hardware:   {}", info.storage);
        }

        Commands::InspectPit { pit } => {
            match pit {
                Some(pit_path) => {
                    let table = PitTable::from_file(&pit_path)?;
                    table.print_summary();
                }
                None => {
                    let _ = usb::detect_device(&samloader_bin, backend)?;
                    let tmp = TempDir::new()?;
                    let dump_path = tmp.path().join("device.pit");
                    flasher::dump_device_pit(&samloader_bin, backend, &dump_path)?;
                    let table = PitTable::from_file(&dump_path)?;
                    table.print_summary();
                }
            }
        }

        Commands::FetchStock {
            model,
            region,
            out_dir,
        } => {
            let (target_model, target_region) = match (model, region) {
                (Some(m), Some(r)) => (m, r),
                (m_opt, r_opt) => {
                    let info = usb::detect_device(&samloader_bin, backend)?;
                    (
                        m_opt.unwrap_or(info.model),
                        r_opt.unwrap_or(info.sales_code),
                    )
                }
            };
            flasher::download_stock_firmware(
                &samloader_bin,
                &target_model,
                &target_region,
                &out_dir,
            )?;
        }

        Commands::RescueBoot {
            ap,
            keep_extracted,
            no_reboot,
        } => {
            let _ = usb::detect_device(&samloader_bin, backend)?;
            let ap_archive = archive::locate_ap_archive(&ap)?;

            let (_tmp_guard, work_dir): (Option<TempDir>, PathBuf) = match keep_extracted {
                Some(dir) => {
                    fs::create_dir_all(&dir)?;
                    (None, dir)
                }
                None => {
                    let tmp = TempDir::new()?;
                    let p = tmp.path().to_path_buf();
                    (Some(tmp), p)
                }
            };

            let targets = ["boot.img", "vbmeta.img", "recovery.img"];
            let extracted =
                archive::extract_and_decompress_images(&ap_archive, &work_dir, &targets)?;

            let mappings = vec![
                PartitionMapping {
                    partition_name: "BOOT".to_string(),
                    image_path: extracted["boot.img"].clone(),
                },
                PartitionMapping {
                    partition_name: "VBMETA".to_string(),
                    image_path: extracted["vbmeta.img"].clone(),
                },
                PartitionMapping {
                    partition_name: "RECOVERY".to_string(),
                    image_path: extracted["recovery.img"].clone(),
                },
            ];

            flasher::flash_raw_partitions(&samloader_bin, backend, &mappings, no_reboot, false)?;

            println!(
                "\n{}",
                "Next Steps:\n\
                 1. If the phone enters Samsung Stock Recovery due to leftover custom ROM partitions,\n\
                    select `Wipe data/factory reset` -> `Reboot to bootloader`.\n\
                 2. If the phone bootloops because `super` still contains a custom ROM, run:\n\
                    `exynos-loke-rescue full-raw-restore --stock-dir <DIR>`"
                    .cyan()
            );
        }

        Commands::FullRawRestore {
            stock_dir,
            keep_extracted,
            no_reboot,
        } => {
            let _ = usb::detect_device(&samloader_bin, backend)?;
            let ap_archive = archive::locate_ap_archive(&stock_dir)?;

            let (_tmp_guard, work_dir): (Option<TempDir>, PathBuf) = match keep_extracted {
                Some(dir) => {
                    fs::create_dir_all(&dir)?;
                    (None, dir)
                }
                None => {
                    let tmp = TempDir::new()?;
                    let p = tmp.path().to_path_buf();
                    (Some(tmp), p)
                }
            };

            let targets = [
                "boot.img",
                "vbmeta.img",
                "recovery.img",
                "dtbo.img",
                "super.img",
            ];
            let extracted =
                archive::extract_and_decompress_images(&ap_archive, &work_dir, &targets)?;

            let mappings = vec![
                PartitionMapping {
                    partition_name: "BOOT".to_string(),
                    image_path: extracted["boot.img"].clone(),
                },
                PartitionMapping {
                    partition_name: "VBMETA".to_string(),
                    image_path: extracted["vbmeta.img"].clone(),
                },
                PartitionMapping {
                    partition_name: "RECOVERY".to_string(),
                    image_path: extracted["recovery.img"].clone(),
                },
                PartitionMapping {
                    partition_name: "DTBO".to_string(),
                    image_path: extracted["dtbo.img"].clone(),
                },
                PartitionMapping {
                    partition_name: "SUPER".to_string(),
                    image_path: extracted["super.img"].clone(),
                },
            ];

            flasher::flash_raw_partitions(&samloader_bin, backend, &mappings, no_reboot, false)?;

            println!(
                "\n{}",
                "✓ Raw stock OS partitions flashed successfully!\n\
                 1. If the phone boots into Samsung Stock Recovery, select `Wipe data/factory reset`.\n\
                 2. Boot into One UI, connect to Wi-Fi, and confirm `OEM unlocking` in Developer Options."
                    .green()
            );
        }

        Commands::FlashCustomRecovery {
            recovery,
            vbmeta,
            boot,
            no_reboot,
        } => {
            for p in [&recovery, &vbmeta] {
                if !p.exists() {
                    bail!("Custom image file not found: {}", p.display());
                }
            }

            let _ = usb::detect_device(&samloader_bin, backend)?;
            let mut mappings = vec![
                PartitionMapping {
                    partition_name: "RECOVERY".to_string(),
                    image_path: recovery,
                },
                PartitionMapping {
                    partition_name: "VBMETA".to_string(),
                    image_path: vbmeta,
                },
            ];

            if let Some(b) = boot {
                if !b.exists() {
                    bail!("Custom boot image file not found: {}", b.display());
                }
                mappings.push(PartitionMapping {
                    partition_name: "BOOT".to_string(),
                    image_path: b,
                });
            }

            flasher::flash_raw_partitions(&samloader_bin, backend, &mappings, no_reboot, true)?;
        }
    }

    Ok(())
}
