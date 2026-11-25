use crate::model::{BgJobHandle, ZipEntryInfo};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

const ZIP_BUF_SIZE: usize = 64 * 1024;

pub fn compress_to_zip(
    sources: &[PathBuf],
    output_zip: &Path,
    job: &BgJobHandle,
) -> Result<PathBuf, String> {
    let mut entries: Vec<(PathBuf, String, bool)> = Vec::new();

    for src in sources {
        if !src.exists() {
            continue;
        }
        let base_name = src
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "item".to_string());

        if src.is_dir() {
            let mut had_children = false;
            for walk in WalkDir::new(src).min_depth(1).into_iter().flatten() {
                had_children = true;
                let rel = walk
                    .path()
                    .strip_prefix(src)
                    .unwrap_or(walk.path())
                    .to_string_lossy()
                    .replace('\\', "/");
                let zip_name = format!("{base_name}/{rel}");
                let is_dir = walk.file_type().is_dir();
                entries.push((walk.into_path(), zip_name, is_dir));
            }
            if !had_children {
                entries.push((src.clone(), format!("{base_name}/"), true));
            }
        } else {
            entries.push((src.clone(), base_name, false));
        }
    }

    if let Some(parent) = output_zip.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let file = File::create(output_zip)
        .map_err(|e| format!("ZIP konnte nicht angelegt werden: {e}"))?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);

    let total = entries.len().max(1);
    let mut buf = vec![0u8; ZIP_BUF_SIZE];

    for (idx, (path, zip_name, is_dir)) in entries.iter().enumerate() {
        if job.is_cancelled() {
            drop(zip);
            let _ = fs::remove_file(output_zip);
            return Err("Komprimierung abgebrochen.".to_string());
        }

        job.update(zip_name, idx, total);

        if *is_dir {
            let dir_name = if zip_name.ends_with('/') {
                zip_name.clone()
            } else {
                format!("{zip_name}/")
            };
            zip.add_directory(dir_name, options)
                .map_err(|e| format!("ZIP-Fehler: {e}"))?;
        } else {
            zip.start_file(zip_name, options)
                .map_err(|e| format!("ZIP-Fehler: {e}"))?;
            let mut f = File::open(path).map_err(|e| format!("Lesefehler ({zip_name}): {e}"))?;
            loop {
                if job.is_cancelled() {
                    drop(zip);
                    let _ = fs::remove_file(output_zip);
                    return Err("Komprimierung abgebrochen.".to_string());
                }
                let n = f.read(&mut buf).map_err(|e| e.to_string())?;
                if n == 0 {
                    break;
                }
                zip.write_all(&buf[..n]).map_err(|e| e.to_string())?;
            }
        }
    }

    zip.finish().map_err(|e| format!("ZIP-Abschluss fehlgeschlagen: {e}"))?;
    job.update(
        output_zip
            .file_name()
            .unwrap_or_default()
            .to_string_lossy(),
        total,
        total,
    );
    Ok(output_zip.to_path_buf())
}

pub fn extract_zip(
    zip_path: &Path,
    target_dir: &Path,
    job: &BgJobHandle,
) -> Result<PathBuf, String> {
    fs::create_dir_all(target_dir)
        .map_err(|e| format!("Zielordner konnte nicht erstellt werden: {e}"))?;

    let file = File::open(zip_path).map_err(|e| format!("ZIP konnte nicht geöffnet werden: {e}"))?;
    let mut archive =
        ZipArchive::new(file).map_err(|e| format!("Ungültiges ZIP-Archiv: {e}"))?;

    let total = archive.len().max(1);
    let mut buf = vec![0u8; ZIP_BUF_SIZE];

    for i in 0..archive.len() {
        if job.is_cancelled() {
            return Err("Entpacken abgebrochen.".to_string());
        }

        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        // Zip Slip protection via enclosed_name()
        let rel_path = match entry.enclosed_name() {
            Some(p) => p.to_path_buf(),
            None => continue,
        };

        let display_name = rel_path.to_string_lossy().into_owned();
        job.update(&display_name, i, total);

        let out_path = target_dir.join(&rel_path);
        if entry.is_dir() {
            fs::create_dir_all(&out_path).map_err(|e| e.to_string())?;
        } else {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut out_file = File::create(&out_path)
                .map_err(|e| format!("Schreibfehler ({display_name}): {e}"))?;
            loop {
                if job.is_cancelled() {
                    return Err("Entpacken abgebrochen.".to_string());
                }
                let n = entry.read(&mut buf).map_err(|e| e.to_string())?;
                if n == 0 {
                    break;
                }
                out_file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
            }
            out_file.flush().map_err(|e| e.to_string())?;
        }
    }

    job.update("Entpackt", total, total);
    Ok(target_dir.to_path_buf())
}

pub fn list_zip_entries(zip_path: &Path) -> Result<Vec<ZipEntryInfo>, String> {
    let file = File::open(zip_path).map_err(|e| format!("ZIP konnte nicht geöffnet werden: {e}"))?;
    let mut archive =
        ZipArchive::new(file).map_err(|e| format!("Ungültiges ZIP-Archiv: {e}"))?;

    let mut list = Vec::with_capacity(archive.len());
    for i in 0..archive.len() {
        let entry = archive.by_index(i).map_err(|e| e.to_string())?;
        list.push(ZipEntryInfo {
            name: entry.name().to_string(),
            is_dir: entry.is_dir(),
            size: entry.size(),
            compressed_size: entry.compressed_size(),
        });
    }
    list.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(list)
}

#[allow(dead_code)]
fn _copy_stream(mut r: impl Read, mut w: impl Write) -> io::Result<u64> {
    io::copy(&mut r, &mut w)
}
