use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "exynos-loke-rescue",
    version,
    propagate_version = true,
    about = "Native Linux LOKE protocol driver and deterministic recovery engine for Samsung Exynos devices."
)]
pub struct Cli {
    /// Transport engine: `native` uses built-in Rust nusb Odin v3/v4 driver; `samloader` delegates to samloader-rs
    #[arg(long, global = true, value_enum, default_value_t = TransportEngine::Native)]
    pub transport: TransportEngine,

    /// Optional explicit path to `samloader-rs` (used for `fetch-stock` or `--transport samloader`)
    #[arg(long, global = true, env = "SAMLOADER_BIN")]
    pub samloader_bin: Option<PathBuf>,

    /// Enable verbose wire-level telemetry logging
    #[arg(short, long, global = true)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum TransportEngine {
    Native,
    Samloader,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Install ID_MM_DEVICE_IGNORE udev rules, stop ModemManager, unload cdc_acm, and verify 04e8:685d
    PrepUsb {
        /// Skip systemctl stop ModemManager (for non-systemd Linux hosts)
        #[arg(long)]
        no_systemd: bool,
    },

    /// Perform native ODIN<->LOKE handshake, negotiate protocol version (v3/v4), and dump live PIT summary
    Probe,

    /// Parse and display a Samsung binary PIT file from disk or live device memory
    InspectPit {
        /// Path to local .pit file. If omitted, dumps live PIT directly over USB.
        #[arg(short, long)]
        pit: Option<PathBuf>,
    },

    /// Download and unpack official signed stock firmware via samloader-rs
    FetchStock {
        /// Device model (e.g., SM-A217F)
        #[arg(short, long)]
        model: String,

        /// 3-letter Sales Code / CSC (e.g., INS, BTU, XAA)
        #[arg(short, long)]
        region: String,

        /// Destination directory
        #[arg(short, long, default_value = "./stock_fw")]
        out_dir: PathBuf,
    },

    /// Mode 1: Clear Emergency Recovery screen by flashing signed stock BOOT, VBMETA, and RECOVERY
    RescueBoot {
        /// Path to stock AP_*.tar.md5 file or directory containing it
        #[arg(short, long)]
        ap: PathBuf,

        /// Retain decompressed raw .img files in this directory
        #[arg(long)]
        keep_extracted: Option<PathBuf>,

        /// Keep device in Download Mode after session commit instead of rebooting
        #[arg(long)]
        no_reboot: bool,
    },

    /// Mode 2: Full raw stock OS restore (BOOT + VBMETA + RECOVERY + DTBO + SUPER) bypassing super metadata bugs
    FullRawRestore {
        /// Directory containing stock AP_*.tar.md5 archive
        #[arg(short, long)]
        stock_dir: PathBuf,

        /// Retain decompressed raw .img files in this directory
        #[arg(long)]
        keep_extracted: Option<PathBuf>,

        /// Keep device in Download Mode after session commit
        #[arg(long)]
        no_reboot: bool,
    },

    /// Flash custom recovery + vbmeta (LineageOS / TWRP) with VaultKeeper (LOKE -1) failure diagnosis
    FlashCustomRecovery {
        /// Path to custom recovery.img
        #[arg(short, long)]
        recovery: PathBuf,

        /// Path to custom or disabled vbmeta.img
        #[arg(short, long)]
        vbmeta: PathBuf,

        /// Optional custom boot.img
        #[arg(short, long)]
        boot: Option<PathBuf>,

        /// Keep device in Download Mode after flashing (default: true)
        #[arg(long, default_value_t = true)]
        no_reboot: bool,
    },
}
