use crate::model::{
    categorize_extension, BgJobHandle, FileCategory, FileEntry, SortBy, SortOrder, StorageInfo,
};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const COPY_BUF_SIZE: usize = 128 * 1024;

pub fn list_dir(
    dir: &Path,
    show_hidden: bool,
    sort_by: SortBy,
    sort_order: SortOrder,
    folders_first: bool,
) -> Result<Vec<FileEntry>, String> {
    let rd = fs::read_dir(dir)
        .map_err(|e| format!("Ordner konnte nicht gelesen werden ({}): {e}", dir.display()))?;

    let mut items = Vec::new();
    for entry in rd.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == ".TabManagerTrash" {
            continue;
        }
        if !show_hidden && name.starts_with('.') {
            continue;
        }
        if let Some(item) = FileEntry::from_path(path) {
            items.push(item);
        }
    }

    sort_entries(&mut items, sort_by, sort_order, folders_first);
    Ok(items)
}

pub fn sort_entries(
    entries: &mut [FileEntry],
    sort_by: SortBy,
    sort_order: SortOrder,
    folders_first: bool,
) {
    entries.sort_by(|a, b| {
        if folders_first && a.is_dir != b.is_dir {
            return b.is_dir.cmp(&a.is_dir);
        }
        let ord = match sort_by {
            SortBy::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SortBy::Date => a.modified_secs.cmp(&b.modified_secs),
            SortBy::Size => a.size.cmp(&b.size),
            SortBy::Type => a
                .extension
                .cmp(&b.extension)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
        };
        match sort_order {
            SortOrder::Ascending => ord,
            SortOrder::Descending => ord.reverse(),
        }
    });
}

#[cfg(unix)]
pub fn get_storage_info(name: String, path: PathBuf, is_removable: bool) -> StorageInfo {
    use std::ffi::CString;
    let mut total_bytes = 0u64;
    let mut free_bytes = 0u64;

    if let Ok(c_path) = CString::new(path.to_string_lossy().as_bytes()) {
        unsafe {
            let mut stat: libc::statvfs = std::mem::zeroed();
            if libc::statvfs(c_path.as_ptr(), &mut stat) == 0 {
                let frag = stat.f_frsize as u64;
                total_bytes = (stat.f_blocks as u64).saturating_mul(frag);
                free_bytes = (stat.f_bavail as u64).saturating_mul(frag);
            }
        }
    }

    StorageInfo {
        name,
        path,
        total_bytes,
        free_bytes,
        is_removable,
    }
}

#[cfg(not(unix))]
pub fn get_storage_info(name: String, path: PathBuf, is_removable: bool) -> StorageInfo {
    StorageInfo {
        name,
        path,
        total_bytes: 0,
        free_bytes: 0,
        is_removable,
    }
}

pub fn resolve_unique_path(parent: &Path, desired_name: &str) -> PathBuf {
    let mut candidate = parent.join(desired_name);
    if !candidate.exists() {
        return candidate;
    }

    let (base, ext) = match desired_name.rfind('.') {
        Some(idx) if idx > 0 && idx < desired_name.len() - 1 => {
            (&desired_name[..idx], &desired_name[idx..])
        }
        _ => (desired_name, ""),
    };

    let mut counter = 1u32;
    while candidate.exists() {
        candidate = parent.join(format!("{base} ({counter}){ext}"));
        counter += 1;
    }
    candidate
}

pub fn create_folder(parent: &Path, name: &str) -> Result<PathBuf, String> {
    let clean = name.trim();
    if clean.is_empty() || clean.contains('/') || clean.contains('\\') {
        return Err("Ungültiger Ordnername.".to_string());
    }
    let target = parent.join(clean);
    if target.exists() {
        return Err(format!("'{clean}' existiert bereits."));
    }
    fs::create_dir_all(&target).map_err(|e| format!("Ordner konnte nicht erstellt werden: {e}"))?;
    Ok(target)
}

pub fn create_file(parent: &Path, name: &str) -> Result<PathBuf, String> {
    let clean = name.trim();
    if clean.is_empty() || clean.contains('/') || clean.contains('\\') {
        return Err("Ungültiger Dateiname.".to_string());
    }
    let target = parent.join(clean);
    if target.exists() {
        return Err(format!("'{clean}' existiert bereits."));
    }
    File::create(&target).map_err(|e| format!("Datei konnte nicht erstellt werden: {e}"))?;
    Ok(target)
}

pub fn rename_entry(path: &Path, new_name: &str) -> Result<PathBuf, String> {
    let clean = new_name.trim();
    if clean.is_empty() || clean.contains('/') || clean.contains('\\') {
        return Err("Ungültiger Name.".to_string());
    }
    let parent = path
        .parent()
        .ok_or_else(|| "Kein Elternordner gefunden.".to_string())?;
    let target = parent.join(clean);
    if target.exists() {
        return Err(format!("'{clean}' existiert bereits."));
    }
    fs::rename(path, &target).map_err(|e| format!("Umbenennen fehlgeschlagen: {e}"))?;
    Ok(target)
}

pub fn delete_permanently(paths: &[PathBuf], job: &BgJobHandle) -> Result<usize, String> {
    let total = paths.len().max(1);
    let mut deleted = 0usize;
    for (idx, path) in paths.iter().enumerate() {
        if job.is_cancelled() {
            return Err("Abgebrochen.".to_string());
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        job.update(name, idx, total);
        if path.is_dir() {
            fs::remove_dir_all(path).map_err(|e| format!("Löschen fehlgeschlagen: {e}"))?;
        } else if path.exists() {
            fs::remove_file(path).map_err(|e| format!("Löschen fehlgeschlagen: {e}"))?;
        }
        deleted += 1;
    }
    job.update("Fertig", total, total);
    Ok(deleted)
}

pub fn copy_or_move_files(
    sources: &[PathBuf],
    dest_dir: &Path,
    is_move: bool,
    job: &BgJobHandle,
) -> Result<usize, String> {
    if !dest_dir.exists() {
        fs::create_dir_all(dest_dir)
            .map_err(|e| format!("Zielordner konnte nicht erstellt werden: {e}"))?;
    }

    let dest_canon = fs::canonicalize(dest_dir).unwrap_or_else(|_| dest_dir.to_path_buf());

    // Count total items for smooth progress
    let mut total_items = 0usize;
    for src in sources {
        if src.is_dir() {
            total_items += WalkDir::new(src).into_iter().filter_map(|e| e.ok()).count();
        } else {
            total_items += 1;
        }
    }
    let total_items = total_items.max(1);
    let mut completed = 0usize;

    for src in sources {
        if job.is_cancelled() {
            return Err("Vorgang abgebrochen.".to_string());
        }
        if !src.exists() {
            continue;
        }
        let src_canon = fs::canonicalize(src).unwrap_or_else(|_| src.clone());
        if src.is_dir() && (dest_canon == src_canon || dest_canon.starts_with(&src_canon)) {
            return Err(format!(
                "Ordner '{}' kann nicht in sich selbst kopiert/verschoben werden.",
                src.file_name().unwrap_or_default().to_string_lossy()
            ));
        }
        if is_move && src.parent().and_then(|p| fs::canonicalize(p).ok()).as_ref() == Some(&dest_canon) {
            completed += 1;
            continue;
        }

        let name = src
            .file_name()
            .ok_or_else(|| "Ungültiger Dateiname.".to_string())?
            .to_string_lossy()
            .into_owned();
        let target = resolve_unique_path(dest_dir, &name);

        if is_move && fs::rename(src, &target).is_ok() {
            completed += 1;
            job.update(&name, completed.min(total_items), total_items);
            continue;
        }

        copy_recursive(src, &target, &mut completed, total_items, job)?;

        if is_move {
            if src.is_dir() {
                fs::remove_dir_all(src)
                    .map_err(|e| format!("Quelle konnte nicht entfernt werden: {e}"))?;
            } else {
                fs::remove_file(src)
                    .map_err(|e| format!("Quelle konnte nicht entfernt werden: {e}"))?;
            }
        }
    }

    Ok(sources.len())
}

fn copy_recursive(
    src: &Path,
    dst: &Path,
    completed: &mut usize,
    total: usize,
    job: &BgJobHandle,
) -> Result<(), String> {
    if job.is_cancelled() {
        return Err("Vorgang abgebrochen.".to_string());
    }
    if src.is_dir() {
        fs::create_dir_all(dst).map_err(|e| format!("Ordner anlegen fehlgeschlagen: {e}"))?;
        *completed += 1;
        job.update(
            src.file_name().unwrap_or_default().to_string_lossy(),
            (*completed).min(total),
            total,
        );
        for entry in fs::read_dir(src).map_err(|e| e.to_string())?.flatten() {
            let child_src = entry.path();
            let child_dst = dst.join(entry.file_name());
            copy_recursive(&child_src, &child_dst, completed, total, job)?;
        }
    } else {
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        stream_copy_file(src, dst, job).map_err(|e| format!("Kopieren fehlgeschlagen: {e}"))?;
        *completed += 1;
        job.update(
            src.file_name().unwrap_or_default().to_string_lossy(),
            (*completed).min(total),
            total,
        );
    }
    Ok(())
}

fn stream_copy_file(src: &Path, dst: &Path, job: &BgJobHandle) -> io::Result<()> {
    let mut reader = File::open(src)?;
    let mut writer = File::create(dst)?;
    let mut buf = vec![0u8; COPY_BUF_SIZE];
    loop {
        if job.is_cancelled() {
            let _ = fs::remove_file(dst);
            return Err(io::Error::new(io::ErrorKind::Interrupted, "Abgebrochen"));
        }
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        writer.write_all(&buf[..n])?;
    }
    writer.flush()?;
    Ok(())
}

pub fn search_recursive(
    root: &Path,
    query: &str,
    show_hidden: bool,
    max_results: usize,
) -> Vec<FileEntry> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let mut results = Vec::new();
    for entry in WalkDir::new(root)
        .min_depth(1)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            if name == ".TabManagerTrash" {
                return false;
            }
            show_hidden || !name.starts_with('.')
        })
        .flatten()
    {
        let name = entry.file_name().to_string_lossy();
        if name.to_lowercase().contains(&q) {
            if let Some(item) = FileEntry::from_path(entry.into_path()) {
                results.push(item);
                if results.len() >= max_results {
                    break;
                }
            }
        }
    }
    results
}

pub fn scan_category(
    root: &Path,
    category: FileCategory,
    sort_by: SortBy,
    sort_order: SortOrder,
    max_results: usize,
) -> Vec<FileEntry> {
    let mut results = Vec::new();
    for entry in WalkDir::new(root)
        .min_depth(1)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            !name.starts_with('.') && name != "Android"
        })
        .flatten()
    {
        if entry.file_type().is_file() {
            let ext = entry
                .path()
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            if categorize_extension(&ext, false) == category {
                if let Some(item) = FileEntry::from_path(entry.into_path()) {
                    results.push(item);
                    if results.len() >= max_results {
                        break;
                    }
                }
            }
        }
    }
    sort_entries(&mut results, sort_by, sort_order, false);
    results
}

pub fn compute_sha256(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let digest = hasher.finalize();
    Ok(digest.iter().map(|b| format!("{b:02x}")).collect())
}

pub fn compute_folder_stats(path: &Path) -> (u64, usize) {
    let mut total_size = 0u64;
    let mut file_count = 0usize;
    for entry in WalkDir::new(path).into_iter().flatten() {
        if entry.file_type().is_file() {
            if let Ok(meta) = entry.metadata() {
                total_size = total_size.saturating_add(meta.len());
                file_count += 1;
            }
        }
    }
    (total_size, file_count)
}

pub fn read_text_preview(path: &Path, max_bytes: usize) -> Result<(String, bool), String> {
    let mut file = File::open(path).map_err(|e| format!("Lesefehler: {e}"))?;
    let len = file.metadata().map(|m| m.len() as usize).unwrap_or(0);
    let truncated = len > max_bytes;
    let to_read = len.min(max_bytes);
    let mut buf = vec![0u8; to_read];
    file.read_exact(&mut buf)
        .map_err(|e| format!("Lesefehler: {e}"))?;
    Ok((String::from_utf8_lossy(&buf).into_owned(), truncated))
}

pub fn write_text_file(path: &Path, content: &str) -> Result<(), String> {
    fs::write(path, content).map_err(|e| format!("Speichern fehlgeschlagen: {e}"))
}
