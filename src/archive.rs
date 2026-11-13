use crate::error::RescueError;
use anyhow::{Context, Result};
use colored::*;
use indicatif::{ProgressBar, ProgressStyle};
use lz4_flex::frame::FrameDecoder;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter};
use std::path::{Path, PathBuf};
use tar::Archive;

pub fn locate_ap_archive(path: &Path) -> Result<PathBuf> {
    if path.is_file() {
        return Ok(path.to_path_buf());
    }
    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            let p = entry?.path();
            if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                if name.starts_with("AP_")
                    && (name.ends_with(".tar.md5") || name.ends_with(".tar"))
                {
                    return Ok(p);
                }
            }
        }
    }
    Err(RescueError::ArchiveMissing(path.to_path_buf()).into())
}

pub fn extract_and_decompress_images(
    tar_path: &Path,
    out_dir: &Path,
    targets: &[&str],
) -> Result<HashMap<String, PathBuf>> {
    fs::create_dir_all(out_dir)?;

    let file = File::open(tar_path)
        .with_context(|| format!("Failed to open archive {}", tar_path.display()))?;
    let reader = BufReader::with_capacity(16 * 1024 * 1024, file);
    let mut archive = Archive::new(reader);

    let mut extracted: HashMap<String, PathBuf> = HashMap::new();

    println!(
        "{} {}",
        "==> Streaming signed partitions from:".cyan().bold(),
        tar_path.display()
    );

    for entry_result in archive.entries()? {
        let mut entry = match entry_result {
            Ok(e) => e,
            Err(_) if extracted.len() == targets.len() => break,
            Err(e) => return Err(e).context("Failed reading tar archive entries"),
        };

        let entry_path = entry.path()?.to_path_buf();
        let file_name = match entry_path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        for &target in targets {
            let lz4_variant = format!("{}.lz4", target);
            if file_name == lz4_variant || file_name == target {
                let dest_path = out_dir.join(target);
                let compressed_bytes = entry.size();

                let pb = ProgressBar::new(compressed_bytes);
                pb.set_style(
                    ProgressStyle::default_bar()
                        .template("    {msg:<16} [{bar:32.green/white}] {bytes}/{total_bytes} ({bytes_per_sec})")
                        .unwrap()
                        .progress_chars("█▉▊▋▌▍▎▏ "),
                );
                pb.set_message(target.to_string());

                let out_file = File::create(&dest_path)?;
                let mut writer = BufWriter::with_capacity(16 * 1024 * 1024, out_file);

                if file_name.ends_with(".lz4") {
                    let progress_reader = pb.wrap_read(&mut entry);
                    let mut decoder = FrameDecoder::new(progress_reader);
                    io::copy(&mut decoder, &mut writer)?;
                } else {
                    let mut progress_reader = pb.wrap_read(&mut entry);
                    io::copy(&mut progress_reader, &mut writer)?;
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
            return Err(RescueError::PartitionImageMissing {
                image: target.to_string(),
                archive: tar_path.to_path_buf(),
            }
            .into());
        }
    }

    Ok(extracted)
}
