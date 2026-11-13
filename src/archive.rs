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
use tempfile::TempDir;

/// Expands leading `~` into user's home directory path.
pub fn expand_tilde(path: &Path) -> PathBuf {
    if let Ok(stripped) = path.strip_prefix("~") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(stripped);
        }
    }
    path.to_path_buf()
}

/// Resolves a suitable working directory for extracting partition images.
/// Avoids using /tmp for multi-gigabyte files on systems where /tmp is a small RAM tmpfs.
pub fn resolve_work_dir(
    keep_extracted: Option<PathBuf>,
    preferred_base: &Path,
) -> Result<(Option<TempDir>, PathBuf)> {
    if let Some(d) = keep_extracted {
        let d = expand_tilde(&d);
        fs::create_dir_all(&d)
            .with_context(|| format!("Failed to create output directory {}", d.display()))?;
        return Ok((None, d));
    }

    let base = expand_tilde(preferred_base);
    let target_base = if base.is_dir() {
        base
    } else if let Some(parent) = base.parent() {
        if parent.as_os_str().is_empty() {
            PathBuf::from(".")
        } else {
            parent.to_path_buf()
        }
    } else {
        PathBuf::from(".")
    };

    // 1. Try placing temporary directory next to stock archive on user's persistent filesystem (/home)
    if let Ok(tmp) = tempfile::Builder::new()
        .prefix(".loke_tmp_")
        .tempdir_in(&target_base)
    {
        let p = tmp.path().to_path_buf();
        return Ok((Some(tmp), p));
    }

    // 2. Try current working directory
    if let Ok(tmp) = tempfile::Builder::new()
        .prefix(".loke_tmp_")
        .tempdir_in(".")
    {
        let p = tmp.path().to_path_buf();
        return Ok((Some(tmp), p));
    }

    // 3. Fallback to system temporary directory
    let tmp = TempDir::new().context("Failed to allocate temporary working directory")?;
    let p = tmp.path().to_path_buf();
    Ok((Some(tmp), p))
}

pub fn locate_ap_archive(path: &Path) -> Result<PathBuf> {
    let path = expand_tilde(path);
    if path.is_file() {
        return Ok(path);
    }
    if path.is_dir() {
        for entry in fs::read_dir(&path)? {
            let p = entry?.path();
            if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                if name.starts_with("AP_") && (name.ends_with(".tar.md5") || name.ends_with(".tar"))
                {
                    return Ok(p);
                }
            }
        }
    }
    Err(RescueError::ArchiveMissing(path).into())
}

pub fn extract_and_decompress_images(
    tar_path: &Path,
    out_dir: &Path,
    targets: &[&str],
) -> Result<HashMap<String, PathBuf>> {
    let tar_path = expand_tilde(tar_path);
    let out_dir = expand_tilde(out_dir);
    fs::create_dir_all(&out_dir)?;

    let mut extracted: HashMap<String, PathBuf> = HashMap::new();

    // Check if targets are already present with non-zero size in out_dir or next to archive
    let mut missing_targets = Vec::new();
    for &target in targets {
        let dest_path = out_dir.join(target);
        if dest_path.is_file() && fs::metadata(&dest_path).map(|m| m.len()).unwrap_or(0) > 0 {
            extracted.insert(target.to_string(), dest_path);
            continue;
        }

        if let Some(parent) = tar_path.parent() {
            let parent_path = parent.join(target);
            if parent_path.is_file() && fs::metadata(&parent_path).map(|m| m.len()).unwrap_or(0) > 0 {
                extracted.insert(target.to_string(), parent_path);
                continue;
            }
        }

        missing_targets.push(target);
    }

    if missing_targets.is_empty() {
        println!(
            "{} Reusing verified partition images from storage",
            "[OK]".green().bold()
        );
        return Ok(extracted);
    }

    let file = File::open(&tar_path)
        .with_context(|| format!("Failed to open archive {}", tar_path.display()))?;
    let reader = BufReader::with_capacity(16 * 1024 * 1024, file);
    let mut archive = Archive::new(reader);

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
            if extracted.contains_key(target) {
                continue;
            }

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
