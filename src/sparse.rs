//! Android Sparse Image format parser and validator
//!
//! Exynos LOKE firmware packages use the Android Sparse Image format for large
//! dynamic partitions (e.g. `super.img`, `userdata.img`, `system.img`).
//! This module provides deterministic parsing and geometry verification
//! to guarantee partition images do not exceed device storage boundaries.

use anyhow::{bail, Context, Result};
use byteorder::{LittleEndian, ReadBytesExt};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

pub const SPARSE_HEADER_MAGIC: u32 = 0xed26_ff3a;
pub const SPARSE_HEADER_MAJOR_VER: u16 = 1;

pub const CHUNK_TYPE_RAW: u16 = 0xcac1;
pub const CHUNK_TYPE_FILL: u16 = 0xcac2;
pub const CHUNK_TYPE_DONT_CARE: u16 = 0xcac3;
pub const CHUNK_TYPE_CRC32: u16 = 0xcac4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SparseHeader {
    pub magic: u32,
    pub major_version: u16,
    pub minor_version: u16,
    pub file_header_size: u16,
    pub chunk_header_size: u16,
    pub block_size: u32,
    pub total_blocks: u32,
    pub total_chunks: u32,
    pub image_checksum: u32,
}

impl SparseHeader {
    /// Read and validate an Android Sparse header from a stream.
    pub fn read_from<R: Read>(mut reader: R) -> Result<Self> {
        let magic = reader
            .read_u32::<LittleEndian>()
            .context("Failed to read sparse magic")?;
        if magic != SPARSE_HEADER_MAGIC {
            bail!(
                "Invalid sparse magic: 0x{:08x} (expected 0x{:08x})",
                magic,
                SPARSE_HEADER_MAGIC
            );
        }

        let major_version = reader.read_u16::<LittleEndian>()?;
        let minor_version = reader.read_u16::<LittleEndian>()?;
        let file_header_size = reader.read_u16::<LittleEndian>()?;
        let chunk_header_size = reader.read_u16::<LittleEndian>()?;
        let block_size = reader.read_u32::<LittleEndian>()?;
        let total_blocks = reader.read_u32::<LittleEndian>()?;
        let total_chunks = reader.read_u32::<LittleEndian>()?;
        let image_checksum = reader.read_u32::<LittleEndian>()?;

        if major_version != SPARSE_HEADER_MAJOR_VER {
            bail!(
                "Unsupported sparse major version: {} (supported: {})",
                major_version,
                SPARSE_HEADER_MAJOR_VER
            );
        }

        if block_size == 0 || block_size % 4 != 0 {
            bail!(
                "Invalid block size: {} (must be non-zero and multiple of 4)",
                block_size
            );
        }

        Ok(Self {
            magic,
            major_version,
            minor_version,
            file_header_size,
            chunk_header_size,
            block_size,
            total_blocks,
            total_chunks,
            image_checksum,
        })
    }

    /// Calculate the unsparse (expanded raw) size in bytes.
    pub fn unsparse_size(&self) -> u64 {
        self.total_blocks as u64 * self.block_size as u64
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageFormat {
    Raw { size: u64 },
    Sparse(SparseHeader),
}

impl ImageFormat {
    /// Inspect an image file on disk to determine whether it is raw or sparse.
    pub fn inspect_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        let mut file = std::fs::File::open(path)
            .with_context(|| format!("Failed to open image file: {}", path.display()))?;
        let file_len = file.metadata()?.len();

        if file_len < 28 {
            return Ok(ImageFormat::Raw { size: file_len });
        }

        let mut magic_buf = [0u8; 4];
        if file.read_exact(&mut magic_buf).is_ok() {
            let magic = u32::from_le_bytes(magic_buf);
            if magic == SPARSE_HEADER_MAGIC {
                file.seek(SeekFrom::Start(0))?;
                if let Ok(hdr) = SparseHeader::read_from(&mut file) {
                    return Ok(ImageFormat::Sparse(hdr));
                }
            }
        }

        Ok(ImageFormat::Raw { size: file_len })
    }

    /// Effective uncompressed byte size when flashed to device storage.
    pub fn effective_size(&self) -> u64 {
        match self {
            ImageFormat::Raw { size } => *size,
            ImageFormat::Sparse(hdr) => hdr.unsparse_size(),
        }
    }
}
