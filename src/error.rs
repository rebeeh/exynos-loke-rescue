use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum RescueError {
    #[error("Samsung Download Mode device (04e8:685d) not found on USB bus")]
    DeviceNotDetected,

    #[error("Failed to claim USB Interface 1 on 04e8:685d (ensure cdc_acm is unloaded and ModemManager is stopped): {0}")]
    UsbInterfaceClaimFailed(String),

    #[error("LOKE ASCII handshake failed: expected `LOKE`, received `{0:?}`")]
    HandshakeFailed(Vec<u8>),

    #[error("USB bulk transfer timed out on endpoint {endpoint:#04x} after {timeout_ms}ms")]
    UsbBulkTimeout { endpoint: u8, timeout_ms: u64 },

    #[error("LOKE bootloader rejected operation with FAIL! [code: {code}]")]
    LokeFailure { code: i32 },

    #[error("Partition `{0}` not found in device PIT table")]
    PartitionNotInPit(String),

    #[error("Payload size ({payload_bytes} bytes) exceeds PIT partition `{partition}` capacity ({capacity_bytes} bytes)")]
    PartitionOverflow {
        partition: String,
        payload_bytes: u64,
        capacity_bytes: u64,
    },

    #[error("Archive `{0}` not found or unreadable")]
    ArchiveMissing(PathBuf),

    #[error("Partition image `{image}` missing from archive `{archive}`")]
    PartitionImageMissing { image: String, archive: PathBuf },

    #[error("Invalid Samsung PIT header magic: expected 0x12349876, got {0:#010x}")]
    InvalidPitMagic(u32),

    #[error("samloader-rs binary not found in ./samloader or $PATH")]
    SamloaderNotFound,

    #[error(transparent)]
    Io(#[from] std::io::Error),
}
