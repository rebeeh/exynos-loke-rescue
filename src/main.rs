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
use flasher::PartitionTarget;
use nusb_transport::NativeLokeSession;
use pit::PitTable;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::PrepUsb { no_systemd } => {
            usb::sanitize_linux_usb(no_systemd)?;
        }

        Commands::Probe => {
            let session = NativeLokeSession::connect(cli.verbose)?;
            println!(
                "{} {:?}",
                "✓ LOKE Handshake Successful | Negotiated Protocol Tier:".green().bold(),
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

        Commands::RescueBoot {
            ap,
            keep_extracted,
            no_reboot,
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

            flasher::execute_partition_flash(
                cli.transport,
                cli.samloader_bin.as_deref(),
                &plan,
                no_reboot,
                false,
                cli.verbose,
            )?;
        }

        Commands::FullRawRestore {
            stock_dir,
            keep_extracted,
            no_reboot,
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

            flasher::execute_partition_flash(
                cli.transport,
                cli.samloader_bin.as_deref(),
                &plan,
                no_reboot,
                false,
                cli.verbose,
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

            flasher::execute_partition_flash(
                cli.transport,
                cli.samloader_bin.as_deref(),
                &plan,
                no_reboot,
                true,
                cli.verbose,
            )?;
        }
    }

    Ok(())
}
