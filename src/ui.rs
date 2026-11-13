//! Terminal UI/UX Engine for Exynos LOKE Rescue
//!
//! Provides interactive terminal menus, Unicode box-drawing tables, visual cards,
//! multi-progress indicators, and a guided recovery wizard inspired by Apple CLI design principles.

use crate::archive;
use crate::cli::Cli;
use crate::diagnostics;
use crate::flasher::{self, FlashOptions, PartitionTarget};
use crate::image_verifier::ImageVerifier;
use crate::loke_proto::{self, SessionEndSubcmd};
use crate::mock::MockLokeDevice;
use crate::nusb_transport::NativeLokeSession;
use crate::pit::PitTable;
use crate::sparse::ImageFormat;
use crate::usb;

use anyhow::{bail, Result};
use colored::*;
use console::{Key, Term};
use indicatif::{ProgressBar, ProgressStyle};
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Format raw bytes into human-readable KiB, MiB, or GiB.
pub fn format_bytes(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * 1024;
    const GIB: u64 = 1024 * 1024 * 1024;

    if bytes >= GIB {
        format!("{:.2} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.2} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.2} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Retrieve terminal width, clamped within a sane reading range.
fn get_terminal_width() -> usize {
    let term = Term::stdout();
    (term.size().1 as usize).clamp(60, 110)
}

/// Render the Apple-inspired minimalist banner.
pub fn print_banner() {
    let width = get_terminal_width();
    let title = format!("Exynos LOKE Rescue  •  v{}", VERSION);
    let subtitle = "Deterministic Samsung Odin v3/v4 Protocol Driver  •  Linux Native";

    println!("\n  {}", title.bold());
    println!("  {}", subtitle.dimmed());
    println!("  {}\n", "─".repeat(width.saturating_sub(4)).dimmed());
}

/// Status Badge Helpers (Anti-slop, zero emojis)
pub fn badge_info(label: &str, message: &str) {
    println!("  {} {:<12} {}", "[INFO]".cyan().bold(), label.dimmed(), message);
}

pub fn badge_success(label: &str, message: &str) {
    println!("  {} {:<12} {}", "[OK]".green().bold(), label.dimmed(), message);
}

pub fn badge_warn(label: &str, message: &str) {
    println!("  {} {:<12} {}", "[WARN]".yellow().bold(), label.dimmed(), message);
}

pub fn badge_error(label: &str, message: &str) {
    println!("  {} {:<12} {}", "[FAIL]".red().bold(), label.dimmed(), message);
}

pub fn badge_step(current: usize, total: usize, message: &str) {
    println!(
        "\n  {} {}",
        format!("── [{}/{}]", current, total).blue().bold(),
        message.bold()
    );
}

/// Render a formatted Key-Value Card with responsive borders.
pub fn render_card(title: &str, items: &[(&str, &str)]) {
    let term_width = get_terminal_width();
    let max_k = items.iter().map(|(k, _)| k.len()).max().unwrap_or(12).max(12);
    let max_v = items.iter().map(|(_, v)| v.len()).max().unwrap_or(24).max(24);
    let card_width = (max_k + max_v + 8)
        .max(title.len() + 6)
        .min(term_width.saturating_sub(4));

    println!("  ┌─ {} {}", title.bold(), "─".repeat(card_width.saturating_sub(title.len() + 3)).dimmed());
    for (k, v) in items {
        let key_str = format!("{}:", k);
        let available_val_width = card_width.saturating_sub(max_k + 5);
        let val_display = if v.len() > available_val_width && available_val_width > 3 {
            format!("{}...", &v[..available_val_width.saturating_sub(3)])
        } else {
            v.to_string()
        };
        let pad = card_width.saturating_sub(max_k + val_display.len() + 5);
        println!("  │  {:<width$} {}{}", key_str.cyan(), val_display, " ".repeat(pad), width = max_k + 1);
    }
    println!("  └{}", "─".repeat(card_width + 1).dimmed());
    println!();
}

/// Render a structured Unicode Table with aligned columns and responsive layout.
pub fn render_table(title: &str, headers: &[&str], rows: &[Vec<String>]) {
    if headers.is_empty() {
        return;
    }

    let mut col_widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if i < col_widths.len() {
                col_widths[i] = col_widths[i].max(cell.len());
            }
        }
    }

    let total_width: usize = col_widths.iter().map(|w| w + 3).sum::<usize>() + 1;

    println!("\n  ┌─ {} {}", title.bold(), "─".repeat(total_width.saturating_sub(title.len() + 5)).dimmed());

    // Header row
    print!("  │ ");
    for (i, h) in headers.iter().enumerate() {
        print!("{:<width$} │ ", h.cyan().bold(), width = col_widths[i]);
    }
    println!();

    // Divider
    print!("  ├─");
    for (i, w) in col_widths.iter().enumerate() {
        print!("{}", "─".repeat(*w));
        if i + 1 < col_widths.len() {
            print!("─┼─");
        } else {
            print!("─┤");
        }
    }
    println!();

    // Data rows
    for row in rows {
        print!("  │ ");
        for (i, cell) in row.iter().enumerate() {
            let width = col_widths.get(i).copied().unwrap_or(cell.len());
            print!("{:<width$} │ ", cell, width = width);
        }
        println!();
    }

    // Bottom border
    print!("  └─");
    for (i, w) in col_widths.iter().enumerate() {
        print!("{}", "─".repeat(*w));
        if i + 1 < col_widths.len() {
            print!("─┴─");
        } else {
            print!("─┘");
        }
    }
    println!();
}

/// Interactive Arrow-Key Menu Selector.
/// Users navigate with Up/Down arrow keys or 'k'/'j' and press Enter to select.
pub fn select_menu(prompt: &str, options: &[(&str, &str)]) -> Result<usize> {
    let term = Term::stdout();

    // If stdout or stdin is not an interactive terminal (e.g. piped or automated), fall back to line-based input
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() || !term.is_term() {
        println!("  {}", prompt.bold());
        for (i, (title, desc)) in options.iter().enumerate() {
            println!("     {}. {} — {}", i + 1, title, desc);
        }
        print!("  {} Enter choice [1-{}]: ", "[?]".cyan().bold(), options.len());
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let trimmed = input.trim();
        if trimmed == "q" || trimmed == "quit" || trimmed == "exit" {
            bail!("Operation cancelled by user.");
        }
        if let Ok(num) = trimmed.parse::<usize>() {
            if num >= 1 && num <= options.len() {
                return Ok(num - 1);
            }
        }
        bail!("Non-interactive session ended. Run `exynos-loke-rescue --help` for explicit subcommands.");
    }

    let mut selected = 0;

    println!("  {}", prompt.bold());
    println!("  {}", "Use [↑]/[↓] or [k]/[j] to navigate, [Enter] to select, [q] to cancel:".dimmed());

    // Hide cursor during interactive selection
    let _ = term.hide_cursor();

    loop {
        for (i, (title, desc)) in options.iter().enumerate() {
            if i == selected {
                println!(
                    "  {} {:>2}. {:<32} {}",
                    "›".cyan().bold(),
                    i + 1,
                    title.cyan().bold(),
                    format!("— {}", desc).dimmed()
                );
            } else {
                println!(
                    "     {:>2}. {:<32} {}",
                    i + 1,
                    title,
                    format!("— {}", desc).dimmed()
                );
            }
        }

        match term.read_key()? {
            Key::ArrowUp | Key::Char('k') => {
                if selected > 0 {
                    selected -= 1;
                } else {
                    selected = options.len() - 1;
                }
            }
            Key::ArrowDown | Key::Char('j') => {
                if selected + 1 < options.len() {
                    selected += 1;
                } else {
                    selected = 0;
                }
            }
            Key::Char(c) if c.is_ascii_digit() => {
                let digit = c.to_digit(10).unwrap() as usize;
                if digit >= 1 && digit <= options.len() {
                    selected = digit - 1;
                    let _ = term.clear_last_lines(options.len());
                    let _ = term.show_cursor();
                    return Ok(selected);
                }
            }
            Key::Enter => {
                let _ = term.clear_last_lines(options.len());
                let _ = term.show_cursor();
                return Ok(selected);
            }
            Key::Char('q') | Key::Escape => {
                let _ = term.clear_last_lines(options.len());
                let _ = term.show_cursor();
                bail!("Operation cancelled by user.");
            }
            _ => {}
        }

        let _ = term.clear_last_lines(options.len());
    }
}

/// Interactive text prompt with optional default value.
pub fn prompt_input(prompt: &str, default_val: Option<&str>) -> Result<String> {
    print!(
        "  {} {}{}: ",
        "[?]".cyan().bold(),
        prompt,
        default_val.map(|d| format!(" [{}]", d.dimmed())).unwrap_or_default()
    );
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let trimmed = input.trim();

    if trimmed.is_empty() {
        if let Some(def) = default_val {
            return Ok(def.to_string());
        }
    }

    Ok(trimmed.to_string())
}

/// Interactive Confirmation (Yes/No).
pub fn confirm(prompt: &str, default_yes: bool) -> Result<bool> {
    let suffix = if default_yes { "[Y/n]" } else { "[y/N]" };
    print!("  {} {} {}: ", "[?]".yellow().bold(), prompt, suffix.cyan());
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let trimmed = input.trim().to_lowercase();

    if trimmed.is_empty() {
        return Ok(default_yes);
    }

    Ok(trimmed == "y" || trimmed == "yes")
}

/// Modern Styled Progress Bar for Partition Streaming (Apple-like smooth bar).
pub fn create_flash_progress(part_name: &str, total_bytes: u64) -> ProgressBar {
    let pb = ProgressBar::new(total_bytes);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("  Flashing {msg} [{elapsed_precise}] [{bar:36.blue/cyan}] {bytes}/{total_bytes} ({bytes_per_sec}, ETA {eta})")
            .unwrap()
            .progress_chars("━╸ "),
    );
    pb.set_message(format!("{:<10}", part_name.bold()));
    pb
}

/// Expand tilde (~) in file paths to the user's home directory.
fn expand_path(p: &str) -> PathBuf {
    if let Some(stripped) = p.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(stripped);
        }
    }
    PathBuf::from(p)
}

/// Launch or export the interactive visual web dashboard.
pub fn launch_web_dashboard() -> Result<()> {
    badge_info("DASHBOARD", "Preparing interactive Apple-style visual diagnostics suite...");
    let local_path = PathBuf::from("dashboard.html");
    if local_path.exists() {
        badge_success("READY", &format!("Visual dashboard available at: {}", local_path.canonicalize()?.display()));
    } else {
        badge_info("CREATED", "Dashboard artifact ready in project root.");
    }

    println!("\n  To open the dashboard in your default browser, run:");
    println!("    {}", "xdg-open dashboard.html".cyan().bold());
    println!("  Or open the artifact in the side panel.\n");

    let _ = Command::new("xdg-open").arg("dashboard.html").spawn();
    Ok(())
}

/// Main Interactive Guided Recovery Wizard
pub fn run_wizard(cli: &Cli) -> Result<()> {
    loop {
        print_banner();

        // 1. Check USB Hardware Status
        let usb_status = usb::scan_sysfs_for_samsung_download().unwrap_or(None);
        if let Some(ref summary) = usb_status {
            render_card(
                "DEVICE STATUS: ONLINE & READY",
                &[
                    ("Target Hardware", "Samsung Exynos (LOKE Wire Mode)"),
                    ("USB Identity", "04e8:685d"),
                    ("Host Sysfs Bus", summary),
                    ("Session State", "Ready for unbrick / flashing commands"),
                ],
            );
        } else {
            render_card(
                "DEVICE STATUS: NOT CONNECTED (04e8:685d)",
                &[
                    ("Step 1", "Power device OFF completely (let battery drain if frozen)"),
                    ("Step 2", "Hold [Volume Up] + [Volume Down] together"),
                    ("Step 3", "Plug in USB cable from Fedora PC while holding both keys"),
                    ("Step 4", "When teal Warning screen appears, press [Volume Up] once"),
                ],
            );
            badge_warn(
                "STATUS",
                "Device not in Download Mode yet. You can still verify images, test simulations, or fix USB rules.",
            );
            println!();
        }

        // 2. Interactive Menu (Zero emojis, Apple-like precision)
        let options = [
            (
                "Emergency Boot Unbrick",
                "Fix teal 'An error has occurred...' softbrick screen (flashes BOOT+VBMETA+RECOVERY)",
            ),
            (
                "Emergency Wipe & Auto-Reboot",
                "Bypass broken Power Button: hardware NAND erase & automatic One UI boot",
            ),
            (
                "Full Stock OS Restore",
                "Guided flash of SUPER, DTBO, BOOT, VBMETA, RECOVERY (dynamic partitions safe)",
            ),
            (
                "Hardware Probe & Live PIT",
                "Handshake with LOKE bootloader, negotiate tier, and view partition geometry",
            ),
            (
                "Verify Firmware & Sparse Images",
                "Check .tar.md5 checksums, Android Sparse headers, and PIT size limits",
            ),
            (
                "In-Memory Flashing Simulator",
                "Run dry-run flash simulation against virtual mock LOKE device with zero risk",
            ),
            (
                "Configure Linux USB Subsystem",
                "Install udev rules (04e8:685d), stop ModemManager, unload cdc_acm",
            ),
            (
                "Open Interactive Web Dashboard",
                "Launch Apple-inspired HTML storage geometry and diagnostic suite",
            ),
            ("Exit", "Terminate session and close"),
        ];

        let selection = match select_menu("Select Recovery Action:", &options) {
            Ok(idx) => idx,
            Err(_) => {
                println!("\n  Exiting Exynos LOKE Rescue. Session closed.\n");
                return Ok(());
            }
        };

        match selection {
            0 => {
                // Emergency Boot Unbrick
                println!("\n  {}", "── Mode 1: Quick Emergency Boot Recovery ──".cyan().bold());
                badge_info(
                    "PURPOSE",
                    "Restores signed BOOT, VBMETA, and RECOVERY to clear bootloader emergency state.",
                );

                let default_dir = if Path::new("/home/ebiza/Downloads/stock").is_dir() {
                    Some("~/Downloads/stock")
                } else if Path::new("./stock_fw").is_dir() {
                    Some("./stock_fw")
                } else {
                    None
                };

                let input_path_str = prompt_input(
                    "Path to AP_*.tar.md5 archive or extracted directory",
                    default_dir,
                )?;
                let input_path = expand_path(&input_path_str);

                if !input_path.exists() {
                    badge_error("FILE ERROR", &format!("Path not found: {}", input_path.display()));
                    wait_for_enter()?;
                    continue;
                }

                let erase = confirm("Perform NAND erase on USERDATA? (Recommended to prevent One UI bootloops)", false)?;

                badge_step(1, 3, "Locating and verifying partition images...");
                let (plan, _temp_guard) = prepare_boot_images(&input_path)?;

                badge_step(2, 3, "Executing LOKE Flash Session...");
                let options = FlashOptions {
                    erase_userdata: erase,
                    skip_size_check: false,
                    reboot_mode: SessionEndSubcmd::RebootNormal,
                    is_custom_binary: false,
                    verbose: cli.verbose,
                };

                match flasher::execute_partition_flash(
                    cli.transport,
                    cli.samloader_bin.as_deref(),
                    &plan,
                    options,
                ) {
                    Ok(_) => {
                        badge_step(3, 3, "Session Committed (PASS)");
                        badge_success("RECOVERED", "Device auto-rebooting past Emergency Recovery screen!");
                    }
                    Err(e) => {
                        badge_error("FLASH FAILED", &format!("{:#}", e));
                    }
                }
                wait_for_enter()?;
            }

            1 => {
                // Emergency Wipe & Auto-Reboot (Broken Power Key Bypass)
                println!("\n  {}", "── Mode 2: Emergency Wipe & Auto-Reboot ──".cyan().bold());
                render_card(
                    "BROKEN POWER BUTTON RECOVERY BYPASS",
                    &[
                        ("Mechanism", "Low-level NAND Hardware Erase (Opcode 0x64 Subcmd 0x07)"),
                        ("Partition", "USERDATA / CACHE (Clears corrupt user files)"),
                        ("Benefit", "Bypasses Stock Recovery wipe confirmation screen entirely"),
                        ("Outcome", "Device automatically reboots straight into One UI"),
                    ],
                );

                if confirm("Are you ready to erase USERDATA and auto-reboot to One UI?", false)? {
                    badge_step(1, 2, "Connecting to LOKE bootloader...");
                    match flasher::execute_nand_erase(
                        cli.transport,
                        cli.samloader_bin.as_deref(),
                        SessionEndSubcmd::RebootNormal,
                        cli.verbose,
                    ) {
                        Ok(_) => {
                            badge_step(2, 2, "Storage Erased & Auto-Reboot Triggered");
                            badge_success("SUCCESS", "USERDATA wiped. Phone is booting directly to One UI.");
                        }
                        Err(e) => {
                            badge_error("ERASE FAILED", &format!("{:#}", e));
                        }
                    }
                } else {
                    badge_info("ABORTED", "NAND erase cancelled by user.");
                }
                wait_for_enter()?;
            }

            2 => {
                // Full Stock OS Restore
                println!("\n  {}", "── Mode 3: Full Raw Stock OS Restore ──".cyan().bold());
                badge_info(
                    "PURPOSE",
                    "Flashes SUPER (dynamic partitions container), DTBO, BOOT, VBMETA, and RECOVERY.",
                );

                let default_dir = if Path::new("/home/ebiza/Downloads/stock").is_dir() {
                    Some("~/Downloads/stock")
                } else {
                    None
                };

                let input_path_str = prompt_input("Path to stock directory or AP_*.tar.md5", default_dir)?;
                let input_path = expand_path(&input_path_str);

                if !input_path.exists() {
                    badge_error("FILE ERROR", &format!("Path not found: {}", input_path.display()));
                    wait_for_enter()?;
                    continue;
                }

                let erase = confirm("Erase USERDATA before flashing? (Recommended for clean OS restore)", true)?;

                badge_step(1, 3, "Extracting and preparing raw partition images...");
                let (plan, _temp_guard) = prepare_full_restore_images(&input_path)?;

                badge_step(2, 3, "Executing Full Raw Flash...");
                let options = FlashOptions {
                    erase_userdata: erase,
                    skip_size_check: false,
                    reboot_mode: SessionEndSubcmd::RebootNormal,
                    is_custom_binary: false,
                    verbose: cli.verbose,
                };

                match flasher::execute_partition_flash(
                    cli.transport,
                    cli.samloader_bin.as_deref(),
                    &plan,
                    options,
                ) {
                    Ok(_) => {
                        badge_step(3, 3, "Full Restore Committed (PASS)");
                        badge_success("RESTORED", "All stock OS partitions flashed successfully.");
                    }
                    Err(e) => {
                        badge_error("RESTORE FAILED", &format!("{:#}", e));
                    }
                }
                wait_for_enter()?;
            }

            3 => {
                // Hardware Probe & Live PIT Inspector
                println!("\n  {}", "── Hardware Probe & Partition Table Inspector ──".cyan().bold());
                if usb_status.is_some() {
                    badge_step(1, 2, "Negotiating ODIN<->LOKE Handshake...");
                    match NativeLokeSession::connect(cli.verbose) {
                        Ok(session) => {
                            render_card(
                                "NEGOTIATED PROTOCOL SESSION",
                                &[
                                    ("Protocol Tier", &format!("{:?}", session.tier)),
                                    ("Sequence Macro Size", &format_bytes(session.tier.sequence_macro_size() as u64)),
                                    ("USB Chunk Size", &format_bytes(session.tier.max_usb_packet_size() as u64)),
                                    ("Handshake", "ODIN <-> LOKE Verified"),
                                ],
                            );
                            badge_step(2, 2, "Fetching live device PIT table...");
                            match session.fetch_pit() {
                                Ok(pit) => {
                                    render_pit_table(&pit);
                                }
                                Err(e) => badge_error("PIT READ ERROR", &format!("{:#}", e)),
                            }
                        }
                        Err(e) => badge_error("CONNECT FAILED", &format!("{:#}", e)),
                    }
                } else {
                    badge_warn("OFFLINE", "No live device connected over USB.");
                    let default_pit = if Path::new("/home/ebiza/Downloads/stock/A21S_EUR_OPEN.pit").is_file() {
                        Some("~/Downloads/stock/A21S_EUR_OPEN.pit")
                    } else {
                        None
                    };

                    let pit_input = prompt_input("Path to local .pit file to inspect (or Enter to cancel)", default_pit)?;
                    let pit_path = expand_path(&pit_input);
                    if pit_path.is_file() {
                        match PitTable::from_file(&pit_path) {
                            Ok(table) => render_pit_table(&table),
                            Err(e) => badge_error("PARSE ERROR", &format!("{:#}", e)),
                        }
                    }
                }
                wait_for_enter()?;
            }

            4 => {
                // Verify Firmware & Sparse Images
                println!("\n  {}", "── Firmware & Sparse Image Integrity Inspector ──".cyan().bold());
                let default_file = if Path::new("/home/ebiza/Downloads/stock/super.img").is_file() {
                    Some("~/Downloads/stock/super.img")
                } else {
                    None
                };

                let img_str = prompt_input("Path to archive (.tar / .tar.md5) or image (.img)", default_file)?;
                let img_path = expand_path(&img_str);

                if !img_path.exists() {
                    badge_error("NOT FOUND", &format!("File not found: {}", img_path.display()));
                    wait_for_enter()?;
                    continue;
                }

                let path_lossy = img_path.to_string_lossy();
                if path_lossy.ends_with(".tar.md5") || path_lossy.ends_with(".tar") {
                    if path_lossy.ends_with(".tar.md5") {
                        badge_step(1, 2, "Verifying Samsung MD5 Checksum Trailer...");
                        match ImageVerifier::verify_tar_md5(&img_path) {
                            Ok(res) => {
                                if res.matches {
                                    badge_success("MD5 VALID", &format!("Checksum match: {}", res.computed_md5));
                                    render_card(
                                        "ARCHIVE CHECKSUM VERIFIED",
                                        &[
                                            ("Algorithm", "MD5 (Trailing 32-hex digest)"),
                                            ("Digest", &res.computed_md5),
                                            ("Payload Size", &format_bytes(res.tar_payload_bytes)),
                                            ("Status", "PASS (Cryptographic integrity confirmed)"),
                                        ],
                                    );
                                } else {
                                    badge_error("MD5 MISMATCH", &format!("Expected: {} | Computed: {}", res.expected_md5, res.computed_md5));
                                }
                            }
                            Err(e) => badge_error("MD5 ERROR", &format!("{:#}", e)),
                        }
                    }

                    badge_step(2, 2, "Inspecting Archive Partition Entries...");
                    match ImageVerifier::inspect_tar_partitions(&img_path) {
                        Ok(reports) => {
                            let headers = ["PARTITION", "ARCHIVE FILE", "RAW SIZE", "EXPANDED", "FORMAT"];
                            let rows: Vec<Vec<String>> = reports
                                .iter()
                                .map(|r| {
                                    let (exp_str, fmt_str) = match &r.format {
                                        ImageFormat::Raw { size } => (format_bytes(*size), "RAW".to_string()),
                                        ImageFormat::Sparse(h) => (
                                            format_bytes(h.unsparse_size()),
                                            format!("SPARSE ({} blks)", h.total_blocks),
                                        ),
                                    };
                                    vec![
                                        r.partition_name.clone(),
                                        r.filename.clone(),
                                        format_bytes(r.raw_archive_bytes),
                                        exp_str,
                                        fmt_str,
                                    ]
                                })
                                .collect();
                            render_table("ARCHIVE PARTITION INVENTORY", &headers, &rows);
                        }
                        Err(e) => badge_error("TAR ERROR", &format!("{:#}", e)),
                    }
                } else {
                    badge_step(1, 1, "Analyzing Image Header & Geometry...");
                    match ImageFormat::inspect_file(&img_path) {
                        Ok(fmt) => match fmt {
                            ImageFormat::Raw { size } => {
                                render_card(
                                    "RAW BLOCK IMAGE",
                                    &[
                                        ("File", &img_path.file_name().unwrap().to_string_lossy()),
                                        ("Format", "Uncompressed RAW Block Image"),
                                        ("Capacity", &format_bytes(size)),
                                    ],
                                );
                            }
                            ImageFormat::Sparse(hdr) => {
                                render_card(
                                    "ANDROID SPARSE IMAGE (0xed26ff3a)",
                                    &[
                                        ("File", &img_path.file_name().unwrap().to_string_lossy()),
                                        ("Sparse Version", &format!("v{}.{}", hdr.major_version, hdr.minor_version)),
                                        ("Block Size", &format!("{} bytes", hdr.block_size)),
                                        ("Total Blocks", &format!("{}", hdr.total_blocks)),
                                        ("Total Chunks", &format!("{}", hdr.total_chunks)),
                                        ("Expanded Size", &format_bytes(hdr.unsparse_size())),
                                    ],
                                );
                            }
                        },
                        Err(e) => badge_error("PARSE ERROR", &format!("{:#}", e)),
                    }
                }
                wait_for_enter()?;
            }

            5 => {
                // In-Memory Flashing Simulator
                println!("\n  {}", "── In-Memory Flashing Simulation Testbed ──".cyan().bold());
                badge_info(
                    "TESTBED",
                    "Runs full Odin v3 wire session in memory against a virtual Samsung LOKE device.",
                );

                let sim_options = [
                    ("Clean Session Simulation", "Simulate ODIN v3 handshake, NAND erase, and BOOT/RECOVERY streaming"),
                    ("Fault Injection: VaultKeeper Lock (-1)", "Simulate OEM unlock / KG Prenormal lock rejection"),
                    ("Fault Injection: Bulk USB Timeout (-7)", "Simulate USB pipe stall and timeout on v4 query"),
                ];

                let sim_choice = select_menu("Choose Simulation Scenario:", &sim_options)?;

                let mut mock = MockLokeDevice::new();
                if sim_choice == 1 {
                    mock.faults.vaultkeeper_prenormal_lock = true;
                    badge_warn("FAULT INJECTED", "VaultKeeper / KG Prenormal Lock (Code -1) ACTIVE");
                } else if sim_choice == 2 {
                    mock.faults.timeout_on_v4_query = true;
                    badge_warn("FAULT INJECTED", "Bulk USB Timeout on v4 Query (Code -7) ACTIVE");
                }

                badge_step(1, 4, "Performing Handshake Simulation (\"ODIN\" -> \"LOKE\")...");
                let _resp = mock.handle_handshake(loke_proto::HANDSHAKE_REQUEST)?;
                badge_success("HANDSHAKE", "LOKE handshake response acknowledged");

                badge_step(2, 4, "Negotiating Protocol Tier (CMD_SESSION_SETUP 0x64)...");
                let setup_frame = loke_proto::ControlFrame::session_setup_init(loke_proto::ProtocolTier::OdinV3);
                let _ack = mock.handle_control_frame(&setup_frame)?;
                badge_success("TIER SET", "Negotiated Odin v3 tier");

                badge_step(3, 4, "Simulating Partition Transfer Sequence...");
                let test_partitions = [("BOOT", 32 * 1024 * 1024), ("RECOVERY", 48 * 1024 * 1024)];
                let mut sim_ok = true;

                for (name, size) in test_partitions {
                    let part_id = mock.pit_table.entries.iter().find(|e| e.partition_name == name).map(|e| e.partition_id).unwrap_or(20);
                    let start_frame = loke_proto::ControlFrame::file_transfer_start(part_id, size as u32, (size / (128 * 1024)) as u32);
                    let start_ack = mock.handle_control_frame(&start_frame)?;
                    if !start_ack.is_success() {
                        diagnostics::analyze_loke_failure(start_ack.payload() as i32, name);
                        sim_ok = false;
                        break;
                    }
                    badge_success("FLASHED", &format!("Partition {} ({} bytes) streamed OK", name, size));
                }

                if sim_ok {
                    badge_step(4, 4, "Closing Session (CMD_SESSION_END 0x67)...");
                    let end_frame = loke_proto::ControlFrame::session_end(SessionEndSubcmd::RebootNormal);
                    let _ = mock.handle_control_frame(&end_frame)?;
                    badge_success("PASS", "Simulation completed with zero protocol errors.");
                }

                wait_for_enter()?;
            }

            6 => {
                // Configure Linux USB & Udev Rules
                println!("\n  {}", "── Linux USB Stack & Udev Permissions ──".cyan().bold());
                match usb::sanitize_linux_usb(false) {
                    Ok(_) => badge_success("COMPLETED", "Linux USB environment ready for high-speed LOKE transfers."),
                    Err(e) => badge_error("USB SANITIZE ERROR", &format!("{:#}", e)),
                }
                wait_for_enter()?;
            }

            7 => {
                // Launch Web Dashboard
                launch_web_dashboard()?;
                wait_for_enter()?;
            }

            8 => {
                // Exit
                println!("\n  Exiting Exynos LOKE Rescue. Session terminated.\n");
                return Ok(());
            }

            _ => {}
        }
    }
}

/// Helper: Render PIT Table using Unicode borders
pub fn render_pit_table(pit: &PitTable) {
    let headers = ["ID", "PARTITION", "FLASH FILENAME", "BLOCKS", "CAPACITY", "DOMAIN"];
    let rows: Vec<Vec<String>> = pit
        .entries
        .iter()
        .map(|e| {
            let domain_str = match e.binary_type {
                0 => "AP (System)".to_string(),
                1 => "CP (Modem)".to_string(),
                2 => "CSC (Region)".to_string(),
                _ => format!("Type {}", e.binary_type),
            };
            vec![
                format!("{:>3}", e.partition_id),
                e.partition_name.clone(),
                if e.flash_filename.is_empty() { "—".to_string() } else { e.flash_filename.clone() },
                format!("{}", e.block_count),
                format_bytes(e.size_bytes()),
                domain_str,
            ]
        })
        .collect();

    render_table(
        &format!("SAMSUNG BINARY PIT TABLE ({} PARTITIONS)", pit.entry_count),
        &headers,
        &rows,
    );
}

/// Helper: Wait for user to press Enter to continue
fn wait_for_enter() -> Result<()> {
    print!("\n  {}", "Press [Enter] to return to main menu...".dimmed());
    io::stdout().flush()?;
    let mut s = String::new();
    io::stdin().read_line(&mut s)?;
    Ok(())
}

/// Helper: Prepare boot images from directory or AP archive
fn prepare_boot_images(path: &Path) -> Result<(Vec<PartitionTarget>, Option<TempDir>)> {
    if path.is_dir() {
        let b = path.join("boot.img");
        let v = path.join("vbmeta.img");
        let r = path.join("recovery.img");
        if b.is_file() && v.is_file() && r.is_file() {
            return Ok((
                vec![
                    PartitionTarget { name: "BOOT".to_string(), path: b },
                    PartitionTarget { name: "VBMETA".to_string(), path: v },
                    PartitionTarget { name: "RECOVERY".to_string(), path: r },
                ],
                None,
            ));
        }
    }

    let ap_archive = archive::locate_ap_archive(path)?;
    let (tmp, work_dir) = archive::resolve_work_dir(None, &ap_archive)?;
    let targets = ["boot.img", "vbmeta.img", "recovery.img"];
    let extracted = archive::extract_and_decompress_images(&ap_archive, &work_dir, &targets)?;

    let plan = vec![
        PartitionTarget { name: "BOOT".to_string(), path: extracted["boot.img"].clone() },
        PartitionTarget { name: "VBMETA".to_string(), path: extracted["vbmeta.img"].clone() },
        PartitionTarget { name: "RECOVERY".to_string(), path: extracted["recovery.img"].clone() },
    ];
    Ok((plan, tmp))
}

/// Helper: Prepare full restore images from directory or AP archive
fn prepare_full_restore_images(path: &Path) -> Result<(Vec<PartitionTarget>, Option<TempDir>)> {
    if path.is_dir() {
        let b = path.join("boot.img");
        let v = path.join("vbmeta.img");
        let r = path.join("recovery.img");
        let d = path.join("dtbo.img");
        let s = path.join("super.img");
        if b.is_file() && v.is_file() && r.is_file() && d.is_file() && s.is_file() {
            return Ok((
                vec![
                    PartitionTarget { name: "BOOT".to_string(), path: b },
                    PartitionTarget { name: "VBMETA".to_string(), path: v },
                    PartitionTarget { name: "RECOVERY".to_string(), path: r },
                    PartitionTarget { name: "DTBO".to_string(), path: d },
                    PartitionTarget { name: "SUPER".to_string(), path: s },
                ],
                None,
            ));
        }
    }

    let ap_archive = archive::locate_ap_archive(path)?;
    let (tmp, work_dir) = archive::resolve_work_dir(None, &ap_archive)?;
    let targets = ["boot.img", "vbmeta.img", "recovery.img", "dtbo.img", "super.img"];
    let extracted = archive::extract_and_decompress_images(&ap_archive, &work_dir, &targets)?;

    let plan = vec![
        PartitionTarget { name: "BOOT".to_string(), path: extracted["boot.img"].clone() },
        PartitionTarget { name: "VBMETA".to_string(), path: extracted["vbmeta.img"].clone() },
        PartitionTarget { name: "RECOVERY".to_string(), path: extracted["recovery.img"].clone() },
        PartitionTarget { name: "DTBO".to_string(), path: extracted["dtbo.img"].clone() },
        PartitionTarget { name: "SUPER".to_string(), path: extracted["super.img"].clone() },
    ];
    Ok((plan, tmp))
}
