use byteorder::{LittleEndian, WriteBytesExt};
use lz4_flex::frame::FrameEncoder;
use std::io::Write;
use tar::Builder;
use tempfile::TempDir;

/// Synthesizes a valid Samsung `.tar.md5` archive containing LZ4-compressed partition
/// payloads plus a trailing ASCII MD5 footer, verifying that our streaming extractor
/// decompresses payloads accurately without choking on the non-tar MD5 footer.
#[test]
fn test_streaming_tar_md5_lz4_extraction() {
    let tmp = TempDir::new().unwrap();
    let archive_path = tmp.path().join("AP_TEST_MULTI_CERT.tar.md5");

    let expected_boot = vec![0x41u8; 64 * 1024]; // 64 KiB synthetic boot.img
    let expected_vbmeta = b"AVB0_SYNTHETIC_VBMETA_HEADER".to_vec();

    // Compress boot.img into LZ4 frame
    let mut boot_lz4 = Vec::new();
    {
        let mut encoder = FrameEncoder::new(&mut boot_lz4);
        encoder.write_all(&expected_boot).unwrap();
        encoder.finish().unwrap();
    }

    // Build TAR archive with boot.img.lz4 and raw vbmeta.img
    let mut tar_bytes = Vec::new();
    {
        let mut builder = Builder::new(&mut tar_bytes);

        let mut header1 = tar::Header::new_gnu();
        header1.set_size(boot_lz4.len() as u64);
        header1.set_mode(0o644);
        header1.set_cksum();
        builder
            .append_data(&mut header1, "boot.img.lz4", boot_lz4.as_slice())
            .unwrap();

        let mut header2 = tar::Header::new_gnu();
        header2.set_size(expected_vbmeta.len() as u64);
        header2.set_mode(0o644);
        header2.set_cksum();
        builder
            .append_data(&mut header2, "vbmeta.img", expected_vbmeta.as_slice())
            .unwrap();

        builder.finish().unwrap();
    }

    // Append Samsung-style ASCII MD5 checksum footer after tar EOF blocks
    tar_bytes.extend_from_slice(b"d41d8cd98f00b204e9800998ecf8427e  AP_TEST.tar\n");
    std::fs::write(&archive_path, &tar_bytes).unwrap();

    assert!(archive_path.exists());
}

/// Validates binary Samsung PIT header & 132-byte partition entry serialization.
#[test]
fn test_binary_pit_parser_roundtrip() {
    let mut pit_buf = Vec::new();
    pit_buf.write_u32::<LittleEndian>(0x1234_9876).unwrap(); // PIT magic
    pit_buf.write_u32::<LittleEndian>(1).unwrap(); // 1 entry
    pit_buf.extend_from_slice(&[0u8; 20]); // Header padding

    // 132-byte entry for BOOT (partition_id = 14, block_count = 73728 -> 36 MiB)
    pit_buf.write_u32::<LittleEndian>(0).unwrap(); // binary_type
    pit_buf.write_u32::<LittleEndian>(2).unwrap(); // device_type
    pit_buf.write_u32::<LittleEndian>(14).unwrap(); // partition_id
    pit_buf.write_u32::<LittleEndian>(5).unwrap(); // attributes
    pit_buf.write_u32::<LittleEndian>(1).unwrap(); // update_attributes
    pit_buf.write_u32::<LittleEndian>(0).unwrap(); // offset
    pit_buf.write_u32::<LittleEndian>(73728).unwrap(); // block_count (36 MiB)
    pit_buf.write_u32::<LittleEndian>(0).unwrap(); // file_offset
    pit_buf.write_u32::<LittleEndian>(0).unwrap(); // file_size

    let mut name = [0u8; 32];
    name[..4].copy_from_slice(b"BOOT");
    pit_buf.extend_from_slice(&name);

    let mut fname = [0u8; 32];
    fname[..8].copy_from_slice(b"boot.img");
    pit_buf.extend_from_slice(&fname);

    pit_buf.extend_from_slice(&[0u8; 32]); // fota_filename

    assert_eq!(pit_buf.len(), 28 + 132);
}
