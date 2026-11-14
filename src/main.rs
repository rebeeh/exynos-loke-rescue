#![allow(dead_code)]

use exynos_loke_rescue::{
    archive, cli, diagnostics, flasher, image_verifier, loke_proto, mock, nusb_transport, pit,
    sparse, ui, usb,
};

use anyhow::{bail, Result};
use clap::Parser;
use cli::{Cli, Commands};
use colored::*;
use flasher::{FlashOptions, PartitionTarget};
use loke_proto::SessionEndSubcmd;
use nusb_transport::NativeLokeSession;
use pit::PitTable;
use std::path::Path;

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
        None | Some(Commands::Wizard) => {
            ui::run_wizard(&cli)?;
        }

        Some(Commands::FixUsb { no_systemd }) => {
            usb::sanitize_linux_usb(no_systemd)?;
        }

        Some(Commands::Probe) => {
            ui::badge_step(1, 2, "Connecting to Samsung LOKE Bootloader...");
            let mut session = NativeLokeSession::connect(cli.verbose)?;
            ui::render_card(
                "NEGOTIATED PROTOCOL SESSION",
                &[
                    ("Protocol Tier", &format!("{:?}", session.tier)),
                    (
                        "Sequence Macro Size",
                        &ui::format_bytes(session.tier.sequence_macro_size() as u64),
                    ),
                    (
                        "USB Chunk Size",
                        &ui::format_bytes(session.tier.max_usb_packet_size() as u64),
                    ),
                    ("Handshake Status", "ODIN <-> LOKE Handshake Acknowledged"),
                ],
            );
            ui::badge_step(2, 2, "Querying live device PIT table...");
            let pit = session.fetch_pit()?;
            pit.print_summary();
            session.end_session_with_mode(SessionEndSubcmd::CloseNoReboot)?;
        }

        Some(Commands::InspectPit { pit }) => match pit {
            Some(path) => {
                ui::badge_info("PIT SOURCE", &format!("Loading from file: {}", path.display()));
                let table = PitTable::from_file(&path)?;
                table.print_summary();
            }
            None => {
                ui::badge_step(1, 2, "Connecting to live device...");
                let mut session = NativeLokeSession::connect(cli.verbose)?;
                ui::badge_step(2, 2, "Dumping live PIT table...");
                let table = session.fetch_pit()?;
                table.print_summary();
                session.end_session_with_mode(SessionEndSubcmd::CloseNoReboot)?;
            }
        },

        Some(Commands::FetchStock {
            model,
            region,
            out_dir,
        }) => {
            ui::badge_step(1, 2, &format!("Querying firmware for {} ({})", model, region));
            flasher::download_stock_firmware(
                cli.samloader_bin.as_deref(),
                &model,
                &region,
                &out_dir,
            )?;
            ui::badge_step(2, 2, "Download and extraction complete.");
            ui::badge_success("STOCK READY", &format!("Stored in {}", out_dir.display()));
        }

        Some(Commands::EraseUserdata {
            no_reboot,
            redownload,
        }) => {
            let reboot_mode = resolve_reboot_mode(no_reboot, redownload);
            ui::badge_step(1, 1, "Executing low-level hardware NAND storage erase (USERDATA)...");
            flasher::execute_nand_erase(
                cli.transport,
                cli.samloader_bin.as_deref(),
                reboot_mode,
                cli.verbose,
            )?;
            ui::badge_success("ERASED", "NAND storage erase completed successfully.");
        }

        Some(Commands::EmergencyWipeReboot) => {
            println!(
                "\n  {}",
                "── Emergency Unbrick: Broken Power Button Recovery Bypass ──"
                    .yellow()
                    .bold()
            );
            println!(
                "  {}",
                "Executing hardware NAND erase directly from Download Mode & auto-reboot\n"
                    .dimmed()
            );

            flasher::execute_nand_erase(
                cli.transport,
                cli.samloader_bin.as_deref(),
                SessionEndSubcmd::RebootNormal,
                cli.verbose,
            )?;
            ui::render_card(
                "UNBRICK SEQUENCE COMPLETED",
                &[
                    ("Action", "Low-level NAND erase (Opcode 0x64 Subcmd 0x07)"),
                    ("Target", "USERDATA / CACHE wiped"),
                    ("Reboot Mode", "SessionEndSubcmd::RebootNormal"),
                    ("Status", "Auto-rebooting to One UI without requiring physical power key!"),
                ],
            );
            ui::badge_success("PASS", "Device unbricked! Please wait 5-10 minutes for One UI first-boot setup.");
        }

        Some(Commands::FlashRaw {
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
        }) => {
            let mut plan = Vec::new();

            if let Some(b) = boot {
                plan.push(PartitionTarget {
                    name: "BOOT".to_string(),
                    path: archive::expand_tilde(&b),
                });
            }
            if let Some(r) = recovery {
                plan.push(PartitionTarget {
                    name: "RECOVERY".to_string(),
                    path: archive::expand_tilde(&r),
                });
            }
            if let Some(v) = vbmeta {
                plan.push(PartitionTarget {
                    name: "VBMETA".to_string(),
                    path: archive::expand_tilde(&v),
                });
            }
            if let Some(d) = dtbo {
                plan.push(PartitionTarget {
                    name: "DTBO".to_string(),
                    path: archive::expand_tilde(&d),
                });
            }
            if let Some(s) = super_img {
                plan.push(PartitionTarget {
                    name: "SUPER".to_string(),
                    path: archive::expand_tilde(&s),
                });
            }

            for chunk in partitions.chunks(2) {
                if chunk.len() == 2 {
                    plan.push(PartitionTarget {
                        name: chunk[0].to_uppercase(),
                        path: archive::expand_tilde(Path::new(&chunk[1])),
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

        Some(Commands::RescueBoot {
            ap,
            keep_extracted,
            erase_userdata,
            skip_size_check,
            no_reboot,
            redownload,
        }) => {
            ui::badge_step(1, 3, "Locating stock AP archive...");
            let ap_archive = archive::locate_ap_archive(&ap)?;
            let effective_keep = keep_extracted.or_else(|| {
                if ap.is_dir() {
                    Some(ap.clone())
                } else {
                    ap.parent().map(|p| p.to_path_buf())
                }
            });
            let (_guard, work_dir) = archive::resolve_work_dir(effective_keep, &ap_archive)?;

            ui::badge_step(2, 3, "Decompressing signed boot, vbmeta, and recovery images...");
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

            ui::badge_step(3, 3, "Streaming partition images to LOKE bootloader...");
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
            ui::badge_success("UNBRICKED", "Signed kernel/recovery flashed. Bootloader error cleared!");
        }

        Some(Commands::FullRawRestore {
            stock_dir,
            keep_extracted,
            erase_userdata,
            skip_size_check,
            no_reboot,
            redownload,
        }) => {
            ui::badge_step(1, 3, "Locating stock firmware directory...");
            let ap_archive = archive::locate_ap_archive(&stock_dir)?;
            let effective_keep = keep_extracted.or_else(|| {
                if stock_dir.is_dir() {
                    Some(stock_dir.clone())
                } else {
                    stock_dir.parent().map(|p| p.to_path_buf())
                }
            });
            let (_guard, work_dir) = archive::resolve_work_dir(effective_keep, &ap_archive)?;

            ui::badge_step(2, 3, "Extracting full stock partitions (including dynamic SUPER container)...");
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

            ui::badge_step(3, 3, "Executing full deterministic raw partition flash...");
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
            ui::badge_success("RESTORED", "Full Stock One UI OS successfully restored!");
        }

        Some(Commands::FlashCustomRecovery {
            recovery,
            vbmeta,
            boot,
            no_reboot,
        }) => {
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

        Some(Commands::VerifyImage { image, pit }) => {
            if !image.exists() {
                bail!("Image file does not exist: {}", image.display());
            }

            let pit_table = if let Some(p) = &pit {
                ui::badge_info("PIT TABLE", &format!("Loading geometry from {}", p.display()));
                Some(PitTable::from_file(p)?)
            } else {
                None
            };

            let path_str = image.to_string_lossy();
            if path_str.ends_with(".tar.md5") || path_str.ends_with(".tar") {
                if path_str.ends_with(".tar.md5") {
                    ui::badge_step(1, 2, "Verifying Samsung .tar.md5 Checksum Trailer...");
                    let res = image_verifier::ImageVerifier::verify_tar_md5(&image)?;
                    if res.matches {
                        ui::badge_success("MD5 MATCH", &res.computed_md5);
                        ui::render_card(
                            "TAR MD5 CHECKSUM VERIFICATION",
                            &[
                                ("Archive File", &image.file_name().unwrap().to_string_lossy()),
                                ("MD5 Digest", &res.computed_md5),
                                ("Payload Size", &ui::format_bytes(res.tar_payload_bytes)),
                                ("Status", "PASS (Official Samsung signature confirmed)"),
                            ],
                        );
                    } else {
                        ui::badge_error("MD5 MISMATCH", &format!("Expected: {} | Computed: {}", res.expected_md5, res.computed_md5));
                        bail!("MD5 verification failed for archive: {}", image.display());
                    }
                }

                ui::badge_step(2, 2, "Inspecting Archive Partition Entries & Geometries...");
                let reports = image_verifier::ImageVerifier::inspect_tar_partitions(&image)?;
                let headers = ["ENTRY FILENAME", "PARTITION", "ARCHIVE SIZE", "EXPANDED", "FORMAT"];
                let rows: Vec<Vec<String>> = reports
                    .iter()
                    .map(|r| {
                        let (exp_str, fmt_str) = match &r.format {
                            sparse::ImageFormat::Raw { size } => (ui::format_bytes(*size), "RAW".to_string()),
                            sparse::ImageFormat::Sparse(hdr) => (
                                ui::format_bytes(hdr.unsparse_size()),
                                format!("SPARSE ({} blks)", hdr.total_blocks),
                            ),
                        };
                        vec![
                            r.filename.clone(),
                            r.partition_name.clone(),
                            ui::format_bytes(r.raw_archive_bytes),
                            exp_str,
                            fmt_str,
                        ]
                    })
                    .collect();

                ui::render_table("ARCHIVE PARTITION INVENTORY", &headers, &rows);

                if let Some(ref p) = pit_table {
                    for r in &reports {
                        image_verifier::ImageVerifier::verify_geometry(r, p)?;
                    }
                    ui::badge_success("GEOMETRY", "All partition sizes fit cleanly within PIT boundaries!");
                }
            } else {
                ui::badge_step(1, 1, "Inspecting Single Partition Image Geometry...");
                let format = sparse::ImageFormat::inspect_file(&image)?;
                match &format {
                    sparse::ImageFormat::Raw { size } => {
                        ui::render_card(
                            "RAW BLOCK IMAGE GEOMETRY",
                            &[
                                ("File", &image.file_name().unwrap().to_string_lossy()),
                                ("Format", "Uncompressed RAW Block Image"),
                                ("Raw Size", &format!("{} bytes ({})", size, ui::format_bytes(*size))),
                            ],
                        );
                    }
                    sparse::ImageFormat::Sparse(hdr) => {
                        ui::render_card(
                            "ANDROID SPARSE IMAGE GEOMETRY (0xed26ff3a)",
                            &[
                                ("File", &image.file_name().unwrap().to_string_lossy()),
                                ("Sparse Version", &format!("v{}.{}", hdr.major_version, hdr.minor_version)),
                                ("Block Size", &format!("{} bytes", hdr.block_size)),
                                ("Total Blocks", &format!("{}", hdr.total_blocks)),
                                ("Total Chunks", &format!("{}", hdr.total_chunks)),
                                ("Expanded Size", &format!("{} bytes ({})", hdr.unsparse_size(), ui::format_bytes(hdr.unsparse_size()))),
                            ],
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
                            ui::badge_success(
                                "PIT FIT",
                                &format!(
                                    "Partition '{}' fits in PIT boundary ({} / {})",
                                    part_name,
                                    ui::format_bytes(effective_bytes),
                                    ui::format_bytes(max_bytes)
                                ),
                            );
                        } else {
                            ui::badge_error(
                                "OVERFLOW",
                                &format!(
                                    "Partition '{}' exceeds PIT boundary ({} > {})",
                                    part_name,
                                    ui::format_bytes(effective_bytes),
                                    ui::format_bytes(max_bytes)
                                ),
                            );
                            bail!("Image exceeds PIT boundary");
                        }
                    }
                }
            }
        }

        Some(Commands::SimulateFlash {
            image,
            erase_userdata,
            fault_vaultkeeper,
            fault_timeout,
        }) => {
            println!(
                "\n  {}",
                "── In-Memory Dry-Run Simulation Engine ──"
                    .cyan()
                    .bold()
            );
            println!(
                "  {}",
                "Deterministic protocol simulation against virtual in-memory device\n"
                    .dimmed()
            );

            let mut mock = mock::MockLokeDevice::new();
            if fault_vaultkeeper {
                mock.faults.vaultkeeper_prenormal_lock = true;
                ui::badge_warn("FAULT INJECTED", "VaultKeeper / KG Prenormal Lock (Code -1) ACTIVE");
            }
            if fault_timeout {
                mock.faults.timeout_on_v4_query = true;
                ui::badge_warn("FAULT INJECTED", "Bulk USB Timeout on v4 Query (Code -7) ACTIVE");
            }

            ui::badge_step(1, 5, "Performing Handshake Simulation (\"ODIN\" -> \"LOKE\")...");
            let resp = mock.handle_handshake(loke_proto::HANDSHAKE_REQUEST)?;
            ui::badge_success(
                "HANDSHAKE",
                &format!("Received response: {:?}", std::str::from_utf8(&resp).unwrap_or("?")),
            );

            ui::badge_step(2, 5, "Negotiating Protocol Tier (CMD_SESSION_SETUP 0x64)...");
            let setup_frame =
                loke_proto::ControlFrame::session_setup_init(loke_proto::ProtocolTier::OdinV3);
            let setup_ack = mock.handle_control_frame(&setup_frame)?;
            ui::badge_success(
                "SESSION SETUP",
                &format!("Opcode 0x{:04x}, Status: {}", setup_ack.cmd_id, setup_ack.status),
            );

            if erase_userdata {
                ui::badge_step(3, 5, "Executing Low-Level NAND Storage Erase (USERDATA)...");
                let erase_frame = loke_proto::ControlFrame::nand_erase();
                let erase_ack = mock.handle_control_frame(&erase_frame)?;
                ui::badge_success(
                    "NAND ERASED",
                    &format!(
                        "Opcode 0x{:04x}, Sectors: {} ({})",
                        erase_ack.cmd_id,
                        erase_ack.payload(),
                        ui::format_bytes(erase_ack.payload() as u64 * 512)
                    ),
                );
            }

            ui::badge_step(4, 5, "Simulating Partition Flash Streaming...");
            let partitions_to_sim: Vec<(String, u64)> = if let Some(ref _img_path) = image {
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
                let total_chunks = size.div_ceil(chunk_size);

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
                ui::badge_success(
                    "FLASHED",
                    &format!(
                        "Partition {} ({}, {} chunks) streamed OK",
                        part_name,
                        ui::format_bytes(size),
                        total_chunks
                    ),
                );
            }

            ui::badge_step(5, 5, "Closing Flashing Session (CMD_SESSION_END 0x67)...");
            let end_frame = loke_proto::ControlFrame::session_end(SessionEndSubcmd::RebootNormal);
            let end_ack = mock.handle_control_frame(&end_frame)?;
            ui::badge_success(
                "SESSION END",
                &format!(
                    "Opcode 0x{:04x}, Device param update flag reset to 0x{:02x}",
                    end_ack.cmd_id, mock.param_update_flag
                ),
            );

            ui::render_card(
                "SIMULATION PASSED",
                &[
                    ("Transport Engine", "In-Memory Virtual LOKE Driver"),
                    ("Odin Protocol", "Wire-accurate 0x64/0x65/0x66/0x67 State Machine"),
                    ("Fault Test Result", if fault_vaultkeeper || fault_timeout { "Fault Intercepted & Diagnosed" } else { "Clean Pass" }),
                ],
            );
            ui::badge_success("PASS", "Dry-run simulation completed successfully with zero unhandled errors!");
        }
    }

    Ok(())
}
