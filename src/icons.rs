use crate::model::FileCategory;
use egui::{pos2, vec2, Color32, CornerRadius, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Sidebar,
    Back,
    Forward,
    Up,
    Refresh,
    Search,
    Close,
    Plus,
    List,
    Grid,
    DualPane,
    Info,
    Sort,
    MoreVert,
    Sun,
    Moon,
    Home,
    Download,
    Folder,
    File,
    Image,
    Video,
    Audio,
    Document,
    Archive,
    Code,
    Apk,
    Storage,
    SdCard,
    Trash,
    Star,
    StarFilled,
    Copy,
    Cut,
    Paste,
    Rename,
    ZipAdd,
    Unzip,
    Share,
    Transfer,
    Check,
    Shield,
    Hash,
    Save,
    ExternalOpen,
    Restore,
    ZoomIn,
    ZoomOut,
}

impl Icon {
    pub fn for_category(cat: FileCategory) -> Self {
        match cat {
            FileCategory::Folder => Icon::Folder,
            FileCategory::Image => Icon::Image,
            FileCategory::Video => Icon::Video,
            FileCategory::Audio => Icon::Audio,
            FileCategory::Document => Icon::Document,
            FileCategory::Archive => Icon::Archive,
            FileCategory::TextCode => Icon::Code,
            FileCategory::Apk => Icon::Apk,
            FileCategory::Other => Icon::File,
        }
    }
}

/// Draw a crisp, resolution-independent vector icon at `rect` using `egui::Painter`.
pub fn paint_icon(painter: &egui::Painter, rect: Rect, icon: Icon, color: Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height());
    let u = s / 24.0; // 24x24 Lucide-like coordinate grid
    let p = |x: f32, y: f32| pos2(c.x + (x - 12.0) * u, c.y + (y - 12.0) * u);
    let sw = (1.8 * u).max(1.3);
    let stroke = Stroke::new(sw, color);

    match icon {
        Icon::Sidebar => {
            let r = Rect::from_min_max(p(3.0, 4.0), p(21.0, 20.0));
            painter.rect_stroke(r, CornerRadius::same((3.0 * u) as u8), stroke, egui::StrokeKind::Middle);
            painter.line_segment([p(9.0, 4.0), p(9.0, 20.0)], stroke);
        }
        Icon::Back => {
            painter.line_segment([p(15.0, 6.0), p(9.0, 12.0)], stroke);
            painter.line_segment([p(9.0, 12.0), p(15.0, 18.0)], stroke);
        }
        Icon::Forward => {
            painter.line_segment([p(9.0, 6.0), p(15.0, 12.0)], stroke);
            painter.line_segment([p(15.0, 12.0), p(9.0, 18.0)], stroke);
        }
        Icon::Up => {
            painter.line_segment([p(12.0, 18.0), p(12.0, 6.0)], stroke);
            painter.line_segment([p(7.0, 11.0), p(12.0, 6.0)], stroke);
            painter.line_segment([p(17.0, 11.0), p(12.0, 6.0)], stroke);
        }
        Icon::Refresh => {
            let r = 7.0 * u;
            let mut pts = Vec::with_capacity(20);
            for i in 0..=18 {
                let a = -0.6 + (i as f32 / 18.0) * std::f32::consts::TAU * 0.82;
                pts.push(pos2(c.x + a.cos() * r, c.y + a.sin() * r));
            }
            painter.add(egui::Shape::line(pts, stroke));
            // Arrow head
            painter.line_segment([p(18.0, 5.0), p(19.0, 9.5)], stroke);
            painter.line_segment([p(14.5, 9.0), p(19.0, 9.5)], stroke);
        }
        Icon::Search => {
            painter.circle_stroke(p(10.5, 10.5), 6.0 * u, stroke);
            painter.line_segment([p(15.0, 15.0), p(20.0, 20.0)], stroke);
        }
        Icon::Close => {
            painter.line_segment([p(6.5, 6.5), p(17.5, 17.5)], stroke);
            painter.line_segment([p(17.5, 6.5), p(6.5, 17.5)], stroke);
        }
        Icon::Plus => {
            painter.line_segment([p(12.0, 5.0), p(12.0, 19.0)], stroke);
            painter.line_segment([p(5.0, 12.0), p(19.0, 12.0)], stroke);
        }
        Icon::List => {
            for y in [7.0, 12.0, 17.0] {
                painter.circle_filled(p(5.0, y), 1.3 * u, color);
                painter.line_segment([p(9.0, y), p(20.0, y)], stroke);
            }
        }
        Icon::Grid => {
            let cr = CornerRadius::same((2.0 * u) as u8);
            for (x0, y0) in [(4.0, 4.0), (13.5, 4.0), (4.0, 13.5), (13.5, 13.5)] {
                let r = Rect::from_min_max(p(x0, y0), p(x0 + 6.5, y0 + 6.5));
                painter.rect_stroke(r, cr, stroke, egui::StrokeKind::Middle);
            }
        }
        Icon::DualPane => {
            let r = Rect::from_min_max(p(3.0, 4.0), p(21.0, 20.0));
            painter.rect_stroke(r, CornerRadius::same((3.0 * u) as u8), stroke, egui::StrokeKind::Middle);
            painter.line_segment([p(12.0, 4.0), p(12.0, 20.0)], stroke);
        }
        Icon::Info => {
            painter.circle_stroke(c, 8.5 * u, stroke);
            painter.circle_filled(p(12.0, 8.0), 1.2 * u, color);
            painter.line_segment([p(12.0, 11.2), p(12.0, 16.5)], stroke);
        }
        Icon::Sort => {
            painter.line_segment([p(8.0, 6.0), p(8.0, 18.0)], stroke);
            painter.line_segment([p(5.0, 15.0), p(8.0, 18.0)], stroke);
            painter.line_segment([p(11.0, 15.0), p(8.0, 18.0)], stroke);
            painter.line_segment([p(16.0, 18.0), p(16.0, 6.0)], stroke);
            painter.line_segment([p(13.0, 9.0), p(16.0, 6.0)], stroke);
            painter.line_segment([p(19.0, 9.0), p(16.0, 6.0)], stroke);
        }
        Icon::MoreVert => {
            for y in [6.0, 12.0, 18.0] {
                painter.circle_filled(p(12.0, y), 1.6 * u, color);
            }
        }
        Icon::Sun => {
            painter.circle_stroke(c, 4.2 * u, stroke);
            for i in 0..8 {
                let a = (i as f32) * std::f32::consts::FRAC_PI_4;
                let p0 = pos2(c.x + a.cos() * 6.8 * u, c.y + a.sin() * 6.8 * u);
                let p1 = pos2(c.x + a.cos() * 9.2 * u, c.y + a.sin() * 9.2 * u);
                painter.line_segment([p0, p1], stroke);
            }
        }
        Icon::Moon => {
            let mut pts = Vec::new();
            for i in 0..=16 {
                let a = 0.5 + (i as f32 / 16.0) * std::f32::consts::PI * 1.45;
                pts.push(pos2(c.x + a.cos() * 7.5 * u, c.y + a.sin() * 7.5 * u));
            }
            painter.add(egui::Shape::line(pts, stroke));
            painter.line_segment([p(14.5, 5.2), p(12.2, 11.5)], stroke);
            painter.line_segment([p(12.2, 11.5), p(18.2, 15.0)], stroke);
        }
        Icon::Home => {
            painter.line_segment([p(3.5, 11.5), p(12.0, 4.0)], stroke);
            painter.line_segment([p(12.0, 4.0), p(20.5, 11.5)], stroke);
            painter.line_segment([p(5.5, 10.0), p(5.5, 19.5)], stroke);
            painter.line_segment([p(18.5, 10.0), p(18.5, 19.5)], stroke);
            painter.line_segment([p(5.5, 19.5), p(18.5, 19.5)], stroke);
            let door = Rect::from_min_max(p(10.0, 14.0), p(14.0, 19.5));
            painter.rect_stroke(door, CornerRadius::same(1), stroke, egui::StrokeKind::Middle);
        }
        Icon::Download => {
            painter.line_segment([p(12.0, 4.0), p(12.0, 15.0)], stroke);
            painter.line_segment([p(7.5, 10.5), p(12.0, 15.0)], stroke);
            painter.line_segment([p(16.5, 10.5), p(12.0, 15.0)], stroke);
            painter.line_segment([p(4.5, 19.0), p(19.5, 19.0)], stroke);
        }
        Icon::Folder => {
            // Filled modern rounded folder
            let tab_rect = Rect::from_min_max(p(3.0, 5.0), p(11.0, 10.0));
            painter.rect_filled(tab_rect, CornerRadius::same((2.5 * u) as u8), color.gamma_multiply(0.85));
            let body = Rect::from_min_max(p(3.0, 8.0), p(21.0, 19.5));
            painter.rect_filled(body, CornerRadius::same((3.0 * u) as u8), color);
            // Subtle top highlight line
            painter.line_segment(
                [p(4.5, 10.2), p(19.5, 10.2)],
                Stroke::new((1.1 * u).max(1.0), Color32::from_white_alpha(65)),
            );
        }
        Icon::File => {
            let body = Rect::from_min_max(p(5.5, 3.5), p(18.5, 20.5));
            painter.rect_stroke(body, CornerRadius::same((2.5 * u) as u8), stroke, egui::StrokeKind::Middle);
            painter.line_segment([p(9.0, 10.0), p(15.0, 10.0)], stroke);
            painter.line_segment([p(9.0, 14.0), p(15.0, 14.0)], stroke);
        }
        Icon::Image => {
            let body = Rect::from_min_max(p(3.5, 4.5), p(20.5, 19.5));
            painter.rect_stroke(body, CornerRadius::same((3.0 * u) as u8), stroke, egui::StrokeKind::Middle);
            painter.circle_filled(p(8.5, 9.0), 1.7 * u, color);
            painter.line_segment([p(4.5, 17.5), p(10.0, 12.5)], stroke);
            painter.line_segment([p(10.0, 12.5), p(13.5, 15.5)], stroke);
            painter.line_segment([p(13.5, 15.5), p(16.5, 12.0)], stroke);
            painter.line_segment([p(16.5, 12.0), p(19.5, 16.5)], stroke);
        }
        Icon::Video => {
            let body = Rect::from_min_max(p(3.5, 5.0), p(20.5, 19.0));
            painter.rect_stroke(body, CornerRadius::same((3.0 * u) as u8), stroke, egui::StrokeKind::Middle);
            painter.add(egui::Shape::convex_polygon(
                vec![p(10.0, 8.8), p(16.2, 12.0), p(10.0, 15.2)],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Audio => {
            painter.line_segment([p(9.5, 16.5), p(9.5, 6.5)], stroke);
            painter.line_segment([p(9.5, 6.5), p(18.0, 5.0)], stroke);
            painter.line_segment([p(18.0, 5.0), p(18.0, 15.0)], stroke);
            painter.circle_filled(p(7.5, 17.0), 2.5 * u, color);
            painter.circle_filled(p(16.0, 15.5), 2.5 * u, color);
        }
        Icon::Document => {
            let body = Rect::from_min_max(p(5.0, 3.5), p(19.0, 20.5));
            painter.rect_stroke(body, CornerRadius::same((2.5 * u) as u8), stroke, egui::StrokeKind::Middle);
            painter.line_segment([p(8.5, 8.5), p(15.5, 8.5)], stroke);
            painter.line_segment([p(8.5, 12.0), p(15.5, 12.0)], stroke);
            painter.line_segment([p(8.5, 15.5), p(13.0, 15.5)], stroke);
        }
        Icon::Archive => {
            let body = Rect::from_min_max(p(4.0, 4.5), p(20.0, 19.5));
            painter.rect_stroke(body, CornerRadius::same((3.0 * u) as u8), stroke, egui::StrokeKind::Middle);
            painter.line_segment([p(4.0, 9.5), p(20.0, 9.5)], stroke);
            let clasp = Rect::from_min_max(p(10.0, 12.5), p(14.0, 16.0));
            painter.rect_filled(clasp, CornerRadius::same((1.5 * u) as u8), color);
        }
        Icon::Code => {
            painter.line_segment([p(8.5, 7.5), p(4.0, 12.0)], stroke);
            painter.line_segment([p(4.0, 12.0), p(8.5, 16.5)], stroke);
            painter.line_segment([p(15.5, 7.5), p(20.0, 12.0)], stroke);
            painter.line_segment([p(20.0, 12.0), p(15.5, 16.5)], stroke);
            painter.line_segment([p(13.2, 6.0), p(10.8, 18.0)], stroke);
        }
        Icon::Apk => {
            let body = Rect::from_min_max(p(4.5, 5.5), p(19.5, 19.5));
            painter.rect_stroke(body, CornerRadius::same((4.0 * u) as u8), stroke, egui::StrokeKind::Middle);
            painter.circle_filled(p(9.0, 10.5), 1.3 * u, color);
            painter.circle_filled(p(15.0, 10.5), 1.3 * u, color);
            painter.line_segment([p(8.5, 15.0), p(15.5, 15.0)], stroke);
        }
        Icon::Storage => {
            let body = Rect::from_min_max(p(3.5, 6.0), p(20.5, 18.0));
            painter.rect_stroke(body, CornerRadius::same((3.0 * u) as u8), stroke, egui::StrokeKind::Middle);
            painter.line_segment([p(3.5, 13.0), p(20.5, 13.0)], stroke);
            painter.circle_filled(p(7.5, 15.5), 1.2 * u, color);
            painter.circle_filled(p(11.0, 15.5), 1.2 * u, color);
        }
        Icon::SdCard => {
            let body = Rect::from_min_max(p(6.0, 3.5), p(18.0, 20.5));
            painter.rect_stroke(body, CornerRadius::same((2.5 * u) as u8), stroke, egui::StrokeKind::Middle);
            for x in [9.0, 12.0, 15.0] {
                painter.line_segment([p(x, 6.0), p(x, 9.5)], stroke);
            }
        }
        Icon::Trash => {
            painter.line_segment([p(4.5, 6.5), p(19.5, 6.5)], stroke);
            painter.line_segment([p(9.5, 4.0), p(14.5, 4.0)], stroke);
            let bin = Rect::from_min_max(p(6.5, 6.5), p(17.5, 20.0));
            painter.rect_stroke(bin, CornerRadius::same((2.0 * u) as u8), stroke, egui::StrokeKind::Middle);
            painter.line_segment([p(10.0, 10.0), p(10.0, 16.5)], stroke);
            painter.line_segment([p(14.0, 10.0), p(14.0, 16.5)], stroke);
        }
        Icon::Star | Icon::StarFilled => {
            let mut pts = Vec::with_capacity(10);
            for i in 0..10 {
                let a = -std::f32::consts::FRAC_PI_2 + (i as f32) * std::f32::consts::PI / 5.0;
                let r = if i % 2 == 0 { 8.5 * u } else { 3.8 * u };
                pts.push(pos2(c.x + a.cos() * r, c.y + a.sin() * r));
            }
            if icon == Icon::StarFilled {
                painter.add(egui::Shape::convex_polygon(
                    vec![pts[0], pts[2], pts[4], pts[6], pts[8]],
                    color,
                    Stroke::NONE,
                ));
                pts.push(pts[0]);
                painter.add(egui::Shape::line(pts, Stroke::new(sw * 1.1, color)));
            } else {
                pts.push(pts[0]);
                painter.add(egui::Shape::line(pts, stroke));
            }
        }
        Icon::Copy => {
            let back = Rect::from_min_max(p(4.5, 4.5), p(14.5, 15.5));
            let front = Rect::from_min_max(p(9.5, 8.5), p(19.5, 19.5));
            painter.rect_stroke(back, CornerRadius::same((2.0 * u) as u8), stroke, egui::StrokeKind::Middle);
            painter.rect_stroke(front, CornerRadius::same((2.0 * u) as u8), stroke, egui::StrokeKind::Middle);
        }
        Icon::Cut => {
            painter.circle_stroke(p(7.0, 17.0), 2.5 * u, stroke);
            painter.circle_stroke(p(17.0, 17.0), 2.5 * u, stroke);
            painter.line_segment([p(8.5, 15.0), p(16.5, 5.5)], stroke);
            painter.line_segment([p(15.5, 15.0), p(7.5, 5.5)], stroke);
        }
        Icon::Paste => {
            let board = Rect::from_min_max(p(5.5, 5.0), p(18.5, 20.0));
            painter.rect_stroke(board, CornerRadius::same((2.5 * u) as u8), stroke, egui::StrokeKind::Middle);
            let clip = Rect::from_min_max(p(9.0, 3.2), p(15.0, 6.8));
            painter.rect_filled(clip, CornerRadius::same((1.5 * u) as u8), color);
        }
        Icon::Rename => {
            painter.line_segment([p(5.0, 19.0), p(8.5, 18.0)], stroke);
            painter.line_segment([p(5.0, 19.0), p(6.0, 15.5)], stroke);
            painter.line_segment([p(6.0, 15.5), p(16.0, 5.5)], stroke);
            painter.line_segment([p(8.5, 18.0), p(18.5, 8.0)], stroke);
            painter.line_segment([p(16.0, 5.5), p(18.5, 8.0)], stroke);
        }
        Icon::ZipAdd => {
            let body = Rect::from_min_max(p(4.0, 5.0), p(20.0, 19.0));
            painter.rect_stroke(body, CornerRadius::same((3.0 * u) as u8), stroke, egui::StrokeKind::Middle);
            painter.line_segment([p(12.0, 8.5), p(12.0, 15.5)], stroke);
            painter.line_segment([p(8.5, 12.0), p(15.5, 12.0)], stroke);
        }
        Icon::Unzip => {
            let body = Rect::from_min_max(p(4.0, 9.0), p(20.0, 19.5));
            painter.rect_stroke(body, CornerRadius::same((3.0 * u) as u8), stroke, egui::StrokeKind::Middle);
            painter.line_segment([p(12.0, 14.5), p(12.0, 4.5)], stroke);
            painter.line_segment([p(8.5, 8.0), p(12.0, 4.5)], stroke);
            painter.line_segment([p(15.5, 8.0), p(12.0, 4.5)], stroke);
        }
        Icon::Share => {
            painter.circle_filled(p(17.5, 6.0), 2.3 * u, color);
            painter.circle_filled(p(6.5, 12.0), 2.3 * u, color);
            painter.circle_filled(p(17.5, 18.0), 2.3 * u, color);
            painter.line_segment([p(8.5, 11.0), p(15.5, 7.0)], stroke);
            painter.line_segment([p(8.5, 13.0), p(15.5, 17.0)], stroke);
        }
        Icon::Transfer => {
            painter.line_segment([p(5.0, 9.0), p(19.0, 9.0)], stroke);
            painter.line_segment([p(15.0, 5.5), p(19.0, 9.0)], stroke);
            painter.line_segment([p(15.0, 12.5), p(19.0, 9.0)], stroke);
            painter.line_segment([p(19.0, 15.5), p(5.0, 15.5)], stroke);
            painter.line_segment([p(9.0, 12.0), p(5.0, 15.5)], stroke);
            painter.line_segment([p(9.0, 19.0), p(5.0, 15.5)], stroke);
        }
        Icon::Check => {
            painter.line_segment([p(5.5, 12.5), p(10.0, 17.0)], stroke);
            painter.line_segment([p(10.0, 17.0), p(18.5, 7.5)], stroke);
        }
        Icon::Shield => {
            let pts = vec![
                p(12.0, 3.5),
                p(19.0, 6.5),
                p(19.0, 12.5),
                p(12.0, 20.5),
                p(5.0, 12.5),
                p(5.0, 6.5),
                p(12.0, 3.5),
            ];
            painter.add(egui::Shape::line(pts, stroke));
        }
        Icon::Hash => {
            painter.line_segment([p(9.5, 4.5), p(7.5, 19.5)], stroke);
            painter.line_segment([p(16.5, 4.5), p(14.5, 19.5)], stroke);
            painter.line_segment([p(4.5, 9.5), p(19.5, 9.5)], stroke);
            painter.line_segment([p(4.0, 14.5), p(19.0, 14.5)], stroke);
        }
        Icon::Save => {
            let body = Rect::from_min_max(p(4.5, 4.5), p(19.5, 19.5));
            painter.rect_stroke(body, CornerRadius::same((2.5 * u) as u8), stroke, egui::StrokeKind::Middle);
            let top = Rect::from_min_max(p(8.0, 4.5), p(15.5, 9.5));
            painter.rect_stroke(top, CornerRadius::same(1), stroke, egui::StrokeKind::Middle);
            let bot = Rect::from_min_max(p(8.0, 13.5), p(16.0, 19.5));
            painter.rect_stroke(bot, CornerRadius::same(1), stroke, egui::StrokeKind::Middle);
        }
        Icon::ExternalOpen => {
            painter.line_segment([p(14.0, 5.0), p(19.5, 5.0)], stroke);
            painter.line_segment([p(19.5, 5.0), p(19.5, 10.5)], stroke);
            painter.line_segment([p(11.0, 13.5), p(19.5, 5.0)], stroke);
            painter.line_segment([p(10.5, 6.5), p(5.5, 6.5)], stroke);
            painter.line_segment([p(5.5, 6.5), p(5.5, 18.5)], stroke);
            painter.line_segment([p(5.5, 18.5), p(17.5, 18.5)], stroke);
            painter.line_segment([p(17.5, 18.5), p(17.5, 13.5)], stroke);
        }
        Icon::Restore => {
            painter.line_segment([p(6.0, 9.0), p(14.0, 9.0)], stroke);
            painter.line_segment([p(14.0, 9.0), p(17.5, 12.0)], stroke);
            painter.line_segment([p(17.5, 12.0), p(17.5, 16.5)], stroke);
            painter.line_segment([p(17.5, 16.5), p(10.0, 16.5)], stroke);
            painter.line_segment([p(9.5, 5.5), p(6.0, 9.0)], stroke);
            painter.line_segment([p(9.5, 12.5), p(6.0, 9.0)], stroke);
        }
        Icon::ZoomIn => {
            painter.circle_stroke(p(10.5, 10.5), 6.0 * u, stroke);
            painter.line_segment([p(15.0, 15.0), p(20.0, 20.0)], stroke);
            painter.line_segment([p(10.5, 8.0), p(10.5, 13.0)], stroke);
            painter.line_segment([p(8.0, 10.5), p(13.0, 10.5)], stroke);
        }
        Icon::ZoomOut => {
            painter.circle_stroke(p(10.5, 10.5), 6.0 * u, stroke);
            painter.line_segment([p(15.0, 15.0), p(20.0, 20.0)], stroke);
            painter.line_segment([p(8.0, 10.5), p(13.0, 10.5)], stroke);
        }
    }
}

/// Draw a standalone icon widget (no button chrome).
pub fn icon_widget(ui: &mut Ui, icon: Icon, size: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    paint_icon(ui.painter(), rect, icon, color);
}

/// Modern borderless ghost icon button (like macOS Finder / One UI 7) with soft hover/active pill.
pub fn icon_button(
    ui: &mut Ui,
    icon: Icon,
    active: bool,
    enabled: bool,
    hover_bg: Color32,
    active_bg: Color32,
    fg: Color32,
    active_fg: Color32,
) -> Response {
    let size = vec2(38.0, 38.0);
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(size, sense);

    let bg = if !enabled {
        Color32::TRANSPARENT
    } else if active {
        active_bg
    } else if resp.is_pointer_button_down_on() {
        active_bg.gamma_multiply(0.85)
    } else if resp.hovered() {
        hover_bg
    } else {
        Color32::TRANSPARENT
    };

    if bg != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, CornerRadius::same(10), bg);
    }

    let icon_color = if !enabled {
        fg.gamma_multiply(0.35)
    } else if active {
        active_fg
    } else {
        fg
    };
    let icon_rect = Rect::from_center_size(rect.center(), vec2(20.0, 20.0));
    paint_icon(ui.painter(), icon_rect, icon, icon_color);
    resp
}

/// Modern pill button with vector icon + label (for floating action bar, primary CTA, sidebar).
pub fn pill_button(
    ui: &mut Ui,
    icon: Icon,
    label: &str,
    bg: Color32,
    hover_bg: Color32,
    fg: Color32,
    min_width: Option<f32>,
) -> Response {
    let font_id = egui::FontId::proportional(13.5);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), font_id, fg);
    let pad_x = 14.0;
    let icon_sz = 18.0;
    let gap = 8.0;
    let desired_w = (pad_x * 2.0 + icon_sz + gap + galley.size().x).max(min_width.unwrap_or(0.0));
    let desired_h = 38.0;

    let (rect, resp) = ui.allocate_exact_size(vec2(desired_w, desired_h), Sense::click());
    let fill = if resp.hovered() { hover_bg } else { bg };
    if fill != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, CornerRadius::same(10), fill);
    }

    let icon_center = Pos2::new(rect.left() + pad_x + icon_sz * 0.5, rect.center().y);
    paint_icon(
        ui.painter(),
        Rect::from_center_size(icon_center, vec2(icon_sz, icon_sz)),
        icon,
        fg,
    );

    let text_pos = Pos2::new(
        rect.left() + pad_x + icon_sz + gap,
        rect.center().y - galley.size().y * 0.5,
    );
    ui.painter().galley(text_pos, galley, fg);
    resp
}

/// Draw a modern rounded icon badge (tinted circle/squircle + crisp vector icon inside).
pub fn category_icon_badge(
    ui: &mut Ui,
    cat: FileCategory,
    box_size: f32,
    color: Color32,
) {
    let (rect, _) = ui.allocate_exact_size(vec2(box_size, box_size), Sense::hover());
    let bg = color.gamma_multiply(0.16);
    ui.painter()
        .rect_filled(rect, CornerRadius::same((box_size * 0.28) as u8), bg);
    let icon_rect = Rect::from_center_size(rect.center(), Vec2::splat(box_size * 0.58));
    paint_icon(ui.painter(), icon_rect, Icon::for_category(cat), color);
}
