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
}

/// Subcommands for `LokeCommand::SessionSetup` (`0x64`).
#[repr(u32)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SessionSetupSubcmd {
    BeginSession = 0x00,
    SetTotalBytes = 0x01,
    SetTflashMode = 0x02,
    QueryDeviceType = 0x05, // Odin v4 only; omitted on v3 to avoid `-7` timeout
    SetSalesCode = 0x08,
}

/// Subcommands for `LokeCommand::PitTransfer` (`0x65`).
#[repr(u32)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PitSubcmd {
    DumpStart = 0x00,
    FlashStart = 0x01,
    FlashEnd = 0x02,
    DumpEnd = 0x03,
}

/// Subcommands for `LokeCommand::FileTransfer` (`0x66`).
#[repr(u32)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum FileTransferSubcmd {
    StartPartition = 0x00,
    DataSequence = 0x01,
    CommitPartition = 0x02,
}

/// Subcommands for `LokeCommand::SessionEnd` (`0x67`).
#[repr(u32)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SessionEndSubcmd {
    CloseNoReboot = 0x00,
    RebootNormal = 0x01,
    RebootDownload = 0x02,
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
}
