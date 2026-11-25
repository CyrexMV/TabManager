use std::fs;
use std::path::PathBuf;
use tabmanager::fs_ops;
use tabmanager::model::{BgJobHandle, SortBy, SortOrder};
use tabmanager::trash;
use tabmanager::zip_ops;

fn temp_test_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tabmanager_test_{name}_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn test_file_ops_and_zip_roundtrip() {
    let root = temp_test_dir("zip_roundtrip");
    let src_dir = fs_ops::create_folder(&root, "Projekt").unwrap();
    let file_a = fs_ops::create_file(&src_dir, "notiz.txt").unwrap();
    fs_ops::write_text_file(&file_a, "Hallo Galaxy Tab S12 Ultra!").unwrap();

    // Rename
    let renamed = fs_ops::rename_entry(&file_a, "readme.txt").unwrap();
    assert!(renamed.exists());

    // Compress to ZIP
    let zip_out = root.join("projekt.zip");
    let job = BgJobHandle::new("zip");
    zip_ops::compress_to_zip(&[src_dir.clone()], &zip_out, &job).unwrap();
    assert!(zip_out.exists());

    // List ZIP entries
    let entries = zip_ops::list_zip_entries(&zip_out).unwrap();
    assert!(entries.iter().any(|e| e.name.ends_with("readme.txt")));

    // Extract ZIP
    let extract_dir = root.join("extracted");
    let job2 = BgJobHandle::new("unzip");
    zip_ops::extract_zip(&zip_out, &extract_dir, &job2).unwrap();
    let extracted_file = extract_dir.join("Projekt").join("readme.txt");
    assert_eq!(
        fs::read_to_string(&extracted_file).unwrap(),
        "Hallo Galaxy Tab S12 Ultra!"
    );

    // Copy & Move
    let dest_dir = fs_ops::create_folder(&root, "Ziel").unwrap();
    let job3 = BgJobHandle::new("copy");
    fs_ops::copy_or_move_files(&[extracted_file.clone()], &dest_dir, false, &job3).unwrap();
    assert!(dest_dir.join("readme.txt").exists());

    // List directory
    let listed = fs_ops::list_dir(&dest_dir, false, SortBy::Name, SortOrder::Ascending, true).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, "readme.txt");

    // Trash & Restore
    let moved = trash::move_to_trash(&root, &[dest_dir.join("readme.txt")]).unwrap();
    assert_eq!(moved, 1);
    assert!(!dest_dir.join("readme.txt").exists());
    let items = trash::list_trash(&root);
    assert_eq!(items.len(), 1);
    trash::restore_trash_item(&root, &items[0]).unwrap();
    assert!(dest_dir.join("readme.txt").exists());

    let _ = fs::remove_dir_all(&root);
}
