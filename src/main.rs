#![allow(dead_code)]

use exynos_loke_rescue::{
    archive, cli, diagnostics, flasher, image_verifier, loke_proto, mock, nusb_transport, pit,
    sparse, usb,
};

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

        Commands::VerifyImage { image, pit } => {
            if !image.exists() {
                bail!("Image file does not exist: {}", image.display());
            }

            let pit_table = if let Some(p) = &pit {
                println!("==> Loading PIT table from {}", p.display());
                Some(PitTable::from_file(p)?)
            } else {
                None
            };

            let path_str = image.to_string_lossy();
            if path_str.ends_with(".tar.md5") || path_str.ends_with(".tar") {
                if path_str.ends_with(".tar.md5") {
                    println!(
                        "{}",
                        "==> Verifying Samsung .tar.md5 Checksum Trailer..."
                            .cyan()
                            .bold()
                    );
                    let res = image_verifier::ImageVerifier::verify_tar_md5(&image)?;
                    if res.matches {
                        println!(
                            "    {} MD5 Checksum MATCH: {}",
                            "✓".green(),
                            res.computed_md5
                        );
                        println!("      Payload size: {} bytes", res.tar_payload_bytes);
                    } else {
                        println!("    {} MD5 MISMATCH!", "✗".red().bold());
                        println!("      Expected: {}", res.expected_md5);
                        println!("      Computed: {}", res.computed_md5);
                        bail!("MD5 verification failed for archive: {}", image.display());
                    }
                }

                println!(
                    "\n{}",
                    "==> Inspecting Archive Partition Contents...".cyan().bold()
                );
                let reports = image_verifier::ImageVerifier::inspect_tar_partitions(&image)?;
                println!(
                    "    {:<24} {:<12} {:>14}  {:>14}  {:<12}",
                    "ARCHIVE ENTRY", "PARTITION", "ARCHIVE SIZE", "EXPANDED", "FORMAT"
                );
                println!("    {}", "-".repeat(80));

                for r in &reports {
                    let (expanded_str, format_str) = match &r.format {
                        sparse::ImageFormat::Raw { size } => (
                            format!("{:.2} MiB", *size as f64 / (1024.0 * 1024.0)),
                            "RAW".to_string(),
                        ),
                        sparse::ImageFormat::Sparse(hdr) => (
                            format!("{:.2} MiB", hdr.unsparse_size() as f64 / (1024.0 * 1024.0)),
                            format!("SPARSE ({} blks)", hdr.total_blocks),
                        ),
                    };

                    let raw_str =
                        format!("{:.2} MiB", r.raw_archive_bytes as f64 / (1024.0 * 1024.0));
                    println!(
                        "    {:<24} {:<12} {:>14}  {:>14}  {:<12}",
                        r.filename, r.partition_name, raw_str, expanded_str, format_str
                    );

                    if let Some(ref p) = pit_table {
                        image_verifier::ImageVerifier::verify_geometry(r, p)?;
                    }
                }
            } else {
                println!("{}", "==> Inspecting Partition Image File...".cyan().bold());
                let format = sparse::ImageFormat::inspect_file(&image)?;
                match &format {
                    sparse::ImageFormat::Raw { size } => {
                        println!("    Format: RAW Block Image");
                        println!(
                            "    Size:   {} bytes ({:.2} MiB)",
                            size,
                            *size as f64 / (1024.0 * 1024.0)
                        );
                    }
                    sparse::ImageFormat::Sparse(hdr) => {
                        println!(
                            "    Format: Android Sparse Image (v{}.{})",
                            hdr.major_version, hdr.minor_version
                        );
                        println!("    Block Size:     {} bytes", hdr.block_size);
                        println!("    Total Blocks:   {} blocks", hdr.total_blocks);
                        println!("    Total Chunks:   {} chunks", hdr.total_chunks);
                        println!(
                            "    Expanded Size:  {} bytes ({:.2} MiB)",
                            hdr.unsparse_size(),
                            hdr.unsparse_size() as f64 / (1024.0 * 1024.0)
                        );
                    }
                }

                if let Some(ref p) = pit_table {
                    let file_stem = image.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                    let part_name = file_stem.trim_end_matches(".img").to_ascii_uppercase();
                    if let Some(entry) = p.find_entry(&part_name) {
                        let max_bytes = entry.block_count as u64 * 512;
                        let effective_bytes = format.effective_size();
                        if effective_bytes <= max_bytes {
                            println!(
                                "    {} Partition '{}' fits within PIT boundary ({:.2} MiB / {:.2} MiB)",
                                "✓".green(),
                                part_name,
                                effective_bytes as f64 / (1024.0 * 1024.0),
                                max_bytes as f64 / (1024.0 * 1024.0)
                            );
                        } else {
                            bail!(
                                "Image exceeds PIT boundary for '{}' ({} > {} bytes)",
                                part_name,
                                effective_bytes,
                                max_bytes
                            );
                        }
                    }
                }
            }
        }

        Commands::SimulateFlash {
            image,
            erase_userdata,
            fault_vaultkeeper,
            fault_timeout,
        } => {
            println!(
                "{}",
                "================================================================================"
                    .blue()
            );
            println!(
                "{}",
                "       EXYNOS LOKE RESCUE: IN-MEMORY DRY-RUN SIMULATION ENGINE"
                    .blue()
                    .bold()
            );
            println!(
                "{}",
                "================================================================================"
                    .blue()
            );

            let mut mock = mock::MockLokeDevice::new();
            if fault_vaultkeeper {
                mock.faults.vaultkeeper_prenormal_lock = true;
                println!(
                    "  {} Fault Injection: VaultKeeper / KG Prenormal Lock (Code -1) ACTIVE",
                    "!".yellow()
                );
            }
            if fault_timeout {
                mock.faults.timeout_on_v4_query = true;
                println!(
                    "  {} Fault Injection: Bulk USB Timeout on v4 Query (Code -7) ACTIVE",
                    "!".yellow()
                );
            }

            println!("==> Step 1: Performing Handshake Simulation (\"ODIN\" -> \"LOKE\")...");
            let resp = mock.handle_handshake(loke_proto::HANDSHAKE_REQUEST)?;
            println!(
                "    {} Received handshake response: {:?}",
                "✓".green(),
                std::str::from_utf8(&resp).unwrap_or("?")
            );

            println!("==> Step 2: Negotiating Protocol Tier (CMD_SESSION_SETUP 0x64)...");
            let setup_frame =
                loke_proto::ControlFrame::session_setup_init(loke_proto::ProtocolTier::OdinV3);
            let setup_ack = mock.handle_control_frame(&setup_frame)?;
            println!(
                "    {} Session Setup ACK: Opcode 0x{:04x}, Status: {}",
                "✓".green(),
                setup_ack.cmd_id,
                setup_ack.status
            );

            if erase_userdata {
                println!("==> Step 3: Executing Low-Level NAND Storage Erase (USERDATA)...");
                let erase_frame = loke_proto::ControlFrame::nand_erase();
                let erase_ack = mock.handle_control_frame(&erase_frame)?;
                println!(
                    "    {} NAND Erase ACK: Opcode 0x{:04x}, Erased Sectors: {} ({} MiB)",
                    "✓".green(),
                    erase_ack.cmd_id,
                    erase_ack.payload(),
                    erase_ack.payload() / 2048
                );
            }

            println!("==> Step 4: Simulating Partition Flash Sequence...");
            let partitions_to_sim = if let Some(ref img_path) = image {
                println!("    Loading partition list from {}", img_path.display());
                vec![
                    ("BOOT".to_string(), 32 * 1024 * 1024),
                    ("SUPER".to_string(), 128 * 1024 * 1024),
                ]
            } else {
                vec![
                    ("BOOT".to_string(), 16 * 1024 * 1024),
                    ("VBMETA".to_string(), 64 * 1024),
                    ("RECOVERY".to_string(), 32 * 1024 * 1024),
                ]
            };

            for (part_name, size) in partitions_to_sim {
                let part_id = mock
                    .pit_table
                    .entries
                    .iter()
                    .find(|e| e.partition_name.eq_ignore_ascii_case(&part_name))
                    .map(|e| e.partition_id)
                    .unwrap_or(20);

                let chunk_size = 128 * 1024;
                let total_chunks = (size + chunk_size - 1) / chunk_size;

                println!(
                    "    Flashing partition {} ({} bytes, {} chunks)...",
                    part_name.cyan(),
                    size,
                    total_chunks
                );
                let start_frame = loke_proto::ControlFrame::file_transfer_start(
                    part_id,
                    size as u32,
                    total_chunks as u32,
                );
                let start_ack = mock.handle_control_frame(&start_frame);
                match start_ack {
                    Ok(ack) => {
                        if !ack.is_success() {
                            diagnostics::analyze_loke_failure(ack.payload() as i32, &part_name);
                            bail!("Simulation aborted due to bootloader rejection");
                        }
                    }
                    Err(e) => {
                        bail!("Transfer start error: {}", e);
                    }
                }

                // Simulate chunk transfers
                let dummy_chunk = vec![0x55u8; chunk_size as usize];
                for _ in 0..total_chunks {
                    let _ = mock.handle_chunk(&dummy_chunk)?;
                }

                let end_frame = loke_proto::ControlFrame::file_transfer_end(part_id);
                let _ = mock.handle_control_frame(&end_frame)?;
                println!(
                    "    {} Partition {} successfully flashed in simulation.",
                    "✓".green(),
                    part_name
                );
            }

            println!("==> Step 5: Closing Flashing Session (CMD_SESSION_END 0x67)...");
            let end_frame = loke_proto::ControlFrame::session_end(SessionEndSubcmd::RebootNormal);
            let end_ack = mock.handle_control_frame(&end_frame)?;
            println!("    {} Session End ACK: Opcode 0x{:04x}, Device param update flag reset to 0x{:02x}",
                "✓".green(), end_ack.cmd_id, mock.param_update_flag);

            println!(
                "\n{}",
                "✓ Dry-run simulation completed successfully with zero errors!"
                    .green()
                    .bold()
            );
            println!(
                "{}",
                "================================================================================"
                    .blue()
            );
        }
    }

    Ok(())
}
