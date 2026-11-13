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
    /// Configure Linux udev rules (04e8:685d), stop ModemManager, and sanitize USB stack
    #[command(name = "fix-usb", alias = "prep-usb")]
    FixUsb {
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

        /// Perform low-level NAND hardware erase on USERDATA before flashing
        #[arg(short = 'e', long = "erase")]
        erase_userdata: bool,

        /// Skip verifying that partition image file size fits within PIT partition boundary
        #[arg(long)]
        skip_size_check: bool,

        /// Keep device in Download Mode after session commit instead of rebooting
        #[arg(long)]
        no_reboot: bool,

        /// Reboot the device back into Download Mode after flashing
        #[arg(long)]
        redownload: bool,
    },

    /// Mode 2: Full raw stock OS restore (BOOT + VBMETA + RECOVERY + DTBO + SUPER) bypassing super metadata bugs
    FullRawRestore {
        /// Directory containing stock AP_*.tar.md5 archive
        #[arg(short, long)]
        stock_dir: PathBuf,

        /// Retain decompressed raw .img files in this directory
        #[arg(long)]
        keep_extracted: Option<PathBuf>,

        /// Perform low-level NAND hardware erase on USERDATA before flashing
        #[arg(short = 'e', long = "erase")]
        erase_userdata: bool,

        /// Skip verifying that partition image file size fits within PIT partition boundary
        #[arg(long)]
        skip_size_check: bool,

        /// Keep device in Download Mode after session commit
        #[arg(long)]
        no_reboot: bool,

        /// Reboot the device back into Download Mode after flashing
        #[arg(long)]
        redownload: bool,
    },

    /// Flash arbitrary raw partition images (BOOT, VBMETA, RECOVERY, DTBO, SUPER, etc.) directly by partition name
    FlashRaw {
        /// Explicit partition name and image file (e.g. -p SUPER super.img -p DTBO dtbo.img)
        #[arg(short = 'p', long = "partition", num_args = 2, value_names = ["NAME", "FILE"])]
        partitions: Vec<String>,

        /// Direct shortcut for BOOT image
        #[arg(long)]
        boot: Option<PathBuf>,

        /// Direct shortcut for RECOVERY image
        #[arg(long)]
        recovery: Option<PathBuf>,

        /// Direct shortcut for VBMETA image
        #[arg(short = 'm', long)]
        vbmeta: Option<PathBuf>,

        /// Direct shortcut for DTBO image
        #[arg(long)]
        dtbo: Option<PathBuf>,

        /// Direct shortcut for SUPER image (dynamic partitions container)
        #[arg(long)]
        super_img: Option<PathBuf>,

        /// Perform low-level NAND hardware erase on USERDATA before flashing
        #[arg(short = 'e', long = "erase")]
        erase_userdata: bool,

        /// Skip verifying that partition image file size fits within PIT partition boundary
        #[arg(long)]
        skip_size_check: bool,

        /// Keep device in Download Mode after flashing
        #[arg(long)]
        no_reboot: bool,

        /// Reboot the device back into Download Mode after flashing
        #[arg(long)]
        redownload: bool,
    },

    /// Perform low-level hardware NAND storage erase on USERDATA directly from Download Mode
    /// (Bypasses broken physical power buttons and corrupt stock recovery wipe prompts)
    EraseUserdata {
        /// Keep device in Download Mode after erasing instead of rebooting
        #[arg(long)]
        no_reboot: bool,

        /// Reboot the device back into Download Mode after erasing
        #[arg(long)]
        redownload: bool,
    },

    /// Emergency unbrick for devices stuck in Stock Recovery with a broken Power Button:
    /// Executes low-level NAND erase on USERDATA from Download Mode and auto-reboots to One UI
    EmergencyWipeReboot,

    /// Flash custom recovery + vbmeta (LineageOS / TWRP) with VaultKeeper (LOKE -1) failure diagnosis
    FlashCustomRecovery {
        /// Path to custom recovery.img
        #[arg(short, long)]
        recovery: PathBuf,

        /// Path to custom or disabled vbmeta.img
        #[arg(short = 'm', long)]
        vbmeta: PathBuf,

        /// Optional custom boot.img
        #[arg(short, long)]
        boot: Option<PathBuf>,

        /// Keep device in Download Mode after flashing (default: true)
        #[arg(long, default_value_t = true)]
        no_reboot: bool,
    },

    /// Verify MD5 trailer checksum and examine partition geometry/sparse headers of an archive or image file
    VerifyImage {
        /// Path to .tar, .tar.md5, or raw/sparse .img file
        #[arg(short, long)]
        image: PathBuf,

        /// Optional path to .pit file to verify that partitions fit within device boundaries
        #[arg(short, long)]
        pit: Option<PathBuf>,
    },

    /// Perform a software dry-run flashing simulation against the in-memory mock LOKE device
    SimulateFlash {
        /// Optional path to AP archive or raw images to simulate flashing
        #[arg(short, long)]
        image: Option<PathBuf>,

        /// Simulate low-level NAND hardware erase on USERDATA
        #[arg(short = 'e', long = "erase")]
        erase_userdata: bool,

        /// Inject mock VaultKeeper / Knox Guard Prenormal lock failure (Code -1)
        #[arg(long)]
        fault_vaultkeeper: bool,

        /// Inject mock USB timeout failure (Code -7)
        #[arg(long)]
        fault_timeout: bool,
    },
}
