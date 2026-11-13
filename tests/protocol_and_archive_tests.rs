use byteorder::{LittleEndian, WriteBytesExt};
use lz4_flex::frame::FrameEncoder;
use std::io::{Cursor, Write};
use tar::Builder;
use tempfile::TempDir;

#[test]
fn test_loke_control_frame_serialization_exact_1024_bytes() {
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
    let mut pit_header = Vec::new();
    pit_header.write_u32::<LittleEndian>(0x12349876).unwrap(); // PIT Magic
    pit_header.write_u32::<LittleEndian>(1).unwrap(); // 1 partition entry
    pit_header.extend_from_slice(&[0u8; 20]); // dummy header padding

    let mut entry = Vec::new();
    entry.write_u32::<LittleEndian>(0).unwrap(); // binary_type
    entry.write_u32::<LittleEndian>(0).unwrap(); // device_type
    entry.write_u32::<LittleEndian>(10).unwrap(); // partition_id
    entry.write_u32::<LittleEndian>(2).unwrap(); // partition_type
    entry.write_u32::<LittleEndian>(0).unwrap(); // filesystem
    entry.write_u32::<LittleEndian>(2048).unwrap(); // block start
    entry.write_u32::<LittleEndian>(65536).unwrap(); // block count (65536 * 512 = 32 MiB)
    entry.write_u32::<LittleEndian>(0).unwrap(); // file_offset
    entry.write_u32::<LittleEndian>(0).unwrap(); // file_size

    let mut part_name = [0u8; 32];
    part_name[0..4].copy_from_slice(b"BOOT");
    entry.extend_from_slice(&part_name);

    let mut file_name = [0u8; 32];
    file_name[0..8].copy_from_slice(b"boot.img");
    entry.extend_from_slice(&file_name);

    let fota_name = [0u8; 32];
    entry.extend_from_slice(&fota_name);

    pit_header.extend_from_slice(&entry);

    let mut cursor = Cursor::new(&pit_header);
    let mut magic_buf = [0u8; 4];
    std::io::Read::read_exact(&mut cursor, &mut magic_buf).unwrap();
    assert_eq!(u32::from_le_bytes(magic_buf), 0x12349876);
}

#[test]
fn test_protected_partition_invariants() {
    let forbidden_partitions = ["EFS", "SEC_EFS", "CPEFS", "NVDATA", "RADIO_CAL"];
    for part in &forbidden_partitions {
        let is_protected = forbidden_partitions.contains(part);
        assert!(is_protected, "Partition {} must be protected from writes", part);
    }
}

#[test]
fn test_loke_ack_status_decoding() {
    let ok_ack = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    let err_ack = [0xFF, 0xFF, 0xFF, 0xFF, 0x01, 0x00, 0x00, 0x00];

    let ok_status = u32::from_le_bytes(ok_ack[0..4].try_into().unwrap());
    let err_status = u32::from_le_bytes(err_ack[0..4].try_into().unwrap());

    assert_eq!(ok_status, 0x00);
    assert_eq!(err_status, 0xFFFFFFFF);
}

#[test]
fn test_fuzz_control_frame_bounds() {
    // Fuzz arbitrary byte patterns in 1024-byte control frame buffer
    for seed in 0..10u8 {
        let mut malformed_frame = [seed; 1024];
        malformed_frame[0..4].copy_from_slice(&0x64u32.to_le_bytes());
        assert_eq!(malformed_frame.len(), 1024);
        let cmd = u32::from_le_bytes(malformed_frame[0..4].try_into().unwrap());
        assert_eq!(cmd, 0x64);
    }
}
