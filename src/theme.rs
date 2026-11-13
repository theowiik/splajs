//! Visual style: warm paper, black ink, one orange accent, monospace type.

use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, FontId, Stroke};
use std::sync::Arc;

pub const PAPER: Color32 = Color32::from_rgb(0xEC, 0xEA, 0xE5);
pub const PANEL: Color32 = Color32::from_rgb(0xE0, 0xDE, 0xD8);
pub const HOVER: Color32 = Color32::from_rgb(0xD6, 0xD3, 0xCC);
pub const LINE: Color32 = Color32::from_rgb(0xCB, 0xC8, 0xC1);
pub const MUTED: Color32 = Color32::from_rgb(0x8E, 0x8B, 0x85);
pub const INK: Color32 = Color32::from_rgb(0x17, 0x17, 0x17);
pub const INK_SOFT: Color32 = Color32::from_rgb(0x3A, 0x3A, 0x3A);
pub const ORANGE: Color32 = Color32::from_rgb(0xFF, 0x55, 0x1F);

const REGULAR: &[&str] = &[
    "JetBrainsMonoNerdFontMono-Regular.ttf",
    "IBMPlexMono-Regular.ttf",
    "CascadiaMono.ttf",
    "consola.ttf",
];
const BOLD: &[&str] = &[
    "JetBrainsMonoNerdFontMono-Bold.ttf",
    "IBMPlexMono-Bold.ttf",
    "consolab.ttf",
];

fn load_font(names: &[&str]) -> Option<Vec<u8>> {
    let mut dirs = vec![std::path::PathBuf::from(r"C:\Windows\Fonts")];
    if let Ok(l) = std::env::var("LOCALAPPDATA") {
        dirs.push(std::path::Path::new(&l).join(r"Microsoft\Windows\Fonts"));
    }
    names.iter().find_map(|n| dirs.iter().find_map(|d| std::fs::read(d.join(n)).ok()))
}

pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}

pub fn setup(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let fallback = fonts.families[&FontFamily::Proportional].clone();
    if let Some(r) = load_font(REGULAR) {
        fonts.font_data.insert("main".into(), Arc::new(FontData::from_owned(r)));
        for fam in [FontFamily::Proportional, FontFamily::Monospace] {
            fonts.families.get_mut(&fam).unwrap().insert(0, "main".into());
        }
    }
    let mut bold_fam = fallback;
    if let Some(b) = load_font(BOLD) {
        fonts.font_data.insert("bold".into(), Arc::new(FontData::from_owned(b)));
        bold_fam.insert(0, "bold".into());
    } else if fonts.font_data.contains_key("main") {
        bold_fam.insert(0, "main".into());
    }
    fonts.families.insert(FontFamily::Name("bold".into()), bold_fam);
    ctx.set_fonts(fonts);

    ctx.set_theme(egui::Theme::Light);
    ctx.all_styles_mut(|s| {
        use egui::{FontFamily::Proportional, TextStyle::*};
        s.text_styles = [
            (Heading, FontId::new(16.0, Proportional)),
            (Body, FontId::new(13.0, Proportional)),
            (Button, FontId::new(12.0, Proportional)),
            (Small, FontId::new(10.0, Proportional)),
            (Monospace, FontId::new(13.0, FontFamily::Monospace)),
        ]
        .into();
        s.spacing.scroll = egui::style::ScrollStyle::thin();
        s.spacing.item_spacing = egui::vec2(8.0, 0.0);
        let v = &mut s.visuals;
        *v = egui::Visuals::light();
        v.panel_fill = PAPER;
        v.window_fill = PAPER;
        v.extreme_bg_color = PAPER;
        v.faint_bg_color = PANEL;
        v.override_text_color = Some(INK);
        v.hyperlink_color = INK;
        v.selection.bg_fill = ORANGE.gamma_multiply(0.45);
        v.selection.stroke = Stroke::new(1.0, INK);
        v.text_cursor.stroke = Stroke::new(2.0, ORANGE);
        for w in [
            &mut v.widgets.noninteractive,
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            w.corner_radius = 2.0.into();
            w.bg_stroke = Stroke::new(1.0, LINE);
            w.fg_stroke = Stroke::new(1.0, INK);
            w.weak_bg_fill = PAPER;
            w.bg_fill = PAPER;
        }
        v.widgets.hovered.weak_bg_fill = HOVER;
        v.widgets.active.weak_bg_fill = HOVER;
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
    });
}

/// Small filled triangle: pointing right when closed, down when open.
pub fn chevron(p: &egui::Painter, c: egui::Pos2, open: bool, color: Color32) {
    let pts = if open {
        vec![c + egui::vec2(-4.0, -2.0), c + egui::vec2(4.0, -2.0), c + egui::vec2(0.0, 3.0)]
    } else {
        vec![c + egui::vec2(-2.0, -4.0), c + egui::vec2(3.0, 0.0), c + egui::vec2(-2.0, 4.0)]
    };
    p.add(egui::Shape::convex_polygon(pts, color, Stroke::NONE));
}

/// Hardware-style key: outlined box with a bold label.
pub fn key(ui: &mut egui::Ui, label: &str, width: f32) -> egui::Response {
    let (rect, r) = ui.allocate_exact_size(egui::vec2(width, 24.0), egui::Sense::click());
    let p = ui.painter();
    let fill = if r.is_pointer_button_down_on() { INK } else if r.hovered() { HOVER } else { PAPER };
    let text = if r.is_pointer_button_down_on() { PAPER } else { INK };
    p.rect(rect, 3.0, fill, Stroke::new(1.0, INK), egui::StrokeKind::Inside);
    p.text(rect.center(), egui::Align2::CENTER_CENTER, label, bold(11.0), text);
    if r.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    r
}

/// Small caps-style section label.
pub fn label(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).font(bold(10.0)).color(MUTED));
}
