use crate::fs_ops::{compute_folder_stats, resolve_unique_path};
use crate::model::{now_secs, TrashItem};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TRASH_SEQ: AtomicU64 = AtomicU64::new(1);

fn trash_root(base_storage: &Path) -> PathBuf {
    base_storage.join(".TabManagerTrash")
}

fn trash_files_dir(base_storage: &Path) -> PathBuf {
    trash_root(base_storage).join("files")
}

fn trash_meta_dir(base_storage: &Path) -> PathBuf {
    trash_root(base_storage).join("meta")
}

pub fn move_to_trash(base_storage: &Path, paths: &[PathBuf]) -> Result<usize, String> {
    let files_dir = trash_files_dir(base_storage);
    let meta_dir = trash_meta_dir(base_storage);
    fs::create_dir_all(&files_dir).map_err(|e| e.to_string())?;
    fs::create_dir_all(&meta_dir).map_err(|e| e.to_string())?;

    let mut count = 0usize;
    for path in paths {
        if !path.exists() {
            continue;
        }
        let seq = TRASH_SEQ.fetch_add(1, Ordering::Relaxed);
        let id = format!("{}_{}", now_secs(), seq);
        let dst = files_dir.join(&id);
        let is_dir = path.is_dir();
        let size = if is_dir {
            compute_folder_stats(path).0
        } else {
            fs::metadata(path).map(|m| m.len()).unwrap_or(0)
        };
        let original_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| id.clone());

        if fs::rename(path, &dst).is_err() {
            // Cross-device fallback
            let job = crate::model::BgJobHandle::new("Papierkorb");
            let single = vec![path.clone()];
            // Copy into files_dir, then rename to id
            crate::fs_ops::copy_or_move_files(&single, &files_dir, true, &job)?;
            let copied = files_dir.join(&original_name);
            if copied.exists() && copied != dst {
                let _ = fs::rename(copied, &dst);
            }
        }

        let item = TrashItem {
            id: id.clone(),
            trashed_path: dst,
            original_path: path.clone(),
            original_name,
            deleted_at_secs: now_secs(),
            is_dir,
            size,
        };
        let json = serde_json::to_string_pretty(&item).map_err(|e| e.to_string())?;
        fs::write(meta_dir.join(format!("{id}.json")), json).map_err(|e| e.to_string())?;
        count += 1;
    }

    Ok(count)
}

pub fn list_trash(base_storage: &Path) -> Vec<TrashItem> {
    let meta_dir = trash_meta_dir(base_storage);
    let Ok(rd) = fs::read_dir(&meta_dir) else {
        return Vec::new();
    };
    let mut items = Vec::new();
    for entry in rd.flatten() {
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if let Ok(text) = fs::read_to_string(&p) {
            if let Ok(item) = serde_json::from_str::<TrashItem>(&text) {
                if item.trashed_path.exists() {
                    items.push(item);
                } else {
                    let _ = fs::remove_file(&p);
                }
            }
        }
    }
    items.sort_by(|a, b| b.deleted_at_secs.cmp(&a.deleted_at_secs));
    items
}

pub fn restore_trash_item(base_storage: &Path, item: &TrashItem) -> Result<PathBuf, String> {
    let parent = item
        .original_path
        .parent()
        .unwrap_or(base_storage);
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let target = resolve_unique_path(parent, &item.original_name);

    if fs::rename(&item.trashed_path, &target).is_err() {
        let job = crate::model::BgJobHandle::new("Wiederherstellen");
        crate::fs_ops::copy_or_move_files(
            std::slice::from_ref(&item.trashed_path),
            parent,
            true,
            &job,
        )?;
        let moved = parent.join(
            item.trashed_path
                .file_name()
                .unwrap_or_default(),
        );
        if moved.exists() && moved != target {
            let _ = fs::rename(moved, &target);
        }
    }
    let _ = fs::remove_file(trash_meta_dir(base_storage).join(format!("{}.json", item.id)));
    Ok(target)
}

pub fn delete_trash_item(base_storage: &Path, item: &TrashItem) -> Result<(), String> {
    if item.trashed_path.is_dir() {
        fs::remove_dir_all(&item.trashed_path).map_err(|e| e.to_string())?;
    } else if item.trashed_path.exists() {
        fs::remove_file(&item.trashed_path).map_err(|e| e.to_string())?;
    }
    let _ = fs::remove_file(trash_meta_dir(base_storage).join(format!("{}.json", item.id)));
    Ok(())
}

pub fn empty_trash(base_storage: &Path) -> Result<(), String> {
    let files_dir = trash_files_dir(base_storage);
    let meta_dir = trash_meta_dir(base_storage);
    if files_dir.exists() {
        fs::remove_dir_all(&files_dir).map_err(|e| e.to_string())?;
    }
    if meta_dir.exists() {
        fs::remove_dir_all(&meta_dir).map_err(|e| e.to_string())?;
    }
    Ok(())
}
