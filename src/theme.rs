use crate::model::{FileCategory, ThemeMode};
use egui::{Color32, CornerRadius, Margin, Stroke, Style, Visuals};

#[derive(Clone, Copy)]
pub struct Palette {
    pub bg_canvas: Color32,
    pub bg_sidebar: Color32,
    pub bg_panel: Color32,
    pub bg_card: Color32,
    pub bg_card_hover: Color32,
    pub bg_selected: Color32,
    pub accent: Color32,
    pub accent_soft: Color32,
    pub cyan: Color32,
    pub text_primary: Color32,
    pub text_secondary: Color32,
    pub text_muted: Color32,
    pub border: Color32,
    pub danger: Color32,
    pub success: Color32,
    pub warning: Color32,
}

impl Palette {
    pub fn for_mode(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Dark => Self {
                // True AMOLED deep obsidian palette for Galaxy Tab S12 Ultra
                bg_canvas: Color32::from_rgb(10, 14, 20),
                bg_sidebar: Color32::from_rgb(15, 20, 29),
                bg_panel: Color32::from_rgb(18, 24, 36),
                bg_card: Color32::from_rgb(24, 32, 47),
                bg_card_hover: Color32::from_rgb(33, 44, 64),
                bg_selected: Color32::from_rgb(44, 53, 98),
                accent: Color32::from_rgb(99, 102, 241),       // Indigo 500
                accent_soft: Color32::from_rgb(49, 46, 129),   // Indigo 900
                cyan: Color32::from_rgb(56, 189, 248),         // Sky 400
                text_primary: Color32::from_rgb(241, 245, 249),
                text_secondary: Color32::from_rgb(148, 163, 184),
                text_muted: Color32::from_rgb(100, 116, 139),
                border: Color32::from_rgb(38, 49, 70),
                danger: Color32::from_rgb(239, 68, 68),
                success: Color32::from_rgb(34, 197, 94),
                warning: Color32::from_rgb(245, 158, 11),
            },
            ThemeMode::Light => Self {
                bg_canvas: Color32::from_rgb(244, 247, 251),
                bg_sidebar: Color32::from_rgb(255, 255, 255),
                bg_panel: Color32::from_rgb(250, 251, 254),
                bg_card: Color32::from_rgb(255, 255, 255),
                bg_card_hover: Color32::from_rgb(238, 242, 255),
                bg_selected: Color32::from_rgb(224, 231, 255),
                accent: Color32::from_rgb(79, 70, 229),
                accent_soft: Color32::from_rgb(224, 231, 255),
                cyan: Color32::from_rgb(2, 132, 199),
                text_primary: Color32::from_rgb(15, 23, 42),
                text_secondary: Color32::from_rgb(71, 85, 105),
                text_muted: Color32::from_rgb(148, 163, 184),
                border: Color32::from_rgb(226, 232, 240),
                danger: Color32::from_rgb(220, 38, 38),
                success: Color32::from_rgb(22, 163, 74),
                warning: Color32::from_rgb(217, 119, 6),
            },
        }
    }

    pub fn category_color(cat: FileCategory) -> Color32 {
        match cat {
            FileCategory::Folder => Color32::from_rgb(251, 191, 36),   // Amber
            FileCategory::Image => Color32::from_rgb(56, 189, 248),    // Sky
            FileCategory::Video => Color32::from_rgb(244, 114, 182),   // Pink
            FileCategory::Audio => Color32::from_rgb(168, 85, 247),    // Purple
            FileCategory::Document => Color32::from_rgb(59, 130, 246), // Blue
            FileCategory::Archive => Color32::from_rgb(249, 115, 22),  // Orange
            FileCategory::TextCode => Color32::from_rgb(45, 212, 191), // Teal
            FileCategory::Apk => Color32::from_rgb(34, 197, 94),       // Green
            FileCategory::Other => Color32::from_rgb(148, 163, 184),   // Slate
        }
    }
}

pub fn apply_theme(ctx: &egui::Context, mode: ThemeMode) -> Palette {
    let p = Palette::for_mode(mode);
    let mut visuals = match mode {
        ThemeMode::Dark => Visuals::dark(),
        ThemeMode::Light => Visuals::light(),
    };

    visuals.panel_fill = p.bg_canvas;
    visuals.window_fill = p.bg_panel;
    visuals.faint_bg_color = p.bg_card;
    visuals.extreme_bg_color = p.bg_sidebar;
    visuals.selection.bg_fill = p.accent;
    visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    visuals.hyperlink_color = p.cyan;

    visuals.widgets.noninteractive.bg_fill = p.bg_card;
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.text_primary);
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    visuals.widgets.noninteractive.corner_radius = CornerRadius::same(10);

    visuals.widgets.inactive.bg_fill = p.bg_card;
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, p.text_primary);
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, p.border);
    visuals.widgets.inactive.corner_radius = CornerRadius::same(10);

    visuals.widgets.hovered.bg_fill = p.bg_card_hover;
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, p.text_primary);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, p.accent);
    visuals.widgets.hovered.corner_radius = CornerRadius::same(10);

    visuals.widgets.active.bg_fill = p.accent;
    visuals.widgets.active.fg_stroke = Stroke::new(1.5, Color32::WHITE);
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, p.accent);
    visuals.widgets.active.corner_radius = CornerRadius::same(10);

    visuals.window_corner_radius = CornerRadius::same(14);
    visuals.window_stroke = Stroke::new(1.0, p.border);

    let mut style = Style::default();
    style.visuals = visuals;
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 8.0);
    style.spacing.interact_size = egui::vec2(40.0, 36.0); // Comfortable touch targets for Galaxy Tab
    style.spacing.window_margin = Margin::same(16);

    let egui_theme = match mode {
        ThemeMode::Dark => egui::Theme::Dark,
        ThemeMode::Light => egui::Theme::Light,
    };
    ctx.set_theme(egui_theme);
    ctx.set_style_of(egui_theme, style);
    p
}
