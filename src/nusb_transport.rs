use crate::error::RescueError;
use crate::loke_proto::{
    ControlFrame, FileTransferSubcmd, LokeAck, LokeCommand, PitSubcmd, ProtocolTier,
    SessionEndSubcmd, SessionSetupSubcmd, ACK_PACKET_SIZE, HANDSHAKE_REQUEST, HANDSHAKE_RESPONSE,
};
use crate::pit::PitTable;
use anyhow::{Context, Result};
use colored::*;
use futures_lite::future::block_on;
use indicatif::{ProgressBar, ProgressStyle};
use nusb::transfer::RequestBuffer;
use nusb::Interface;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use std::time::Duration;

const SAMSUNG_VID: u16 = 0x04e8;
const SAMSUNG_DOWNLOAD_PID: u16 = 0x685d;
const LOKE_INTERFACE_NUM: u8 = 1;
const EP_OUT: u8 = 0x01;
const EP_IN: u8 = 0x81;
const USB_MAX_PACKET_SIZE: usize = 512;

pub struct NativeLokeSession {
    iface: Interface,
    pub tier: ProtocolTier,
    pub verbose: bool,
}

impl NativeLokeSession {
    /// Opens Samsung USB device `04e8:685d`, claims Interface 1, and executes the
    /// `ODIN` <-> `LOKE` handshake + adaptive Odin v3/v4 session setup.
    pub fn connect(verbose: bool) -> Result<Self> {
        let dev_info = nusb::list_devices()
            .context("Failed to enumerate host USB devices via nusb")?
            .find(|d| d.vendor_id() == SAMSUNG_VID && d.product_id() == SAMSUNG_DOWNLOAD_PID)
            .ok_or(RescueError::DeviceNotDetected)?;

        let device = dev_info
            .open()
            .context("Failed to open Samsung USB device (check udev permissions)")?;

        // Detach cdc_acm if the kernel attached it to Interface 1
        let _ = device.detach_kernel_driver(LOKE_INTERFACE_NUM);

        let iface = device
            .claim_interface(LOKE_INTERFACE_NUM)
            .map_err(|e| RescueError::UsbInterfaceClaimFailed(e.to_string()))?;

        let mut session = Self {
            iface,
            tier: ProtocolTier::OdinV3,
            verbose,
        };

        session.perform_handshake()?;
        session.negotiate_session()?;
        Ok(session)
    }

    fn bulk_write(&self, data: &[u8], timeout_ms: u64) -> Result<()> {
        let fut = async {
            let comp = self.iface.bulk_out(EP_OUT, data.to_vec()).await;
            comp.status.map_err(|_| RescueError::UsbBulkTimeout {
                endpoint: EP_OUT,
                timeout_ms,
            })?;

            // Transmit Zero-Length Packet (ZLP) if payload is an exact multiple of 512B
            if !data.is_empty() && data.len().is_multiple_of(USB_MAX_PACKET_SIZE) {
                let zlp = self.iface.bulk_out(EP_OUT, Vec::new()).await;
                zlp.status.map_err(|_| RescueError::UsbBulkTimeout {
                    endpoint: EP_OUT,
                    timeout_ms,
                })?;
            }
            Ok::<(), anyhow::Error>(())
        };

        block_on(fut)
    }

    fn bulk_read(&self, len: usize, timeout_ms: u64) -> Result<Vec<u8>> {
        let fut = async {
            let comp = self.iface.bulk_in(EP_IN, RequestBuffer::new(len)).await;
            comp.status.map_err(|_| RescueError::UsbBulkTimeout {
                endpoint: EP_IN,
                timeout_ms,
            })?;
            Ok::<Vec<u8>, anyhow::Error>(comp.data)
        };

        block_on(fut)
    }

    fn send_control_and_ack(&self, frame: &ControlFrame, timeout_ms: u64) -> Result<LokeAck> {
        let wire = frame.to_bytes();
        if self.verbose {
            println!(
                "    [TX] cmd={:#04x} sub={:#04x} args=({:#x}, {:#x}, {:#x}, {:#x})",
                frame.cmd_id, frame.subcmd_id, frame.arg0, frame.arg1, frame.arg2, frame.arg3
            );
        }
        self.bulk_write(&wire, timeout_ms)?;
        let raw_ack = self.bulk_read(ACK_PACKET_SIZE, timeout_ms)?;
        let ack = LokeAck::parse(&raw_ack)?;
        if self.verbose {
            println!("    [RX] cmd={:#04x} status={}", ack.cmd_id, ack.status);
        }
        Ok(ack)
    }

    fn perform_handshake(&self) -> Result<()> {
        self.bulk_write(HANDSHAKE_REQUEST, 2000)?;
        let resp = self.bulk_read(4, 2000)?;
        if resp.as_slice() != HANDSHAKE_RESPONSE {
            return Err(RescueError::HandshakeFailed(resp).into());
        }
        Ok(())
    }

    /// Negotiates protocol version (`v4` vs `v3`). Critically, if the bootloader
    /// negotiates `v3` (as on Exynos 850), we NEVER send `QueryDeviceType (0x05)`,
    /// preventing the `error: -7` bulk read timeout seen in `odin4`.
    fn negotiate_session(&mut self) -> Result<()> {
        let req = ControlFrame::new(
            LokeCommand::SessionSetup,
            SessionSetupSubcmd::BeginSession as u32,
        )
        .with_args(0x04, 0, 0, 0);

        let ack = self.send_control_and_ack(&req, 5000)?;
        if ack.status >= 4 {
            self.tier = ProtocolTier::OdinV4;
        } else {
            self.tier = ProtocolTier::OdinV3;
        }

        if self.verbose {
            println!(
                "    {} Negotiated LOKE transport tier: {:?}",
                "[OK]".green().bold(),
                self.tier
            );
        }
        Ok(())
    }

    /// Dumps the live Partition Information Table directly from bootloader memory.
    pub fn fetch_pit(&self) -> Result<PitTable> {
        let start = ControlFrame::new(LokeCommand::PitTransfer, PitSubcmd::DumpStart as u32);
        let ack = self.send_control_and_ack(&start, 5000)?;
        let total_bytes = ack.status as usize;

        let mut pit_bytes = Vec::with_capacity(total_bytes);
        while pit_bytes.len() < total_bytes {
            let remaining = total_bytes - pit_bytes.len();
            let read_len = remaining.min(USB_MAX_PACKET_SIZE);
            let chunk = self.bulk_read(USB_MAX_PACKET_SIZE, 5000)?;
            pit_bytes.extend_from_slice(&chunk[..read_len.min(chunk.len())]);
        }

        let end = ControlFrame::new(LokeCommand::PitTransfer, PitSubcmd::DumpEnd as u32);
        let _ = self.send_control_and_ack(&end, 5000)?;

        PitTable::from_bytes(&pit_bytes)
    }

    /// Announces total session byte volume (`CMD_SESSION_SETUP` subcommand `0x01`).
    pub fn set_total_bytes(&self, total_bytes: u64) -> Result<()> {
        let low = (total_bytes & 0xFFFF_FFFF) as u32;
        let high = (total_bytes >> 32) as u32;
        let frame = ControlFrame::new(
            LokeCommand::SessionSetup,
            SessionSetupSubcmd::SetTotalBytes as u32,
        )
        .with_args(low, high, 0, 0);
        let _ = self.send_control_and_ack(&frame, 5000)?;
        Ok(())
    }

    /// Streams a raw `.img` file into a specific PIT partition ID using LOKE macro-sequences.
    pub fn flash_partition_file(
        &self,
        partition_name: &str,
        partition_id: u32,
        image_path: &Path,
    ) -> Result<()> {
        let file = File::open(image_path)?;
        let total_size = file.metadata()?.len();
        let mut reader = BufReader::with_capacity(8 * 1024 * 1024, file);

        let pb = ProgressBar::new(total_size);
        pb.set_style(
            ProgressStyle::default_bar()
                .template(
                    "    {msg:<12} [{bar:34.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec})",
                )
                .unwrap()
                .progress_chars("█▉▊▋▌▍▎▏ "),
        );
        pb.set_message(partition_name.to_string());

        let macro_size = self.tier.sequence_macro_size();
        let pkt_size = self.tier.max_usb_packet_size();
        let mut bytes_sent: u64 = 0;
        let mut seq_buf = vec![0u8; macro_size];

        while bytes_sent < total_size {
            let remaining = (total_size - bytes_sent) as usize;
            let current_seq_len = remaining.min(macro_size);

            // Pad sequence buffer to 512-byte sector boundary
            let aligned_len = (current_seq_len + 511) & !511;
            seq_buf[..aligned_len].fill(0);
            reader.read_exact(&mut seq_buf[..current_seq_len])?;

            // 1. Announce sequence byte count (`0x66`, subcmd `0x00`)
            let start_seq = ControlFrame::new(
                LokeCommand::FileTransfer,
                FileTransferSubcmd::StartPartition as u32,
            )
            .with_args(aligned_len as u32, 0, 0, 0);
            let _ = self.send_control_and_ack(&start_seq, 10_000)?;

            // 2. Transmit sequence in `pkt_size` USB bulk chunks
            let mut offset = 0;
            while offset < aligned_len {
                let end = (offset + pkt_size).min(aligned_len);
                self.bulk_write(&seq_buf[offset..end], 15_000)?;
                offset = end;
                pb.set_position((bytes_sent + offset as u64).min(total_size));
            }

            bytes_sent += current_seq_len as u64;
            let is_final_sequence = if bytes_sent >= total_size { 1 } else { 0 };

            // 3. Commit sequence & verify cryptographic signature on final block (`0x66`, subcmd `0x02`)
            let commit = ControlFrame::new(
                LokeCommand::FileTransfer,
                FileTransferSubcmd::CommitPartition as u32,
            )
            .with_args(0, aligned_len as u32, partition_id, is_final_sequence);

            let _ = self.send_control_and_ack(&commit, 60_000)?;
        }

        pb.finish_with_message(format!("{} OK", partition_name));
        Ok(())
    }

    /// Performs low-level hardware NAND storage erase on USERDATA / cache (`0x64`, subcommand `0x07`).
    /// Essential for unbricking devices stuck at Samsung Stock Recovery wipe prompts with broken physical power buttons.
    pub fn nand_erase(&self) -> Result<u32> {
        println!("{}", "==> Erasing storage (USERDATA)...".yellow().bold());
        let frame = ControlFrame::new(
            LokeCommand::SessionSetup,
            SessionSetupSubcmd::NandErase as u32,
        );
        // NAND hardware erase on eMMC / UFS flash can take up to 60 seconds
        let ack = self.send_control_and_ack(&frame, 60_000)?;
        let sectors = ack.payload();
        println!(
            "{}",
            format!("[OK] Storage erased successfully ({} sectors)", sectors)
                .green()
                .bold()
        );
        Ok(sectors)
    }

    /// Sets the 3-letter CSC / Sales Code in device storage (e.g. BTU, INS, TUR, XAA).
    pub fn set_sales_code(&self, code: &str) -> Result<()> {
        let mut chars = [0u32; 3];
        for (i, c) in code.chars().take(3).enumerate() {
            chars[i] = c as u32;
        }
        let frame = ControlFrame::new(
            LokeCommand::SessionSetup,
            SessionSetupSubcmd::SetSalesCode as u32,
        )
        .with_args(4, chars[0], chars[1], chars[2])
        .with_string(code);
        let _ = self.send_control_and_ack(&frame, 5_000)?;
        Ok(())
    }

    /// Sends `CMD_SESSION_END (0x67)` to clear Emergency Recovery flag `0x01` -> `0x00`
    /// and optionally trigger warm reboot or redownload.
    pub fn end_session_with_mode(&self, mode: SessionEndSubcmd) -> Result<()> {
        let msg = match mode {
            SessionEndSubcmd::CloseNoReboot => {
                "Ending session (keeping device in Download Mode)..."
            }
            SessionEndSubcmd::RebootNormal => "Ending session... Rebooting device...",
            SessionEndSubcmd::RebootDownload => "Ending session... Rebooting to Download Mode...",
        };
        println!("{}", msg.cyan());

        let end_frame = ControlFrame::new(LokeCommand::SessionEnd, mode as u32);
        let _ = self.send_control_and_ack(&end_frame, 10_000)?;
        std::thread::sleep(Duration::from_millis(250));
        Ok(())
    }

    /// Sends `CMD_SESSION_END (0x67)` to clear Emergency Recovery flag `0x01` -> `0x00`
    /// and optionally trigger warm reboot.
    pub fn end_session(&self, no_reboot: bool) -> Result<()> {
        let mode = if no_reboot {
            SessionEndSubcmd::CloseNoReboot
        } else {
            SessionEndSubcmd::RebootNormal
        };
        self.end_session_with_mode(mode)
    }
}
