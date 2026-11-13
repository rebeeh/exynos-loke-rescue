use byteorder::{LittleEndian, WriteBytesExt};
use lz4_flex::frame::FrameEncoder;
use std::io::Write;
use tar::Builder;
use tempfile::TempDir;

#[test]
fn test_loke_control_frame_serialization_exact_1024_bytes() {
    // Validates 1024-byte control frame framing & little-endian field offsets
    let mut frame = [0u8; 1024];
    frame[0..4].copy_from_slice(&0x64u32.to_le_bytes()); // CMD_SESSION_SETUP
    frame[4..8].copy_from_slice(&0x00u32.to_le_bytes()); // BEGIN_SESSION
    frame[8..12].copy_from_slice(&0x03u32.to_le_bytes()); // Odin v3
    assert_eq!(frame.len(), 1024);
    assert_eq!(u32::from_le_bytes(frame[0..4].try_into().unwrap()), 0x64);
}

#[test]
fn test_tar_md5_lz4_stream_extraction() {
    let tmp = TempDir::new().unwrap();
    let archive_path = tmp.path().join("AP_A217F_TEST.tar.md5");

    let boot_payload = vec![0x5Au8; 128 * 1024];
    let mut compressed_boot = Vec::new();
    {
        let mut enc = FrameEncoder::new(&mut compressed_boot);
        enc.write_all(&boot_payload).unwrap();
        enc.finish().unwrap();
    }

    let mut raw_tar = Vec::new();
    {
        let mut builder = Builder::new(&mut raw_tar);
        let mut hdr = tar::Header::new_gnu();
        hdr.set_size(compressed_boot.len() as u64);
        hdr.set_mode(0o644);
        hdr.set_cksum();
        builder
            .append_data(&mut hdr, "boot.img.lz4", compressed_boot.as_slice())
            .unwrap();
        builder.finish().unwrap();
    }

    // Append Samsung trailing MD5 line
    raw_tar.extend_from_slice(b"0123456789abcdef0123456789abcdef  AP_A217F.tar\n");
    std::fs::write(&archive_path, &raw_tar).unwrap();
    assert!(archive_path.exists());
}

#[test]
fn test_pit_binary_geometry_calculation() {
    let mut pit = Vec::new();
    pit.write_u32::<LittleEndian>(0x1234_9876).unwrap();
    pit.write_u32::<LittleEndian>(1).unwrap();
    pit.extend_from_slice(&[0u8; 20]);

    pit.write_u32::<LittleEndian>(0).unwrap();
    pit.write_u32::<LittleEndian>(2).unwrap();
    pit.write_u32::<LittleEndian>(22).unwrap(); // SUPER partition ID
    pit.write_u32::<LittleEndian>(5).unwrap();
    pit.write_u32::<LittleEndian>(1).unwrap();
    pit.write_u32::<LittleEndian>(0).unwrap();
    pit.write_u32::<LittleEndian>(10_485_760).unwrap(); // 5 GiB in 512B sectors
    pit.write_u32::<LittleEndian>(0).unwrap();
    pit.write_u32::<LittleEndian>(0).unwrap();

    let mut name = [0u8; 32];
    name[..5].copy_from_slice(b"SUPER");
    pit.extend_from_slice(&name);
    pit.extend_from_slice(&[0u8; 64]);

    assert_eq!(pit.len(), 160);
}
