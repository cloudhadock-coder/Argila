//! Barra de status: arquivo, posicao, codificacao, quebra de linha, linguagem, projeto e compilador.

use eframe::egui::{self, Align, Color32, Layout, RichText, Ui};

use crate::themes::Palette;

#[derive(Default)]
pub struct StatusInfo<'a> {
    /// Mensagem temporaria (ex.: "Find in Files is not implemented yet") e sua cor.
    pub flash: Option<(&'a str, Color32)>,
    pub file: Option<&'a str>,
    pub position: Option<(usize, usize)>,
    pub encoding: Option<&'a str>,
    pub line_ending: Option<&'a str>,
    pub language: Option<&'a str>,
    pub project: Option<&'a str>,
    pub compiler: Option<&'a str>,
}

pub fn show(ui: &mut Ui, p: &Palette, info: &StatusInfo) {
    ui.horizontal(|ui| {
        match info.flash {
            Some((text, color)) => ui.label(RichText::new(text).color(color)),
            None => ui.label(RichText::new(info.file.unwrap_or("Ready")).color(p.text_weak)),
        };
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let dash = "—";
            let segments = [
                info.compiler.unwrap_or("No compiler").to_owned(),
                info.project.unwrap_or("No project").to_owned(),
                info.language.unwrap_or(dash).to_owned(),
                info.line_ending.unwrap_or(dash).to_owned(),
                info.encoding.unwrap_or(dash).to_owned(),
                match info.position {
                    Some((line, col)) => format!("Ln {line}, Col {col}"),
                    None => format!("Ln {dash}, Col {dash}"),
                },
            ];
            for (i, text) in segments.iter().enumerate() {
                if i > 0 {
                    ui.add(egui::Separator::default().vertical().spacing(10.0));
                }
                ui.label(RichText::new(text).size(12.0).color(p.text_weak));
            }
        });
    });
}
