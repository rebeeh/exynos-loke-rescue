use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "exynos-loke-rescue",
    version,
    propagate_version = true,
    about = "Deterministic Linux recovery and partition orchestration engine for Samsung Exynos devices."
)]
pub struct Cli {
    /// Explicit path to the samloader-rs binary (defaults to ./samloader or $PATH)
    #[arg(long, global = true, env = "SAMLOADER_BIN")]
    pub samloader_bin: Option<PathBuf>,

    /// USB transport backend passed to samloader-rs
    #[arg(long, global = true, value_enum, default_value_t = UsbBackend::Nusb)]
    pub backend: UsbBackend,

    /// Print full diagnostic traces and partition geometry
    #[arg(short, long, global = true)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum UsbBackend {
    Nusb,
    Vcom,
}

impl UsbBackend {
    pub fn as_str(&self) -> &'static str {
        match self {
            UsbBackend::Nusb => "nusb",
            UsbBackend::Vcom => "vcom",
        }
    }
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Install ID_MM_DEVICE_IGNORE udev rules, stop ModemManager, unload cdc_acm, and verify USB bus
    PrepUsb {
        /// Skip systemctl stop ModemManager (for non-systemd Linux distributions)
        #[arg(long)]
        no_systemd: bool,
    },

    /// Query connected Exynos bootloader for Model, Serial, Bootloader Revision, and CSC
    Inspect,

    /// Parse and display a Samsung binary PIT file (either from disk or dumped from device)
    InspectPit {
        /// Path to a .pit file (or CSC_*.tar.md5 containing one). If omitted, dumps live PIT from device.
        #[arg(short, long)]
        pit: Option<PathBuf>,
    },

    /// Auto-detect Model/CSC and download + extract official signed Samsung stock firmware
    FetchStock {
        /// Override device model (e.g., SM-A217F)
        #[arg(short, long)]
        model: Option<String>,

        /// Override 3-letter CSC / Sales Code (e.g., INS, BTU, XAA)
        #[arg(short, long)]
        region: Option<String>,

        /// Destination directory for stock firmware
        #[arg(short, long, default_value = "./stock_fw")]
        out_dir: PathBuf,
    },

    /// Mode 1: Clear Emergency Recovery screen by flashing signed stock BOOT, VBMETA, and RECOVERY
    RescueBoot {
        /// Path to official AP_*.tar.md5 file or directory containing it
        #[arg(short, long)]
        ap: PathBuf,

        /// Retain decompressed raw .img files in the specified directory
        #[arg(long)]
        keep_extracted: Option<PathBuf>,

        /// Keep device in Download Mode after flash instead of rebooting
        #[arg(long)]
        no_reboot: bool,
    },

    /// Mode 2: Full raw stock OS restore (BOOT + VBMETA + RECOVERY + DTBO + SUPER) bypassing AP pre-flight crash
    FullRawRestore {
        /// Directory containing extracted stock BL, AP, CP, and CSC archives
        #[arg(short, long)]
        stock_dir: PathBuf,

        /// Retain decompressed raw .img files in the specified directory
        #[arg(long)]
        keep_extracted: Option<PathBuf>,

        /// Keep device in Download Mode after flash instead of rebooting
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

        /// Stay in Download Mode after flashing (recommended before key-combo recovery boot)
        #[arg(long, default_value_t = true)]
        no_reboot: bool,
    },
}
