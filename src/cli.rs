use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "exynos-loke-rescue",
    version,
    about = "Linux recovery orchestrator for Samsung Exynos devices stuck in Emergency Recovery or dynamic partition bootloops."
)]
pub struct Cli {
    /// Path to the samloader-rs binary (defaults to searching $PATH and ./samloader)
    #[arg(long, global = true)]
    pub samloader_bin: Option<PathBuf>,

    /// USB backend passed through to samloader-rs
    #[arg(long, global = true, value_enum, default_value_t = UsbBackend::Nusb)]
    pub backend: UsbBackend,

    /// Enable verbose diagnostic logging
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
    /// Configure udev rules, stop ModemManager, unload cdc_acm, and verify Samsung 04e8:685d detection
    PrepUsb {
        /// Skip systemctl stop ModemManager (if running without systemd)
        #[arg(long)]
        no_systemd: bool,
    },

    /// Read connected device Model, Bootloader version, and Sales Code (CSC)
    Inspect,

    /// Download official signed firmware matching the connected device (or explicit model/CSC)
    FetchStock {
        /// Device model (e.g., SM-A217F). Auto-detected if omitted.
        #[arg(short, long)]
        model: Option<String>,

        /// 3-letter region/CSC code (e.g., INS, BTU, XAA). Auto-detected if omitted.
        #[arg(short, long)]
        region: Option<String>,

        /// Output directory for downloaded and unzipped stock firmware
        #[arg(short, long, default_value = "./stock_fw")]
        out_dir: PathBuf,
    },

    /// Mode 1: Clear "An error has occurred..." screen by flashing ONLY signed boot, vbmeta, and recovery
    RescueBoot {
        /// Path to stock AP_*.tar.md5 archive (or directory containing it)
        #[arg(short, long)]
        ap: PathBuf,

        /// Keep extracted raw .img files instead of cleaning up temp directory
        #[arg(long)]
        keep_extracted: Option<PathBuf>,

        /// Prevent automatic reboot after flashing
        #[arg(long)]
        no_reboot: bool,
    },

    /// Mode 2: Full raw stock restore (SUPER + DTBO + BOOT + VBMETA + RECOVERY) bypassing samloader-rs pre-flight crash
    FullRawRestore {
        /// Directory containing extracted stock BL, AP, CP, and CSC/HOME_CSC .tar.md5 files
        #[arg(short, long)]
        stock_dir: PathBuf,

        /// Also flash BL and CP archives after raw AP partitions complete
        #[arg(long)]
        include_bl_cp: bool,

        /// Keep extracted raw .img files in a specified directory
        #[arg(long)]
        keep_extracted: Option<PathBuf>,
    },

    /// Flash custom recovery + vbmeta (LineageOS / TWRP) with automatic VaultKeeper (LOKE -1) diagnosis
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

        /// Stay in Download Mode after flashing (recommended before booting recovery via keys)
        #[arg(long, default_value_t = true)]
        no_reboot: bool,
    },
}
