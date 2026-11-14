use crate::error::RescueError;
use anyhow::Result;

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::{Cursor, Write};

pub const CONTROL_PACKET_SIZE: usize = 1024;
pub const ACK_PACKET_SIZE: usize = 8;

pub const HANDSHAKE_REQUEST: &[u8; 4] = b"ODIN";
pub const HANDSHAKE_RESPONSE: &[u8; 4] = b"LOKE";

/// Primary LOKE command opcodes (Section 3.3 of PROTOCOL.md).
#[repr(u32)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum LokeCommand {
    SessionSetup = 0x64,
    PitTransfer = 0x65,
    FileTransfer = 0x66,
    SessionEnd = 0x67,
    DeviceInfo = 0x69, // Odin v4 DVIF query (causes error: -7 timeout on Exynos 850)
    DynamicSuperCheck = 0x6a, // Dynamic partition pre-flight sizing check
}

impl LokeCommand {
    pub fn from_u32(val: u32) -> Option<Self> {
        match val {
            0x64 => Some(Self::SessionSetup),
            0x65 => Some(Self::PitTransfer),
            0x66 => Some(Self::FileTransfer),
            0x67 => Some(Self::SessionEnd),
            0x69 => Some(Self::DeviceInfo),
            0x6a => Some(Self::DynamicSuperCheck),
            _ => None,
        }
    }
}

/// Subcommands for `LokeCommand::SessionSetup` (`0x64`).
#[repr(u32)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SessionSetupSubcmd {
    BeginSession = 0x00,
    SetTotalBytes = 0x01,
    SetTflashMode = 0x02,
    SetSalesCode = 0x05, // Set 3-letter CSC / Sales Code in device storage
    NandErase = 0x07,    // Low-level hardware NAND storage erase of USERDATA/cache
}

impl SessionSetupSubcmd {
    pub fn from_u32(val: u32) -> Option<Self> {
        match val {
            0x00 => Some(Self::BeginSession),
            0x01 => Some(Self::SetTotalBytes),
            0x02 => Some(Self::SetTflashMode),
            0x05 => Some(Self::SetSalesCode),
            0x07 => Some(Self::NandErase),
            _ => None,
        }
    }
}

/// Subcommands for `LokeCommand::PitTransfer` (`0x65`).
#[repr(u32)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PitSubcmd {
    FlashStart = 0x00,
    DumpStart = 0x01,
    DumpChunk = 0x02,
    DumpEnd = 0x03,
}

impl PitSubcmd {
    pub fn from_u32(val: u32) -> Option<Self> {
        match val {
            0x00 => Some(Self::FlashStart),
            0x01 => Some(Self::DumpStart),
            0x02 => Some(Self::DumpChunk),
            0x03 => Some(Self::DumpEnd),
            _ => None,
        }
    }
}

/// Subcommands for `LokeCommand::FileTransfer` (`0x66`).
#[repr(u32)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum FileTransferSubcmd {
    StartPartition = 0x00,
    DataSequence = 0x01,
    CommitPartition = 0x02,
}

impl FileTransferSubcmd {
    pub fn from_u32(val: u32) -> Option<Self> {
        match val {
            0x00 => Some(Self::StartPartition),
            0x01 => Some(Self::DataSequence),
            0x02 => Some(Self::CommitPartition),
            _ => None,
        }
    }
}

/// Subcommands for `LokeCommand::SessionEnd` (`0x67`).
#[repr(u32)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum SessionEndSubcmd {
    CloseNoReboot = 0x00, // Keep device in Download Mode
    #[default]
    RebootNormal = 0x01, // Normal reboot into Android OS
    RebootDownload = 0x02, // Redownload: reboot back into Download Mode
}

impl SessionEndSubcmd {
    pub fn from_u32(val: u32) -> Option<Self> {
        match val {
            0x00 => Some(Self::CloseNoReboot),
            0x01 => Some(Self::RebootNormal),
            0x02 => Some(Self::RebootDownload),
            _ => None,
        }
    }
}

/// Negotiated protocol tier governing chunk sizes and v4 query eligibility.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ProtocolTier {
    OdinV3,
    OdinV4,
}

impl ProtocolTier {
    pub fn max_usb_packet_size(&self) -> usize {
        match self {
            ProtocolTier::OdinV3 => 131_072,   // 128 KiB bulk transfer blocks
            ProtocolTier::OdinV4 => 1_048_576, // 1 MiB bulk transfer blocks
        }
    }

    pub fn sequence_macro_size(&self) -> usize {
        31_457_280 // 30 MiB per sequence ACK checkpoint
    }
}

/// Builder for fixed 1024-byte little-endian LOKE control frames.
#[derive(Debug, Clone)]
pub struct ControlFrame {
    pub cmd_id: u32,
    pub subcmd_id: u32,
    pub arg0: u32,
    pub arg1: u32,
    pub arg2: u32,
    pub arg3: u32,
    pub arg4: u32,
    pub arg5: u32,
    pub string_arg: Option<String>,
}

impl ControlFrame {
    pub fn new(cmd: LokeCommand, subcmd: u32) -> Self {
        Self {
            cmd_id: cmd as u32,
            subcmd_id: subcmd,
            arg0: 0,
            arg1: 0,
            arg2: 0,
            arg3: 0,
            arg4: 0,
            arg5: 0,
            string_arg: None,
        }
    }

    pub fn with_args(mut self, a0: u32, a1: u32, a2: u32, a3: u32) -> Self {
        self.arg0 = a0;
        self.arg1 = a1;
        self.arg2 = a2;
        self.arg3 = a3;
        self
    }

    pub fn with_string(mut self, s: impl Into<String>) -> Self {
        self.string_arg = Some(s.into());
        self
    }

    pub fn session_setup_init(tier: ProtocolTier) -> Self {
        Self::new(
            LokeCommand::SessionSetup,
            SessionSetupSubcmd::BeginSession as u32,
        )
        .with_args(tier as u32, 0, 0, 0)
    }

    pub fn nand_erase() -> Self {
        Self::new(
            LokeCommand::SessionSetup,
            SessionSetupSubcmd::NandErase as u32,
        )
        .with_args(0, 0, 0, 0)
    }

    pub fn file_transfer_start(part_id: u32, size: u32, chunks: u32) -> Self {
        Self::new(
            LokeCommand::FileTransfer,
            FileTransferSubcmd::StartPartition as u32,
        )
        .with_args(part_id, size, chunks, 0)
    }

    pub fn file_transfer_end(part_id: u32) -> Self {
        Self::new(
            LokeCommand::FileTransfer,
            FileTransferSubcmd::CommitPartition as u32,
        )
        .with_args(part_id, 0, 0, 0)
    }

    pub fn file_transfer_flash() -> Self {
        Self::new(
            LokeCommand::FileTransfer,
            FileTransferSubcmd::StartPartition as u32,
        )
    }

    pub fn file_transfer_slice(aligned_len: u32) -> Self {
        Self::new(
            LokeCommand::FileTransfer,
            0x02, // Slice subcommand
        )
        .with_args(aligned_len, 0, 0, 0)
    }

    pub fn file_transfer_commit(
        slice_size: u32,
        partition_id: u32,
        is_end_of_file: u32,
    ) -> Self {
        let mut frame = Self::new(
            LokeCommand::FileTransfer,
            0x03, // End/Commit subcommand
        );
        frame.arg0 = 0; // Unified layout magic = 0
        frame.arg1 = slice_size;
        frame.arg2 = 0; // BinaryType::ApplicationProcessor = 0
        frame.arg3 = 0; // DeviceType = 0
        frame.arg4 = partition_id;
        frame.arg5 = is_end_of_file;
        frame
    }

    pub fn session_end(mode: SessionEndSubcmd) -> Self {
        Self::new(LokeCommand::SessionEnd, mode as u32).with_args(0, 0, 0, 0)
    }

    /// Serializes the frame into an exact 1024-byte zero-padded wire buffer.
    pub fn to_bytes(&self) -> [u8; CONTROL_PACKET_SIZE] {
        let mut buf = [0u8; CONTROL_PACKET_SIZE];
        let mut cursor = Cursor::new(&mut buf[..]);

        cursor.write_u32::<LittleEndian>(self.cmd_id).unwrap();
        cursor.write_u32::<LittleEndian>(self.subcmd_id).unwrap();
        cursor.write_u32::<LittleEndian>(self.arg0).unwrap();
        cursor.write_u32::<LittleEndian>(self.arg1).unwrap();
        cursor.write_u32::<LittleEndian>(self.arg2).unwrap();
        cursor.write_u32::<LittleEndian>(self.arg3).unwrap();
        cursor.write_u32::<LittleEndian>(self.arg4).unwrap();
        cursor.write_u32::<LittleEndian>(self.arg5).unwrap();

        if let Some(ref s) = self.string_arg {
            let bytes = s.as_bytes();
            let copy_len = bytes.len().min(31);
            cursor.write_all(&bytes[..copy_len]).unwrap();
        }

        buf
    }
}

/// Parsed 8-byte device acknowledgment frame read from `EP_IN (0x81)`.
#[derive(Debug, Copy, Clone)]
pub struct LokeAck {
    pub cmd_id: u32,
    pub status: i32,
}

impl LokeAck {
    pub fn parse(raw: &[u8]) -> Result<Self> {
        if raw.len() < ACK_PACKET_SIZE {
            anyhow::bail!("Short LOKE ACK packet ({} bytes)", raw.len());
        }
        let mut cursor = Cursor::new(raw);
        let cmd_id = cursor.read_u32::<LittleEndian>()?;
        let status = cursor.read_i32::<LittleEndian>()?;

        if cmd_id == 0xFFFF_FFFF || status < 0 {
            return Err(RescueError::LokeFailure { code: status }.into());
        }

        Ok(Self { cmd_id, status })
    }

    /// Returns the positive payload / sector count or data volume returned by LOKE.
    pub fn payload(&self) -> u32 {
        self.status.max(0) as u32
    }

    pub fn is_success(&self) -> bool {
        self.status >= 0
    }
}
