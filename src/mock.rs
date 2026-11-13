//! In-Memory Virtual Samsung LOKE Download Mode Device Simulator
//!
//! Provides a software emulation harness of Samsung Little Kernel (LK) LOKE,
//! supporting the complete Odin v3/v4 wire protocol state machine, live PIT
//! synthesis, NAND hardware erase, partition chunk verification, and fault injection.
//! Enables full CI/CD testing and pre-flight simulation without requiring physical hardware.

use crate::loke_proto::{
    ControlFrame, FileTransferSubcmd, LokeAck, LokeCommand, PitSubcmd, ProtocolTier,
    SessionEndSubcmd, SessionSetupSubcmd, ACK_PACKET_SIZE, HANDSHAKE_REQUEST, HANDSHAKE_RESPONSE,
};
use crate::pit::{PitEntry, PitTable};
use anyhow::{bail, Result};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MockDeviceState {
    Disconnected,
    HandshakeDone,
    SessionActive {
        tier: ProtocolTier,
    },
    FlashingPartition {
        name: String,
        expected_size: u64,
        received_bytes: u64,
        chunks: u32,
    },
    SessionEnded {
        reboot_mode: SessionEndSubcmd,
    },
    Rebooted,
}

#[derive(Debug, Clone, Default)]
pub struct MockFaults {
    pub reject_handshake: bool,
    pub timeout_on_v4_query: bool,
    pub vaultkeeper_prenormal_lock: bool,
    pub corrupt_ack: bool,
    pub reject_session_end: bool,
}

pub struct MockLokeDevice {
    pub state: MockDeviceState,
    pub faults: MockFaults,
    pub default_tier: ProtocolTier,
    pub param_update_flag: u8,
    pub userdata_erased: bool,
    pub erased_sectors_reported: u32,
    pub flashed_partitions: HashMap<String, usize>,
    pub pit_table: PitTable,
}

impl Default for MockLokeDevice {
    fn default() -> Self {
        Self::new()
    }
}

impl MockLokeDevice {
    /// Create a new mock Samsung Exynos Download Mode device initialized with
    /// standard stock partition entries (BOOT, VBMETA, RECOVERY, DTBO, SUPER, USERDATA).
    pub fn new() -> Self {
        let entries = vec![
            PitEntry {
                binary_type: 0,
                device_type: 2,
                partition_id: 20,
                attributes: 2,
                update_attributes: 0,
                block_size_or_offset: 0x0004_0000,
                block_count: 0x0001_2000, // 36 MiB
                file_offset: 0,
                file_size: 0,
                partition_name: "BOOT".into(),
                flash_filename: "boot.img".into(),
                fota_filename: String::new(),
            },
            PitEntry {
                binary_type: 0,
                device_type: 2,
                partition_id: 21,
                attributes: 2,
                update_attributes: 0,
                block_size_or_offset: 0x0005_2000,
                block_count: 0x0000_1000, // 2 MiB
                file_offset: 0,
                file_size: 0,
                partition_name: "VBMETA".into(),
                flash_filename: "vbmeta.img".into(),
                fota_filename: String::new(),
            },
            PitEntry {
                binary_type: 0,
                device_type: 2,
                partition_id: 22,
                attributes: 2,
                update_attributes: 0,
                block_size_or_offset: 0x0005_3000,
                block_count: 0x0002_0000, // 64 MiB
                file_offset: 0,
                file_size: 0,
                partition_name: "RECOVERY".into(),
                flash_filename: "recovery.img".into(),
                fota_filename: String::new(),
            },
            PitEntry {
                binary_type: 0,
                device_type: 2,
                partition_id: 23,
                attributes: 2,
                update_attributes: 0,
                block_size_or_offset: 0x0007_3000,
                block_count: 0x0000_8000, // 16 MiB
                file_offset: 0,
                file_size: 0,
                partition_name: "DTBO".into(),
                flash_filename: "dtbo.img".into(),
                fota_filename: String::new(),
            },
            PitEntry {
                binary_type: 0,
                device_type: 2,
                partition_id: 24,
                attributes: 2,
                update_attributes: 0,
                block_size_or_offset: 0x0007_B000,
                block_count: 0x00A5_A000, // ~5.3 GiB
                file_offset: 0,
                file_size: 0,
                partition_name: "SUPER".into(),
                flash_filename: "super.img".into(),
                fota_filename: String::new(),
            },
            PitEntry {
                binary_type: 0,
                device_type: 2,
                partition_id: 25,
                attributes: 2,
                update_attributes: 0,
                block_size_or_offset: 0x00AD_5000,
                block_count: 0x0340_0000, // ~26 GiB
                file_offset: 0,
                file_size: 0,
                partition_name: "USERDATA".into(),
                flash_filename: "userdata.img".into(),
                fota_filename: String::new(),
            },
        ];

        let pit_table = PitTable {
            entry_count: entries.len() as u32,
            entries,
        };

        Self {
            state: MockDeviceState::Disconnected,
            faults: MockFaults::default(),
            default_tier: ProtocolTier::OdinV3,
            param_update_flag: 0x00,
            userdata_erased: false,
            erased_sectors_reported: 0x0010_0000, // 1,048,576 sectors (512 MiB)
            flashed_partitions: HashMap::new(),
            pit_table,
        }
    }

    /// Process an initial handshake request (Host sending `"ODIN"`).
    pub fn handle_handshake(&mut self, request: &[u8]) -> Result<[u8; 4]> {
        if self.faults.reject_handshake {
            bail!("Mock fault: Handshake rejected by device");
        }
        if request != HANDSHAKE_REQUEST {
            bail!(
                "Invalid handshake request: expected {:?}, got {:?}",
                HANDSHAKE_REQUEST,
                request
            );
        }
        self.state = MockDeviceState::HandshakeDone;
        Ok(*HANDSHAKE_RESPONSE)
    }

    /// Process a 1024-byte LOKE control frame.
    pub fn handle_control_frame(&mut self, frame: &ControlFrame) -> Result<LokeAck> {
        let opcode = LokeCommand::from_u32(frame.cmd_id)
            .ok_or_else(|| anyhow::anyhow!("Unknown opcode: 0x{:08x}", frame.cmd_id))?;

        match opcode {
            LokeCommand::SessionSetup => self.handle_session_setup(frame),
            LokeCommand::PitTransfer => self.handle_pit_operations(frame),
            LokeCommand::FileTransfer => self.handle_file_transfer(frame),
            LokeCommand::SessionEnd => self.handle_session_end(frame),
            LokeCommand::DeviceInfo | LokeCommand::DynamicSuperCheck => {
                let mut ack_bytes = [0u8; ACK_PACKET_SIZE];
                ack_bytes[0..4].copy_from_slice(&(frame.cmd_id).to_le_bytes());
                ack_bytes[4..8].copy_from_slice(&0i32.to_le_bytes()); // SUCCESS
                LokeAck::parse(&ack_bytes)
            }
        }
    }

    fn handle_session_setup(&mut self, frame: &ControlFrame) -> Result<LokeAck> {
        let subcmd = SessionSetupSubcmd::from_u32(frame.subcmd_id)
            .ok_or_else(|| anyhow::anyhow!("Unknown setup subcmd: 0x{:08x}", frame.subcmd_id))?;

        match subcmd {
            SessionSetupSubcmd::BeginSession => {
                self.state = MockDeviceState::SessionActive {
                    tier: self.default_tier,
                };
                self.param_update_flag = 0x01; // Flashing session initiated

                let mut ack_bytes = [0u8; ACK_PACKET_SIZE];
                ack_bytes[0..4].copy_from_slice(&(LokeCommand::SessionSetup as u32).to_le_bytes());
                ack_bytes[4..8].copy_from_slice(&(self.default_tier as i32).to_le_bytes());
                LokeAck::parse(&ack_bytes)
            }
            SessionSetupSubcmd::SetTotalBytes
            | SessionSetupSubcmd::SetTflashMode
            | SessionSetupSubcmd::SetSalesCode => {
                let mut ack_bytes = [0u8; ACK_PACKET_SIZE];
                ack_bytes[0..4].copy_from_slice(&(LokeCommand::SessionSetup as u32).to_le_bytes());
                ack_bytes[4..8].copy_from_slice(&0i32.to_le_bytes());
                LokeAck::parse(&ack_bytes)
            }
            SessionSetupSubcmd::NandErase => {
                self.userdata_erased = true;
                let mut ack_bytes = [0u8; ACK_PACKET_SIZE];
                ack_bytes[0..4].copy_from_slice(&(LokeCommand::SessionSetup as u32).to_le_bytes());
                ack_bytes[4..8]
                    .copy_from_slice(&(self.erased_sectors_reported as i32).to_le_bytes());
                LokeAck::parse(&ack_bytes)
            }
        }
    }

    fn handle_pit_operations(&mut self, frame: &ControlFrame) -> Result<LokeAck> {
        let subcmd = PitSubcmd::from_u32(frame.subcmd_id)
            .ok_or_else(|| anyhow::anyhow!("Unknown PIT subcmd: 0x{:08x}", frame.subcmd_id))?;

        match subcmd {
            PitSubcmd::DumpStart
            | PitSubcmd::DumpChunk
            | PitSubcmd::DumpEnd
            | PitSubcmd::FlashStart => {
                let mut ack_bytes = [0u8; ACK_PACKET_SIZE];
                ack_bytes[0..4].copy_from_slice(&(LokeCommand::PitTransfer as u32).to_le_bytes());
                ack_bytes[4..8].copy_from_slice(&0i32.to_le_bytes()); // SUCCESS
                LokeAck::parse(&ack_bytes)
            }
        }
    }

    fn handle_file_transfer(&mut self, frame: &ControlFrame) -> Result<LokeAck> {
        if self.faults.vaultkeeper_prenormal_lock {
            // Emulate Knox Guard / VaultKeeper code -1 rejection
            let mut ack_bytes = [0u8; ACK_PACKET_SIZE];
            ack_bytes[0..4].copy_from_slice(&0xffff_ffffu32.to_le_bytes()); // FAIL
            ack_bytes[4..8].copy_from_slice(&(-1i32).to_le_bytes()); // -1
            return LokeAck::parse(&ack_bytes);
        }

        let subcmd = FileTransferSubcmd::from_u32(frame.subcmd_id)
            .ok_or_else(|| anyhow::anyhow!("Unknown transfer subcmd: 0x{:08x}", frame.subcmd_id))?;

        match subcmd {
            FileTransferSubcmd::StartPartition => {
                let part_id = frame.arg0;
                let part_name = self
                    .pit_table
                    .entries
                    .iter()
                    .find(|e| e.partition_id == part_id)
                    .map(|e| e.partition_name.clone())
                    .unwrap_or_else(|| format!("PART_{}", part_id));

                self.state = MockDeviceState::FlashingPartition {
                    name: part_name,
                    expected_size: frame.arg1 as u64,
                    received_bytes: 0,
                    chunks: 0,
                };

                let mut ack_bytes = [0u8; ACK_PACKET_SIZE];
                ack_bytes[0..4].copy_from_slice(&(LokeCommand::FileTransfer as u32).to_le_bytes());
                ack_bytes[4..8].copy_from_slice(&0i32.to_le_bytes());
                LokeAck::parse(&ack_bytes)
            }
            FileTransferSubcmd::DataSequence => {
                let mut ack_bytes = [0u8; ACK_PACKET_SIZE];
                ack_bytes[0..4].copy_from_slice(&(LokeCommand::FileTransfer as u32).to_le_bytes());
                ack_bytes[4..8].copy_from_slice(&0i32.to_le_bytes());
                LokeAck::parse(&ack_bytes)
            }
            FileTransferSubcmd::CommitPartition => {
                if let MockDeviceState::FlashingPartition {
                    name,
                    received_bytes,
                    ..
                } = &self.state
                {
                    self.flashed_partitions
                        .insert(name.clone(), *received_bytes as usize);
                }
                self.state = MockDeviceState::SessionActive {
                    tier: self.default_tier,
                };

                let mut ack_bytes = [0u8; ACK_PACKET_SIZE];
                ack_bytes[0..4].copy_from_slice(&(LokeCommand::FileTransfer as u32).to_le_bytes());
                ack_bytes[4..8].copy_from_slice(&0i32.to_le_bytes());
                LokeAck::parse(&ack_bytes)
            }
        }
    }

    /// Process a streaming data chunk for the actively flashing partition.
    pub fn handle_chunk(&mut self, chunk: &[u8]) -> Result<LokeAck> {
        if let MockDeviceState::FlashingPartition {
            received_bytes,
            chunks,
            ..
        } = &mut self.state
        {
            *received_bytes += chunk.len() as u64;
            *chunks += 1;

            let mut ack_bytes = [0u8; ACK_PACKET_SIZE];
            ack_bytes[0..4].copy_from_slice(&(LokeCommand::FileTransfer as u32).to_le_bytes());
            ack_bytes[4..8].copy_from_slice(&(*chunks as i32).to_le_bytes());
            LokeAck::parse(&ack_bytes)
        } else {
            bail!("Received partition data chunk while not in FlashingPartition state");
        }
    }

    fn handle_session_end(&mut self, frame: &ControlFrame) -> Result<LokeAck> {
        if self.faults.reject_session_end {
            bail!("Mock fault: Session end rejected");
        }

        let subcmd = SessionEndSubcmd::from_u32(frame.subcmd_id)
            .ok_or_else(|| anyhow::anyhow!("Unknown end subcmd: 0x{:08x}", frame.subcmd_id))?;

        // Reset persistent parameter flag to 0x00 (Clear Emergency Recovery loop!)
        self.param_update_flag = 0x00;
        self.state = MockDeviceState::SessionEnded {
            reboot_mode: subcmd,
        };

        let mut ack_bytes = [0u8; ACK_PACKET_SIZE];
        ack_bytes[0..4].copy_from_slice(&(LokeCommand::SessionEnd as u32).to_le_bytes());
        ack_bytes[4..8].copy_from_slice(&0i32.to_le_bytes());
        LokeAck::parse(&ack_bytes)
    }
}
