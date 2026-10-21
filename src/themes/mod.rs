//! Tema "Earthy Developer" (cafe, madeira, argila, bronze, areia) e fontes da IDE.

use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, Stroke};

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub bg_main: Color32,
    pub bg_editor: Color32,
    pub bg_panel: Color32,
    pub bg_top: Color32,
    pub accent: Color32,
    pub selection: Color32,
    pub text: Color32,
    pub text_weak: Color32,
    pub border: Color32,
    pub success: Color32,
    pub error: Color32,
    pub warning: Color32,
}

pub const EARTHY: Palette = Palette {
    bg_main: Color32::from_rgb(0x21, 0x1F, 0x1B),
    bg_editor: Color32::from_rgb(0x29, 0x26, 0x20),
    bg_panel: Color32::from_rgb(0x33, 0x2E, 0x27),
    bg_top: Color32::from_rgb(0x3D, 0x34, 0x2B),
    accent: Color32::from_rgb(0xC4, 0x8A, 0x5A),
    selection: Color32::from_rgb(0x66, 0x51, 0x3D),
    text: Color32::from_rgb(0xE8, 0xDE, 0xD0),
    text_weak: Color32::from_rgb(0xAF, 0xA3, 0x94),
    border: Color32::from_rgb(0x51, 0x48, 0x3C),
    success: Color32::from_rgb(0x8D, 0xA4, 0x7E),
    error: Color32::from_rgb(0xC5, 0x6B, 0x5D),
    // nao faz parte da paleta original: ambar derivado do bronze para avisos
    warning: Color32::from_rgb(0xD6, 0xA8, 0x5C),
};

impl Palette {
    /// Faixa de cabecalho dos paineis: entre o painel e a barra superior.
    pub fn header(&self) -> Color32 {
        mix(self.bg_panel, self.bg_top, 0.5)
    }

    /// Fundo de hover discreto sobre os paineis.
    pub fn hover(&self) -> Color32 {
        mix(self.bg_panel, self.selection, 0.45)
    }
}

pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

pub fn apply(ctx: &egui::Context, p: &Palette) {
    let mut v = egui::Visuals::dark();
    v.override_text_color = None;
    v.window_fill = p.bg_panel;
    v.panel_fill = p.bg_main;
    v.extreme_bg_color = p.bg_editor;
    v.faint_bg_color = mix(p.bg_panel, p.bg_top, 0.35);
    v.code_bg_color = p.bg_editor;
    v.window_stroke = Stroke::new(1.0_f32, p.border);
    v.hyperlink_color = p.accent;
    v.warn_fg_color = p.warning;
    v.error_fg_color = p.error;
    v.selection.bg_fill = p.selection;
    v.selection.stroke = Stroke::new(1.0_f32, p.text);

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.bg_panel;
    w.noninteractive.weak_bg_fill = p.bg_panel;
    w.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0_f32, p.text);

    w.inactive.bg_fill = mix(p.bg_panel, p.bg_top, 0.6);
    w.inactive.weak_bg_fill = mix(p.bg_panel, p.bg_top, 0.6);
    w.inactive.bg_stroke = Stroke::new(1.0_f32, p.border);
    w.inactive.fg_stroke = Stroke::new(1.0_f32, p.text);

    w.hovered.bg_fill = p.hover();
    w.hovered.weak_bg_fill = p.hover();
    w.hovered.bg_stroke = Stroke::new(1.0_f32, p.accent);
    w.hovered.fg_stroke = Stroke::new(1.5_f32, p.text);

    w.active.bg_fill = p.selection;
    w.active.weak_bg_fill = p.selection;
    w.active.bg_stroke = Stroke::new(1.0_f32, p.accent);
    w.active.fg_stroke = Stroke::new(2.0_f32, p.text);

    w.open.bg_fill = p.selection;
    w.open.weak_bg_fill = p.selection;
    w.open.bg_stroke = Stroke::new(1.0_f32, p.border);
    w.open.fg_stroke = Stroke::new(1.0_f32, p.text);

    ctx.set_visuals(v);
    ctx.style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 4.0);
        s.spacing.button_padding = egui::vec2(8.0, 3.0);
        s.spacing.menu_margin = egui::Margin::same(6);
    });
}

/// Usa Segoe UI e Consolas do proprio Windows quando existirem (nada e redistribuido);
/// as fontes embutidas do egui ficam como reserva para simbolos e outros sistemas.
pub fn install_fonts(ctx: &egui::Context) -> Vec<&'static str> {
    let dir = std::env::var_os("WINDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:\\Windows"))
        .join("Fonts");
    let mut fonts = FontDefinitions::default();
    let mut loaded = Vec::new();
    for (file, name, family) in [
        ("segoeui.ttf", "Segoe UI", FontFamily::Proportional),
        ("consola.ttf", "Consolas", FontFamily::Monospace),
    ] {
        if let Ok(bytes) = std::fs::read(dir.join(file)) {
            fonts
                .font_data
                .insert(name.to_owned(), Arc::new(FontData::from_owned(bytes)));
            fonts
                .families
                .entry(family)
                .or_default()
                .insert(0, name.to_owned());
            loaded.push(name);
        }
    }
    ctx.set_fonts(fonts);
    loaded
}
