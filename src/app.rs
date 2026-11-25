use crate::android_bridge::{ImeEvent, PlatformBridge};
use crate::fs_ops;
use crate::icons::{category_icon_badge, icon_button, icon_widget, paint_icon, pill_button, Icon};
use crate::model::{
    format_size, format_timestamp, BgJobHandle, ClipboardAction, ClipboardState, FileCategory,
    FileEntry, Location, PaneState, SortBy, SortOrder, SpecialView, StorageInfo, TabState,
    ThemeMode, TrashItem, ViewMode, ZipEntryInfo,
};
use crate::prefs::AppPrefs;
use crate::theme::{apply_theme, Palette};
use crate::trash;
use crate::zip_ops;
use egui::{
    vec2, Align, Color32, CornerRadius, Frame, Layout, Margin, Rect, RichText, ScrollArea, Sense,
    Shadow, Stroke, TextureHandle, Vec2,
};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

enum DialogState {
    NewFolder {
        pane: usize,
        name: String,
    },
    NewFile {
        pane: usize,
        name: String,
    },
    Rename {
        target: PathBuf,
        new_name: String,
    },
    CompressZip {
        pane: usize,
        sources: Vec<PathBuf>,
        archive_name: String,
    },
    ExtractZip {
        pane: usize,
        zip_path: PathBuf,
        subfolder_name: String,
        extract_into_subfolder: bool,
    },
    ConfirmDelete {
        pane: usize,
        targets: Vec<PathBuf>,
        permanent: bool,
    },
    ConfirmEmptyTrash,
    ContextSheet {
        pane: usize,
        entry: FileEntry,
    },
    PickFolder {
        pane: usize,
        sources: Vec<PathBuf>,
        is_move: bool,
        browse_dir: PathBuf,
    },
}

enum ViewerState {
    Text {
        path: PathBuf,
        content: String,
        truncated: bool,
        dirty: bool,
    },
    Image {
        path: PathBuf,
        texture: Option<TextureHandle>,
        dimensions: Option<(usize, usize)>,
        error: Option<String>,
        zoom: f32,
    },
    Zip {
        path: PathBuf,
        entries: Vec<ZipEntryInfo>,
        error: Option<String>,
    },
}

struct Toast {
    message: String,
    is_error: bool,
    expires_at: Instant,
}

struct SearchResult {
    pane: usize,
    entries: Vec<FileEntry>,
}

struct InspectorExtra {
    path: PathBuf,
    sha256: Option<String>,
    folder_stats: Option<(u64, usize)>,
}

struct DecodedThumb {
    path: PathBuf,
    image: egui::ColorImage,
}

struct LongPressTracker {
    entry: FileEntry,
    started_at: Instant,
    start_pos: egui::Pos2,
    fired: bool,
}

pub struct TabManagerApp {
    bridge: PlatformBridge,
    config_dir: PathBuf,
    prefs: AppPrefs,
    storage_volumes: Vec<StorageInfo>,
    primary_root: PathBuf,
    has_storage_access: bool,
    last_perm_check: Instant,

    panes: [PaneState; 2],
    active_pane: usize,
    next_tab_id: u64,

    trash_items: Vec<TrashItem>,
    clipboard: ClipboardState,

    search_query: String,
    search_recursive_mode: bool,
    search_rx: Option<Receiver<SearchResult>>,
    is_searching: bool,

    bg_job: Option<BgJobHandle>,
    dialog: Option<DialogState>,
    viewer: Option<ViewerState>,
    toast: Option<Toast>,

    inspector_extra: Option<InspectorExtra>,
    inspector_rx: Option<Receiver<InspectorExtra>>,

    // Async image thumbnail cache
    thumbs: HashMap<PathBuf, TextureHandle>,
    thumbs_requested: HashSet<PathBuf>,
    thumb_tx: Sender<DecodedThumb>,
    thumb_rx: Receiver<DecodedThumb>,

    long_press: Option<LongPressTracker>,
    sidebar_open: bool,
    keyboard_was_open: bool,
}

impl TabManagerApp {
    pub fn new(bridge: PlatformBridge, config_dir: PathBuf) -> Self {
        let prefs = AppPrefs::load(&config_dir);
        let primary_root = bridge.primary_storage_root();
        let has_storage_access = bridge.has_all_files_access();

        let roots = bridge.get_storage_roots();
        let storage_volumes: Vec<StorageInfo> = roots
            .into_iter()
            .map(|(name, path, rem)| fs_ops::get_storage_info(name, path, rem))
            .collect();

        let pane0 = PaneState::new(0, Location::Special(SpecialView::Home), 1);
        let pane1 = PaneState::new(1, Location::Dir(primary_root.clone()), 2);
        let (thumb_tx, thumb_rx) = mpsc::channel();

        let mut app = Self {
            bridge,
            config_dir,
            prefs,
            storage_volumes,
            primary_root,
            has_storage_access,
            last_perm_check: Instant::now(),
            panes: [pane0, pane1],
            active_pane: 0,
            next_tab_id: 3,
            trash_items: Vec::new(),
            clipboard: ClipboardState::default(),
            search_query: String::new(),
            search_recursive_mode: false,
            search_rx: None,
            is_searching: false,
            bg_job: None,
            dialog: None,
            viewer: None,
            toast: None,
            inspector_extra: None,
            inspector_rx: None,
            thumbs: HashMap::new(),
            thumbs_requested: HashSet::new(),
            thumb_tx,
            thumb_rx,
            long_press: None,
            sidebar_open: true,
            keyboard_was_open: false,
        };

        app.refresh_pane(0);
        app.refresh_pane(1);
        app
    }

    fn save_prefs(&self) {
        self.prefs.save(&self.config_dir);
    }

    fn show_toast(&mut self, msg: impl Into<String>, is_error: bool) {
        self.toast = Some(Toast {
            message: msg.into(),
            is_error,
            expires_at: Instant::now() + Duration::from_secs(4),
        });
    }

    fn request_thumbnail(&mut self, entry: &FileEntry) {
        if !entry.is_image() || entry.size > 20 * 1024 * 1024 {
            return;
        }
        if self.thumbs.contains_key(&entry.path) || self.thumbs_requested.contains(&entry.path) {
            return;
        }
        if self.thumbs_requested.len() > 200 {
            return;
        }
        self.thumbs_requested.insert(entry.path.clone());
        let path = entry.path.clone();
        let tx = self.thumb_tx.clone();
        std::thread::spawn(move || {
            if let Ok(img) = image::open(&path) {
                let thumb = img.thumbnail(112, 112).to_rgba8();
                let size = [thumb.width() as usize, thumb.height() as usize];
                let color_img = egui::ColorImage::from_rgba_unmultiplied(size, thumb.as_raw());
                let _ = tx.send(DecodedThumb {
                    path,
                    image: color_img,
                });
            }
        });
    }

    fn refresh_volumes(&mut self) {
        let roots = self.bridge.get_storage_roots();
        self.storage_volumes = roots
            .into_iter()
            .map(|(name, path, rem)| fs_ops::get_storage_info(name, path, rem))
            .collect();
    }

    fn refresh_all_panes(&mut self) {
        self.refresh_volumes();
        self.refresh_pane(0);
        self.refresh_pane(1);
    }

    fn refresh_pane(&mut self, pane_idx: usize) {
        let loc = self.panes[pane_idx].current_location().clone();
        self.panes[pane_idx].error = None;
        self.panes[pane_idx].selected.clear();

        match loc {
            Location::Dir(dir) => {
                match fs_ops::list_dir(
                    &dir,
                    self.prefs.show_hidden,
                    self.prefs.sort_by,
                    self.prefs.sort_order,
                    self.prefs.folders_first,
                ) {
                    Ok(entries) => {
                        self.panes[pane_idx].entries = entries;
                    }
                    Err(e) => {
                        self.panes[pane_idx].entries.clear();
                        self.panes[pane_idx].error = Some(e);
                    }
                }
            }
            Location::Special(SpecialView::Home) => {
                self.panes[pane_idx].entries.clear();
            }
            Location::Special(SpecialView::Trash) => {
                self.trash_items = trash::list_trash(&self.primary_root);
                self.panes[pane_idx].entries.clear();
            }
            Location::Special(SpecialView::Category(cat)) => {
                let root = self.primary_root.clone();
                let sort_by = self.prefs.sort_by;
                let sort_order = self.prefs.sort_order;
                self.panes[pane_idx].is_loading = true;
                let (tx, rx) = mpsc::channel();
                self.search_rx = Some(rx);
                self.is_searching = true;
                std::thread::spawn(move || {
                    let entries = fs_ops::scan_category(&root, cat, sort_by, sort_order, 500);
                    let _ = tx.send(SearchResult {
                        pane: pane_idx,
                        entries,
                    });
                });
            }
        }
    }

    fn navigate(&mut self, pane_idx: usize, new_loc: Location) {
        self.active_pane = pane_idx;
        let prev = self.panes[pane_idx].current_location().clone();
        let tab = self.panes[pane_idx].current_tab_mut();
        tab.history_back.push(prev);
        tab.history_forward.clear();
        tab.location = new_loc;
        self.search_query.clear();
        self.search_recursive_mode = false;
        self.refresh_pane(pane_idx);
    }

    fn go_back(&mut self, pane_idx: usize) {
        let tab = self.panes[pane_idx].current_tab_mut();
        if let Some(prev) = tab.history_back.pop() {
            let cur = std::mem::replace(&mut tab.location, prev);
            tab.history_forward.push(cur);
            self.refresh_pane(pane_idx);
        }
    }

    fn go_forward(&mut self, pane_idx: usize) {
        let tab = self.panes[pane_idx].current_tab_mut();
        if let Some(next) = tab.history_forward.pop() {
            let cur = std::mem::replace(&mut tab.location, next);
            tab.history_back.push(cur);
            self.refresh_pane(pane_idx);
        }
    }

    fn go_up(&mut self, pane_idx: usize) {
        if let Some(dir) = self.panes[pane_idx].current_dir().map(Path::to_path_buf) {
            if let Some(parent) = dir.parent() {
                if !parent.as_os_str().is_empty() {
                    self.navigate(pane_idx, Location::Dir(parent.to_path_buf()));
                }
            }
        }
    }

    fn add_tab(&mut self, pane_idx: usize, loc: Location) {
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        self.panes[pane_idx].tabs.push(TabState {
            id,
            location: loc,
            history_back: Vec::new(),
            history_forward: Vec::new(),
        });
        self.panes[pane_idx].active_tab = self.panes[pane_idx].tabs.len() - 1;
        self.active_pane = pane_idx;
        self.refresh_pane(pane_idx);
    }

    fn close_tab(&mut self, pane_idx: usize, tab_idx: usize) {
        if self.panes[pane_idx].tabs.len() <= 1 {
            return;
        }
        self.panes[pane_idx].tabs.remove(tab_idx);
        if self.panes[pane_idx].active_tab >= self.panes[pane_idx].tabs.len() {
            self.panes[pane_idx].active_tab = self.panes[pane_idx].tabs.len() - 1;
        }
        self.refresh_pane(pane_idx);
    }

    fn toggle_favorite(&mut self, path: PathBuf) {
        if let Some(pos) = self.prefs.favorites.iter().position(|p| p == &path) {
            self.prefs.favorites.remove(pos);
            self.show_toast("Aus Favoriten entfernt", false);
        } else {
            self.prefs.favorites.push(path);
            self.show_toast("Zu Favoriten hinzugefügt", false);
        }
        self.save_prefs();
    }

    fn set_sort(&mut self, by: SortBy) {
        if self.prefs.sort_by == by {
            self.prefs.sort_order = self.prefs.sort_order.toggled();
        } else {
            self.prefs.sort_by = by;
            self.prefs.sort_order = SortOrder::Ascending;
        }
        self.save_prefs();
        self.refresh_all_panes();
    }

    fn open_entry(&mut self, pane_idx: usize, entry: &FileEntry, ctx: &egui::Context) {
        if entry.is_dir {
            self.navigate(pane_idx, Location::Dir(entry.path.clone()));
            return;
        }
        if entry.is_zip() {
            match zip_ops::list_zip_entries(&entry.path) {
                Ok(entries) => {
                    self.viewer = Some(ViewerState::Zip {
                        path: entry.path.clone(),
                        entries,
                        error: None,
                    });
                }
                Err(e) => {
                    self.viewer = Some(ViewerState::Zip {
                        path: entry.path.clone(),
                        entries: Vec::new(),
                        error: Some(e),
                    });
                }
            }
            return;
        }
        if entry.is_image() {
            self.open_image_viewer(&entry.path, ctx);
            return;
        }
        if entry.is_text_editable() {
            match fs_ops::read_text_preview(&entry.path, 512 * 1024) {
                Ok((content, truncated)) => {
                    self.viewer = Some(ViewerState::Text {
                        path: entry.path.clone(),
                        content,
                        truncated,
                        dirty: false,
                    });
                }
                Err(e) => self.show_toast(e, true),
            }
            return;
        }
        if let Err(e) = self.bridge.open_file(&entry.path) {
            self.show_toast(e, true);
        }
    }

    fn open_image_viewer(&mut self, path: &Path, ctx: &egui::Context) {
        match image::open(path) {
            Ok(img) => {
                let rgba = img.to_rgba8();
                let size = [rgba.width() as usize, rgba.height() as usize];
                let color_img = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
                let tex = ctx.load_texture(
                    format!("img_{}", path.display()),
                    color_img,
                    egui::TextureOptions::LINEAR,
                );
                self.viewer = Some(ViewerState::Image {
                    path: path.to_path_buf(),
                    texture: Some(tex),
                    dimensions: Some((size[0], size[1])),
                    error: None,
                    zoom: 1.0,
                });
            }
            Err(e) => {
                self.viewer = Some(ViewerState::Image {
                    path: path.to_path_buf(),
                    texture: None,
                    dimensions: None,
                    error: Some(format!("Bild konnte nicht geladen werden: {e}")),
                    zoom: 1.0,
                });
            }
        }
    }

    fn start_copy_or_move(&mut self, sources: Vec<PathBuf>, dest_dir: PathBuf, is_move: bool) {
        if sources.is_empty() {
            return;
        }
        let title = if is_move {
            "Verschiebe Dateien…"
        } else {
            "Kopiere Dateien…"
        };
        let handle = BgJobHandle::new(title);
        let worker_handle = handle.clone();
        self.bg_job = Some(handle);
        if is_move {
            self.clipboard = ClipboardState::default();
        }
        std::thread::spawn(move || {
            match fs_ops::copy_or_move_files(&sources, &dest_dir, is_move, &worker_handle) {
                Ok(n) => {
                    let verb = if is_move { "verschoben" } else { "kopiert" };
                    worker_handle.finish_ok(format!("{n} Element(e) erfolgreich {verb}."));
                }
                Err(e) => worker_handle.finish_err(e),
            }
        });
    }

    fn start_compress_zip(
        &mut self,
        sources: Vec<PathBuf>,
        dest_dir: PathBuf,
        archive_name: String,
    ) {
        let mut clean = archive_name.trim().to_string();
        if clean.is_empty() {
            clean = "Archiv.zip".to_string();
        }
        if !clean.to_lowercase().ends_with(".zip") {
            clean.push_str(".zip");
        }
        let output_zip = fs_ops::resolve_unique_path(&dest_dir, &clean);
        let handle = BgJobHandle::new(format!("Komprimiere in {clean}…"));
        let worker_handle = handle.clone();
        self.bg_job = Some(handle);

        std::thread::spawn(move || {
            match zip_ops::compress_to_zip(&sources, &output_zip, &worker_handle) {
                Ok(p) => {
                    let name = p
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    worker_handle.finish_ok(format!("ZIP erstellt: {name}"));
                }
                Err(e) => worker_handle.finish_err(e),
            }
        });
    }

    fn start_extract_zip(
        &mut self,
        zip_path: PathBuf,
        dest_dir: PathBuf,
        subfolder: Option<String>,
    ) {
        let target_dir = match subfolder {
            Some(sub) if !sub.trim().is_empty() => {
                fs_ops::resolve_unique_path(&dest_dir, sub.trim())
            }
            _ => dest_dir,
        };
        let zip_name = zip_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let handle = BgJobHandle::new(format!("Entpacke {zip_name}…"));
        let worker_handle = handle.clone();
        self.bg_job = Some(handle);

        std::thread::spawn(move || {
            match zip_ops::extract_zip(&zip_path, &target_dir, &worker_handle) {
                Ok(out) => {
                    let name = out
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    worker_handle.finish_ok(format!("Entpackt nach: {name}"));
                }
                Err(e) => worker_handle.finish_err(e),
            }
        });
    }

    fn start_delete(&mut self, targets: Vec<PathBuf>, permanent: bool) {
        if targets.is_empty() {
            return;
        }
        let primary = self.primary_root.clone();
        let title = if permanent {
            "Lösche endgültig…"
        } else {
            "Verschiebe in Papierkorb…"
        };
        let handle = BgJobHandle::new(title);
        let worker_handle = handle.clone();
        self.bg_job = Some(handle);

        std::thread::spawn(move || {
            let res = if permanent {
                fs_ops::delete_permanently(&targets, &worker_handle)
            } else {
                trash::move_to_trash(&primary, &targets)
            };
            match res {
                Ok(n) => {
                    let msg = if permanent {
                        format!("{n} Element(e) endgültig gelöscht.")
                    } else {
                        format!("{n} Element(e) in den Papierkorb verschoben.")
                    };
                    worker_handle.finish_ok(msg);
                }
                Err(e) => worker_handle.finish_err(e),
            }
        });
    }

    fn start_recursive_search(&mut self, pane_idx: usize) {
        let query = self.search_query.trim().to_string();
        if query.is_empty() {
            self.search_recursive_mode = false;
            self.refresh_pane(pane_idx);
            return;
        }
        let root = self.panes[pane_idx]
            .current_dir()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.primary_root.clone());
        let show_hidden = self.prefs.show_hidden;
        let (tx, rx) = mpsc::channel();
        self.search_rx = Some(rx);
        self.is_searching = true;
        self.search_recursive_mode = true;
        self.panes[pane_idx].is_loading = true;

        std::thread::spawn(move || {
            let entries = fs_ops::search_recursive(&root, &query, show_hidden, 300);
            let _ = tx.send(SearchResult {
                pane: pane_idx,
                entries,
            });
        });
    }

    fn poll_async(&mut self, ctx: &egui::Context) {
        // Upload any freshly decoded image thumbnails
        let mut got_thumb = false;
        while let Ok(decoded) = self.thumb_rx.try_recv() {
            if self.thumbs.len() > 160 {
                self.thumbs.clear();
                self.thumbs_requested.clear();
            }
            let tex = ctx.load_texture(
                format!("thumb_{}", decoded.path.display()),
                decoded.image,
                egui::TextureOptions::LINEAR,
            );
            self.thumbs.insert(decoded.path, tex);
            got_thumb = true;
        }
        if got_thumb {
            ctx.request_repaint();
        }

        if !self.has_storage_access && self.last_perm_check.elapsed() > Duration::from_millis(800) {
            self.last_perm_check = Instant::now();
            if self.bridge.has_all_files_access() {
                self.has_storage_access = true;
                self.refresh_all_panes();
                self.show_toast("Vollständiger Speicherzugriff aktiviert!", false);
            }
        }

        let back_key_pressed = ctx.input(|i| {
            i.key_pressed(egui::Key::Escape) || i.key_pressed(egui::Key::BrowserBack)
        });
        if self.bridge.take_back_pressed() || back_key_pressed {
            self.handle_back();
            ctx.request_repaint();
        }

        // Gentle periodic poll on Android so Back button & IME events respond immediately even when idle
        #[cfg(target_os = "android")]
        ctx.request_repaint_after(Duration::from_millis(90));

        if let Some(job) = &self.bg_job {
            let snap = job.snapshot();
            if snap.done {
                self.bg_job = None;
                if let Some(err) = snap.error_message {
                    self.show_toast(err, true);
                } else if let Some(msg) = snap.result_message {
                    self.show_toast(msg, false);
                }
                self.refresh_all_panes();
            } else {
                ctx.request_repaint_after(Duration::from_millis(50));
            }
        }

        if let Some(rx) = &self.search_rx {
            if let Ok(res) = rx.try_recv() {
                self.panes[res.pane].entries = res.entries;
                self.panes[res.pane].is_loading = false;
                self.is_searching = false;
                self.search_rx = None;
            } else {
                ctx.request_repaint_after(Duration::from_millis(60));
            }
        }

        if let Some(rx) = &self.inspector_rx {
            if let Ok(extra) = rx.try_recv() {
                self.inspector_extra = Some(extra);
                self.inspector_rx = None;
            } else {
                ctx.request_repaint_after(Duration::from_millis(80));
            }
        }

        if let Some(t) = &self.toast {
            if Instant::now() >= t.expires_at {
                self.toast = None;
            } else {
                ctx.request_repaint_after(Duration::from_millis(250));
            }
        }

        let wants_kb = ctx.egui_wants_keyboard_input();
        if wants_kb && !self.keyboard_was_open {
            self.bridge.show_keyboard();
            self.keyboard_was_open = true;
        } else if !wants_kb && self.keyboard_was_open {
            self.bridge.hide_keyboard();
            self.keyboard_was_open = false;
        }
    }

    fn handle_back(&mut self) {
        let ap = self.active_pane;
        if self.dialog.is_some() {
            self.dialog = None;
        } else if self.viewer.is_some() {
            self.viewer = None;
        } else if !self.search_query.is_empty() {
            self.search_query.clear();
            if self.search_recursive_mode {
                self.search_recursive_mode = false;
                self.refresh_pane(ap);
            }
        } else if !self.panes[ap].selected.is_empty() {
            self.panes[ap].selected.clear();
        } else if !self.panes[ap].current_tab().history_back.is_empty() {
            self.go_back(ap);
        } else if let Some(dir) = self.panes[ap].current_dir().map(Path::to_path_buf) {
            if dir != self.primary_root
                && dir.starts_with(&self.primary_root)
                && dir.parent().is_some()
            {
                self.go_up(ap);
            } else {
                self.navigate(ap, Location::Special(SpecialView::Home));
            }
        } else if !matches!(
            self.panes[ap].current_location(),
            Location::Special(SpecialView::Home)
        ) {
            self.navigate(ap, Location::Special(SpecialView::Home));
        } else {
            self.bridge.move_to_background();
        }
    }

    fn draw_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let p = apply_theme(&ctx, self.prefs.theme);

        // Respect Android system status bar, navigation bar & cutout insets!
        let ppp = ctx.pixels_per_point().max(1.0);
        let (in_top_px, in_bot_px, in_left_px, in_right_px) = self.bridge.system_insets_px();
        let top_pad = (in_top_px / ppp).round() as i8;
        let bot_pad = (in_bot_px / ppp).round() as i8;
        let left_pad = (in_left_px / ppp).round() as i8;
        let right_pad = (in_right_px / ppp).round() as i8;

        let screen_width = ctx.content_rect().width();
        let is_wide_tablet = screen_width >= 880.0;
        let dual = self.prefs.dual_pane && screen_width >= 760.0;

        // 1. Clean Single-Row Top Header Bar (sits cleanly below Android status bar)
        egui::Panel::top("top_header")
            .frame(
                Frame::NONE
                    .fill(p.bg_header)
                    .inner_margin(Margin {
                        left: 14 + left_pad,
                        right: 14 + right_pad,
                        top: 10 + top_pad,
                        bottom: 10,
                    })
                    .stroke(Stroke::new(1.0, p.divider)),
            )
            .show(ui, |ui| {
                self.draw_top_header(ui, &p, is_wide_tablet);
            });

        // 2. Permission Banner (only shown if All Files Access is missing on Android 11+)
        if !self.has_storage_access {
            egui::Panel::top("perm_banner")
                .frame(
                    Frame::NONE
                        .fill(p.accent_soft)
                        .inner_margin(Margin::symmetric(18, 10)),
                )
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        icon_widget(ui, Icon::Shield, 18.0, p.accent);
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new("Speicherzugriff erforderlich — Erlaube 'Zugriff auf alle Dateien', um alle Ordner und Dateien zu verwalten.")
                                .size(13.5)
                                .color(p.text_primary),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if pill_button(
                                ui,
                                Icon::Shield,
                                "Berechtigung öffnen",
                                p.accent,
                                p.accent_hover,
                                Color32::WHITE,
                                None,
                            )
                            .clicked()
                            {
                                self.bridge.request_all_files_access();
                            }
                        });
                    });
                });
        }

        // 3. Docked Full-Width Selection Action Bar (whenever 1+ items are selected!)
        let active_sel_count = self.panes[self.active_pane].selected.len();
        if active_sel_count > 0 {
            egui::Panel::bottom("selection_action_bar")
                .frame(
                    Frame::NONE
                        .fill(p.bg_floating)
                        .inner_margin(Margin {
                            left: 16 + left_pad,
                            right: 16 + right_pad,
                            top: 10,
                            bottom: 10 + bot_pad,
                        })
                        .stroke(Stroke::new(1.5, p.accent)),
                )
                .show(ui, |ui| {
                    self.draw_selection_action_bar(ui, self.active_pane, &p, dual);
                });
        }

        // 4. Persistent Clipboard Bar (when files are copied/cut)
        if !self.clipboard.is_empty() {
            egui::Panel::bottom("clipboard_bar")
                .frame(
                    Frame::NONE
                        .fill(p.bg_floating)
                        .inner_margin(Margin {
                            left: 18 + left_pad,
                            right: 18 + right_pad,
                            top: 10,
                            bottom: 10 + if active_sel_count > 0 { 0 } else { bot_pad },
                        })
                        .stroke(Stroke::new(1.0, p.accent)),
                )
                .show(ui, |ui| {
                    self.draw_clipboard_bar(ui, &p);
                });
        }

        // 5. Sleek Finder / One UI Sidebar
        if self.sidebar_open {
            egui::Panel::left("sidebar")
                .resizable(true)
                .default_size(248.0)
                .size_range(210.0..=310.0)
                .frame(
                    Frame::NONE
                        .fill(p.bg_sidebar)
                        .inner_margin(Margin {
                            left: 12 + left_pad,
                            right: 12,
                            top: 14,
                            bottom: 14 + bot_pad,
                        })
                        .stroke(Stroke::new(1.0, p.divider)),
                )
                .show(ui, |ui| {
                    self.draw_sidebar(ui, &p);
                });
        }

        // 6. Optional Right Inspector Panel
        if self.prefs.show_inspector && is_wide_tablet {
            egui::Panel::right("inspector")
                .resizable(true)
                .default_size(290.0)
                .size_range(250.0..=360.0)
                .frame(
                    Frame::NONE
                        .fill(p.bg_sidebar)
                        .inner_margin(Margin {
                            left: 16,
                            right: 16 + right_pad,
                            top: 16,
                            bottom: 16 + bot_pad,
                        })
                        .stroke(Stroke::new(1.0, p.divider)),
                )
                .show(ui, |ui| {
                    self.draw_inspector_panel(ui, &p);
                });
        }

        // 7. Main File Workspace
        egui::CentralPanel::default()
            .frame(
                Frame::NONE
                    .fill(p.bg_canvas)
                    .inner_margin(Margin {
                        left: 14,
                        right: 14 + right_pad,
                        top: 12,
                        bottom: 12 + bot_pad,
                    }),
            )
            .show(ui, |ui| {
                if dual {
                    let full_rect = ui.available_rect_before_wrap();
                    let gap = 12.0;
                    let half_w = ((full_rect.width() - gap) * 0.5).max(160.0);
                    let left_rect =
                        Rect::from_min_size(full_rect.min, vec2(half_w, full_rect.height()));
                    let right_rect = Rect::from_min_size(
                        egui::pos2(left_rect.right() + gap, full_rect.top()),
                        vec2((full_rect.width() - half_w - gap).max(160.0), full_rect.height()),
                    );

                    // Activate whichever pane is tapped
                    if ui.input(|i| i.pointer.any_pressed()) {
                        if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                            if left_rect.contains(pos) {
                                self.active_pane = 0;
                            } else if right_rect.contains(pos) {
                                self.active_pane = 1;
                            }
                        }
                    }

                    ui.scope_builder(
                        egui::UiBuilder::new()
                            .max_rect(left_rect)
                            .layout(Layout::top_down(Align::LEFT)),
                        |ui| {
                            ui.set_clip_rect(left_rect);
                            self.draw_pane(ui, &ctx, 0, &p, true);
                        },
                    );
                    ui.scope_builder(
                        egui::UiBuilder::new()
                            .max_rect(right_rect)
                            .layout(Layout::top_down(Align::LEFT)),
                        |ui| {
                            ui.set_clip_rect(right_rect);
                            self.draw_pane(ui, &ctx, 1, &p, true);
                        },
                    );
                } else {
                    self.draw_pane(ui, &ctx, 0, &p, false);
                }
            });

        // 8. Modals, Viewers, Progress & Toasts
        self.draw_dialogs(&ctx, &p);
        self.draw_viewer(&ctx, &p);
        self.draw_progress_overlay(&ctx, &p);
        self.draw_toast(&ctx, &p, bot_pad as f32);
    }

    fn draw_top_header(&mut self, ui: &mut egui::Ui, p: &Palette, is_wide: bool) {
        let ap = self.active_pane;
        ui.horizontal(|ui| {
            // Sidebar toggle
            if icon_button(
                ui,
                Icon::Sidebar,
                self.sidebar_open,
                true,
                p.bg_hover,
                p.accent_soft,
                p.text_secondary,
                p.accent,
            )
            .on_hover_text("Seitenleiste")
            .clicked()
            {
                self.sidebar_open = !self.sidebar_open;
            }

            ui.add_space(4.0);

            // Navigation cluster
            let can_back = !self.panes[ap].current_tab().history_back.is_empty();
            let can_fwd = !self.panes[ap].current_tab().history_forward.is_empty();
            let can_up = self.panes[ap]
                .current_dir()
                .and_then(|d| d.parent())
                .is_some();

            if icon_button(
                ui,
                Icon::Back,
                false,
                can_back,
                p.bg_hover,
                p.accent_soft,
                p.text_primary,
                p.accent,
            )
            .clicked()
            {
                self.go_back(ap);
            }
            if icon_button(
                ui,
                Icon::Forward,
                false,
                can_fwd,
                p.bg_hover,
                p.accent_soft,
                p.text_primary,
                p.accent,
            )
            .clicked()
            {
                self.go_forward(ap);
            }
            if icon_button(
                ui,
                Icon::Up,
                false,
                can_up,
                p.bg_hover,
                p.accent_soft,
                p.text_primary,
                p.accent,
            )
            .clicked()
            {
                self.go_up(ap);
            }
            if icon_button(
                ui,
                Icon::Refresh,
                false,
                true,
                p.bg_hover,
                p.accent_soft,
                p.text_secondary,
                p.accent,
            )
            .clicked()
            {
                self.refresh_all_panes();
            }

            ui.add_space(8.0);

            // Modern rounded search pill
            let search_w = if is_wide { 260.0 } else { 150.0 };
            Frame::NONE
                .fill(p.bg_surface)
                .corner_radius(CornerRadius::same(12))
                .stroke(Stroke::new(1.0, p.divider))
                .inner_margin(Margin::symmetric(10, 4))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        icon_widget(ui, Icon::Search, 16.0, p.text_muted);
                        let resp = ui.add(
                            egui::TextEdit::singleline(&mut self.search_query)
                                .hint_text("Dateien suchen…")
                                .frame(Frame::NONE)
                                .desired_width(search_w),
                        );
                        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            self.start_recursive_search(ap);
                        }
                        if !self.search_query.is_empty() {
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("Tief").size(11.5).color(p.cyan),
                                    )
                                    .frame(false),
                                )
                                .on_hover_text("In allen Unterordnern suchen")
                                .clicked()
                            {
                                self.start_recursive_search(ap);
                            }
                            if icon_button(
                                ui,
                                Icon::Close,
                                false,
                                true,
                                p.bg_hover,
                                p.accent_soft,
                                p.text_muted,
                                p.text_primary,
                            )
                            .clicked()
                            {
                                self.search_query.clear();
                                if self.search_recursive_mode {
                                    self.search_recursive_mode = false;
                                    self.refresh_pane(ap);
                                }
                            }
                        }
                    });
                });

            // Right-aligned toolbar controls
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                // Primary "+ Neu" CTA pill
                let in_dir = self.panes[ap].current_dir().is_some();
                ui.add_enabled_ui(in_dir, |ui| {
                    ui.menu_button(
                        RichText::new("  + Neu  ")
                            .size(13.5)
                            .strong()
                            .color(Color32::WHITE),
                        |ui| {
                            if pill_button(
                                ui,
                                Icon::Folder,
                                "Neuer Ordner",
                                Color32::TRANSPARENT,
                                p.bg_hover,
                                p.text_primary,
                                Some(170.0),
                            )
                            .clicked()
                            {
                                self.dialog = Some(DialogState::NewFolder {
                                    pane: ap,
                                    name: String::new(),
                                });
                                ui.close();
                            }
                            if pill_button(
                                ui,
                                Icon::File,
                                "Neue Textdatei",
                                Color32::TRANSPARENT,
                                p.bg_hover,
                                p.text_primary,
                                Some(170.0),
                            )
                            .clicked()
                            {
                                self.dialog = Some(DialogState::NewFile {
                                    pane: ap,
                                    name: "neu.txt".to_string(),
                                });
                                ui.close();
                            }
                        },
                    );
                });

                // Theme toggle
                let theme_icon = match self.prefs.theme {
                    ThemeMode::Dark => Icon::Sun,
                    ThemeMode::Light => Icon::Moon,
                };
                if icon_button(
                    ui,
                    theme_icon,
                    false,
                    true,
                    p.bg_hover,
                    p.accent_soft,
                    p.text_secondary,
                    p.accent,
                )
                .on_hover_text("Design wechseln (AMOLED / Hell)")
                .clicked()
                {
                    self.prefs.theme = match self.prefs.theme {
                        ThemeMode::Dark => ThemeMode::Light,
                        ThemeMode::Light => ThemeMode::Dark,
                    };
                    self.save_prefs();
                }

                // Inspector toggle
                if is_wide {
                    if icon_button(
                        ui,
                        Icon::Info,
                        self.prefs.show_inspector,
                        true,
                        p.bg_hover,
                        p.accent_soft,
                        p.text_secondary,
                        p.accent,
                    )
                    .on_hover_text("Inspektor / Details")
                    .clicked()
                    {
                        self.prefs.show_inspector = !self.prefs.show_inspector;
                        self.save_prefs();
                    }
                }

                // Dual-Pane toggle
                if icon_button(
                    ui,
                    Icon::DualPane,
                    self.prefs.dual_pane,
                    true,
                    p.bg_hover,
                    p.accent_soft,
                    p.text_secondary,
                    p.accent,
                )
                .on_hover_text("Dual-Pane Split-Ansicht")
                .clicked()
                {
                    self.prefs.dual_pane = !self.prefs.dual_pane;
                    if !self.prefs.dual_pane {
                        self.active_pane = 0;
                    }
                    self.save_prefs();
                }

                // List / Grid toggle
                let view_icon = match self.prefs.view_mode {
                    ViewMode::List => Icon::Grid,
                    ViewMode::Grid => Icon::List,
                };
                if icon_button(
                    ui,
                    view_icon,
                    false,
                    true,
                    p.bg_hover,
                    p.accent_soft,
                    p.text_secondary,
                    p.accent,
                )
                .on_hover_text("Liste / Raster wechseln")
                .clicked()
                {
                    self.prefs.view_mode = match self.prefs.view_mode {
                        ViewMode::List => ViewMode::Grid,
                        ViewMode::Grid => ViewMode::List,
                    };
                    self.save_prefs();
                }

                // Sort & View options menu
                ui.menu_button(
                    RichText::new("Sortieren").size(13.0).color(p.text_secondary),
                    |ui| {
                        for opt in [SortBy::Name, SortBy::Date, SortBy::Size, SortBy::Type] {
                            let selected = self.prefs.sort_by == opt;
                            let suffix = if selected {
                                match self.prefs.sort_order {
                                    SortOrder::Ascending => " (Aufst.)",
                                    SortOrder::Descending => " (Abst.)",
                                }
                            } else {
                                ""
                            };
                            if ui
                                .selectable_label(selected, format!("{}{}", opt.label(), suffix))
                                .clicked()
                            {
                                self.set_sort(opt);
                                ui.close();
                            }
                        }
                        ui.separator();
                        if ui
                            .checkbox(&mut self.prefs.folders_first, "Ordner zuerst")
                            .changed()
                        {
                            self.save_prefs();
                            self.refresh_all_panes();
                        }
                        if ui
                            .checkbox(&mut self.prefs.show_hidden, "Versteckte Dateien (.dot)")
                            .changed()
                        {
                            self.save_prefs();
                            self.refresh_all_panes();
                        }
                    },
                );
            });
        });
    }

    fn draw_sidebar(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let ap = self.active_pane;
        ScrollArea::vertical().show(ui, |ui| {
            // App Branding Header
            ui.horizontal(|ui| {
                category_icon_badge(ui, FileCategory::Folder, 34.0, p.accent);
                ui.add_space(4.0);
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new("TabManager")
                            .size(16.5)
                            .strong()
                            .color(p.text_primary),
                    );
                    ui.label(
                        RichText::new("Rust Native • 120 Hz")
                            .size(11.0)
                            .color(p.text_muted),
                    );
                });
            });

            ui.add_space(16.0);
            self.sidebar_section(ui, "SCHNELLZUGRIFF", p);

            if self.sidebar_nav_item(
                ui,
                Icon::Home,
                "Übersicht",
                matches!(
                    self.panes[ap].current_location(),
                    Location::Special(SpecialView::Home)
                ),
                p.accent,
                p,
            ) {
                self.navigate(ap, Location::Special(SpecialView::Home));
            }

            let root = self.primary_root.clone();
            if self.sidebar_nav_item(
                ui,
                Icon::Storage,
                "Interner Speicher",
                self.panes[ap].current_dir() == Some(root.as_path()),
                p.cyan,
                p,
            ) {
                self.navigate(ap, Location::Dir(root));
            }

            let dl = {
                let d1 = self.primary_root.join("Download");
                if d1.exists() {
                    d1
                } else {
                    self.primary_root.join("Downloads")
                }
            };
            if self.sidebar_nav_item(
                ui,
                Icon::Download,
                "Downloads",
                self.panes[ap].current_dir() == Some(dl.as_path()),
                Color32::from_rgb(59, 130, 246),
                p,
            ) {
                self.navigate(ap, Location::Dir(dl));
            }

            let docs = self.primary_root.join("Documents");
            if self.sidebar_nav_item(
                ui,
                Icon::Document,
                "Dokumente",
                self.panes[ap].current_dir() == Some(docs.as_path()),
                Color32::from_rgb(99, 102, 241),
                p,
            ) {
                self.navigate(ap, Location::Dir(docs));
            }

            let dcim = self.primary_root.join("DCIM");
            if self.sidebar_nav_item(
                ui,
                Icon::Image,
                "Kamera (DCIM)",
                self.panes[ap].current_dir() == Some(dcim.as_path()),
                Color32::from_rgb(56, 189, 248),
                p,
            ) {
                self.navigate(ap, Location::Dir(dcim));
            }

            ui.add_space(14.0);
            self.sidebar_section(ui, "KATEGORIEN", p);
            for cat in [
                FileCategory::Image,
                FileCategory::Video,
                FileCategory::Audio,
                FileCategory::Document,
                FileCategory::Archive,
                FileCategory::Apk,
            ] {
                let active = matches!(
                    self.panes[ap].current_location(),
                    Location::Special(SpecialView::Category(c)) if *c == cat
                );
                if self.sidebar_nav_item(
                    ui,
                    Icon::for_category(cat),
                    cat.label(),
                    active,
                    Palette::category_color(cat),
                    p,
                ) {
                    self.navigate(ap, Location::Special(SpecialView::Category(cat)));
                }
            }

            if !self.prefs.favorites.is_empty() {
                ui.add_space(14.0);
                self.sidebar_section(ui, "FAVORITEN", p);
                let favs = self.prefs.favorites.clone();
                for fav in favs {
                    let name = fav
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| fav.to_string_lossy().into_owned());
                    let active = self.panes[ap].current_dir() == Some(fav.as_path());
                    if self.sidebar_nav_item(
                        ui,
                        Icon::StarFilled,
                        &name,
                        active,
                        p.warning,
                        p,
                    ) {
                        self.navigate(ap, Location::Dir(fav));
                    }
                }
            }

            ui.add_space(14.0);
            self.sidebar_section(ui, "SPEICHER", p);
            let volumes = self.storage_volumes.clone();
            for vol in &volumes {
                let is_cur = self.panes[ap].current_dir() == Some(vol.path.as_path());
                let resp = Frame::NONE
                    .fill(if is_cur { p.bg_selected } else { p.bg_surface })
                    .corner_radius(CornerRadius::same(12))
                    .inner_margin(Margin::same(12))
                    .stroke(Stroke::new(
                        1.0,
                        if is_cur { p.accent } else { p.divider },
                    ))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            icon_widget(
                                ui,
                                if vol.is_removable {
                                    Icon::SdCard
                                } else {
                                    Icon::Storage
                                },
                                18.0,
                                p.cyan,
                            );
                            ui.label(
                                RichText::new(&vol.name)
                                    .size(13.5)
                                    .strong()
                                    .color(p.text_primary),
                            );
                        });
                        ui.add_space(8.0);
                        let frac = vol.used_fraction();
                        let bar_color = if frac > 0.9 {
                            p.danger
                        } else if frac > 0.75 {
                            p.warning
                        } else {
                            p.accent
                        };
                        self.draw_slim_bar(ui, frac, bar_color, p.bg_hover, 6.0);
                        ui.add_space(5.0);
                        ui.label(
                            RichText::new(format!(
                                "{} frei von {}",
                                format_size(vol.free_bytes),
                                format_size(vol.total_bytes)
                            ))
                            .size(11.5)
                            .color(p.text_secondary),
                        );
                    })
                    .response
                    .interact(Sense::click());
                if resp.clicked() {
                    self.navigate(ap, Location::Dir(vol.path.clone()));
                }
                ui.add_space(6.0);
            }

            ui.add_space(10.0);
            let trash_active = matches!(
                self.panes[ap].current_location(),
                Location::Special(SpecialView::Trash)
            );
            if self.sidebar_nav_item(ui, Icon::Trash, "Papierkorb", trash_active, p.danger, p) {
                self.navigate(ap, Location::Special(SpecialView::Trash));
            }
        });
    }

    fn sidebar_section(&self, ui: &mut egui::Ui, label: &str, p: &Palette) {
        ui.label(
            RichText::new(label)
                .size(10.5)
                .strong()
                .color(p.text_muted),
        );
        ui.add_space(4.0);
    }

    fn sidebar_nav_item(
        &self,
        ui: &mut egui::Ui,
        icon: Icon,
        label: &str,
        active: bool,
        icon_tint: Color32,
        p: &Palette,
    ) -> bool {
        let w = ui.available_width();
        let (rect, resp) = ui.allocate_exact_size(vec2(w, 38.0), Sense::click());
        let bg = if active {
            p.accent_soft
        } else if resp.hovered() {
            p.bg_hover
        } else {
            Color32::TRANSPARENT
        };
        if bg != Color32::TRANSPARENT {
            ui.painter().rect_filled(rect, CornerRadius::same(10), bg);
        }
        if active {
            let indicator = Rect::from_min_size(
                egui::pos2(rect.left() + 2.0, rect.top() + 8.0),
                vec2(3.5, rect.height() - 16.0),
            );
            ui.painter()
                .rect_filled(indicator, CornerRadius::same(2), p.accent);
        }

        let ic_rect = Rect::from_center_size(
            egui::pos2(rect.left() + 22.0, rect.center().y),
            vec2(18.0, 18.0),
        );
        paint_icon(
            ui.painter(),
            ic_rect,
            icon,
            if active { p.accent } else { icon_tint },
        );

        let text_color = if active {
            p.text_primary
        } else {
            p.text_secondary
        };
        let galley = ui.painter().layout_no_wrap(
            label.to_string(),
            egui::FontId::proportional(14.0),
            text_color,
        );
        ui.painter().galley(
            egui::pos2(rect.left() + 40.0, rect.center().y - galley.size().y * 0.5),
            galley,
            text_color,
        );
        resp.clicked()
    }

    fn draw_slim_bar(
        &self,
        ui: &mut egui::Ui,
        fraction: f32,
        fill: Color32,
        track: Color32,
        height: f32,
    ) {
        let w = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(vec2(w, height), Sense::hover());
        let cr = CornerRadius::same((height * 0.5) as u8);
        ui.painter().rect_filled(rect, cr, track);
        let filled_w = (rect.width() * fraction.clamp(0.0, 1.0)).max(if fraction > 0.0 {
            height
        } else {
            0.0
        });
        if filled_w > 0.0 {
            let filled = Rect::from_min_size(rect.min, vec2(filled_w, rect.height()));
            ui.painter().rect_filled(filled, cr, fill);
        }
    }

    fn draw_pane(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        pane_idx: usize,
        p: &Palette,
        is_dual: bool,
    ) {
        let is_active = self.active_pane == pane_idx;
        let stroke = if is_dual && is_active {
            Stroke::new(1.5, p.accent)
        } else {
            Stroke::new(1.0, p.divider)
        };

        Frame::NONE
            .fill(p.bg_surface_alt)
            .corner_radius(CornerRadius::same(16))
            .stroke(stroke)
            .inner_margin(Margin::same(if is_dual { 10 } else { 14 }))
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                ui.set_max_width(ui.available_width());

                // 0. Per-Pane Header Bar in Split-View (Active badge + independent nav!)
                if is_dual {
                    let can_back = !self.panes[pane_idx].current_tab().history_back.is_empty();
                    let can_up = self.panes[pane_idx]
                        .current_dir()
                        .and_then(|d| d.parent())
                        .is_some();
                    ui.horizontal(|ui| {
                        let badge_label = if pane_idx == 0 { "Links" } else { "Rechts" };
                        let badge_bg = if is_active { p.accent } else { p.bg_surface };
                        let badge_fg = if is_active { Color32::WHITE } else { p.text_secondary };
                        let b = Frame::NONE
                            .fill(badge_bg)
                            .corner_radius(CornerRadius::same(8))
                            .inner_margin(Margin::symmetric(10, 4))
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new(badge_label)
                                        .size(12.0)
                                        .strong()
                                        .color(badge_fg),
                                );
                            })
                            .response
                            .interact(Sense::click());
                        if b.clicked() {
                            self.active_pane = pane_idx;
                        }

                        if icon_button(ui, Icon::Back, false, can_back, p.bg_hover, p.accent_soft, p.text_primary, p.accent).clicked() {
                            self.go_back(pane_idx);
                        }
                        if icon_button(ui, Icon::Up, false, can_up, p.bg_hover, p.accent_soft, p.text_primary, p.accent).clicked() {
                            self.go_up(pane_idx);
                        }
                        if icon_button(ui, Icon::Home, false, true, p.bg_hover, p.accent_soft, p.text_secondary, p.accent).clicked() {
                            self.navigate(pane_idx, Location::Special(SpecialView::Home));
                        }
                        if icon_button(ui, Icon::Storage, false, true, p.bg_hover, p.accent_soft, p.text_secondary, p.accent).clicked() {
                            let root = self.primary_root.clone();
                            self.navigate(pane_idx, Location::Dir(root));
                        }
                    });
                    ui.add_space(4.0);
                    let (div_r, _) =
                        ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
                    ui.painter()
                        .rect_filled(div_r, CornerRadius::ZERO, p.divider);
                    ui.add_space(6.0);
                }

                // 1. Tab Strip (only prominent when >1 tab, plus subtle + tab button in breadcrumb)
                if self.panes[pane_idx].tabs.len() > 1 {
                    self.draw_tab_strip(ui, pane_idx, p);
                    ui.add_space(8.0);
                }

                // 2. Clean Breadcrumb Header
                self.draw_breadcrumb_bar(ui, pane_idx, p);
                ui.add_space(8.0);

                // 3. Content
                let loc = self.panes[pane_idx].current_location().clone();
                match loc {
                    Location::Special(SpecialView::Home) => {
                        self.draw_home_dashboard(ui, pane_idx, p);
                    }
                    Location::Special(SpecialView::Trash) => {
                        self.draw_trash_view(ui, p);
                    }
                    Location::Dir(_) | Location::Special(SpecialView::Category(_)) => {
                        self.draw_file_browser(ui, ctx, pane_idx, p);
                    }
                }
            });
    }

    fn draw_tab_strip(&mut self, ui: &mut egui::Ui, pane_idx: usize, p: &Palette) {
        let mut switch_to: Option<usize> = None;
        let mut close_idx: Option<usize> = None;
        let mut add_new = false;

        ui.horizontal(|ui| {
            let tab_count = self.panes[pane_idx].tabs.len();
            let active_idx = self.panes[pane_idx].active_tab;

            for i in 0..tab_count {
                let title = self.panes[pane_idx].tabs[i].location.title();
                let selected = i == active_idx;
                Frame::NONE
                    .fill(if selected { p.bg_surface } else { Color32::TRANSPARENT })
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(Margin::symmetric(12, 6))
                    .stroke(if selected {
                        Stroke::new(1.0, p.divider)
                    } else {
                        Stroke::NONE
                    })
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let lbl = ui.add(
                                egui::Label::new(
                                    RichText::new(&title)
                                        .size(13.0)
                                        .strong()
                                        .color(if selected {
                                            p.text_primary
                                        } else {
                                            p.text_muted
                                        }),
                                )
                                .sense(Sense::click()),
                            );
                            if lbl.clicked() {
                                switch_to = Some(i);
                            }
                            if tab_count > 1 {
                                let (r, resp) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
                                paint_icon(
                                    ui.painter(),
                                    Rect::from_center_size(r.center(), vec2(12.0, 12.0)),
                                    Icon::Close,
                                    if resp.hovered() { p.text_primary } else { p.text_muted },
                                );
                                if resp.clicked() {
                                    close_idx = Some(i);
                                }
                            }
                        });
                    });
            }

            if icon_button(
                ui,
                Icon::Plus,
                false,
                true,
                p.bg_hover,
                p.accent_soft,
                p.text_secondary,
                p.accent,
            )
            .on_hover_text("Neuen Tab öffnen")
            .clicked()
            {
                add_new = true;
            }
        });

        if let Some(idx) = switch_to {
            self.active_pane = pane_idx;
            self.panes[pane_idx].active_tab = idx;
            self.refresh_pane(pane_idx);
        }
        if let Some(idx) = close_idx {
            self.close_tab(pane_idx, idx);
        }
        if add_new {
            let root = self.primary_root.clone();
            self.add_tab(pane_idx, Location::Dir(root));
        }
    }

    fn draw_breadcrumb_bar(&mut self, ui: &mut egui::Ui, pane_idx: usize, p: &Palette) {
        let loc = self.panes[pane_idx].current_location().clone();
        let mut nav_target: Option<Location> = None;
        let mut toggle_fav: Option<PathBuf> = None;
        let mut add_tab = false;

        ui.horizontal(|ui| {
            match &loc {
                Location::Special(sp) => {
                    ui.label(
                        RichText::new(sp.title())
                            .size(18.0)
                            .strong()
                            .color(p.text_primary),
                    );
                }
                Location::Dir(dir) => {
                    let is_fav = self.prefs.favorites.contains(dir);
                    if icon_button(
                        ui,
                        if is_fav { Icon::StarFilled } else { Icon::Star },
                        is_fav,
                        true,
                        p.bg_hover,
                        Color32::TRANSPARENT,
                        p.text_muted,
                        p.warning,
                    )
                    .on_hover_text("Als Favorit anheften")
                    .clicked()
                    {
                        toggle_fav = Some(dir.clone());
                    }

                    let mut accum = PathBuf::new();
                    let components: Vec<_> = dir.components().collect();
                    for (i, comp) in components.iter().enumerate() {
                        accum.push(comp);
                        let seg_path = accum.clone();
                        if (seg_path == Path::new("/")
                            || seg_path == Path::new("/storage")
                            || seg_path == Path::new("/storage/emulated"))
                            && dir.starts_with("/storage/emulated/0")
                        {
                            continue;
                        }
                        let label = if seg_path == Path::new("/storage/emulated/0") {
                            "Interner Speicher".to_string()
                        } else {
                            comp.as_os_str().to_string_lossy().into_owned()
                        };
                        if i > 0 && seg_path != Path::new("/storage/emulated/0") {
                            ui.label(RichText::new("/").size(14.0).color(p.text_muted));
                        }
                        let is_last = i == components.len() - 1;
                        let resp = ui.add(
                            egui::Label::new(
                                RichText::new(label)
                                    .size(if is_last { 16.0 } else { 14.5 })
                                    .strong()
                                    .color(if is_last {
                                        p.text_primary
                                    } else {
                                        p.text_secondary
                                    }),
                            )
                            .sense(Sense::click()),
                        );
                        if resp.clicked() {
                            nav_target = Some(Location::Dir(seg_path));
                        }
                    }
                }
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if self.panes[pane_idx].tabs.len() == 1 {
                    if icon_button(
                        ui,
                        Icon::Plus,
                        false,
                        true,
                        p.bg_hover,
                        p.accent_soft,
                        p.text_muted,
                        p.accent,
                    )
                    .on_hover_text("Neuen Tab öffnen")
                    .clicked()
                    {
                        add_tab = true;
                    }
                }
                if matches!(
                    loc,
                    Location::Dir(_) | Location::Special(SpecialView::Category(_))
                ) {
                    let count = self.panes[pane_idx].entries.len();
                    ui.label(
                        RichText::new(format!("{count} Elemente"))
                            .size(12.5)
                            .color(p.text_muted),
                    );
                }
            });
        });

        if let Some(target) = nav_target {
            self.navigate(pane_idx, target);
        }
        if let Some(fav_path) = toggle_fav {
            self.toggle_favorite(fav_path);
        }
        if add_tab {
            let root = self.primary_root.clone();
            self.add_tab(pane_idx, Location::Dir(root));
        }
    }

    fn draw_home_dashboard(&mut self, ui: &mut egui::Ui, pane_idx: usize, p: &Palette) {
        let mut nav_target: Option<Location> = None;

        ScrollArea::vertical().show(ui, |ui| {
            ui.add_space(4.0);
            ui.label(
                RichText::new("Speicherübersicht")
                    .size(13.0)
                    .strong()
                    .color(p.text_muted),
            );
            ui.add_space(10.0);

            // Hero Storage Cards
            let volumes = self.storage_volumes.clone();
            ui.horizontal_wrapped(|ui| {
                for vol in &volumes {
                    let resp = Frame::NONE
                        .fill(p.bg_surface)
                        .corner_radius(CornerRadius::same(16))
                        .stroke(Stroke::new(1.0, p.divider))
                        .inner_margin(Margin::same(18))
                        .show(ui, |ui| {
                            ui.set_width(300.0);
                            ui.horizontal(|ui| {
                                category_icon_badge(
                                    ui,
                                    FileCategory::Document,
                                    44.0,
                                    if vol.is_removable { p.warning } else { p.accent },
                                );
                                ui.add_space(6.0);
                                ui.vertical(|ui| {
                                    ui.label(
                                        RichText::new(&vol.name)
                                            .size(16.0)
                                            .strong()
                                            .color(p.text_primary),
                                    );
                                    ui.label(
                                        RichText::new(format!(
                                            "{:.0}% belegt",
                                            vol.used_fraction() * 100.0
                                        ))
                                        .size(12.5)
                                        .color(p.text_secondary),
                                    );
                                });
                            });
                            ui.add_space(14.0);
                            self.draw_slim_bar(ui, vol.used_fraction(), p.accent, p.bg_hover, 8.0);
                            ui.add_space(10.0);
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(format!("Belegt: {}", format_size(vol.used_bytes())))
                                        .size(12.5)
                                        .color(p.text_secondary),
                                );
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    ui.label(
                                        RichText::new(format!("Frei: {}", format_size(vol.free_bytes)))
                                            .size(12.5)
                                            .strong()
                                            .color(p.cyan),
                                    );
                                });
                            });
                        })
                        .response
                        .interact(Sense::click());
                    if resp.clicked() {
                        nav_target = Some(Location::Dir(vol.path.clone()));
                    }
                    ui.add_space(6.0);
                }
            });

            ui.add_space(24.0);
            ui.label(
                RichText::new("Kategorien")
                    .size(13.0)
                    .strong()
                    .color(p.text_muted),
            );
            ui.add_space(10.0);

            let cats = [
                ("Bilder", "Fotos & Screenshots", FileCategory::Image),
                ("Videos", "Filme & Aufnahmen", FileCategory::Video),
                ("Audio", "Musik & Sprachmemos", FileCategory::Audio),
                ("Dokumente", "PDF, Office & Text", FileCategory::Document),
                ("Archive", "ZIP & komprimiert", FileCategory::Archive),
                ("APKs", "Installationspakete", FileCategory::Apk),
            ];

            ui.horizontal_wrapped(|ui| {
                for (title, subtitle, cat) in cats {
                    let color = Palette::category_color(cat);
                    let resp = Frame::NONE
                        .fill(p.bg_surface)
                        .corner_radius(CornerRadius::same(16))
                        .stroke(Stroke::new(1.0, p.divider))
                        .inner_margin(Margin::same(16))
                        .show(ui, |ui| {
                            ui.set_width(210.0);
                            ui.horizontal(|ui| {
                                category_icon_badge(ui, cat, 44.0, color);
                                ui.add_space(6.0);
                                ui.vertical(|ui| {
                                    ui.label(
                                        RichText::new(title)
                                            .size(15.0)
                                            .strong()
                                            .color(p.text_primary),
                                    );
                                    ui.label(
                                        RichText::new(subtitle)
                                            .size(11.5)
                                            .color(p.text_muted),
                                    );
                                });
                            });
                        })
                        .response
                        .interact(Sense::click());
                    if resp.clicked() {
                        nav_target = Some(Location::Special(SpecialView::Category(cat)));
                    }
                    ui.add_space(4.0);
                }
            });

            ui.add_space(24.0);
            ui.label(
                RichText::new("Schnellordner")
                    .size(13.0)
                    .strong()
                    .color(p.text_muted),
            );
            ui.add_space(10.0);

            let quick_dirs = [
                ("Downloads", self.primary_root.join("Download")),
                ("Dokumente", self.primary_root.join("Documents")),
                ("Kamera (DCIM)", self.primary_root.join("DCIM")),
                ("Bilder", self.primary_root.join("Pictures")),
            ];
            ui.horizontal_wrapped(|ui| {
                for (name, path) in quick_dirs {
                    if pill_button(
                        ui,
                        Icon::Folder,
                        name,
                        p.bg_surface,
                        p.bg_hover,
                        p.text_primary,
                        Some(150.0),
                    )
                    .clicked()
                    {
                        nav_target = Some(Location::Dir(path));
                    }
                }
            });
        });

        if let Some(target) = nav_target {
            self.navigate(pane_idx, target);
        }
    }

    fn draw_trash_view(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let mut restore_item: Option<TrashItem> = None;
        let mut delete_item: Option<TrashItem> = None;

        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("{} Element(e) im Papierkorb", self.trash_items.len()))
                    .size(13.5)
                    .color(p.text_secondary),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if !self.trash_items.is_empty()
                    && pill_button(
                        ui,
                        Icon::Trash,
                        "Papierkorb leeren",
                        p.danger_soft,
                        p.danger,
                        p.danger,
                        None,
                    )
                    .clicked()
                {
                    self.dialog = Some(DialogState::ConfirmEmptyTrash);
                }
            });
        });
        ui.add_space(10.0);

        if self.trash_items.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.vertical_centered(|ui| {
                    icon_widget(ui, Icon::Trash, 42.0, p.text_muted);
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new("Der Papierkorb ist leer.")
                            .size(15.0)
                            .color(p.text_muted),
                    );
                });
            });
            return;
        }

        let items = self.trash_items.clone();
        ScrollArea::vertical().show(ui, |ui| {
            for item in items {
                Frame::NONE
                    .fill(p.bg_surface)
                    .corner_radius(CornerRadius::same(12))
                    .inner_margin(Margin::symmetric(14, 10))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            category_icon_badge(
                                ui,
                                if item.is_dir {
                                    FileCategory::Folder
                                } else {
                                    FileCategory::Other
                                },
                                36.0,
                                if item.is_dir {
                                    Palette::category_color(FileCategory::Folder)
                                } else {
                                    p.text_secondary
                                },
                            );
                            ui.add_space(6.0);
                            ui.vertical(|ui| {
                                ui.label(
                                    RichText::new(&item.original_name)
                                        .size(14.5)
                                        .strong()
                                        .color(p.text_primary),
                                );
                                ui.label(
                                    RichText::new(format!(
                                        "{}  •  {}  •  {}",
                                        item.original_path.display(),
                                        format_size(item.size),
                                        format_timestamp(item.deleted_at_secs)
                                    ))
                                    .size(11.5)
                                    .color(p.text_muted),
                                );
                            });
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if icon_button(
                                    ui,
                                    Icon::Trash,
                                    false,
                                    true,
                                    p.danger_soft,
                                    p.danger_soft,
                                    p.danger,
                                    p.danger,
                                )
                                .on_hover_text("Endgültig löschen")
                                .clicked()
                                {
                                    delete_item = Some(item.clone());
                                }
                                if pill_button(
                                    ui,
                                    Icon::Restore,
                                    "Wiederherstellen",
                                    p.bg_hover,
                                    p.accent_soft,
                                    p.text_primary,
                                    None,
                                )
                                .clicked()
                                {
                                    restore_item = Some(item.clone());
                                }
                            });
                        });
                    });
                ui.add_space(6.0);
            }
        });

        if let Some(item) = restore_item {
            match trash::restore_trash_item(&self.primary_root, &item) {
                Ok(_) => {
                    self.show_toast(format!("'{}' wiederhergestellt", item.original_name), false);
                    self.refresh_all_panes();
                }
                Err(e) => self.show_toast(e, true),
            }
        }
        if let Some(item) = delete_item {
            match trash::delete_trash_item(&self.primary_root, &item) {
                Ok(_) => {
                    self.show_toast(format!("'{}' endgültig gelöscht", item.original_name), false);
                    self.refresh_all_panes();
                }
                Err(e) => self.show_toast(e, true),
            }
        }
    }

    fn draw_file_browser(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        pane_idx: usize,
        p: &Palette,
    ) {
        if let Some(err) = self.panes[pane_idx].error.clone() {
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new(err).size(14.5).color(p.danger));
            });
            return;
        }

        if self.panes[pane_idx].is_loading {
            ui.centered_and_justified(|ui| {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new("Lade Dateien…").color(p.text_secondary));
                });
            });
            return;
        }

        let filter = if self.search_recursive_mode {
            String::new()
        } else {
            self.search_query.trim().to_lowercase()
        };

        let entries: Vec<FileEntry> = self.panes[pane_idx]
            .entries
            .iter()
            .filter(|e| filter.is_empty() || e.name.to_lowercase().contains(&filter))
            .cloned()
            .collect();

        if entries.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.vertical_centered(|ui| {
                    icon_widget(ui, Icon::Folder, 44.0, p.text_muted);
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new("Dieser Ordner ist leer.")
                            .size(15.0)
                            .color(p.text_muted),
                    );
                });
            });
            return;
        }

        // Request async thumbnails for visible image entries
        for entry in entries.iter().take(60) {
            if entry.is_image() {
                self.request_thumbnail(entry);
            }
        }

        let mut open_target: Option<FileEntry> = None;
        let mut toggle_select: Option<PathBuf> = None;
        let mut set_active = false;
        let mut long_press_open: Option<FileEntry> = None;
        let mut sort_click: Option<SortBy> = None;
        let pointer_down = ui.input(|i| i.pointer.primary_down());
        let pointer_pos = ui.input(|i| i.pointer.interact_pos());

        // Clear long-press tracker if finger lifted
        let mut suppress_click_for_long_press = false;
        if !pointer_down {
            if let Some(lp) = self.long_press.take() {
                if lp.fired {
                    suppress_click_for_long_press = true;
                }
            }
        } else if let Some(lp) = &mut self.long_press {
            if let Some(pos) = pointer_pos {
                if pos.distance(lp.start_pos) > 18.0 {
                    self.long_press = None;
                } else if !lp.fired && lp.started_at.elapsed() >= Duration::from_millis(380) {
                    lp.fired = true;
                    long_press_open = Some(lp.entry.clone());
                } else if !lp.fired {
                    ctx.request_repaint_after(Duration::from_millis(40));
                }
            }
        }

        // Clean Table Column Header in List Mode
        if self.prefs.view_mode == ViewMode::List {
            let show_cols = ui.available_width() >= 540.0;
            Frame::NONE
                .inner_margin(Margin::symmetric(12, 6))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add_space(74.0);
                        let name_hdr = self.col_header_label("Name", SortBy::Name);
                        if ui
                            .add(
                                egui::Label::new(
                                    RichText::new(name_hdr)
                                        .size(12.0)
                                        .strong()
                                        .color(p.text_muted),
                                )
                                .sense(Sense::click()),
                            )
                            .clicked()
                        {
                            sort_click = Some(SortBy::Name);
                        }

                        if show_cols {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.add_space(40.0);
                                ui.allocate_ui(vec2(125.0, 20.0), |ui| {
                                    let date_hdr = self.col_header_label("Geändert", SortBy::Date);
                                    if ui
                                        .add(
                                            egui::Label::new(
                                                RichText::new(date_hdr)
                                                    .size(12.0)
                                                    .strong()
                                                    .color(p.text_muted),
                                            )
                                            .sense(Sense::click()),
                                        )
                                        .clicked()
                                    {
                                        sort_click = Some(SortBy::Date);
                                    }
                                });
                                ui.allocate_ui(vec2(90.0, 20.0), |ui| {
                                    let size_hdr = self.col_header_label("Größe", SortBy::Size);
                                    if ui
                                        .add(
                                            egui::Label::new(
                                                RichText::new(size_hdr)
                                                    .size(12.0)
                                                    .strong()
                                                    .color(p.text_muted),
                                            )
                                            .sense(Sense::click()),
                                        )
                                        .clicked()
                                    {
                                        sort_click = Some(SortBy::Size);
                                    }
                                });
                            });
                        }
                    });
                });
            let (line_rect, _) =
                ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
            ui.painter()
                .rect_filled(line_rect, CornerRadius::ZERO, p.divider);
        }

        ScrollArea::vertical()
            .id_salt(format!("pane_scroll_{pane_idx}"))
            .show(ui, |ui| match self.prefs.view_mode {
                ViewMode::List => {
                    let show_cols = ui.available_width() >= 540.0;
                    for entry in &entries {
                        let is_sel = self.panes[pane_idx].selected.contains(&entry.path);
                        let row_bg = if is_sel {
                            p.bg_selected
                        } else {
                            Color32::TRANSPARENT
                        };
                        let mut cb_rect = Rect::NOTHING;
                        let mut menu_rect = Rect::NOTHING;

                        let frame_resp = Frame::NONE
                            .fill(row_bg)
                            .corner_radius(CornerRadius::same(10))
                            .inner_margin(Margin::symmetric(10, 6))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    // 1. Dedicated left selection square (non-overlapped!)
                                    let cb_resp = draw_select_box(ui, is_sel, p);
                                    cb_rect = cb_resp.rect;
                                    if cb_resp.clicked() {
                                        toggle_select = Some(entry.path.clone());
                                        set_active = true;
                                    }

                                    // 2. Real image thumbnail OR crisp vector category badge
                                    if let Some(tex) = self.thumbs.get(&entry.path) {
                                        let (r, _) =
                                            ui.allocate_exact_size(vec2(38.0, 38.0), Sense::hover());
                                        ui.painter().image(
                                            tex.id(),
                                            r,
                                            Rect::from_min_max(
                                                egui::pos2(0.0, 0.0),
                                                egui::pos2(1.0, 1.0),
                                            ),
                                            Color32::WHITE,
                                        );
                                        ui.painter().rect_stroke(
                                            r,
                                            CornerRadius::same(8),
                                            Stroke::new(1.0, p.divider),
                                            egui::StrokeKind::Inside,
                                        );
                                    } else {
                                        category_icon_badge(
                                            ui,
                                            entry.category,
                                            38.0,
                                            Palette::category_color(entry.category),
                                        );
                                    }

                                    ui.add_space(6.0);

                                    // Filename (cleanly truncated so long names never overflow Split-View!)
                                    let right_reserved = if show_cols { 265.0 } else { 48.0 };
                                    let max_name_w = (ui.available_width() - right_reserved).max(80.0);
                                    ui.allocate_ui_with_layout(
                                        vec2(max_name_w, 40.0),
                                        Layout::top_down(Align::LEFT),
                                        |ui| {
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(&entry.name)
                                                        .size(14.5)
                                                        .strong()
                                                        .color(p.text_primary),
                                                )
                                                .truncate(),
                                            );
                                            if !show_cols {
                                                let sub = if entry.is_dir {
                                                    format!(
                                                        "{} Elemente",
                                                        entry.child_count.unwrap_or(0)
                                                    )
                                                } else {
                                                    format!(
                                                        "{}  •  {}",
                                                        format_size(entry.size),
                                                        format_timestamp(entry.modified_secs)
                                                    )
                                                };
                                                ui.add(
                                                    egui::Label::new(
                                                        RichText::new(sub)
                                                            .size(11.5)
                                                            .color(p.text_muted),
                                                    )
                                                    .truncate(),
                                                );
                                            } else if entry.is_dir {
                                                ui.label(
                                                    RichText::new(format!(
                                                        "{} Elemente",
                                                        entry.child_count.unwrap_or(0)
                                                    ))
                                                    .size(11.5)
                                                    .color(p.text_muted),
                                                );
                                            }
                                        },
                                    );

                                    // 3. Right-aligned columns + Action Sheet trigger button
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        let m_resp = icon_button(
                                            ui,
                                            Icon::MoreVert,
                                            false,
                                            true,
                                            p.bg_hover,
                                            p.accent_soft,
                                            p.text_muted,
                                            p.text_primary,
                                        );
                                        menu_rect = m_resp.rect;
                                        if m_resp.clicked() {
                                            long_press_open = Some(entry.clone());
                                        }

                                        if show_cols {
                                            ui.allocate_ui(vec2(125.0, 22.0), |ui| {
                                                ui.label(
                                                    RichText::new(format_timestamp(
                                                        entry.modified_secs,
                                                    ))
                                                    .size(12.5)
                                                    .color(p.text_secondary),
                                                );
                                            });
                                            ui.allocate_ui(vec2(90.0, 22.0), |ui| {
                                                let sz = if entry.is_dir {
                                                    "—".to_string()
                                                } else {
                                                    format_size(entry.size)
                                                };
                                                ui.label(
                                                    RichText::new(sz)
                                                        .size(12.5)
                                                        .color(p.text_secondary),
                                                );
                                            });
                                        }
                                    });
                                });
                            })
                            .response;

                        // Interactive hit region ONLY between checkbox and menu button — zero overlap!
                        let row_rect = frame_resp.rect;
                        let mid_left = (cb_rect.right() + 4.0).min(row_rect.right());
                        let mid_right = (menu_rect.left() - 4.0).max(mid_left);
                        let mid_rect = Rect::from_min_max(
                            egui::pos2(mid_left, row_rect.top()),
                            egui::pos2(mid_right, row_rect.bottom()),
                        );
                        let resp = ui.interact(
                            mid_rect,
                            ui.scope_id().with(("list_row_mid", pane_idx, &entry.path)),
                            Sense::click(),
                        );

                        if resp.is_pointer_button_down_on() && self.long_press.is_none() {
                            if let Some(pos) = pointer_pos {
                                self.long_press = Some(LongPressTracker {
                                    entry: entry.clone(),
                                    started_at: Instant::now(),
                                    start_pos: pos,
                                    fired: false,
                                });
                                ctx.request_repaint_after(Duration::from_millis(40));
                            }
                        }

                        if !suppress_click_for_long_press {
                            if resp.clicked() {
                                set_active = true;
                                if !self.panes[pane_idx].selected.is_empty() {
                                    toggle_select = Some(entry.path.clone());
                                } else {
                                    open_target = Some(entry.clone());
                                }
                            } else if resp.secondary_clicked() {
                                set_active = true;
                                long_press_open = Some(entry.clone());
                            }
                        }

                        let (div_r, _) =
                            ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
                        ui.painter()
                            .rect_filled(div_r, CornerRadius::ZERO, p.divider);
                    }
                    ui.add_space(20.0);
                }
                ViewMode::Grid => {
                    let avail_w = ui.available_width();
                    let tile_w = 160.0;
                    let cols = ((avail_w / (tile_w + 12.0)).floor() as usize).max(2);

                    for chunk in entries.chunks(cols) {
                        ui.horizontal(|ui| {
                            for entry in chunk {
                                let is_sel = self.panes[pane_idx].selected.contains(&entry.path);
                                let cat_color = Palette::category_color(entry.category);
                                let mut header_bottom = 0.0f32;

                                let frame_resp = Frame::NONE
                                    .fill(if is_sel { p.bg_selected } else { p.bg_surface })
                                    .corner_radius(CornerRadius::same(16))
                                    .stroke(Stroke::new(
                                        if is_sel { 1.5 } else { 1.0 },
                                        if is_sel { p.accent } else { p.divider },
                                    ))
                                    .inner_margin(Margin::same(10))
                                    .show(ui, |ui| {
                                        ui.set_width(tile_w - 20.0);
                                        ui.set_height(142.0);
                                        let hdr = ui.horizontal(|ui| {
                                            if draw_select_box(ui, is_sel, p).clicked() {
                                                toggle_select = Some(entry.path.clone());
                                                set_active = true;
                                            }
                                            ui.with_layout(
                                                Layout::right_to_left(Align::Center),
                                                |ui| {
                                                    if icon_button(
                                                        ui,
                                                        Icon::MoreVert,
                                                        false,
                                                        true,
                                                        p.bg_hover,
                                                        p.accent_soft,
                                                        p.text_muted,
                                                        p.text_primary,
                                                    )
                                                    .clicked()
                                                    {
                                                        long_press_open = Some(entry.clone());
                                                    }
                                                },
                                            );
                                        });
                                        header_bottom = hdr.response.rect.bottom();

                                        ui.vertical_centered(|ui| {
                                            if let Some(tex) = self.thumbs.get(&entry.path) {
                                                let (r, _) = ui.allocate_exact_size(
                                                    vec2(56.0, 56.0),
                                                    Sense::hover(),
                                                );
                                                ui.painter().image(
                                                    tex.id(),
                                                    r,
                                                    Rect::from_min_max(
                                                        egui::pos2(0.0, 0.0),
                                                        egui::pos2(1.0, 1.0),
                                                    ),
                                                    Color32::WHITE,
                                                );
                                            } else {
                                                category_icon_badge(
                                                    ui,
                                                    entry.category,
                                                    52.0,
                                                    cat_color,
                                                );
                                            }
                                            ui.add_space(6.0);
                                            let short_name = truncate_middle(&entry.name, 18);
                                            ui.label(
                                                RichText::new(short_name)
                                                    .size(13.0)
                                                    .strong()
                                                    .color(p.text_primary),
                                            );
                                            let sub = if entry.is_dir {
                                                format!(
                                                    "{} Elemente",
                                                    entry.child_count.unwrap_or(0)
                                                )
                                            } else {
                                                format_size(entry.size)
                                            };
                                            ui.label(
                                                RichText::new(sub)
                                                    .size(11.0)
                                                    .color(p.text_muted),
                                            );
                                        });
                                    })
                                    .response;

                                // Interactive body ONLY below the top checkbox/menu header — zero overlap!
                                let tile_rect = frame_resp.rect;
                                let body_top = (header_bottom + 2.0).min(tile_rect.bottom());
                                let body_rect = Rect::from_min_max(
                                    egui::pos2(tile_rect.left(), body_top),
                                    tile_rect.max,
                                );
                                let resp = ui.interact(
                                    body_rect,
                                    ui.scope_id().with(("grid_tile_body", pane_idx, &entry.path)),
                                    Sense::click(),
                                );

                                if resp.is_pointer_button_down_on() && self.long_press.is_none() {
                                    if let Some(pos) = pointer_pos {
                                        self.long_press = Some(LongPressTracker {
                                            entry: entry.clone(),
                                            started_at: Instant::now(),
                                            start_pos: pos,
                                            fired: false,
                                        });
                                        ctx.request_repaint_after(Duration::from_millis(40));
                                    }
                                }

                                if !suppress_click_for_long_press {
                                    if resp.clicked() {
                                        set_active = true;
                                        if !self.panes[pane_idx].selected.is_empty() {
                                            toggle_select = Some(entry.path.clone());
                                        } else {
                                            open_target = Some(entry.clone());
                                        }
                                    } else if resp.secondary_clicked() {
                                        set_active = true;
                                        long_press_open = Some(entry.clone());
                                    }
                                }
                                ui.add_space(4.0);
                            }
                        });
                        ui.add_space(8.0);
                    }
                    ui.add_space(20.0);
                }
            });

        if let Some(by) = sort_click {
            self.set_sort(by);
        }
        if set_active {
            self.active_pane = pane_idx;
        }
        if let Some(path) = toggle_select {
            if !self.panes[pane_idx].selected.remove(&path) {
                self.panes[pane_idx].selected.insert(path);
            }
        }
        if let Some(entry) = long_press_open {
            self.active_pane = pane_idx;
            self.dialog = Some(DialogState::ContextSheet {
                pane: pane_idx,
                entry,
            });
        } else if let Some(entry) = open_target {
            self.open_entry(pane_idx, &entry, ctx);
        }
    }

    fn execute_entry_action(
        &mut self,
        pane_idx: usize,
        action: &str,
        entry: FileEntry,
        ctx: &egui::Context,
    ) {
        self.active_pane = pane_idx;
        match action {
            "open" => self.open_entry(pane_idx, &entry, ctx),
            "open_ext" => {
                if let Err(e) = self.bridge.open_file(&entry.path) {
                    self.show_toast(e, true);
                }
            }
            "copy" => {
                self.clipboard = ClipboardState {
                    paths: vec![entry.path],
                    action: Some(ClipboardAction::Copy),
                };
                self.show_toast("Zum Kopieren gemerkt — Zielordner öffnen & Einfügen", false);
            }
            "cut" => {
                self.clipboard = ClipboardState {
                    paths: vec![entry.path],
                    action: Some(ClipboardAction::Move),
                };
                self.show_toast("Zum Verschieben gemerkt — Zielordner öffnen & Einfügen", false);
            }
            "move_to" => {
                let start = self.panes[pane_idx]
                    .current_dir()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| self.primary_root.clone());
                self.dialog = Some(DialogState::PickFolder {
                    pane: pane_idx,
                    sources: vec![entry.path],
                    is_move: true,
                    browse_dir: start,
                });
            }
            "copy_to" => {
                let start = self.panes[pane_idx]
                    .current_dir()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| self.primary_root.clone());
                self.dialog = Some(DialogState::PickFolder {
                    pane: pane_idx,
                    sources: vec![entry.path],
                    is_move: false,
                    browse_dir: start,
                });
            }
            "rename" => {
                self.dialog = Some(DialogState::Rename {
                    target: entry.path,
                    new_name: entry.name,
                });
            }
            "unzip" => {
                let stem = entry
                    .path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Entpackt".to_string());
                self.dialog = Some(DialogState::ExtractZip {
                    pane: pane_idx,
                    zip_path: entry.path,
                    subfolder_name: stem,
                    extract_into_subfolder: true,
                });
            }
            "zip" => {
                self.dialog = Some(DialogState::CompressZip {
                    pane: pane_idx,
                    sources: vec![entry.path],
                    archive_name: format!("{}.zip", entry.name),
                });
            }
            "share" => {
                if let Err(e) = self.bridge.share_files(&[entry.path]) {
                    self.show_toast(e, true);
                }
            }
            "inspect" => {
                self.panes[pane_idx].selected.clear();
                self.panes[pane_idx].selected.insert(entry.path);
                self.prefs.show_inspector = true;
                self.save_prefs();
            }
            "delete" => {
                self.dialog = Some(DialogState::ConfirmDelete {
                    pane: pane_idx,
                    targets: vec![entry.path],
                    permanent: false,
                });
            }
            _ => {}
        }
    }

    fn col_header_label(&self, title: &str, by: SortBy) -> String {
        if self.prefs.sort_by == by {
            match self.prefs.sort_order {
                SortOrder::Ascending => format!("{title}  ▲"),
                SortOrder::Descending => format!("{title}  ▼"),
            }
        } else {
            title.to_string()
        }
    }

    fn draw_selection_action_bar(
        &mut self,
        ui: &mut egui::Ui,
        pane_idx: usize,
        p: &Palette,
        is_dual: bool,
    ) {
        let selected_entries = self.panes[pane_idx].selected_entries();
        let selected_paths: Vec<PathBuf> = selected_entries.iter().map(|e| e.path.clone()).collect();
        let count = selected_paths.len();
        let other_pane = 1 - pane_idx;
        let other_dir = self.panes[other_pane].current_dir().map(Path::to_path_buf);

        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(format!("{count} ausgewählt"))
                    .size(14.0)
                    .strong()
                    .color(p.accent),
            );

            if ui.button("Alle").clicked() {
                let all: Vec<PathBuf> = self.panes[pane_idx]
                    .entries
                    .iter()
                    .map(|e| e.path.clone())
                    .collect();
                self.panes[pane_idx].selected.extend(all);
            }
            if ui.button("Abwählen").clicked() {
                self.panes[pane_idx].selected.clear();
            }

            ui.separator();

            if pill_button(ui, Icon::Copy, "Kopieren", p.bg_surface, p.bg_hover, p.text_primary, None).clicked() {
                self.clipboard = ClipboardState {
                    paths: selected_paths.clone(),
                    action: Some(ClipboardAction::Copy),
                };
                self.panes[pane_idx].selected.clear();
                self.show_toast(format!("{count} Element(e) zum Kopieren gemerkt"), false);
            }
            if pill_button(ui, Icon::Cut, "Verschieben", p.bg_surface, p.bg_hover, p.text_primary, None).clicked() {
                self.clipboard = ClipboardState {
                    paths: selected_paths.clone(),
                    action: Some(ClipboardAction::Move),
                };
                self.panes[pane_idx].selected.clear();
                self.show_toast(format!("{count} Element(e) zum Verschieben gemerkt"), false);
            }
            if pill_button(ui, Icon::Folder, "In Ordner…", p.accent_soft, p.accent, p.text_primary, None).clicked() {
                let start = self.panes[pane_idx]
                    .current_dir()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| self.primary_root.clone());
                self.dialog = Some(DialogState::PickFolder {
                    pane: pane_idx,
                    sources: selected_paths.clone(),
                    is_move: true,
                    browse_dir: start,
                });
            }

            if is_dual {
                if let Some(dst) = &other_dir {
                    if pill_button(ui, Icon::Transfer, "→ Kopieren", p.bg_surface, p.accent_soft, p.cyan, None).clicked() {
                        self.panes[pane_idx].selected.clear();
                        self.start_copy_or_move(selected_paths.clone(), dst.clone(), false);
                    }
                    if pill_button(ui, Icon::Transfer, "→ Verschieben", p.bg_surface, p.accent_soft, p.cyan, None).clicked() {
                        self.panes[pane_idx].selected.clear();
                        self.start_copy_or_move(selected_paths.clone(), dst.clone(), true);
                    }
                }
            }

            if count == 1 {
                let single = &selected_entries[0];
                if pill_button(ui, Icon::Rename, "Umbenennen", p.bg_surface, p.bg_hover, p.text_primary, None).clicked() {
                    self.dialog = Some(DialogState::Rename {
                        target: single.path.clone(),
                        new_name: single.name.clone(),
                    });
                }
                if single.is_zip()
                    && pill_button(ui, Icon::Unzip, "Entpacken", p.bg_surface, p.bg_hover, p.cyan, None).clicked()
                {
                    let stem = single
                        .path
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "Entpackt".to_string());
                    self.dialog = Some(DialogState::ExtractZip {
                        pane: pane_idx,
                        zip_path: single.path.clone(),
                        subfolder_name: stem,
                        extract_into_subfolder: true,
                    });
                }
            }

            if pill_button(ui, Icon::ZipAdd, "In ZIP", p.bg_surface, p.bg_hover, p.text_primary, None).clicked() {
                let default_name = if count == 1 {
                    format!("{}.zip", selected_entries[0].name)
                } else {
                    "Archiv.zip".to_string()
                };
                self.dialog = Some(DialogState::CompressZip {
                    pane: pane_idx,
                    sources: selected_paths.clone(),
                    archive_name: default_name,
                });
            }

            if selected_entries.iter().all(|e| !e.is_dir)
                && pill_button(ui, Icon::Share, "Teilen", p.bg_surface, p.bg_hover, p.text_primary, None).clicked()
            {
                if let Err(e) = self.bridge.share_files(&selected_paths) {
                    self.show_toast(e, true);
                }
            }

            if pill_button(ui, Icon::Trash, "Löschen", p.danger_soft, p.danger, p.danger, None).clicked() {
                self.dialog = Some(DialogState::ConfirmDelete {
                    pane: pane_idx,
                    targets: selected_paths.clone(),
                    permanent: false,
                });
            }
        });
    }

    fn draw_clipboard_bar(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let count = self.clipboard.paths.len();
        let is_move = self.clipboard.action == Some(ClipboardAction::Move);
        let label = if is_move {
            format!("{count} Element(e) zum Verschieben bereit")
        } else {
            format!("{count} Element(e) zum Kopieren bereit")
        };

        let target_dir = self.panes[self.active_pane]
            .current_dir()
            .map(Path::to_path_buf);

        ui.horizontal(|ui| {
            icon_widget(
                ui,
                if is_move { Icon::Cut } else { Icon::Copy },
                18.0,
                p.accent,
            );
            ui.label(RichText::new(label).size(14.0).strong().color(p.text_primary));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Abbrechen").clicked() {
                    self.clipboard = ClipboardState::default();
                }
                if let Some(dir) = target_dir {
                    if pill_button(
                        ui,
                        Icon::Paste,
                        "Hier einfügen",
                        p.accent,
                        p.accent_hover,
                        Color32::WHITE,
                        None,
                    )
                    .clicked()
                    {
                        let srcs = self.clipboard.paths.clone();
                        self.start_copy_or_move(srcs, dir, is_move);
                    }
                }
            });
        });
    }

    fn draw_inspector_panel(&mut self, ui: &mut egui::Ui, p: &Palette) {
        ui.label(
            RichText::new("DETAILS & INSPEKTOR")
                .size(11.0)
                .strong()
                .color(p.text_muted),
        );
        ui.add_space(12.0);

        let selected = self.panes[self.active_pane].selected_entries();
        if selected.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                icon_widget(ui, Icon::Info, 36.0, p.text_muted);
                ui.add_space(10.0);
                ui.label(
                    RichText::new("Markiere eine Datei oder einen Ordner für Details, Größe & SHA-256.")
                        .size(13.0)
                        .color(p.text_muted),
                );
            });
            return;
        }

        if selected.len() > 1 {
            let total_size: u64 = selected.iter().map(|e| e.size).sum();
            ui.label(
                RichText::new(format!("{} Elemente ausgewählt", selected.len()))
                    .size(16.0)
                    .strong(),
            );
            ui.add_space(6.0);
            ui.label(
                RichText::new(format!("Gesamtgröße: {}", format_size(total_size)))
                    .color(p.text_secondary),
            );
            return;
        }

        let item = selected[0].clone();
        let cat_color = Palette::category_color(item.category);

        Frame::NONE
            .fill(p.bg_surface)
            .corner_radius(CornerRadius::same(14))
            .stroke(Stroke::new(1.0, p.divider))
            .inner_margin(Margin::same(14))
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    category_icon_badge(ui, item.category, 56.0, cat_color);
                    ui.add_space(10.0);
                    ui.label(
                        RichText::new(&item.name)
                            .size(15.5)
                            .strong()
                            .color(p.text_primary),
                    );
                    ui.label(
                        RichText::new(if item.is_dir {
                            "Ordner"
                        } else {
                            item.category.label()
                        })
                        .size(12.0)
                        .color(p.text_muted),
                    );
                });
                ui.add_space(12.0);
                ui.separator();
                ui.add_space(8.0);
                ui.label(RichText::new("Pfad").size(11.0).color(p.text_muted));
                ui.label(
                    RichText::new(item.path.to_string_lossy())
                        .size(11.5)
                        .color(p.text_secondary),
                );
                ui.add_space(8.0);
                if !item.is_dir {
                    ui.label(format!("Größe: {}", format_size(item.size)));
                }
                ui.label(format!("Geändert: {}", format_timestamp(item.modified_secs)));
                ui.label(format!(
                    "Schreibschutz: {}",
                    if item.readonly { "Ja" } else { "Nein" }
                ));
            });

        ui.add_space(12.0);

        let matching_extra = self.inspector_extra.as_ref().filter(|e| e.path == item.path);

        if item.is_dir {
            if let Some(extra) = matching_extra {
                if let Some((bytes, files)) = extra.folder_stats {
                    ui.label(
                        RichText::new(format!(
                            "Ordnergröße: {} ({files} Dateien)",
                            format_size(bytes)
                        ))
                        .strong()
                        .color(p.cyan),
                    );
                    ui.add_space(6.0);
                }
            }
            if pill_button(
                ui,
                Icon::Info,
                "Ordnergröße berechnen",
                p.bg_surface,
                p.bg_hover,
                p.text_primary,
                Some(ui.available_width()),
            )
            .clicked()
            {
                let path = item.path.clone();
                let (tx, rx) = mpsc::channel();
                self.inspector_rx = Some(rx);
                std::thread::spawn(move || {
                    let stats = fs_ops::compute_folder_stats(&path);
                    let _ = tx.send(InspectorExtra {
                        path,
                        sha256: None,
                        folder_stats: Some(stats),
                    });
                });
            }
        } else {
            if let Some(extra) = matching_extra {
                if let Some(hash) = &extra.sha256 {
                    ui.label(RichText::new("SHA-256").size(11.0).color(p.text_muted));
                    ui.label(RichText::new(hash).monospace().size(10.5).color(p.cyan));
                    ui.add_space(6.0);
                }
            }
            if pill_button(
                ui,
                Icon::Hash,
                "SHA-256 berechnen",
                p.bg_surface,
                p.bg_hover,
                p.text_primary,
                Some(ui.available_width()),
            )
            .clicked()
            {
                let path = item.path.clone();
                let (tx, rx) = mpsc::channel();
                self.inspector_rx = Some(rx);
                std::thread::spawn(move || {
                    let hash = fs_ops::compute_sha256(&path).ok();
                    let _ = tx.send(InspectorExtra {
                        path,
                        sha256: hash,
                        folder_stats: None,
                    });
                });
            }
        }
    }

    fn draw_dialogs(&mut self, ctx: &egui::Context, p: &Palette) {
        let mut close = false;
        let mut action: Option<Box<dyn FnOnce(&mut Self)>> = None;

        if let Some(dialog) = &mut self.dialog {
            egui::Window::new(match dialog {
                DialogState::NewFolder { .. } => "Neuer Ordner",
                DialogState::NewFile { .. } => "Neue Datei",
                DialogState::Rename { .. } => "Umbenennen",
                DialogState::CompressZip { .. } => "In ZIP komprimieren",
                DialogState::ExtractZip { .. } => "ZIP entpacken",
                DialogState::ConfirmDelete { .. } => "Löschen bestätigen",
                DialogState::ConfirmEmptyTrash => "Papierkorb leeren",
                DialogState::ContextSheet { entry, .. } => entry.name.as_str(),
                DialogState::PickFolder { is_move, .. } => {
                    if *is_move {
                        "Zielordner zum Verschieben wählen"
                    } else {
                        "Zielordner zum Kopieren wählen"
                    }
                }
            })
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .min_width(400.0)
            .show(ctx, |ui| match dialog {
                DialogState::NewFolder { pane, name } => {
                    ui.label(RichText::new("Ordnername").size(13.0).color(p.text_secondary));
                    ui.add_space(4.0);
                    ui.add(egui::TextEdit::singleline(name).desired_width(f32::INFINITY));
                    ui.add_space(16.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if pill_button(ui, Icon::Check, "Erstellen", p.accent, p.accent_hover, Color32::WHITE, None).clicked() {
                            let pane_i = *pane;
                            let folder_name = name.clone();
                            close = true;
                            action = Some(Box::new(move |app| {
                                if let Some(dir) = app.panes[pane_i].current_dir().map(Path::to_path_buf) {
                                    match fs_ops::create_folder(&dir, &folder_name) {
                                        Ok(_) => {
                                            app.show_toast(format!("Ordner '{folder_name}' erstellt"), false);
                                            app.refresh_all_panes();
                                        }
                                        Err(e) => app.show_toast(e, true),
                                    }
                                }
                            }));
                        }
                        if ui.button("Abbrechen").clicked() {
                            close = true;
                        }
                    });
                }
                DialogState::NewFile { pane, name } => {
                    ui.label(RichText::new("Dateiname").size(13.0).color(p.text_secondary));
                    ui.add_space(4.0);
                    ui.add(egui::TextEdit::singleline(name).desired_width(f32::INFINITY));
                    ui.add_space(16.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if pill_button(ui, Icon::Check, "Erstellen", p.accent, p.accent_hover, Color32::WHITE, None).clicked() {
                            let pane_i = *pane;
                            let file_name = name.clone();
                            close = true;
                            action = Some(Box::new(move |app| {
                                if let Some(dir) = app.panes[pane_i].current_dir().map(Path::to_path_buf) {
                                    match fs_ops::create_file(&dir, &file_name) {
                                        Ok(_) => {
                                            app.show_toast(format!("Datei '{file_name}' erstellt"), false);
                                            app.refresh_all_panes();
                                        }
                                        Err(e) => app.show_toast(e, true),
                                    }
                                }
                            }));
                        }
                        if ui.button("Abbrechen").clicked() {
                            close = true;
                        }
                    });
                }
                DialogState::Rename { target, new_name } => {
                    ui.label(RichText::new("Neuer Name").size(13.0).color(p.text_secondary));
                    ui.add_space(4.0);
                    ui.add(egui::TextEdit::singleline(new_name).desired_width(f32::INFINITY));
                    ui.add_space(16.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if pill_button(ui, Icon::Check, "Umbenennen", p.accent, p.accent_hover, Color32::WHITE, None).clicked() {
                            let t = target.clone();
                            let n = new_name.clone();
                            close = true;
                            action = Some(Box::new(move |app| match fs_ops::rename_entry(&t, &n) {
                                Ok(_) => {
                                    app.show_toast(format!("Umbenannt in '{n}'"), false);
                                    app.refresh_all_panes();
                                }
                                Err(e) => app.show_toast(e, true),
                            }));
                        }
                        if ui.button("Abbrechen").clicked() {
                            close = true;
                        }
                    });
                }
                DialogState::CompressZip {
                    pane,
                    sources,
                    archive_name,
                } => {
                    ui.label(
                        RichText::new(format!("{} Element(e) in ZIP-Archiv packen", sources.len()))
                            .size(13.5)
                            .color(p.text_secondary),
                    );
                    ui.add_space(6.0);
                    ui.add(egui::TextEdit::singleline(archive_name).desired_width(f32::INFINITY));
                    ui.add_space(16.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if pill_button(ui, Icon::ZipAdd, "Komprimieren", p.accent, p.accent_hover, Color32::WHITE, None).clicked() {
                            let pane_i = *pane;
                            let srcs = sources.clone();
                            let name = archive_name.clone();
                            close = true;
                            action = Some(Box::new(move |app| {
                                if let Some(dir) = app.panes[pane_i].current_dir().map(Path::to_path_buf) {
                                    app.panes[pane_i].selected.clear();
                                    app.start_compress_zip(srcs, dir, name);
                                }
                            }));
                        }
                        if ui.button("Abbrechen").clicked() {
                            close = true;
                        }
                    });
                }
                DialogState::ExtractZip {
                    pane,
                    zip_path,
                    subfolder_name,
                    extract_into_subfolder,
                } => {
                    ui.label(
                        RichText::new(format!(
                            "Archiv: {}",
                            zip_path.file_name().unwrap_or_default().to_string_lossy()
                        ))
                        .size(13.5)
                        .color(p.text_secondary),
                    );
                    ui.add_space(8.0);
                    ui.checkbox(extract_into_subfolder, "In neuen Unterordner entpacken");
                    if *extract_into_subfolder {
                        ui.add_space(4.0);
                        ui.add(egui::TextEdit::singleline(subfolder_name).desired_width(f32::INFINITY));
                    }
                    ui.add_space(16.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if pill_button(ui, Icon::Unzip, "Entpacken", p.accent, p.accent_hover, Color32::WHITE, None).clicked() {
                            let pane_i = *pane;
                            let zp = zip_path.clone();
                            let sub = if *extract_into_subfolder {
                                Some(subfolder_name.clone())
                            } else {
                                None
                            };
                            close = true;
                            action = Some(Box::new(move |app| {
                                let dest = app.panes[pane_i]
                                    .current_dir()
                                    .map(Path::to_path_buf)
                                    .or_else(|| zp.parent().map(Path::to_path_buf))
                                    .unwrap_or_else(|| app.primary_root.clone());
                                app.panes[pane_i].selected.clear();
                                app.start_extract_zip(zp, dest, sub);
                            }));
                        }
                        if ui.button("Abbrechen").clicked() {
                            close = true;
                        }
                    });
                }
                DialogState::ConfirmDelete {
                    pane,
                    targets,
                    permanent,
                } => {
                    ui.label(
                        RichText::new(format!("{} Element(e) löschen?", targets.len()))
                            .size(14.5)
                            .color(p.text_primary),
                    );
                    ui.add_space(6.0);
                    ui.checkbox(permanent, "Endgültig löschen (Papierkorb überspringen)");
                    ui.add_space(16.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let lbl = if *permanent {
                            "Endgültig löschen"
                        } else {
                            "In Papierkorb"
                        };
                        if pill_button(ui, Icon::Trash, lbl, p.danger, p.danger, Color32::WHITE, None).clicked() {
                            let pane_i = *pane;
                            let t = targets.clone();
                            let perm = *permanent;
                            close = true;
                            action = Some(Box::new(move |app| {
                                app.panes[pane_i].selected.clear();
                                app.start_delete(t, perm);
                            }));
                        }
                        if ui.button("Abbrechen").clicked() {
                            close = true;
                        }
                    });
                }
                DialogState::ConfirmEmptyTrash => {
                    ui.label("Alle Elemente im Papierkorb unwiderruflich löschen?");
                    ui.add_space(16.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if pill_button(ui, Icon::Trash, "Leeren", p.danger, p.danger, Color32::WHITE, None).clicked() {
                            close = true;
                            action = Some(Box::new(|app| match trash::empty_trash(&app.primary_root) {
                                Ok(_) => {
                                    app.show_toast("Papierkorb geleert", false);
                                    app.refresh_all_panes();
                                }
                                Err(e) => app.show_toast(e, true),
                            }));
                        }
                        if ui.button("Abbrechen").clicked() {
                            close = true;
                        }
                    });
                }
                DialogState::ContextSheet { pane, entry } => {
                    let pane_i = *pane;
                    let ent = entry.clone();
                    let ctx_clone = ctx.clone();
                    let w = 360.0;

                    ui.horizontal(|ui| {
                        category_icon_badge(
                            ui,
                            ent.category,
                            40.0,
                            Palette::category_color(ent.category),
                        );
                        ui.add_space(6.0);
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new(&ent.name)
                                    .size(15.0)
                                    .strong()
                                    .color(p.text_primary),
                            );
                            let info = if ent.is_dir {
                                format!("Ordner • {} Elemente", ent.child_count.unwrap_or(0))
                            } else {
                                format!(
                                    "{} • {}",
                                    format_size(ent.size),
                                    format_timestamp(ent.modified_secs)
                                )
                            };
                            ui.label(RichText::new(info).size(12.0).color(p.text_muted));
                        });
                    });
                    ui.add_space(10.0);
                    ui.separator();
                    ui.add_space(6.0);

                    let mut chosen: Option<&'static str> = None;
                    if pill_button(ui, Icon::ExternalOpen, "Öffnen", Color32::TRANSPARENT, p.bg_hover, p.text_primary, Some(w)).clicked() {
                        chosen = Some("open");
                    }
                    if !ent.is_dir
                        && pill_button(ui, Icon::ExternalOpen, "Mit anderer App öffnen", Color32::TRANSPARENT, p.bg_hover, p.text_primary, Some(w)).clicked()
                    {
                        chosen = Some("open_ext");
                    }
                    if pill_button(ui, Icon::Folder, "In anderen Ordner verschieben…", Color32::TRANSPARENT, p.bg_hover, p.cyan, Some(w)).clicked() {
                        chosen = Some("move_to");
                    }
                    if pill_button(ui, Icon::Copy, "In anderen Ordner kopieren…", Color32::TRANSPARENT, p.bg_hover, p.text_primary, Some(w)).clicked() {
                        chosen = Some("copy_to");
                    }
                    if pill_button(ui, Icon::Copy, "In Zwischenablage kopieren", Color32::TRANSPARENT, p.bg_hover, p.text_primary, Some(w)).clicked() {
                        chosen = Some("copy");
                    }
                    if pill_button(ui, Icon::Cut, "Ausschneiden", Color32::TRANSPARENT, p.bg_hover, p.text_primary, Some(w)).clicked() {
                        chosen = Some("cut");
                    }
                    if pill_button(ui, Icon::Rename, "Umbenennen", Color32::TRANSPARENT, p.bg_hover, p.text_primary, Some(w)).clicked() {
                        chosen = Some("rename");
                    }
                    if ent.is_zip()
                        && pill_button(ui, Icon::Unzip, "ZIP entpacken…", Color32::TRANSPARENT, p.bg_hover, p.cyan, Some(w)).clicked()
                    {
                        chosen = Some("unzip");
                    }
                    if pill_button(ui, Icon::ZipAdd, "In ZIP komprimieren…", Color32::TRANSPARENT, p.bg_hover, p.text_primary, Some(w)).clicked() {
                        chosen = Some("zip");
                    }
                    if !ent.is_dir
                        && pill_button(ui, Icon::Share, "Teilen", Color32::TRANSPARENT, p.bg_hover, p.text_primary, Some(w)).clicked()
                    {
                        chosen = Some("share");
                    }
                    if pill_button(ui, Icon::Info, "Details & Eigenschaften", Color32::TRANSPARENT, p.bg_hover, p.text_primary, Some(w)).clicked() {
                        chosen = Some("inspect");
                    }
                    ui.separator();
                    if pill_button(ui, Icon::Trash, "Löschen", p.danger_soft, p.danger, p.danger, Some(w)).clicked() {
                        chosen = Some("delete");
                    }
                    ui.add_space(6.0);
                    if ui.button("Schließen").clicked() {
                        close = true;
                    }

                    if let Some(act) = chosen {
                        close = true;
                        action = Some(Box::new(move |app| {
                            app.execute_entry_action(pane_i, act, ent, &ctx_clone);
                        }));
                    }
                }
                DialogState::PickFolder {
                    pane,
                    sources,
                    is_move,
                    browse_dir,
                } => {
                    ui.horizontal(|ui| {
                        let can_up = browse_dir.parent().is_some();
                        if icon_button(ui, Icon::Up, false, can_up, p.bg_hover, p.accent_soft, p.text_primary, p.accent).clicked() {
                            if let Some(parent) = browse_dir.parent().map(Path::to_path_buf) {
                                *browse_dir = parent;
                            }
                        }
                        ui.label(
                            RichText::new(browse_dir.to_string_lossy())
                                .size(13.0)
                                .strong()
                                .color(p.text_primary),
                        );
                    });

                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.checkbox(is_move, "Verschieben (statt Kopieren)");
                    });
                    ui.separator();

                    // List subdirectories in browse_dir
                    let subdirs: Vec<PathBuf> = std::fs::read_dir(&*browse_dir)
                        .ok()
                        .map(|rd| {
                            let mut v: Vec<PathBuf> = rd
                                .flatten()
                                .map(|e| e.path())
                                .filter(|path| {
                                    path.is_dir()
                                        && !path
                                            .file_name()
                                            .map(|n| n.to_string_lossy().starts_with('.'))
                                            .unwrap_or(false)
                                })
                                .collect();
                            v.sort_by_key(|p| {
                                p.file_name()
                                    .map(|n| n.to_string_lossy().to_lowercase())
                                    .unwrap_or_default()
                            });
                            v
                        })
                        .unwrap_or_default();

                    let mut next_dir: Option<PathBuf> = None;
                    ScrollArea::vertical()
                        .max_height(260.0)
                        .show(ui, |ui| {
                            if subdirs.is_empty() {
                                ui.label(
                                    RichText::new("Keine Unterordner")
                                        .size(12.5)
                                        .color(p.text_muted),
                                );
                            }
                            for dir in subdirs {
                                let name = dir
                                    .file_name()
                                    .map(|n| n.to_string_lossy().into_owned())
                                    .unwrap_or_default();
                                if pill_button(
                                    ui,
                                    Icon::Folder,
                                    &name,
                                    Color32::TRANSPARENT,
                                    p.bg_hover,
                                    p.text_primary,
                                    Some(360.0),
                                )
                                .clicked()
                                {
                                    next_dir = Some(dir);
                                }
                            }
                        });

                    if let Some(d) = next_dir {
                        *browse_dir = d;
                    }

                    ui.add_space(12.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let cta = if *is_move {
                            "Hierher verschieben"
                        } else {
                            "Hierher kopieren"
                        };
                        if pill_button(ui, Icon::Check, cta, p.accent, p.accent_hover, Color32::WHITE, None).clicked() {
                            let pane_i = *pane;
                            let srcs = sources.clone();
                            let dst = browse_dir.clone();
                            let mv = *is_move;
                            close = true;
                            action = Some(Box::new(move |app| {
                                app.panes[pane_i].selected.clear();
                                app.start_copy_or_move(srcs, dst, mv);
                            }));
                        }
                        if ui.button("Abbrechen").clicked() {
                            close = true;
                        }
                    });
                }
            });
        }

        if close {
            self.dialog = None;
        }
        if let Some(f) = action {
            f(self);
        }
    }

    fn draw_viewer(&mut self, ctx: &egui::Context, p: &Palette) {
        let mut close = false;
        let mut open_external: Option<PathBuf> = None;
        let mut save_text: Option<(PathBuf, String)> = None;
        let mut extract_zip_req: Option<(PathBuf, bool)> = None;

        if let Some(viewer) = &mut self.viewer {
            let title = match viewer {
                ViewerState::Text { path, .. }
                | ViewerState::Image { path, .. }
                | ViewerState::Zip { path, .. } => path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Vorschau".to_string()),
            };

            let max_size = ctx.content_rect().size() * 0.84;
            egui::Window::new(title)
                .collapsible(false)
                .resizable(true)
                .default_size(max_size)
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .show(ctx, |ui| match viewer {
                    ViewerState::Text {
                        path,
                        content,
                        truncated,
                        dirty,
                    } => {
                        ui.horizontal(|ui| {
                            if !*truncated && *dirty {
                                if pill_button(ui, Icon::Save, "Speichern", p.accent, p.accent_hover, Color32::WHITE, None).clicked() {
                                    save_text = Some((path.clone(), content.clone()));
                                    *dirty = false;
                                }
                            }
                            if pill_button(ui, Icon::ExternalOpen, "Externe App", p.bg_surface, p.bg_hover, p.text_primary, None).clicked() {
                                open_external = Some(path.clone());
                            }
                            if *truncated {
                                ui.label(
                                    RichText::new("Nur Vorschau (Datei > 512 KB)").color(p.warning),
                                );
                            }
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if icon_button(ui, Icon::Close, false, true, p.bg_hover, p.accent_soft, p.text_secondary, p.text_primary).clicked() {
                                    close = true;
                                }
                            });
                        });
                        ui.separator();
                        ScrollArea::both().show(ui, |ui| {
                            let resp = ui.add_sized(
                                ui.available_size(),
                                egui::TextEdit::multiline(content)
                                    .code_editor()
                                    .interactive(!*truncated),
                            );
                            if resp.changed() {
                                *dirty = true;
                            }
                        });
                    }
                    ViewerState::Image {
                        path,
                        texture,
                        dimensions,
                        error,
                        zoom,
                    } => {
                        ui.horizontal(|ui| {
                            if let Some((w, h)) = dimensions {
                                ui.label(RichText::new(format!("{w} × {h} px")).color(p.text_secondary));
                            }
                            if icon_button(ui, Icon::ZoomOut, false, true, p.bg_hover, p.accent_soft, p.text_primary, p.accent).clicked() {
                                *zoom = (*zoom * 0.8).clamp(0.2, 5.0);
                            }
                            ui.label(format!("{:.0}%", *zoom * 100.0));
                            if icon_button(ui, Icon::ZoomIn, false, true, p.bg_hover, p.accent_soft, p.text_primary, p.accent).clicked() {
                                *zoom = (*zoom * 1.25).clamp(0.2, 5.0);
                            }
                            if ui.button("100%").clicked() {
                                *zoom = 1.0;
                            }
                            if pill_button(ui, Icon::ExternalOpen, "Externe App", p.bg_surface, p.bg_hover, p.text_primary, None).clicked() {
                                open_external = Some(path.clone());
                            }
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if icon_button(ui, Icon::Close, false, true, p.bg_hover, p.accent_soft, p.text_secondary, p.text_primary).clicked() {
                                    close = true;
                                }
                            });
                        });
                        ui.separator();
                        if let Some(err) = error {
                            ui.label(RichText::new(err.as_str()).color(p.danger));
                        } else if let Some(tex) = texture {
                            ScrollArea::both().show(ui, |ui| {
                                let avail = ui.available_size();
                                let tex_sz = tex.size_vec2();
                                let fit_scale = (avail.x / tex_sz.x).min(avail.y / tex_sz.y).min(1.0);
                                let display_size = tex_sz * fit_scale * *zoom;
                                ui.image(egui::load::SizedTexture::new(tex.id(), display_size));
                            });
                        }
                    }
                    ViewerState::Zip {
                        path,
                        entries,
                        error,
                    } => {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("{} Einträge", entries.len()))
                                    .color(p.text_secondary),
                            );
                            if pill_button(ui, Icon::Unzip, "In Ordner entpacken", p.accent, p.accent_hover, Color32::WHITE, None).clicked() {
                                extract_zip_req = Some((path.clone(), true));
                                close = true;
                            }
                            if pill_button(ui, Icon::Unzip, "Hier entpacken", p.bg_surface, p.bg_hover, p.text_primary, None).clicked() {
                                extract_zip_req = Some((path.clone(), false));
                                close = true;
                            }
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if icon_button(ui, Icon::Close, false, true, p.bg_hover, p.accent_soft, p.text_secondary, p.text_primary).clicked() {
                                    close = true;
                                }
                            });
                        });
                        ui.separator();
                        if let Some(err) = error {
                            ui.label(RichText::new(err.as_str()).color(p.danger));
                        } else {
                            ScrollArea::vertical().show(ui, |ui| {
                                for e in entries.iter() {
                                    ui.horizontal(|ui| {
                                        icon_widget(
                                            ui,
                                            if e.is_dir { Icon::Folder } else { Icon::File },
                                            16.0,
                                            if e.is_dir {
                                                Palette::category_color(FileCategory::Folder)
                                            } else {
                                                p.text_secondary
                                            },
                                        );
                                        ui.label(&e.name);
                                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                            if !e.is_dir {
                                                ui.label(
                                                    RichText::new(format!(
                                                        "{} ({} kompr.)",
                                                        format_size(e.size),
                                                        format_size(e.compressed_size)
                                                    ))
                                                    .size(11.5)
                                                    .color(p.text_muted),
                                                );
                                            }
                                        });
                                    });
                                }
                            });
                        }
                    }
                });
        }

        if close {
            self.viewer = None;
        }
        if let Some(path) = open_external {
            if let Err(e) = self.bridge.open_file(&path) {
                self.show_toast(e, true);
            }
        }
        if let Some((path, content)) = save_text {
            match fs_ops::write_text_file(&path, &content) {
                Ok(_) => self.show_toast("Datei gespeichert", false),
                Err(e) => self.show_toast(e, true),
            }
        }
        if let Some((zip_path, into_sub)) = extract_zip_req {
            let dest = self.panes[self.active_pane]
                .current_dir()
                .map(Path::to_path_buf)
                .or_else(|| zip_path.parent().map(Path::to_path_buf))
                .unwrap_or_else(|| self.primary_root.clone());
            let sub = if into_sub {
                Some(
                    zip_path
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "Entpackt".to_string()),
                )
            } else {
                None
            };
            self.start_extract_zip(zip_path, dest, sub);
        }
    }

    fn draw_progress_overlay(&mut self, ctx: &egui::Context, p: &Palette) {
        let Some(job) = self.bg_job.clone() else {
            return;
        };
        let snap = job.snapshot();
        egui::Window::new(&snap.title)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .min_width(380.0)
            .show(ctx, |ui| {
                ui.label(RichText::new(&snap.current_item).color(p.text_secondary));
                ui.add_space(10.0);
                self.draw_slim_bar(ui, snap.fraction(), p.accent, p.bg_hover, 8.0);
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("{} / {}", snap.completed, snap.total))
                            .size(12.0)
                            .color(p.text_muted),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .button(RichText::new("Abbrechen").color(p.danger))
                            .clicked()
                        {
                            job.cancel();
                        }
                    });
                });
            });
    }

    fn draw_toast(&self, ctx: &egui::Context, p: &Palette, bot_pad: f32) {
        let Some(toast) = &self.toast else {
            return;
        };
        egui::Area::new(egui::Id::NULL.with("toast_overlay"))
            .anchor(egui::Align2::RIGHT_BOTTOM, Vec2::new(-22.0, -(22.0 + bot_pad)))
            .show(ctx, |ui| {
                Frame::NONE
                    .fill(if toast.is_error { p.danger } else { p.bg_floating })
                    .corner_radius(CornerRadius::same(12))
                    .stroke(Stroke::new(
                        1.0,
                        if toast.is_error { p.danger } else { p.accent },
                    ))
                    .shadow(Shadow {
                        offset: [0, 6],
                        blur: 18,
                        spread: 0,
                        color: Color32::from_black_alpha(100),
                    })
                    .inner_margin(Margin::symmetric(16, 10))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(&toast.message)
                                .size(13.5)
                                .strong()
                                .color(Color32::WHITE),
                        );
                    });
            });
    }
}

fn draw_select_box(ui: &mut egui::Ui, selected: bool, p: &Palette) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(38.0, 38.0), Sense::click());
    let box_rect = Rect::from_center_size(rect.center(), vec2(22.0, 22.0));
    if selected {
        ui.painter()
            .rect_filled(box_rect, CornerRadius::same(6), p.accent);
        paint_icon(
            ui.painter(),
            box_rect.shrink(3.0),
            Icon::Check,
            Color32::WHITE,
        );
    } else {
        ui.painter().rect_stroke(
            box_rect,
            CornerRadius::same(6),
            Stroke::new(
                1.8,
                if resp.hovered() {
                    p.accent
                } else {
                    p.text_muted
                },
            ),
            egui::StrokeKind::Inside,
        );
    }
    resp
}

fn truncate_middle(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        return s.to_string();
    }
    let keep_front = (max_chars.saturating_sub(1)) * 2 / 3;
    let keep_back = max_chars.saturating_sub(keep_front + 1);
    let front: String = s.chars().take(keep_front).collect();
    let back: String = s.chars().skip(count - keep_back).collect();
    format!("{front}…{back}")
}

impl eframe::App for TabManagerApp {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        for ev in self.bridge.take_ime_events() {
            match ev {
                ImeEvent::Text(s) => raw.events.push(egui::Event::Text(s)),
                ImeEvent::Backspace => {
                    for pressed in [true, false] {
                        raw.events.push(egui::Event::Key {
                            key: egui::Key::Backspace,
                            physical_key: None,
                            pressed,
                            repeat: false,
                            modifiers: egui::Modifiers::NONE,
                        });
                    }
                }
                ImeEvent::Enter => {
                    for pressed in [true, false] {
                        raw.events.push(egui::Event::Key {
                            key: egui::Key::Enter,
                            physical_key: None,
                            pressed,
                            repeat: false,
                            modifiers: egui::Modifiers::NONE,
                        });
                    }
                }
            }
        }
    }

    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_async(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.draw_ui(ui);
    }
}
