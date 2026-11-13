use crate::error::RescueError;
use anyhow::{Context, Result};

use byteorder::{LittleEndian, ReadBytesExt};
use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek, SeekFrom};
use std::path::Path;

const PIT_MAGIC: u32 = 0x1234_9876;
const PIT_HEADER_SIZE: usize = 28;
const PIT_ENTRY_SIZE: usize = 132;

#[derive(Debug, Clone)]
pub struct PitEntry {
    pub binary_type: u32,
    pub device_type: u32,
    pub partition_id: u32,
    pub attributes: u32,
    pub update_attributes: u32,
    pub block_size_or_offset: u32,
    pub block_count: u32,
    pub file_offset: u32,
    pub file_size: u32,
    pub partition_name: String,
    pub flash_filename: String,
    pub fota_filename: String,
}

impl PitEntry {
    pub fn size_bytes(&self) -> u64 {
        (self.block_count as u64) * 512
    }
}

#[derive(Debug, Clone)]
pub struct PitTable {
    pub entry_count: u32,
    pub entries: Vec<PitEntry>,
}

impl PitTable {
    pub fn from_file(path: &Path) -> Result<Self> {
        let file = File::open(path)
            .with_context(|| format!("Failed to open PIT file {}", path.display()))?;
        let mut buf = Vec::new();
        BufReader::new(file).read_to_end(&mut buf)?;
        Self::from_bytes(&buf)
    }

    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.len() < PIT_HEADER_SIZE {
            anyhow::bail!("PIT buffer truncated ({} bytes)", data.len());
        }

        let mut cursor = Cursor::new(data);
        let magic = cursor.read_u32::<LittleEndian>()?;
        if magic != PIT_MAGIC {
            return Err(RescueError::InvalidPitMagic(magic).into());
        }

        let entry_count = cursor.read_u32::<LittleEndian>()?;
        cursor.seek(SeekFrom::Current(20))?;

        let mut entries = Vec::with_capacity(entry_count as usize);

        for idx in 0..entry_count {
            let offset = PIT_HEADER_SIZE + (idx as usize) * PIT_ENTRY_SIZE;
            if offset + PIT_ENTRY_SIZE > data.len() {
                anyhow::bail!("PIT truncated at entry {}", idx);
            }
            cursor.set_position(offset as u64);

            let binary_type = cursor.read_u32::<LittleEndian>()?;
            let device_type = cursor.read_u32::<LittleEndian>()?;
            let partition_id = cursor.read_u32::<LittleEndian>()?;
            let attributes = cursor.read_u32::<LittleEndian>()?;
            let update_attributes = cursor.read_u32::<LittleEndian>()?;
            let block_size_or_offset = cursor.read_u32::<LittleEndian>()?;
            let block_count = cursor.read_u32::<LittleEndian>()?;
            let file_offset = cursor.read_u32::<LittleEndian>()?;
            let file_size = cursor.read_u32::<LittleEndian>()?;

            let partition_name = read_cstring(&mut cursor, 32)?;
            let flash_filename = read_cstring(&mut cursor, 32)?;
            let fota_filename = read_cstring(&mut cursor, 32)?;

            entries.push(PitEntry {
                binary_type,
                device_type,
                partition_id,
                attributes,
                update_attributes,
                block_size_or_offset,
                block_count,
                file_offset,
                file_size,
                partition_name,
                flash_filename,
                fota_filename,
            });
        }

        Ok(Self {
            entry_count,
            entries,
        })
    }

    pub fn find_partition(&self, name: &str) -> Option<&PitEntry> {
        self.entries
            .iter()
            .find(|e| e.partition_name.eq_ignore_ascii_case(name))
    }

    pub fn find_entry(&self, name: &str) -> Option<&PitEntry> {
        self.find_partition(name)
    }

    pub fn print_summary(&self) {
        crate::ui::render_pit_table(self);
    }
}

fn read_cstring<R: Read>(reader: &mut R, len: usize) -> Result<String> {
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf)?;
    let nul = buf.iter().position(|&b| b == 0).unwrap_or(len);
    Ok(String::from_utf8_lossy(&buf[..nul]).trim().to_string())
}
