use crate::android_bridge::PlatformBridge;
use crate::fs_ops;
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
    Align, Color32, CornerRadius, Frame, Layout, Margin, RichText, ScrollArea, Sense, Stroke,
    TextureHandle, Vec2,
};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
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
        pane: usize,
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
        // Fallback: open with external Android / OS app
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

    fn start_compress_zip(&mut self, sources: Vec<PathBuf>, dest_dir: PathBuf, archive_name: String) {
        let mut clean = archive_name.trim().to_string();
        if clean.is_empty() {
            clean = "Archiv.zip".to_string();
        }
        if !clean.to_lowercase().ends_with(".zip") {
            clean.push_str(".zip");
        }
        let output_zip = fs_ops::resolve_unique_path(&dest_dir, &clean);
        let handle = BgJobHandle::new(format!("Komprimiere in {}…", clean));
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
        // Periodically re-check All Files Access permission when returning from Android Settings
        if !self.has_storage_access && self.last_perm_check.elapsed() > Duration::from_millis(800) {
            self.last_perm_check = Instant::now();
            if self.bridge.has_all_files_access() {
                self.has_storage_access = true;
                self.refresh_all_panes();
                self.show_toast("Vollständiger Speicherzugriff aktiviert!", false);
            }
        }

        // Poll Android back gesture
        if self.bridge.take_back_pressed() {
            if self.dialog.is_some() {
                self.dialog = None;
            } else if self.viewer.is_some() {
                self.viewer = None;
            } else if !self.panes[self.active_pane].selected.is_empty() {
                self.panes[self.active_pane].selected.clear();
            } else if !self.panes[self.active_pane].current_tab().history_back.is_empty() {
                self.go_back(self.active_pane);
            } else {
                self.bridge.move_to_background();
            }
            ctx.request_repaint();
        }

        // Poll background file operation
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

        // Poll async search / category scan
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

        // Poll inspector SHA-256 / folder stats
        if let Some(rx) = &self.inspector_rx {
            if let Ok(extra) = rx.try_recv() {
                self.inspector_extra = Some(extra);
                self.inspector_rx = None;
            } else {
                ctx.request_repaint_after(Duration::from_millis(80));
            }
        }

        // Clear expired toast
        if let Some(t) = &self.toast {
            if Instant::now() >= t.expires_at {
                self.toast = None;
            } else {
                ctx.request_repaint_after(Duration::from_millis(250));
            }
        }

        // Sync Android soft keyboard with egui text focus
        let wants_kb = ctx.egui_wants_keyboard_input();
        if wants_kb && !self.keyboard_was_open {
            self.bridge.show_keyboard();
            self.keyboard_was_open = true;
        } else if !wants_kb && self.keyboard_was_open {
            self.bridge.hide_keyboard();
            self.keyboard_was_open = false;
        }
    }

    fn draw_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let p = apply_theme(&ctx, self.prefs.theme);
        let screen_width = ctx.content_rect().width();
        let is_wide_tablet = screen_width >= 900.0;

        // Top Action Toolbar
        egui::Panel::top("top_toolbar")
            .frame(
                Frame::NONE
                    .fill(p.bg_sidebar)
                    .inner_margin(Margin::symmetric(14, 10))
                    .stroke(Stroke::new(1.0, p.border)),
            )
            .show(ui, |ui| {
                self.draw_top_toolbar(ui, &p, is_wide_tablet);
            });

        // Permission banner on Android 11+ if All Files Access is not granted
        if !self.has_storage_access {
            egui::Panel::top("perm_banner")
                .frame(
                    Frame::NONE
                        .fill(p.accent_soft)
                        .inner_margin(Margin::symmetric(16, 10)),
                )
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("Speicherzugriff erforderlich — Bitte erlaube 'Zugriff auf alle Dateien' für vollständige Dateiverwaltung.")
                                .color(Color32::WHITE)
                                .strong(),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui
                                .button(RichText::new("Berechtigung öffnen").strong())
                                .clicked()
                            {
                                self.bridge.request_all_files_access();
                            }
                        });
                    });
                });
        }

        // Persistent Clipboard Banner
        if !self.clipboard.is_empty() {
            egui::Panel::bottom("clipboard_bar")
                .frame(
                    Frame::NONE
                        .fill(p.bg_panel)
                        .inner_margin(Margin::symmetric(16, 10))
                        .stroke(Stroke::new(1.0, p.accent)),
                )
                .show(ui, |ui| {
                    self.draw_clipboard_bar(ui, &p);
                });
        }

        // Left Navigation Sidebar
        if self.sidebar_open {
            egui::Panel::left("sidebar")
                .resizable(true)
                .default_size(240.0)
                .size_range(200.0..=320.0)
                .frame(
                    Frame::NONE
                        .fill(p.bg_sidebar)
                        .inner_margin(Margin::symmetric(12, 12))
                        .stroke(Stroke::new(1.0, p.border)),
                )
                .show(ui, |ui| {
                    self.draw_sidebar(ui, &p);
                });
        }

        // Optional Right Inspector Panel
        if self.prefs.show_inspector && is_wide_tablet {
            egui::Panel::right("inspector")
                .resizable(true)
                .default_size(290.0)
                .size_range(250.0..=380.0)
                .frame(
                    Frame::NONE
                        .fill(p.bg_sidebar)
                        .inner_margin(Margin::symmetric(14, 12))
                        .stroke(Stroke::new(1.0, p.border)),
                )
                .show(ui, |ui| {
                    self.draw_inspector_panel(ui, &p);
                });
        }

        // Main Workspace (Single Pane or Dual Pane Split-View!)
        egui::CentralPanel::default()
            .frame(
                Frame::NONE
                    .fill(p.bg_canvas)
                    .inner_margin(Margin::same(10)),
            )
            .show(ui, |ui| {
                let dual = self.prefs.dual_pane && screen_width >= 780.0;
                if dual {
                    let avail = ui.available_size();
                    let gap = 10.0;
                    let half_w = ((avail.x - gap) / 2.0).max(200.0);
                    ui.horizontal(|ui| {
                        ui.allocate_ui(Vec2::new(half_w, avail.y), |ui| {
                            self.draw_pane(ui, &ctx, 0, &p, true);
                        });
                        ui.add_space(gap);
                        ui.allocate_ui(Vec2::new(half_w, avail.y), |ui| {
                            self.draw_pane(ui, &ctx, 1, &p, true);
                        });
                    });
                } else {
                    self.draw_pane(ui, &ctx, 0, &p, false);
                }
            });

        // Modals, Viewers, Progress Overlay & Toasts
        self.draw_dialogs(&ctx, &p);
        self.draw_viewer(&ctx, &p);
        self.draw_progress_overlay(&ctx, &p);
        self.draw_toast(&ctx, &p);
    }

    fn draw_top_toolbar(&mut self, ui: &mut egui::Ui, p: &Palette, is_wide: bool) {
        let ap = self.active_pane;
        ui.horizontal_wrapped(|ui| {
            // Sidebar toggle
            if ui
                .button(if self.sidebar_open { "◧" } else { "☰" })
                .on_hover_text("Seitenleiste ein-/ausblenden")
                .clicked()
            {
                self.sidebar_open = !self.sidebar_open;
            }

            // Branding badge
            ui.label(
                RichText::new("TabManager")
                    .size(18.0)
                    .strong()
                    .color(p.text_primary),
            );
            ui.label(
                RichText::new("RUST")
                    .size(10.0)
                    .strong()
                    .color(p.cyan),
            );

            ui.separator();

            // Navigation buttons
            let can_back = !self.panes[ap].current_tab().history_back.is_empty();
            let can_fwd = !self.panes[ap].current_tab().history_forward.is_empty();
            let can_up = self.panes[ap]
                .current_dir()
                .and_then(|d| d.parent())
                .is_some();

            if ui
                .add_enabled(can_back, egui::Button::new("◀"))
                .on_hover_text("Zurück")
                .clicked()
            {
                self.go_back(ap);
            }
            if ui
                .add_enabled(can_fwd, egui::Button::new("▶"))
                .on_hover_text("Vorwärts")
                .clicked()
            {
                self.go_forward(ap);
            }
            if ui
                .add_enabled(can_up, egui::Button::new("▲"))
                .on_hover_text("Übergeordneter Ordner")
                .clicked()
            {
                self.go_up(ap);
            }
            if ui.button("↻").on_hover_text("Aktualisieren").clicked() {
                self.refresh_all_panes();
            }

            ui.separator();

            // Search Box
            let search_w = if is_wide { 240.0 } else { 150.0 };
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.search_query)
                    .hint_text("Suchen…")
                    .desired_width(search_w),
            );
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.start_recursive_search(ap);
            }
            if !self.search_query.is_empty() {
                if ui
                    .button("Tiefensuche")
                    .on_hover_text("Alle Unterordner durchsuchen")
                    .clicked()
                {
                    self.start_recursive_search(ap);
                }
                if ui.button("✕").clicked() {
                    self.search_query.clear();
                    if self.search_recursive_mode {
                        self.search_recursive_mode = false;
                        self.refresh_pane(ap);
                    }
                }
            }

            // Right-aligned controls
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                // Theme toggle
                let theme_lbl = match self.prefs.theme {
                    ThemeMode::Dark => "☀ Hell",
                    ThemeMode::Light => "☾ AMOLED",
                };
                if ui.button(theme_lbl).clicked() {
                    self.prefs.theme = match self.prefs.theme {
                        ThemeMode::Dark => ThemeMode::Light,
                        ThemeMode::Light => ThemeMode::Dark,
                    };
                    self.save_prefs();
                }

                // Inspector toggle
                if is_wide {
                    let insp_btn = egui::Button::new("ℹ Details").fill(if self.prefs.show_inspector {
                        p.accent
                    } else {
                        p.bg_card
                    });
                    if ui.add(insp_btn).clicked() {
                        self.prefs.show_inspector = !self.prefs.show_inspector;
                        self.save_prefs();
                    }
                }

                // Dual-Pane Split-View toggle
                let dual_btn = egui::Button::new("◫ Dual-Pane").fill(if self.prefs.dual_pane {
                    p.accent
                } else {
                    p.bg_card
                });
                if ui
                    .add(dual_btn)
                    .on_hover_text("Zwei Dateifenster nebeneinander")
                    .clicked()
                {
                    self.prefs.dual_pane = !self.prefs.dual_pane;
                    if !self.prefs.dual_pane {
                        self.active_pane = 0;
                    }
                    self.save_prefs();
                }

                // View Mode toggle
                let view_lbl = match self.prefs.view_mode {
                    ViewMode::List => "▦ Raster",
                    ViewMode::Grid => "☰ Liste",
                };
                if ui.button(view_lbl).clicked() {
                    self.prefs.view_mode = match self.prefs.view_mode {
                        ViewMode::List => ViewMode::Grid,
                        ViewMode::Grid => ViewMode::List,
                    };
                    self.save_prefs();
                }

                // Sort & Filter menu
                ui.menu_button("⇅ Sortieren", |ui| {
                    for opt in [SortBy::Name, SortBy::Date, SortBy::Size, SortBy::Type] {
                        let selected = self.prefs.sort_by == opt;
                        let arrow = if selected {
                            match self.prefs.sort_order {
                                SortOrder::Ascending => " ↑",
                                SortOrder::Descending => " ↓",
                            }
                        } else {
                            ""
                        };
                        if ui
                            .selectable_label(selected, format!("{}{}", opt.label(), arrow))
                            .clicked()
                        {
                            if self.prefs.sort_by == opt {
                                self.prefs.sort_order = self.prefs.sort_order.toggled();
                            } else {
                                self.prefs.sort_by = opt;
                                self.prefs.sort_order = SortOrder::Ascending;
                            }
                            self.save_prefs();
                            self.refresh_all_panes();
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
                });

                // "+ Neu" menu (New Folder / New File)
                let in_dir = self.panes[ap].current_dir().is_some();
                ui.add_enabled_ui(in_dir, |ui| {
                    ui.menu_button(
                        RichText::new("+ Neu").strong().color(p.cyan),
                        |ui| {
                            if ui.button("📁 Neuer Ordner").clicked() {
                                self.dialog = Some(DialogState::NewFolder {
                                    pane: ap,
                                    name: String::new(),
                                });
                                ui.close();
                            }
                            if ui.button("📄 Neue Textdatei").clicked() {
                                self.dialog = Some(DialogState::NewFile {
                                    pane: ap,
                                    name: "neu.txt".to_string(),
                                });
                                ui.close();
                            }
                        },
                    );
                });
            });
        });
    }

    fn draw_sidebar(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let ap = self.active_pane;
        ScrollArea::vertical().show(ui, |ui| {
            // Quick Navigation
            self.sidebar_section_title(ui, "SCHNELLZUGRIFF", p);

            if self.sidebar_item(
                ui,
                "🏠  Übersicht",
                matches!(
                    self.panes[ap].current_location(),
                    Location::Special(SpecialView::Home)
                ),
                p,
            ) {
                self.navigate(ap, Location::Special(SpecialView::Home));
            }

            let downloads = self.primary_root.join("Download");
            let downloads_alt = self.primary_root.join("Downloads");
            let dl_path = if downloads.exists() {
                downloads
            } else {
                downloads_alt
            };
            if self.sidebar_item(
                ui,
                "⬇  Downloads",
                self.panes[ap].current_dir() == Some(dl_path.as_path()),
                p,
            ) {
                self.navigate(ap, Location::Dir(dl_path));
            }

            let docs = self.primary_root.join("Documents");
            if self.sidebar_item(
                ui,
                "📄  Dokumente",
                self.panes[ap].current_dir() == Some(docs.as_path()),
                p,
            ) {
                self.navigate(ap, Location::Dir(docs));
            }

            let dcim = self.primary_root.join("DCIM");
            if self.sidebar_item(
                ui,
                "📷  Kamera (DCIM)",
                self.panes[ap].current_dir() == Some(dcim.as_path()),
                p,
            ) {
                self.navigate(ap, Location::Dir(dcim));
            }

            ui.add_space(10.0);
            self.sidebar_section_title(ui, "KATEGORIEN", p);
            for (icon, cat) in [
                ("🖼", FileCategory::Image),
                ("🎬", FileCategory::Video),
                ("🎵", FileCategory::Audio),
                ("📑", FileCategory::Document),
                ("📦", FileCategory::Archive),
                ("🤖", FileCategory::Apk),
            ] {
                let active = matches!(
                    self.panes[ap].current_location(),
                    Location::Special(SpecialView::Category(c)) if *c == cat
                );
                if self.sidebar_item(ui, &format!("{icon}  {}", cat.label()), active, p) {
                    self.navigate(ap, Location::Special(SpecialView::Category(cat)));
                }
            }

            ui.add_space(10.0);
            self.sidebar_section_title(ui, "SPEICHER", p);
            let volumes = self.storage_volumes.clone();
            for vol in &volumes {
                let is_cur = self.panes[ap].current_dir() == Some(vol.path.as_path());
                let card = Frame::NONE
                    .fill(if is_cur { p.bg_selected } else { p.bg_card })
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(Margin::same(10))
                    .stroke(Stroke::new(1.0, if is_cur { p.accent } else { p.border }));

                let resp = card
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        let icon = if vol.is_removable { "💾" } else { "📱" };
                        ui.label(
                            RichText::new(format!("{icon} {}", vol.name))
                                .strong()
                                .color(p.text_primary),
                        );
                        ui.add_space(4.0);
                        let frac = vol.used_fraction();
                        let bar_color = if frac > 0.9 {
                            p.danger
                        } else if frac > 0.75 {
                            p.warning
                        } else {
                            p.accent
                        };
                        ui.add(
                            egui::ProgressBar::new(frac)
                                .desired_height(6.0)
                                .fill(bar_color),
                        );
                        ui.add_space(2.0);
                        ui.label(
                            RichText::new(format!(
                                "{} / {} frei",
                                format_size(vol.free_bytes),
                                format_size(vol.total_bytes)
                            ))
                            .size(11.0)
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

            // Favorites
            if !self.prefs.favorites.is_empty() {
                ui.add_space(6.0);
                self.sidebar_section_title(ui, "FAVORITEN", p);
                let favs = self.prefs.favorites.clone();
                for fav in favs {
                    let name = fav
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| fav.to_string_lossy().into_owned());
                    let is_cur = self.panes[ap].current_dir() == Some(fav.as_path());
                    if self.sidebar_item(ui, &format!("★  {name}"), is_cur, p) {
                        self.navigate(ap, Location::Dir(fav));
                    }
                }
            }

            ui.add_space(10.0);
            self.sidebar_section_title(ui, "SYSTEM", p);
            let trash_active = matches!(
                self.panes[ap].current_location(),
                Location::Special(SpecialView::Trash)
            );
            if self.sidebar_item(ui, "🗑  Papierkorb", trash_active, p) {
                self.navigate(ap, Location::Special(SpecialView::Trash));
            }
        });
    }

    fn sidebar_section_title(&self, ui: &mut egui::Ui, title: &str, p: &Palette) {
        ui.label(
            RichText::new(title)
                .size(11.0)
                .strong()
                .color(p.text_muted),
        );
        ui.add_space(4.0);
    }

    fn sidebar_item(&self, ui: &mut egui::Ui, label: &str, active: bool, p: &Palette) -> bool {
        let btn = egui::Button::new(
            RichText::new(label)
                .size(14.0)
                .color(if active { Color32::WHITE } else { p.text_primary }),
        )
        .fill(if active { p.accent } else { Color32::TRANSPARENT })
        .stroke(Stroke::NONE)
        .min_size(Vec2::new(ui.available_width(), 36.0));
        ui.add(btn).clicked()
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
        let border_color = if is_dual && is_active {
            p.accent
        } else {
            p.border
        };

        Frame::NONE
            .fill(p.bg_panel)
            .corner_radius(CornerRadius::same(14))
            .stroke(Stroke::new(if is_dual && is_active { 1.5 } else { 1.0 }, border_color))
            .inner_margin(Margin::same(10))
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());

                // 1. Multi-Tab Strip
                self.draw_tab_strip(ui, pane_idx, p);
                ui.add_space(6.0);

                // 2. Breadcrumb Bar
                self.draw_breadcrumb_bar(ui, pane_idx, p);
                ui.add_space(6.0);

                // 3. Selection / Context Action Bar (if items selected)
                if !self.panes[pane_idx].selected.is_empty() {
                    self.draw_selection_bar(ui, pane_idx, p, is_dual);
                    ui.add_space(6.0);
                }

                // 4. Content Area
                let loc = self.panes[pane_idx].current_location().clone();
                match loc {
                    Location::Special(SpecialView::Home) => {
                        self.draw_home_dashboard(ui, pane_idx, p);
                    }
                    Location::Special(SpecialView::Trash) => {
                        self.draw_trash_view(ui, pane_idx, p);
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
                let frame = Frame::NONE
                    .fill(if selected { p.bg_card_hover } else { p.bg_card })
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin::symmetric(10, 5))
                    .stroke(Stroke::new(
                        1.0,
                        if selected { p.accent } else { p.border },
                    ));

                frame.show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let lbl = ui.add(
                            egui::Label::new(
                                RichText::new(&title)
                                    .size(13.0)
                                    .color(if selected {
                                        p.text_primary
                                    } else {
                                        p.text_secondary
                                    }),
                            )
                            .sense(Sense::click()),
                        );
                        if lbl.clicked() {
                            switch_to = Some(i);
                        }
                        if tab_count > 1 {
                            if ui
                                .add(
                                    egui::Button::new(RichText::new("×").size(13.0))
                                        .frame(false),
                                )
                                .clicked()
                            {
                                close_idx = Some(i);
                            }
                        }
                    });
                });
            }

            if ui
                .button("+")
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

        Frame::NONE
            .fill(p.bg_card)
            .corner_radius(CornerRadius::same(10))
            .inner_margin(Margin::symmetric(10, 6))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    match &loc {
                        Location::Special(sp) => {
                            ui.label(
                                RichText::new(sp.title())
                                    .strong()
                                    .color(p.text_primary),
                            );
                        }
                        Location::Dir(dir) => {
                            let is_fav = self.prefs.favorites.contains(dir);
                            if ui
                                .button(if is_fav { "★" } else { "☆" })
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
                                let label = if seg_path == Path::new("/storage/emulated/0") {
                                    "Interner Speicher".to_string()
                                } else {
                                    comp.as_os_str().to_string_lossy().into_owned()
                                };
                                // Skip verbose /storage/emulated prefix on Android
                                if seg_path == Path::new("/")
                                    || seg_path == Path::new("/storage")
                                    || seg_path == Path::new("/storage/emulated")
                                {
                                    if dir.starts_with("/storage/emulated/0") {
                                        continue;
                                    }
                                }
                                if i > 0 && seg_path != Path::new("/storage/emulated/0") {
                                    ui.label(RichText::new("›").color(p.text_muted));
                                }
                                let is_last = i == components.len() - 1;
                                if ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new(label).color(if is_last {
                                                p.text_primary
                                            } else {
                                                p.text_secondary
                                            }),
                                        )
                                        .frame(false),
                                    )
                                    .clicked()
                                {
                                    nav_target = Some(Location::Dir(seg_path));
                                }
                            }
                        }
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let count = self.panes[pane_idx].entries.len();
                        if matches!(loc, Location::Dir(_) | Location::Special(SpecialView::Category(_))) {
                            ui.label(
                                RichText::new(format!("{count} Elemente"))
                                    .size(12.0)
                                    .color(p.text_muted),
                            );
                        }
                    });
                });
            });

        if let Some(target) = nav_target {
            self.navigate(pane_idx, target);
        }
        if let Some(fav_path) = toggle_fav {
            self.toggle_favorite(fav_path);
        }
    }

    fn draw_selection_bar(
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

        Frame::NONE
            .fill(p.accent_soft)
            .corner_radius(CornerRadius::same(10))
            .inner_margin(Margin::symmetric(10, 7))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(format!("{count} ausgewählt"))
                            .strong()
                            .color(Color32::WHITE),
                    );

                    if ui.button("Alle").clicked() {
                        let all: Vec<PathBuf> =
                            self.panes[pane_idx].entries.iter().map(|e| e.path.clone()).collect();
                        self.panes[pane_idx].selected.extend(all);
                    }
                    if ui.button("Abwählen").clicked() {
                        self.panes[pane_idx].selected.clear();
                    }

                    ui.separator();

                    if ui.button("📋 Kopieren").clicked() {
                        self.clipboard = ClipboardState {
                            paths: selected_paths.clone(),
                            action: Some(ClipboardAction::Copy),
                        };
                        self.panes[pane_idx].selected.clear();
                        self.show_toast(format!("{count} Element(e) zum Kopieren gemerkt"), false);
                    }
                    if ui.button("✂ Ausschneiden").clicked() {
                        self.clipboard = ClipboardState {
                            paths: selected_paths.clone(),
                            action: Some(ClipboardAction::Move),
                        };
                        self.panes[pane_idx].selected.clear();
                        self.show_toast(format!("{count} Element(e) zum Verschieben gemerkt"), false);
                    }

                    // Dual-Pane direct transfer buttons!
                    if is_dual {
                        if let Some(dst) = &other_dir {
                            if ui
                                .button("⇄ Kopieren →")
                                .on_hover_text(format!("Direkt nach {} kopieren", dst.display()))
                                .clicked()
                            {
                                self.panes[pane_idx].selected.clear();
                                self.start_copy_or_move(selected_paths.clone(), dst.clone(), false);
                            }
                            if ui
                                .button("⇄ Verschieben →")
                                .on_hover_text(format!("Direkt nach {} verschieben", dst.display()))
                                .clicked()
                            {
                                self.panes[pane_idx].selected.clear();
                                self.start_copy_or_move(selected_paths.clone(), dst.clone(), true);
                            }
                        }
                    }

                    // Single-item actions
                    if count == 1 {
                        let single = &selected_entries[0];
                        if ui.button("✏ Umbenennen").clicked() {
                            self.dialog = Some(DialogState::Rename {
                                pane: pane_idx,
                                target: single.path.clone(),
                                new_name: single.name.clone(),
                            });
                        }
                        if single.is_zip() && ui.button("📂 Entpacken").clicked() {
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

                    // Compress to ZIP
                    if ui.button("📦 ZIP erstellen").clicked() {
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

                    // Share
                    if selected_entries.iter().all(|e| !e.is_dir) && ui.button("↗ Teilen").clicked()
                    {
                        if let Err(e) = self.bridge.share_files(&selected_paths) {
                            self.show_toast(e, true);
                        }
                    }

                    // Delete
                    if ui
                        .button(RichText::new("🗑 Löschen").color(Color32::from_rgb(254, 202, 202)))
                        .clicked()
                    {
                        self.dialog = Some(DialogState::ConfirmDelete {
                            pane: pane_idx,
                            targets: selected_paths.clone(),
                            permanent: false,
                        });
                    }
                });
            });
    }

    fn draw_clipboard_bar(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let count = self.clipboard.paths.len();
        let is_move = self.clipboard.action == Some(ClipboardAction::Move);
        let label = if is_move {
            format!("✂ {count} Element(e) zum Verschieben bereit")
        } else {
            format!("📋 {count} Element(e) zum Kopieren bereit")
        };

        let target_dir = self.panes[self.active_pane]
            .current_dir()
            .map(Path::to_path_buf);

        ui.horizontal(|ui| {
            ui.label(RichText::new(label).strong().color(p.text_primary));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Abbrechen").clicked() {
                    self.clipboard = ClipboardState::default();
                }
                if let Some(dir) = target_dir {
                    let btn = egui::Button::new(
                        RichText::new("📥 Hier einfügen")
                            .strong()
                            .color(Color32::WHITE),
                    )
                    .fill(p.accent);
                    if ui.add(btn).clicked() {
                        let srcs = self.clipboard.paths.clone();
                        self.start_copy_or_move(srcs, dir, is_move);
                    }
                }
            });
        });
    }

    fn draw_home_dashboard(&mut self, ui: &mut egui::Ui, pane_idx: usize, p: &Palette) {
        let mut nav_target: Option<Location> = None;

        ScrollArea::vertical().show(ui, |ui| {
            ui.add_space(6.0);
            ui.label(
                RichText::new("Willkommen in TabManager")
                    .size(22.0)
                    .strong()
                    .color(p.text_primary),
            );
            ui.label(
                RichText::new("Nativer Rust-Dateimanager — optimiert für das Galaxy Tab S12 Ultra")
                    .size(13.0)
                    .color(p.text_secondary),
            );
            ui.add_space(16.0);

            // Storage Cards
            ui.label(RichText::new("SPEICHERBEREICHE").size(12.0).strong().color(p.text_muted));
            ui.add_space(8.0);
            let volumes = self.storage_volumes.clone();
            ui.horizontal_wrapped(|ui| {
                for vol in &volumes {
                    let resp = Frame::NONE
                        .fill(p.bg_card)
                        .corner_radius(CornerRadius::same(14))
                        .stroke(Stroke::new(1.0, p.border))
                        .inner_margin(Margin::same(16))
                        .show(ui, |ui| {
                            ui.set_width(260.0);
                            let icon = if vol.is_removable { "💾" } else { "📱" };
                            ui.label(
                                RichText::new(format!("{icon}  {}", vol.name))
                                    .size(16.0)
                                    .strong()
                                    .color(p.text_primary),
                            );
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(vol.path.to_string_lossy())
                                    .size(11.0)
                                    .color(p.text_muted),
                            );
                            ui.add_space(10.0);
                            ui.add(
                                egui::ProgressBar::new(vol.used_fraction())
                                    .desired_height(8.0)
                                    .fill(p.accent),
                            );
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(format!("Belegt: {}", format_size(vol.used_bytes())))
                                        .size(12.0)
                                        .color(p.text_secondary),
                                );
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    ui.label(
                                        RichText::new(format!("Frei: {}", format_size(vol.free_bytes)))
                                            .size(12.0)
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
                }
            });

            ui.add_space(20.0);
            ui.label(RichText::new("KATEGORIEN").size(12.0).strong().color(p.text_muted));
            ui.add_space(8.0);

            let cats = [
                ("🖼", "Bilder", FileCategory::Image),
                ("🎬", "Videos", FileCategory::Video),
                ("🎵", "Audio", FileCategory::Audio),
                ("📑", "Dokumente", FileCategory::Document),
                ("📦", "Archive (ZIP)", FileCategory::Archive),
                ("🤖", "APKs", FileCategory::Apk),
            ];
            ui.horizontal_wrapped(|ui| {
                for (icon, title, cat) in cats {
                    let accent = Palette::category_color(cat);
                    let resp = Frame::NONE
                        .fill(p.bg_card)
                        .corner_radius(CornerRadius::same(14))
                        .stroke(Stroke::new(1.0, p.border))
                        .inner_margin(Margin::same(14))
                        .show(ui, |ui| {
                            ui.set_width(150.0);
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(icon).size(24.0).color(accent));
                                ui.add_space(6.0);
                                ui.vertical(|ui| {
                                    ui.label(
                                        RichText::new(title).strong().color(p.text_primary),
                                    );
                                    ui.label(
                                        RichText::new("Durchsuchen").size(11.0).color(p.text_muted),
                                    );
                                });
                            });
                        })
                        .response
                        .interact(Sense::click());
                    if resp.clicked() {
                        nav_target = Some(Location::Special(SpecialView::Category(cat)));
                    }
                }
            });
        });

        if let Some(target) = nav_target {
            self.navigate(pane_idx, target);
        }
    }

    fn draw_trash_view(&mut self, ui: &mut egui::Ui, _pane_idx: usize, p: &Palette) {
        let mut restore_item: Option<TrashItem> = None;
        let mut delete_item: Option<TrashItem> = None;

        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("{} Element(e) im Papierkorb", self.trash_items.len()))
                    .strong()
                    .color(p.text_primary),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if !self.trash_items.is_empty()
                    && ui
                        .button(RichText::new("🗑 Papierkorb leeren").color(p.danger))
                        .clicked()
                {
                    self.dialog = Some(DialogState::ConfirmEmptyTrash);
                }
            });
        });
        ui.add_space(8.0);

        if self.trash_items.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new("Der Papierkorb ist leer.").color(p.text_muted));
            });
            return;
        }

        let items = self.trash_items.clone();
        ScrollArea::vertical().show(ui, |ui| {
            for item in items {
                Frame::NONE
                    .fill(p.bg_card)
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(if item.is_dir { "📁" } else { "📄" });
                            ui.vertical(|ui| {
                                ui.label(RichText::new(&item.original_name).strong().color(p.text_primary));
                                ui.label(
                                    RichText::new(format!(
                                        "Ursprung: {} • {} • {}",
                                        item.original_path.display(),
                                        format_size(item.size),
                                        format_timestamp(item.deleted_at_secs)
                                    ))
                                    .size(11.0)
                                    .color(p.text_muted),
                                );
                            });
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.button(RichText::new("Löschen").color(p.danger)).clicked() {
                                    delete_item = Some(item.clone());
                                }
                                if ui.button("↩ Wiederherstellen").clicked() {
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
                ui.label(RichText::new(err).color(p.danger));
            });
            return;
        }

        if self.panes[pane_idx].is_loading {
            ui.centered_and_justified(|ui| {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Lade Dateien…");
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
                ui.label(RichText::new("Keine Dateien in diesem Ordner.").color(p.text_muted));
            });
            return;
        }

        let mut open_target: Option<FileEntry> = None;
        let mut toggle_select: Option<PathBuf> = None;
        let mut set_active = false;
        let mut context_action: Option<(String, FileEntry)> = None;

        ScrollArea::vertical()
            .id_salt(format!("pane_scroll_{pane_idx}"))
            .show(ui, |ui| match self.prefs.view_mode {
                ViewMode::List => {
                    for entry in &entries {
                        let is_sel = self.panes[pane_idx].selected.contains(&entry.path);
                        let fill = if is_sel { p.bg_selected } else { p.bg_card };
                        let stroke = Stroke::new(1.0, if is_sel { p.accent } else { Color32::TRANSPARENT });

                        let resp = Frame::NONE
                            .fill(fill)
                            .corner_radius(CornerRadius::same(10))
                            .stroke(stroke)
                            .inner_margin(Margin::symmetric(10, 8))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    // Selection checkbox
                                    let mut checked = is_sel;
                                    if ui.checkbox(&mut checked, "").changed() {
                                        toggle_select = Some(entry.path.clone());
                                        set_active = true;
                                    }

                                    // Colored category badge
                                    let cat_color = Palette::category_color(entry.category);
                                    Frame::NONE
                                        .fill(cat_color.gamma_multiply(0.18))
                                        .corner_radius(CornerRadius::same(8))
                                        .inner_margin(Margin::symmetric(8, 4))
                                        .show(ui, |ui| {
                                            ui.label(
                                                RichText::new(entry.category.badge())
                                                    .size(11.0)
                                                    .strong()
                                                    .color(cat_color),
                                            );
                                        });

                                    ui.add_space(4.0);

                                    // Name & metadata
                                    ui.vertical(|ui| {
                                        ui.label(
                                            RichText::new(&entry.name)
                                                .size(14.5)
                                                .strong()
                                                .color(p.text_primary),
                                        );
                                        let sub = if entry.is_dir {
                                            format!(
                                                "{} Elemente • {}",
                                                entry.child_count.unwrap_or(0),
                                                format_timestamp(entry.modified_secs)
                                            )
                                        } else {
                                            format!(
                                                "{} • {}",
                                                format_size(entry.size),
                                                format_timestamp(entry.modified_secs)
                                            )
                                        };
                                        ui.label(RichText::new(sub).size(11.5).color(p.text_secondary));
                                    });

                                    // Quick right-side menu button
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        ui.menu_button("⋮", |ui| {
                                            if ui.button("Öffnen").clicked() {
                                                context_action = Some(("open".into(), entry.clone()));
                                                ui.close();
                                            }
                                            if !entry.is_dir && ui.button("Mit anderer App öffnen").clicked() {
                                                context_action = Some(("open_ext".into(), entry.clone()));
                                                ui.close();
                                            }
                                            if ui.button("Kopieren").clicked() {
                                                context_action = Some(("copy".into(), entry.clone()));
                                                ui.close();
                                            }
                                            if ui.button("Ausschneiden").clicked() {
                                                context_action = Some(("cut".into(), entry.clone()));
                                                ui.close();
                                            }
                                            if ui.button("Umbenennen").clicked() {
                                                context_action = Some(("rename".into(), entry.clone()));
                                                ui.close();
                                            }
                                            if entry.is_zip() && ui.button("ZIP entpacken…").clicked() {
                                                context_action = Some(("unzip".into(), entry.clone()));
                                                ui.close();
                                            }
                                            if ui.button("In ZIP komprimieren…").clicked() {
                                                context_action = Some(("zip".into(), entry.clone()));
                                                ui.close();
                                            }
                                            if !entry.is_dir && ui.button("Teilen").clicked() {
                                                context_action = Some(("share".into(), entry.clone()));
                                                ui.close();
                                            }
                                            ui.separator();
                                            if ui.button(RichText::new("Löschen").color(p.danger)).clicked() {
                                                context_action = Some(("delete".into(), entry.clone()));
                                                ui.close();
                                            }
                                        });
                                    });
                                });
                            })
                            .response
                            .interact(Sense::click());

                        if resp.clicked() {
                            set_active = true;
                            if !self.panes[pane_idx].selected.is_empty() {
                                toggle_select = Some(entry.path.clone());
                            } else {
                                open_target = Some(entry.clone());
                            }
                        } else if resp.secondary_clicked() {
                            set_active = true;
                            toggle_select = Some(entry.path.clone());
                        }
                        ui.add_space(4.0);
                    }
                }
                ViewMode::Grid => {
                    let avail_w = ui.available_width();
                    let tile_w = 152.0;
                    let cols = ((avail_w / (tile_w + 10.0)).floor() as usize).max(2);

                    for chunk in entries.chunks(cols) {
                        ui.horizontal(|ui| {
                            for entry in chunk {
                                let is_sel = self.panes[pane_idx].selected.contains(&entry.path);
                                let cat_color = Palette::category_color(entry.category);
                                let resp = Frame::NONE
                                    .fill(if is_sel { p.bg_selected } else { p.bg_card })
                                    .corner_radius(CornerRadius::same(12))
                                    .stroke(Stroke::new(
                                        1.0,
                                        if is_sel { p.accent } else { p.border },
                                    ))
                                    .inner_margin(Margin::same(10))
                                    .show(ui, |ui| {
                                        ui.set_width(tile_w - 20.0);
                                        ui.set_height(110.0);
                                        ui.horizontal(|ui| {
                                            let mut checked = is_sel;
                                            if ui.checkbox(&mut checked, "").changed() {
                                                toggle_select = Some(entry.path.clone());
                                                set_active = true;
                                            }
                                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                                ui.label(
                                                    RichText::new(entry.category.badge())
                                                        .size(11.0)
                                                        .strong()
                                                        .color(cat_color),
                                                );
                                            });
                                        });
                                        ui.add_space(8.0);
                                        ui.label(
                                            RichText::new(&entry.name)
                                                .size(13.0)
                                                .strong()
                                                .color(p.text_primary),
                                        );
                                        let sub = if entry.is_dir {
                                            format!("{} Elem.", entry.child_count.unwrap_or(0))
                                        } else {
                                            format_size(entry.size)
                                        };
                                        ui.label(RichText::new(sub).size(11.0).color(p.text_muted));
                                    })
                                    .response
                                    .interact(Sense::click());

                                if resp.clicked() {
                                    set_active = true;
                                    if !self.panes[pane_idx].selected.is_empty() {
                                        toggle_select = Some(entry.path.clone());
                                    } else {
                                        open_target = Some(entry.clone());
                                    }
                                } else if resp.secondary_clicked() {
                                    set_active = true;
                                    toggle_select = Some(entry.path.clone());
                                }
                            }
                        });
                        ui.add_space(6.0);
                    }
                }
            });

        if set_active {
            self.active_pane = pane_idx;
        }
        if let Some(path) = toggle_select {
            if !self.panes[pane_idx].selected.remove(&path) {
                self.panes[pane_idx].selected.insert(path);
            }
        }
        if let Some(entry) = open_target {
            self.open_entry(pane_idx, &entry, ctx);
        }
        if let Some((action, entry)) = context_action {
            self.active_pane = pane_idx;
            match action.as_str() {
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
                    self.show_toast("Zum Kopieren gemerkt", false);
                }
                "cut" => {
                    self.clipboard = ClipboardState {
                        paths: vec![entry.path],
                        action: Some(ClipboardAction::Move),
                    };
                    self.show_toast("Zum Verschieben gemerkt", false);
                }
                "rename" => {
                    self.dialog = Some(DialogState::Rename {
                        pane: pane_idx,
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
    }

    fn draw_inspector_panel(&mut self, ui: &mut egui::Ui, p: &Palette) {
        ui.label(RichText::new("INSPEKTOR").size(12.0).strong().color(p.text_muted));
        ui.add_space(8.0);

        let selected = self.panes[self.active_pane].selected_entries();
        if selected.is_empty() {
            ui.label(
                RichText::new("Wähle eine Datei oder einen Ordner aus, um Details, Größe und SHA-256 zu sehen.")
                    .color(p.text_secondary),
            );
            return;
        }

        if selected.len() > 1 {
            let total_size: u64 = selected.iter().map(|e| e.size).sum();
            ui.label(RichText::new(format!("{} Elemente ausgewählt", selected.len())).strong());
            ui.label(format!("Gesamtgröße (Dateien): {}", format_size(total_size)));
            return;
        }

        let item = selected[0].clone();
        let cat_color = Palette::category_color(item.category);

        Frame::NONE
            .fill(p.bg_card)
            .corner_radius(CornerRadius::same(12))
            .inner_margin(Margin::same(12))
            .show(ui, |ui| {
                ui.label(RichText::new(item.category.badge()).strong().color(cat_color));
                ui.add_space(4.0);
                ui.label(RichText::new(&item.name).size(16.0).strong().color(p.text_primary));
                ui.add_space(8.0);
                ui.label(RichText::new("Pfad:").size(11.0).color(p.text_muted));
                ui.label(RichText::new(item.path.to_string_lossy()).size(11.5).color(p.text_secondary));
                ui.add_space(6.0);
                ui.label(format!(
                    "Typ: {}",
                    if item.is_dir { "Ordner" } else { item.category.label() }
                ));
                if !item.is_dir {
                    ui.label(format!("Größe: {}", format_size(item.size)));
                }
                ui.label(format!("Geändert: {}", format_timestamp(item.modified_secs)));
                ui.label(format!(
                    "Schreibschutz: {}",
                    if item.readonly { "Ja" } else { "Nein" }
                ));
            });

        ui.add_space(10.0);

        // Extra async info (SHA-256 or Folder Size)
        let matching_extra = self
            .inspector_extra
            .as_ref()
            .filter(|e| e.path == item.path);

        if item.is_dir {
            if let Some(extra) = matching_extra {
                if let Some((bytes, files)) = extra.folder_stats {
                    ui.label(
                        RichText::new(format!(
                            "Ordnergröße: {} ({files} Dateien)",
                            format_size(bytes)
                        ))
                        .color(p.cyan),
                    );
                }
            }
            if ui.button("📊 Ordnergröße berechnen").clicked() {
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
                    ui.label(RichText::new("SHA-256:").size(11.0).color(p.text_muted));
                    ui.label(RichText::new(hash).monospace().size(10.5).color(p.cyan));
                }
            }
            if ui.button("🔐 SHA-256 berechnen").clicked() {
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
            })
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .min_width(380.0)
            .show(ctx, |ui| match dialog {
                DialogState::NewFolder { pane, name } => {
                    ui.label("Ordnername:");
                    ui.text_edit_singleline(name);
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if ui.button("Abbrechen").clicked() {
                            close = true;
                        }
                        if ui.button(RichText::new("Erstellen").strong()).clicked() {
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
                    });
                }
                DialogState::NewFile { pane, name } => {
                    ui.label("Dateiname:");
                    ui.text_edit_singleline(name);
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if ui.button("Abbrechen").clicked() {
                            close = true;
                        }
                        if ui.button(RichText::new("Erstellen").strong()).clicked() {
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
                    });
                }
                DialogState::Rename {
                    pane: _,
                    target,
                    new_name,
                } => {
                    ui.label("Neuer Name:");
                    ui.text_edit_singleline(new_name);
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if ui.button("Abbrechen").clicked() {
                            close = true;
                        }
                        if ui.button(RichText::new("Umbenennen").strong()).clicked() {
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
                    });
                }
                DialogState::CompressZip {
                    pane,
                    sources,
                    archive_name,
                } => {
                    ui.label(format!("{} Element(e) in ZIP-Archiv packen:", sources.len()));
                    ui.text_edit_singleline(archive_name);
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if ui.button("Abbrechen").clicked() {
                            close = true;
                        }
                        if ui.button(RichText::new("📦 Komprimieren").strong()).clicked() {
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
                    });
                }
                DialogState::ExtractZip {
                    pane,
                    zip_path,
                    subfolder_name,
                    extract_into_subfolder,
                } => {
                    ui.label(format!(
                        "Archiv: {}",
                        zip_path.file_name().unwrap_or_default().to_string_lossy()
                    ));
                    ui.checkbox(extract_into_subfolder, "In neuen Unterordner entpacken");
                    if *extract_into_subfolder {
                        ui.text_edit_singleline(subfolder_name);
                    }
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if ui.button("Abbrechen").clicked() {
                            close = true;
                        }
                        if ui.button(RichText::new("📂 Entpacken").strong()).clicked() {
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
                    });
                }
                DialogState::ConfirmDelete {
                    pane,
                    targets,
                    permanent,
                } => {
                    ui.label(format!("{} Element(e) löschen?", targets.len()));
                    ui.checkbox(permanent, "Endgültig löschen (Papierkorb überspringen)");
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if ui.button("Abbrechen").clicked() {
                            close = true;
                        }
                        let btn_text = if *permanent {
                            RichText::new("Endgültig löschen").color(p.danger).strong()
                        } else {
                            RichText::new("In Papierkorb").strong()
                        };
                        if ui.button(btn_text).clicked() {
                            let pane_i = *pane;
                            let t = targets.clone();
                            let perm = *permanent;
                            close = true;
                            action = Some(Box::new(move |app| {
                                app.panes[pane_i].selected.clear();
                                app.start_delete(t, perm);
                            }));
                        }
                    });
                }
                DialogState::ConfirmEmptyTrash => {
                    ui.label("Alle Elemente im Papierkorb unwiderruflich löschen?");
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if ui.button("Abbrechen").clicked() {
                            close = true;
                        }
                        if ui
                            .button(RichText::new("Leeren").color(p.danger).strong())
                            .clicked()
                        {
                            close = true;
                            action = Some(Box::new(|app| match trash::empty_trash(&app.primary_root) {
                                Ok(_) => {
                                    app.show_toast("Papierkorb geleert", false);
                                    app.refresh_all_panes();
                                }
                                Err(e) => app.show_toast(e, true),
                            }));
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

            let max_size = ctx.content_rect().size() * 0.85;
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
                            if !*truncated
                                && ui
                                    .add_enabled(*dirty, egui::Button::new("💾 Speichern"))
                                    .clicked()
                            {
                                save_text = Some((path.clone(), content.clone()));
                                *dirty = false;
                            }
                            if ui.button("↗ Mit anderer App öffnen").clicked() {
                                open_external = Some(path.clone());
                            }
                            if *truncated {
                                ui.label(
                                    RichText::new("Nur Vorschau (Datei > 512 KB)")
                                        .color(p.warning),
                                );
                            }
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.button("Schließen").clicked() {
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
                                ui.label(format!("{w} × {h} px"));
                            }
                            if ui.button("−").clicked() {
                                *zoom = (*zoom * 0.8).clamp(0.2, 5.0);
                            }
                            ui.label(format!("{:.0}%", *zoom * 100.0));
                            if ui.button("+").clicked() {
                                *zoom = (*zoom * 1.25).clamp(0.2, 5.0);
                            }
                            if ui.button("100%").clicked() {
                                *zoom = 1.0;
                            }
                            if ui.button("↗ Externe App").clicked() {
                                open_external = Some(path.clone());
                            }
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.button("Schließen").clicked() {
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
                            ui.label(format!("{} Einträge im ZIP-Archiv", entries.len()));
                            if ui.button("📂 In Ordner entpacken").clicked() {
                                extract_zip_req = Some((path.clone(), true));
                                close = true;
                            }
                            if ui.button("📂 Hier entpacken").clicked() {
                                extract_zip_req = Some((path.clone(), false));
                                close = true;
                            }
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.button("Schließen").clicked() {
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
                                        ui.label(if e.is_dir { "📁" } else { "📄" });
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
            .min_width(360.0)
            .show(ctx, |ui| {
                ui.label(RichText::new(&snap.current_item).color(p.text_secondary));
                ui.add_space(8.0);
                ui.add(
                    egui::ProgressBar::new(snap.fraction())
                        .show_percentage()
                        .fill(p.accent),
                );
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("{} / {}", snap.completed, snap.total))
                            .size(12.0)
                            .color(p.text_muted),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button(RichText::new("Abbrechen").color(p.danger)).clicked() {
                            job.cancel();
                        }
                    });
                });
            });
    }

    fn draw_toast(&self, ctx: &egui::Context, p: &Palette) {
        let Some(toast) = &self.toast else {
            return;
        };
        egui::Area::new(egui::Id::NULL.with("toast_overlay"))
            .anchor(egui::Align2::RIGHT_BOTTOM, Vec2::new(-20.0, -20.0))
            .show(ctx, |ui| {
                Frame::NONE
                    .fill(if toast.is_error { p.danger } else { p.bg_card_hover })
                    .corner_radius(CornerRadius::same(10))
                    .stroke(Stroke::new(1.0, if toast.is_error { p.danger } else { p.accent }))
                    .inner_margin(Margin::symmetric(16, 10))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(&toast.message)
                                .strong()
                                .color(Color32::WHITE),
                        );
                    });
            });
    }
}

impl eframe::App for TabManagerApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_async(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.draw_ui(ui);
    }
}
