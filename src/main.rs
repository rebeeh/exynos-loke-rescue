#![allow(dead_code)]

mod archive;
mod cli;
mod diagnostics;
mod error;
mod flasher;
mod loke_proto;
mod nusb_transport;
mod pit;
mod usb;

use anyhow::{bail, Result};
use clap::Parser;
use cli::{Cli, Commands};
use colored::*;
use flasher::{FlashOptions, PartitionTarget};
use loke_proto::SessionEndSubcmd;
use nusb_transport::NativeLokeSession;
use pit::PitTable;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

fn resolve_reboot_mode(no_reboot: bool, redownload: bool) -> SessionEndSubcmd {
    if redownload {
        SessionEndSubcmd::RebootDownload
    } else if no_reboot {
        SessionEndSubcmd::CloseNoReboot
    } else {
        SessionEndSubcmd::RebootNormal
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::FixUsb { no_systemd } => {
            usb::sanitize_linux_usb(no_systemd)?;
        }

        Commands::Probe => {
            let session = NativeLokeSession::connect(cli.verbose)?;
            println!(
                "{} {:?}",
                "✓ LOKE Handshake Successful | Negotiated Protocol Tier:"
                    .green()
                    .bold(),
                session.tier
            );
            let pit = session.fetch_pit()?;
            pit.print_summary();
        }

        Commands::InspectPit { pit } => match pit {
            Some(path) => {
                let table = PitTable::from_file(&path)?;
                table.print_summary();
            }
            None => {
                let session = NativeLokeSession::connect(cli.verbose)?;
                let table = session.fetch_pit()?;
                table.print_summary();
            }
        },

        Commands::FetchStock {
            model,
            region,
            out_dir,
        } => {
            flasher::download_stock_firmware(
                cli.samloader_bin.as_deref(),
                &model,
                &region,
                &out_dir,
            )?;
        }

        Commands::EraseUserdata {
            no_reboot,
            redownload,
        } => {
            let reboot_mode = resolve_reboot_mode(no_reboot, redownload);
            flasher::execute_nand_erase(
                cli.transport,
                cli.samloader_bin.as_deref(),
                reboot_mode,
                cli.verbose,
            )?;
        }

        Commands::EmergencyWipeReboot => {
            println!(
                "{}",
                "==> Emergency Unbrick Triggered (Broken Power Button Recovery Bypass)"
                    .yellow()
                    .bold()
            );
            println!(
                "{}",
                "==> Executing hardware NAND erase on USERDATA directly from Download Mode..."
                    .cyan()
            );
            flasher::execute_nand_erase(
                cli.transport,
                cli.samloader_bin.as_deref(),
                SessionEndSubcmd::RebootNormal,
                cli.verbose,
            )?;
            println!(
                "{}",
                "✓ Wipe complete. Device auto-rebooting to One UI without requiring physical power key confirmation!"
                    .green()
                    .bold()
            );
        }

        Commands::FlashRaw {
            partitions,
            boot,
            recovery,
            vbmeta,
            dtbo,
            super_img,
            erase_userdata,
            skip_size_check,
            no_reboot,
            redownload,
        } => {
            let mut plan = Vec::new();

            if let Some(b) = boot {
                plan.push(PartitionTarget {
                    name: "BOOT".to_string(),
                    path: b,
                });
            }
            if let Some(r) = recovery {
                plan.push(PartitionTarget {
                    name: "RECOVERY".to_string(),
                    path: r,
                });
            }
            if let Some(v) = vbmeta {
                plan.push(PartitionTarget {
                    name: "VBMETA".to_string(),
                    path: v,
                });
            }
            if let Some(d) = dtbo {
                plan.push(PartitionTarget {
                    name: "DTBO".to_string(),
                    path: d,
                });
            }
            if let Some(s) = super_img {
                plan.push(PartitionTarget {
                    name: "SUPER".to_string(),
                    path: s,
                });
            }

            for chunk in partitions.chunks(2) {
                if chunk.len() == 2 {
                    plan.push(PartitionTarget {
                        name: chunk[0].to_uppercase(),
                        path: PathBuf::from(&chunk[1]),
                    });
                }
            }

            if plan.is_empty() && !erase_userdata {
                bail!("No partition targets or erase flag specified. Use -p <NAME> <FILE> or -e to erase.");
            }

            for target in &plan {
                if !target.path.exists() {
                    bail!("Partition image file not found: {}", target.path.display());
                }
            }

            let reboot_mode = resolve_reboot_mode(no_reboot, redownload);
            let options = FlashOptions {
                erase_userdata,
                skip_size_check,
                reboot_mode,
                is_custom_binary: false,
                verbose: cli.verbose,
            };
            flasher::execute_partition_flash(
                cli.transport,
                cli.samloader_bin.as_deref(),
                &plan,
                options,
            )?;
        }

        Commands::RescueBoot {
            ap,
            keep_extracted,
            erase_userdata,
            skip_size_check,
            no_reboot,
            redownload,
        } => {
            let ap_archive = archive::locate_ap_archive(&ap)?;
            let (_guard, work_dir): (Option<TempDir>, PathBuf) = match keep_extracted {
                Some(d) => {
                    fs::create_dir_all(&d)?;
                    (None, d)
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

            let plan = vec![
                PartitionTarget {
                    name: "BOOT".to_string(),
                    path: extracted["boot.img"].clone(),
                },
                PartitionTarget {
                    name: "VBMETA".to_string(),
                    path: extracted["vbmeta.img"].clone(),
                },
                PartitionTarget {
                    name: "RECOVERY".to_string(),
                    path: extracted["recovery.img"].clone(),
                },
            ];

            let reboot_mode = resolve_reboot_mode(no_reboot, redownload);
            let options = FlashOptions {
                erase_userdata,
                skip_size_check,
                reboot_mode,
                is_custom_binary: false,
                verbose: cli.verbose,
            };
            flasher::execute_partition_flash(
                cli.transport,
                cli.samloader_bin.as_deref(),
                &plan,
                options,
            )?;
        }

        Commands::FullRawRestore {
            stock_dir,
            keep_extracted,
            erase_userdata,
            skip_size_check,
            no_reboot,
            redownload,
        } => {
            let ap_archive = archive::locate_ap_archive(&stock_dir)?;
            let (_guard, work_dir): (Option<TempDir>, PathBuf) = match keep_extracted {
                Some(d) => {
                    fs::create_dir_all(&d)?;
                    (None, d)
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

            let plan = vec![
                PartitionTarget {
                    name: "BOOT".to_string(),
                    path: extracted["boot.img"].clone(),
                },
                PartitionTarget {
                    name: "VBMETA".to_string(),
                    path: extracted["vbmeta.img"].clone(),
                },
                PartitionTarget {
                    name: "RECOVERY".to_string(),
                    path: extracted["recovery.img"].clone(),
                },
                PartitionTarget {
                    name: "DTBO".to_string(),
                    path: extracted["dtbo.img"].clone(),
                },
                PartitionTarget {
                    name: "SUPER".to_string(),
                    path: extracted["super.img"].clone(),
                },
            ];

            let reboot_mode = resolve_reboot_mode(no_reboot, redownload);
            let options = FlashOptions {
                erase_userdata,
                skip_size_check,
                reboot_mode,
                is_custom_binary: false,
                verbose: cli.verbose,
            };
            flasher::execute_partition_flash(
                cli.transport,
                cli.samloader_bin.as_deref(),
                &plan,
                options,
            )?;
        }

        Commands::FlashCustomRecovery {
            recovery,
            vbmeta,
            boot,
            no_reboot,
        } => {
            for p in [&recovery, &vbmeta] {
                if !p.exists() {
                    bail!("Custom image not found: {}", p.display());
                }
            }

            let mut plan = vec![
                PartitionTarget {
                    name: "RECOVERY".to_string(),
                    path: recovery,
                },
                PartitionTarget {
                    name: "VBMETA".to_string(),
                    path: vbmeta,
                },
            ];

            if let Some(b) = boot {
                if !b.exists() {
                    bail!("Custom boot image not found: {}", b.display());
                }
                plan.push(PartitionTarget {
                    name: "BOOT".to_string(),
                    path: b,
                });
            }

            let reboot_mode = if no_reboot {
                SessionEndSubcmd::CloseNoReboot
            } else {
                SessionEndSubcmd::RebootNormal
            };
            let options = FlashOptions {
                erase_userdata: false,
                skip_size_check: false,
                reboot_mode,
                is_custom_binary: true,
                verbose: cli.verbose,
            };

            flasher::execute_partition_flash(
                cli.transport,
                cli.samloader_bin.as_deref(),
                &plan,
                options,
            )?;
        }
    }

    Ok(())
}
