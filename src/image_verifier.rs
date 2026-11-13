//! Firmware Image Verification & Geometry Analysis Engine
//!
//! Provides validation of official Samsung `.tar.md5` archives, MD5 footer verification,
//! Android Sparse Image geometry inspection, and partition boundary verification against
//! PIT (Partition Information Table) records.

use crate::pit::PitTable;
use crate::sparse::{ImageFormat, SparseHeader, SPARSE_HEADER_MAGIC};
use anyhow::{bail, Context, Result};
use colored::*;
use indicatif::{ProgressBar, ProgressStyle};
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct Md5VerificationResult {
    pub file_path: String,
    pub expected_md5: String,
    pub computed_md5: String,
    pub matches: bool,
    pub tar_payload_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct PartitionReport {
    pub filename: String,
    pub partition_name: String,
    pub is_lz4_compressed: bool,
    pub raw_archive_bytes: u64,
    pub format: ImageFormat,
}

pub struct ImageVerifier;

impl ImageVerifier {
    /// Verify the trailing MD5 checksum of an official Samsung `.tar.md5` archive.
    pub fn verify_tar_md5<P: AsRef<Path>>(path: P) -> Result<Md5VerificationResult> {
        let path = path.as_ref();
        let mut file = File::open(path).with_context(|| {
            format!(
                "Failed to open archive for MD5 verification: {}",
                path.display()
            )
        })?;
        let file_len = file.metadata()?.len();

        if file_len < 128 {
            bail!("File too small to contain valid Samsung .tar.md5 footer");
        }

        // Read last 512 bytes to find the MD5 trailer line
        let tail_read_sz = 512.min(file_len);
        file.seek(SeekFrom::End(-(tail_read_sz as i64)))?;
        let mut tail_buf = vec![0u8; tail_read_sz as usize];
        file.read_exact(&mut tail_buf)?;

        let tail_str = String::from_utf8_lossy(&tail_buf);
        let last_line = tail_str
            .lines()
            .map(|l| l.trim_matches(|c: char| c.is_whitespace() || c == '\0'))
            .rfind(|l| !l.is_empty())
            .ok_or_else(|| anyhow::anyhow!("No valid MD5 trailer line found in archive footer"))?;

        let parts: Vec<&str> = last_line.split_whitespace().collect();
        if parts.is_empty()
            || parts[0].len() != 32
            || !parts[0].chars().all(|c| c.is_ascii_hexdigit())
        {
            bail!("Malformed MD5 trailer line: '{}'", last_line);
        }

        let expected_md5 = parts[0].to_ascii_lowercase();

        // Locate the exact starting byte of the 32-hex MD5 checksum in the tail
        let md5_bytes = expected_md5.as_bytes();
        let trailer_offset_in_tail = tail_buf
            .windows(32)
            .rposition(|w| w.eq_ignore_ascii_case(md5_bytes))
            .ok_or_else(|| anyhow::anyhow!("Failed to locate exact trailer boundary"))?;

        let tar_payload_bytes = file_len - (tail_read_sz - trailer_offset_in_tail as u64);

        // Stream and compute MD5
        file.seek(SeekFrom::Start(0))?;
        let mut reader = BufReader::with_capacity(1024 * 1024, file);
        let mut context = md5::Context::new();

        let pb = ProgressBar::new(tar_payload_bytes);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("  [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta}) {msg}")
                .unwrap()
                .progress_chars("#>-"),
        );
        pb.set_message("Verifying MD5 integrity...");

        let mut remaining = tar_payload_bytes;
        let mut chunk_buf = vec![0u8; 128 * 1024];

        while remaining > 0 {
            let to_read = remaining.min(chunk_buf.len() as u64) as usize;
            reader.read_exact(&mut chunk_buf[..to_read])?;
            context.consume(&chunk_buf[..to_read]);
            remaining -= to_read as u64;
            pb.set_position(tar_payload_bytes - remaining);
        }

        pb.finish_with_message("MD5 calculation complete.");

        let digest = context.finalize();
        let computed_md5 = format!("{:x}", digest).to_ascii_lowercase();
        let matches = expected_md5 == computed_md5;

        Ok(Md5VerificationResult {
            file_path: path.display().to_string(),
            expected_md5,
            computed_md5,
            matches,
            tar_payload_bytes,
        })
    }

    /// Inspect all partition images inside a `.tar` or `.tar.md5` package,
    /// identifying compression, sparse formatting, and decompressed boundaries.
    pub fn inspect_tar_partitions<P: AsRef<Path>>(path: P) -> Result<Vec<PartitionReport>> {
        let path = path.as_ref();
        let file = File::open(path)
            .with_context(|| format!("Failed to open TAR archive: {}", path.display()))?;
        let mut archive = tar::Archive::new(BufReader::with_capacity(1024 * 1024, file));

        let mut reports = Vec::new();

        for entry_res in archive.entries()? {
            let entry = entry_res?;
            let entry_path = entry.path()?.to_string_lossy().to_string();
            let entry_len = entry.size();

            let is_lz4 = entry_path.ends_with(".lz4");
            let base_name = if is_lz4 {
                entry_path.trim_end_matches(".lz4")
            } else {
                &entry_path
            };

            let part_name = base_name.trim_end_matches(".img").to_ascii_uppercase();

            // Check if uncompressed image starts with sparse header
            let mut format = ImageFormat::Raw { size: entry_len };
            if !is_lz4 && entry_len >= 28 {
                let mut header_buf = [0u8; 28];
                let mut reader = entry;
                if reader.read_exact(&mut header_buf).is_ok() {
                    let magic = u32::from_le_bytes(header_buf[0..4].try_into().unwrap());
                    if magic == SPARSE_HEADER_MAGIC {
                        if let Ok(hdr) = SparseHeader::read_from(&header_buf[..]) {
                            format = ImageFormat::Sparse(hdr);
                        }
                    }
                }
            }

            reports.push(PartitionReport {
                filename: entry_path,
                partition_name: part_name,
                is_lz4_compressed: is_lz4,
                raw_archive_bytes: entry_len,
                format,
            });
        }

        Ok(reports)
    }

    /// Verify that a partition image fits within the PIT partition table boundaries.
    pub fn verify_geometry(report: &PartitionReport, pit: &PitTable) -> Result<()> {
        let entry = match pit.find_entry(&report.partition_name) {
            Some(e) => e,
            None => {
                println!(
                    "  {} Partition '{}' is not declared in device PIT table (ignoring size limit)",
                    "!".yellow(),
                    report.partition_name
                );
                return Ok(());
            }
        };

        let max_part_bytes = entry.block_count as u64 * 512;
        let effective_bytes = report.format.effective_size();

        if effective_bytes > max_part_bytes && !report.is_lz4_compressed {
            bail!(
                "Partition '{}' payload ({} bytes) exceeds PIT boundary ({} bytes, {} sectors)",
                report.partition_name,
                effective_bytes,
                max_part_bytes,
                entry.block_count
            );
        }

        Ok(())
    }
}
