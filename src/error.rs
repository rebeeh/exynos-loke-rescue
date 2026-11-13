use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum RescueError {
    #[error("samloader-rs binary not found in ./samloader or $PATH")]
    SamloaderNotFound,

    #[error("USB device 04e8:685d not detected on host bus (check cable, battery, or replug)")]
    DeviceNotDetected,

    #[error("LOKE handshake or USB bulk transfer stalled on endpoint 0x81/0x01")]
    UsbEndpointHung,

    #[error("LOKE rejected flash with general failure (FAIL!) [code: -1] (VaultKeeper / signature check)")]
    LokeRejectedBinary,

    #[error("Archive `{0}` not found or unreadable")]
    ArchiveMissing(PathBuf),

    #[error("Partition image `{image}` not found inside archive `{archive}`")]
    PartitionImageMissing { image: String, archive: PathBuf },

    #[error("PIT binary header magic mismatch (expected 0x12349876, got {0:#010x})")]
    InvalidPitMagic(u32),

    #[error("External command `{cmd}` exited with non-zero status: {code}")]
    SubprocessFailed { cmd: String, code: i32 },

    #[error(transparent)]
    Io(#[from] std::io::Error),
}
