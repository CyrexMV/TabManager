use crate::model::{FileCategory, ThemeMode};
use egui::{Color32, CornerRadius, FontData, FontDefinitions, FontFamily, Margin, Shadow, Stroke, Style, Visuals};
use std::sync::Arc;

#[derive(Clone, Copy)]
pub struct Palette {
    pub bg_canvas: Color32,
    pub bg_sidebar: Color32,
    pub bg_header: Color32,
    pub bg_surface: Color32,
    pub bg_surface_alt: Color32,
    pub bg_hover: Color32,
    pub bg_selected: Color32,
    pub bg_floating: Color32,
    pub accent: Color32,
    pub accent_hover: Color32,
    pub accent_soft: Color32,
    pub cyan: Color32,
    pub text_primary: Color32,
    pub text_secondary: Color32,
    pub text_muted: Color32,
    pub divider: Color32,
    pub danger: Color32,
    pub danger_soft: Color32,
    pub success: Color32,
    pub warning: Color32,
}

impl Palette {
    pub fn for_mode(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Dark => Self {
                // Sleek One UI 7 / macOS Finder Dark AMOLED palette
                bg_canvas: Color32::from_rgb(11, 14, 20),
                bg_sidebar: Color32::from_rgb(15, 19, 27),
                bg_header: Color32::from_rgb(14, 18, 26),
                bg_surface: Color32::from_rgb(20, 25, 36),
                bg_surface_alt: Color32::from_rgb(16, 20, 29),
                bg_hover: Color32::from_rgb(29, 36, 51),
                bg_selected: Color32::from_rgb(35, 45, 78),
                bg_floating: Color32::from_rgb(24, 30, 44),
                accent: Color32::from_rgb(91, 108, 249),       // Modern Vivid Indigo
                accent_hover: Color32::from_rgb(112, 127, 255),
                accent_soft: Color32::from_rgba_unmultiplied(91, 108, 249, 44),
                cyan: Color32::from_rgb(56, 189, 248),
                text_primary: Color32::from_rgb(241, 245, 249),
                text_secondary: Color32::from_rgb(156, 169, 191),
                text_muted: Color32::from_rgb(102, 116, 138),
                divider: Color32::from_rgba_unmultiplied(255, 255, 255, 16),
                danger: Color32::from_rgb(248, 113, 113),
                danger_soft: Color32::from_rgba_unmultiplied(239, 68, 68, 36),
                success: Color32::from_rgb(52, 211, 153),
                warning: Color32::from_rgb(251, 191, 36),
            },
            ThemeMode::Light => Self {
                // Crisp modern macOS Finder / One UI Light palette
                bg_canvas: Color32::from_rgb(246, 248, 252),
                bg_sidebar: Color32::from_rgb(255, 255, 255),
                bg_header: Color32::from_rgb(255, 255, 255),
                bg_surface: Color32::from_rgb(255, 255, 255),
                bg_surface_alt: Color32::from_rgb(248, 250, 253),
                bg_hover: Color32::from_rgb(239, 243, 250),
                bg_selected: Color32::from_rgb(228, 234, 254),
                bg_floating: Color32::from_rgb(255, 255, 255),
                accent: Color32::from_rgb(79, 70, 229),
                accent_hover: Color32::from_rgb(67, 56, 202),
                accent_soft: Color32::from_rgba_unmultiplied(79, 70, 229, 26),
                cyan: Color32::from_rgb(2, 132, 199),
                text_primary: Color32::from_rgb(15, 23, 42),
                text_secondary: Color32::from_rgb(71, 85, 105),
                text_muted: Color32::from_rgb(138, 152, 173),
                divider: Color32::from_rgba_unmultiplied(15, 23, 42, 18),
                danger: Color32::from_rgb(220, 38, 38),
                danger_soft: Color32::from_rgba_unmultiplied(220, 38, 38, 24),
                success: Color32::from_rgb(22, 163, 74),
                warning: Color32::from_rgb(217, 119, 6),
            },
        }
    }

    pub fn category_color(cat: FileCategory) -> Color32 {
        match cat {
            FileCategory::Folder => Color32::from_rgb(245, 180, 52),   // Warm Gold Folder
            FileCategory::Image => Color32::from_rgb(56, 189, 248),    // Sky Blue
            FileCategory::Video => Color32::from_rgb(244, 114, 182),   // Rose
            FileCategory::Audio => Color32::from_rgb(168, 85, 247),    // Violet
            FileCategory::Document => Color32::from_rgb(59, 130, 246), // Cobalt
            FileCategory::Archive => Color32::from_rgb(251, 146, 60),  // Amber Orange
            FileCategory::TextCode => Color32::from_rgb(45, 212, 191), // Teal
            FileCategory::Apk => Color32::from_rgb(52, 211, 153),      // Emerald
            FileCategory::Other => Color32::from_rgb(148, 163, 184),   // Cool Slate
        }
    }
}

/// Load crisp system sans-serif (Roboto on Android, DejaVu/Ubuntu fallback on Linux)
/// so typography looks like a native One UI / Material 3 app instead of thin default egui text.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let candidates = [
        "/system/fonts/Roboto-Regular.ttf",
        "/system/fonts/NotoSans-Regular.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    ];
    for path in candidates {
        if let Ok(bytes) = std::fs::read(path) {
            fonts.font_data.insert(
                "system_sans".to_owned(),
                Arc::new(FontData::from_owned(bytes)),
            );
            fonts
                .families
                .entry(FontFamily::Proportional)
                .or_default()
                .insert(0, "system_sans".to_owned());
            break;
        }
    }
    ctx.set_fonts(fonts);
}

pub fn apply_theme(ctx: &egui::Context, mode: ThemeMode) -> Palette {
    let p = Palette::for_mode(mode);
    let mut visuals = match mode {
        ThemeMode::Dark => Visuals::dark(),
        ThemeMode::Light => Visuals::light(),
    };

    visuals.panel_fill = p.bg_canvas;
    visuals.window_fill = p.bg_floating;
    visuals.faint_bg_color = p.bg_surface_alt;
    visuals.extreme_bg_color = p.bg_surface;
    visuals.selection.bg_fill = p.accent;
    visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    visuals.hyperlink_color = p.cyan;

    // Flat, clean borderless buttons by default — no clunky 2005 boxes!
    visuals.widgets.noninteractive.bg_fill = Color32::TRANSPARENT;
    visuals.widgets.noninteractive.weak_bg_fill = Color32::TRANSPARENT;
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.text_primary);
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.divider);
    visuals.widgets.noninteractive.corner_radius = CornerRadius::same(10);

    visuals.widgets.inactive.bg_fill = p.bg_surface;
    visuals.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, p.text_primary);
    visuals.widgets.inactive.bg_stroke = Stroke::NONE;
    visuals.widgets.inactive.corner_radius = CornerRadius::same(10);

    visuals.widgets.hovered.bg_fill = p.bg_hover;
    visuals.widgets.hovered.weak_bg_fill = p.bg_hover;
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, p.text_primary);
    visuals.widgets.hovered.bg_stroke = Stroke::NONE;
    visuals.widgets.hovered.corner_radius = CornerRadius::same(10);

    visuals.widgets.active.bg_fill = p.accent;
    visuals.widgets.active.weak_bg_fill = p.accent;
    visuals.widgets.active.fg_stroke = Stroke::new(1.5, Color32::WHITE);
    visuals.widgets.active.bg_stroke = Stroke::NONE;
    visuals.widgets.active.corner_radius = CornerRadius::same(10);

    visuals.widgets.open.bg_fill = p.bg_hover;
    visuals.widgets.open.weak_bg_fill = p.bg_hover;
    visuals.widgets.open.fg_stroke = Stroke::new(1.0, p.text_primary);
    visuals.widgets.open.bg_stroke = Stroke::new(1.0, p.divider);
    visuals.widgets.open.corner_radius = CornerRadius::same(10);

    visuals.window_corner_radius = CornerRadius::same(18);
    visuals.window_stroke = Stroke::new(1.0, p.divider);
    visuals.window_shadow = Shadow {
        offset: [0, 10],
        blur: 28,
        spread: 0,
        color: Color32::from_black_alpha(110),
    };
    visuals.popup_shadow = Shadow {
        offset: [0, 6],
        blur: 18,
        spread: 0,
        color: Color32::from_black_alpha(95),
    };

    let mut style = Style::default();
    style.visuals = visuals;
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(12.0, 8.0);
    style.spacing.interact_size = egui::vec2(38.0, 36.0);
    style.spacing.window_margin = Margin::same(20);
    style.spacing.menu_margin = Margin::same(8);

    let egui_theme = match mode {
        ThemeMode::Dark => egui::Theme::Dark,
        ThemeMode::Light => egui::Theme::Light,
    };
    ctx.set_theme(egui_theme);
    ctx.set_style_of(egui_theme, style);
    p
}
