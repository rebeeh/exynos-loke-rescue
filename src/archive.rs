use anyhow::{bail, Context, Result};
use colored::*;
use indicatif::{ProgressBar, ProgressStyle};
use lz4_flex::frame::FrameDecoder;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read};
use std::path::{Path, PathBuf};
use tar::Archive;

/// Finds the AP_*.tar.md5 file whether the user passed a file path or a directory.
pub fn locate_ap_archive(path: &Path) -> Result<PathBuf> {
    if path.is_file() {
        return Ok(path.to_path_buf());
    }
    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            let p = entry?.path();
            if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                if name.starts_with("AP_") && (name.ends_with(".tar.md5") || name.ends_with(".tar")) {
                    return Ok(p);
                }
            }
        }
    }
    bail!("Could not find an `AP_*.tar.md5` archive at {}", path.display());
}

/// Streams specific `.img.lz4` (or raw `.img`) entries directly out of a Samsung `.tar.md5`
/// archive and decompresses them on the fly into `out_dir`, bypassing the need to unpack the
/// entire 4.5+ GB archive first.
pub fn extract_and_decompress_images(
    tar_path: &Path,
    out_dir: &Path,
    targets: &[&str], // e.g., &["boot.img", "vbmeta.img", "recovery.img"]
) -> Result<HashMap<String, PathBuf>> {
    fs::create_dir_all(out_dir)
        .with_context(|| format!("Failed to create output directory {}", out_dir.display()))?;

    let file = File::open(tar_path)
        .with_context(|| format!("Failed to open archive {}", tar_path.display()))?;
    let reader = BufReader::with_capacity(8 * 1024 * 1024, file);
    let mut archive = Archive::new(reader);

    let mut extracted: HashMap<String, PathBuf> = HashMap::new();

    println!(
        "{} {}",
        "==> Scanning archive for signed partitions:".cyan().bold(),
        tar_path.display()
    );

    for entry_result in archive.entries()? {
        let mut entry = match entry_result {
            Ok(e) => e,
            // Samsung .tar.md5 files append an ASCII MD5 footer after the tar EOF blocks;
            // ignore trailing non-tar bytes once all target images are found.
            Err(_) if extracted.len() == targets.len() => break,
            Err(e) => return Err(e).context("Error reading tar entry"),
        };

        let entry_path = entry.path()?.to_path_buf();
        let file_name = match entry_path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        for &target in targets {
            let lz4_name = format!("{}.lz4", target);
            if file_name == lz4_name || file_name == target {
                let dest_path = out_dir.join(target);
                let compressed_size = entry.size();

                let pb = ProgressBar::new(compressed_size);
                pb.set_style(
                    ProgressStyle::default_bar()
                        .template("    {msg:<16} [{bar:32.green/white}] {bytes}/{total_bytes} ({bytes_per_sec})")
                        .unwrap()
                        .progress_chars("█▉▊▋▌▍▎▏ "),
                );
                pb.set_message(target.to_string());

                let out_file = File::create(&dest_path)
                    .with_context(|| format!("Failed to create {}", dest_path.display()))?;
                let mut writer = BufWriter::with_capacity(8 * 1024 * 1024, out_file);

                if file_name.ends_with(".lz4") {
                    let progress_reader = pb.wrap_read(&mut entry);
                    let mut decoder = FrameDecoder::new(progress_reader);
                    io::copy(&mut decoder, &mut writer)
                        .with_context(|| format!("LZ4 decompression failed for {}", file_name))?;
                } else {
                    let mut progress_reader = pb.wrap_read(&mut entry);
                    io::copy(&mut progress_reader, &mut writer)
                        .with_context(|| format!("Extraction failed for {}", file_name))?;
                }

                pb.finish_with_message(format!("{} (ready)", target));
                extracted.insert(target.to_string(), dest_path);
            }
        }

        if extracted.len() == targets.len() {
            break;
        }
    }

    for &target in targets {
        if !extracted.contains_key(target) {
            bail!(
                "Required partition `{}` (or `{}.lz4`) was not found inside {}",
                target,
                target,
                tar_path.display()
            );
        }
    }

    Ok(extracted)
}
