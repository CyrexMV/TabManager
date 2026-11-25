use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FileCategory {
    Folder,
    Image,
    Video,
    Audio,
    Document,
    Archive,
    TextCode,
    Apk,
    Other,
}

impl FileCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::Folder => "Ordner",
            Self::Image => "Bilder",
            Self::Video => "Videos",
            Self::Audio => "Audio",
            Self::Document => "Dokumente",
            Self::Archive => "Archive",
            Self::TextCode => "Text & Code",
            Self::Apk => "APKs",
            Self::Other => "Sonstige",
        }
    }

    pub fn badge(self) -> &'static str {
        match self {
            Self::Folder => "DIR",
            Self::Image => "IMG",
            Self::Video => "VID",
            Self::Audio => "AUD",
            Self::Document => "DOC",
            Self::Archive => "ZIP",
            Self::TextCode => "TXT",
            Self::Apk => "APK",
            Self::Other => "FILE",
        }
    }
}

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified_secs: i64,
    pub extension: String,
    pub is_hidden: bool,
    pub child_count: Option<usize>,
    pub category: FileCategory,
    pub readonly: bool,
}

impl FileEntry {
    pub fn from_path(path: PathBuf) -> Option<Self> {
        let meta = std::fs::metadata(&path).ok()?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        let is_dir = meta.is_dir();
        let size = if is_dir { 0 } else { meta.len() };
        let modified_secs = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let extension = if is_dir {
            String::new()
        } else {
            path.extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default()
        };
        let is_hidden = name.starts_with('.');
        let child_count = if is_dir {
            std::fs::read_dir(&path).ok().map(|rd| rd.count())
        } else {
            None
        };
        let category = categorize_extension(&extension, is_dir);
        let readonly = meta.permissions().readonly();

        Some(Self {
            path,
            name,
            is_dir,
            size,
            modified_secs,
            extension,
            is_hidden,
            child_count,
            category,
            readonly,
        })
    }

    pub fn is_zip(&self) -> bool {
        !self.is_dir && self.extension == "zip"
    }

    pub fn is_image(&self) -> bool {
        !self.is_dir && self.category == FileCategory::Image
    }

    pub fn is_text_editable(&self) -> bool {
        !self.is_dir
            && (self.category == FileCategory::TextCode || self.extension == "csv")
    }
}

#[derive(Debug, Clone)]
pub struct StorageInfo {
    pub name: String,
    pub path: PathBuf,
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub is_removable: bool,
}

impl StorageInfo {
    pub fn used_bytes(&self) -> u64 {
        self.total_bytes.saturating_sub(self.free_bytes)
    }

    pub fn used_fraction(&self) -> f32 {
        if self.total_bytes == 0 {
            0.0
        } else {
            (self.used_bytes() as f64 / self.total_bytes as f64).clamp(0.0, 1.0) as f32
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortBy {
    Name,
    Date,
    Size,
    Type,
}

impl SortBy {
    pub fn label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Date => "Datum",
            Self::Size => "Größe",
            Self::Type => "Typ",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortOrder {
    Ascending,
    Descending,
}

impl SortOrder {
    pub fn toggled(self) -> Self {
        match self {
            Self::Ascending => Self::Descending,
            Self::Descending => Self::Ascending,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ViewMode {
    List,
    Grid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemeMode {
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardAction {
    Copy,
    Move,
}

#[derive(Debug, Clone, Default)]
pub struct ClipboardState {
    pub paths: Vec<PathBuf>,
    pub action: Option<ClipboardAction>,
}

impl ClipboardState {
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty() || self.action.is_none()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecialView {
    Home,
    Trash,
    Category(FileCategory),
}

impl SpecialView {
    pub fn title(self) -> &'static str {
        match self {
            Self::Home => "Übersicht",
            Self::Trash => "Papierkorb",
            Self::Category(c) => c.label(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Location {
    Dir(PathBuf),
    Special(SpecialView),
}

impl Location {
    pub fn title(&self) -> String {
        match self {
            Self::Special(s) => s.title().to_string(),
            Self::Dir(p) => {
                if p == Path::new("/storage/emulated/0") {
                    "Interner Speicher".to_string()
                } else {
                    p.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| p.to_string_lossy().into_owned())
                }
            }
        }
    }

    pub fn as_dir(&self) -> Option<&Path> {
        match self {
            Self::Dir(p) => Some(p.as_path()),
            Self::Special(_) => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TabState {
    pub id: u64,
    pub location: Location,
    pub history_back: Vec<Location>,
    pub history_forward: Vec<Location>,
}

#[derive(Debug, Clone)]
pub struct PaneState {
    pub id: usize,
    pub tabs: Vec<TabState>,
    pub active_tab: usize,
    pub entries: Vec<FileEntry>,
    pub selected: HashSet<PathBuf>,
    pub error: Option<String>,
    pub is_loading: bool,
}

impl PaneState {
    pub fn new(id: usize, initial: Location, first_tab_id: u64) -> Self {
        Self {
            id,
            tabs: vec![TabState {
                id: first_tab_id,
                location: initial,
                history_back: Vec::new(),
                history_forward: Vec::new(),
            }],
            active_tab: 0,
            entries: Vec::new(),
            selected: HashSet::new(),
            error: None,
            is_loading: false,
        }
    }

    pub fn current_tab(&self) -> &TabState {
        &self.tabs[self.active_tab.min(self.tabs.len().saturating_sub(1))]
    }

    pub fn current_tab_mut(&mut self) -> &mut TabState {
        let idx = self.active_tab.min(self.tabs.len().saturating_sub(1));
        &mut self.tabs[idx]
    }

    pub fn current_location(&self) -> &Location {
        &self.current_tab().location
    }

    pub fn current_dir(&self) -> Option<&Path> {
        self.current_location().as_dir()
    }

    pub fn selected_entries(&self) -> Vec<FileEntry> {
        self.entries
            .iter()
            .filter(|e| self.selected.contains(&e.path))
            .cloned()
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct ZipEntryInfo {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub compressed_size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrashItem {
    pub id: String,
    pub trashed_path: PathBuf,
    pub original_path: PathBuf,
    pub original_name: String,
    pub deleted_at_secs: i64,
    pub is_dir: bool,
    pub size: u64,
}

#[derive(Debug, Clone, Default)]
pub struct ProgressSnapshot {
    pub title: String,
    pub current_item: String,
    pub completed: usize,
    pub total: usize,
    pub done: bool,
    pub result_message: Option<String>,
    pub error_message: Option<String>,
}

impl ProgressSnapshot {
    pub fn fraction(&self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            (self.completed as f32 / self.total as f32).clamp(0.0, 1.0)
        }
    }
}

#[derive(Clone)]
pub struct BgJobHandle {
    pub state: Arc<Mutex<ProgressSnapshot>>,
    pub cancelled: Arc<AtomicBool>,
}

impl BgJobHandle {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            state: Arc::new(Mutex::new(ProgressSnapshot {
                title: title.into(),
                ..Default::default()
            })),
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn update(&self, item: impl Into<String>, completed: usize, total: usize) {
        if let Ok(mut s) = self.state.lock() {
            s.current_item = item.into();
            s.completed = completed;
            s.total = total;
        }
    }

    pub fn finish_ok(&self, msg: impl Into<String>) {
        if let Ok(mut s) = self.state.lock() {
            s.done = true;
            s.result_message = Some(msg.into());
        }
    }

    pub fn finish_err(&self, err: impl Into<String>) {
        if let Ok(mut s) = self.state.lock() {
            s.done = true;
            s.error_message = Some(err.into());
        }
    }

    pub fn snapshot(&self) -> ProgressSnapshot {
        self.state.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

pub fn categorize_extension(ext: &str, is_dir: bool) -> FileCategory {
    if is_dir {
        return FileCategory::Folder;
    }
    match ext {
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "heic" | "heif" | "svg" | "ico"
        | "tiff" | "raw" | "dng" => FileCategory::Image,
        "mp4" | "mkv" | "avi" | "mov" | "wmv" | "flv" | "webm" | "m4v" | "3gp" | "ts" => {
            FileCategory::Video
        }
        "mp3" | "wav" | "flac" | "aac" | "ogg" | "m4a" | "wma" | "opus" | "mid" => {
            FileCategory::Audio
        }
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "odt" | "ods" | "odp"
        | "rtf" | "epub" | "csv" => FileCategory::Document,
        "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" | "tgz" | "jar" => {
            FileCategory::Archive
        }
        "txt" | "md" | "log" | "json" | "xml" | "yaml" | "yml" | "toml" | "ini" | "cfg"
        | "conf" | "properties" | "rs" | "kt" | "kts" | "java" | "py" | "js" | "jsx" | "tsx"
        | "html" | "htm" | "css" | "scss" | "c" | "cpp" | "h" | "hpp" | "cs" | "go" | "rb"
        | "php" | "sh" | "bash" | "sql" | "gradle" | "env" => FileCategory::TextCode,
        "apk" | "apks" | "xapk" | "aab" => FileCategory::Apk,
        _ => FileCategory::Other,
    }
}

pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut val = bytes as f64;
    let mut unit = 0;
    while val >= 1024.0 && unit < UNITS.len() - 1 {
        val /= 1024.0;
        unit += 1;
    }
    format!("{val:.1} {}", UNITS[unit])
}

pub fn format_timestamp(secs: i64) -> String {
    if secs <= 0 {
        return "—".to_string();
    }
    let st = UNIX_EPOCH + std::time::Duration::from_secs(secs as u64);
    let dt: DateTime<Local> = st.into();
    dt.format("%d.%m.%Y %H:%M").to_string()
}

pub fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
