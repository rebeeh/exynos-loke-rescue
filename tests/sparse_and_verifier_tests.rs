use byteorder::{LittleEndian, WriteBytesExt};
use exynos_loke_rescue::image_verifier::{ImageVerifier, PartitionReport};
use exynos_loke_rescue::pit::{PitEntry, PitTable};
use exynos_loke_rescue::sparse::{
    ImageFormat, SparseHeader, SPARSE_HEADER_MAGIC, SPARSE_HEADER_MAJOR_VER,
};
use std::io::{Cursor, Write};
use tar::Builder;
use tempfile::NamedTempFile;

#[test]
fn test_sparse_header_parsing_and_unsparse_size() {
    let mut buf = Vec::new();
    buf.write_u32::<LittleEndian>(SPARSE_HEADER_MAGIC).unwrap();
    buf.write_u16::<LittleEndian>(SPARSE_HEADER_MAJOR_VER)
        .unwrap();
    buf.write_u16::<LittleEndian>(0).unwrap(); // minor
    buf.write_u16::<LittleEndian>(28).unwrap(); // file hdr sz
    buf.write_u16::<LittleEndian>(12).unwrap(); // chunk hdr sz
    buf.write_u32::<LittleEndian>(4096).unwrap(); // block size
    buf.write_u32::<LittleEndian>(1000).unwrap(); // total blocks
    buf.write_u32::<LittleEndian>(10).unwrap(); // total chunks
    buf.write_u32::<LittleEndian>(0).unwrap(); // crc

    let header = SparseHeader::read_from(Cursor::new(&buf)).unwrap();
    assert_eq!(header.magic, SPARSE_HEADER_MAGIC);
    assert_eq!(header.block_size, 4096);
    assert_eq!(header.total_blocks, 1000);
    assert_eq!(header.unsparse_size(), 4_096_000);
}

#[test]
fn test_sparse_header_invalid_magic_rejected() {
    let mut buf = Vec::new();
    buf.write_u32::<LittleEndian>(0x1234_5678).unwrap(); // wrong magic
    buf.write_u16::<LittleEndian>(SPARSE_HEADER_MAJOR_VER)
        .unwrap();
    buf.write_u16::<LittleEndian>(0).unwrap();
    buf.write_u16::<LittleEndian>(28).unwrap();
    buf.write_u16::<LittleEndian>(12).unwrap();
    buf.write_u32::<LittleEndian>(4096).unwrap();
    buf.write_u32::<LittleEndian>(100).unwrap();
    buf.write_u32::<LittleEndian>(1).unwrap();
    buf.write_u32::<LittleEndian>(0).unwrap();

    let res = SparseHeader::read_from(Cursor::new(&buf));
    assert!(res.is_err());
}

#[test]
fn test_tar_md5_verification_pass_and_fail() {
    let mut raw_tar = Vec::new();
    {
        let mut builder = Builder::new(&mut raw_tar);
        let data = b"Sample partition payload data for testing.";
        let mut hdr = tar::Header::new_gnu();
        hdr.set_size(data.len() as u64);
        hdr.set_mode(0o644);
        hdr.set_cksum();
        builder
            .append_data(&mut hdr, "boot.img", &data[..])
            .unwrap();
        builder.finish().unwrap();
    }

    // Compute expected MD5 of raw tar
    let digest = format!("{:x}", md5::compute(&raw_tar));

    // Construct valid .tar.md5 file
    let mut valid_file = NamedTempFile::new().unwrap();
    valid_file.write_all(&raw_tar).unwrap();
    let trailer = format!("{}  test.tar\n", digest);
    valid_file.write_all(trailer.as_bytes()).unwrap();
    valid_file.flush().unwrap();

    let res = ImageVerifier::verify_tar_md5(valid_file.path()).unwrap();
    assert!(res.matches);
    assert_eq!(res.computed_md5, digest);

    // Construct invalid .tar.md5 with corrupt checksum
    let mut invalid_file = NamedTempFile::new().unwrap();
    invalid_file.write_all(&raw_tar).unwrap();
    let bad_trailer = "00000000000000000000000000000000  test.tar\n";
    invalid_file.write_all(bad_trailer.as_bytes()).unwrap();
    invalid_file.flush().unwrap();

    let res_bad = ImageVerifier::verify_tar_md5(invalid_file.path()).unwrap();
    assert!(!res_bad.matches);
}

#[test]
fn test_geometry_validation_within_pit_and_exceeding() {
    let pit = PitTable {
        entry_count: 1,
        entries: vec![PitEntry {
            binary_type: 0,
            device_type: 2,
            partition_id: 20,
            attributes: 2,
            update_attributes: 0,
            block_size_or_offset: 0,
            block_count: 2000, // 2000 sectors * 512 = 1,024,000 bytes (~1 MiB)
            file_offset: 0,
            file_size: 0,
            partition_name: "BOOT".into(),
            flash_filename: "boot.img".into(),
            fota_filename: String::new(),
        }],
    };

    // Partition fitting inside 1 MiB limit
    let fitting_report = PartitionReport {
        filename: "boot.img".into(),
        partition_name: "BOOT".into(),
        is_lz4_compressed: false,
        raw_archive_bytes: 500_000,
        format: ImageFormat::Raw { size: 500_000 },
    };
    assert!(ImageVerifier::verify_geometry(&fitting_report, &pit).is_ok());

    // Partition exceeding 1 MiB limit
    let oversized_report = PartitionReport {
        filename: "boot.img".into(),
        partition_name: "BOOT".into(),
        is_lz4_compressed: false,
        raw_archive_bytes: 2_000_000,
        format: ImageFormat::Raw { size: 2_000_000 },
    };
    assert!(ImageVerifier::verify_geometry(&oversized_report, &pit).is_err());
}
