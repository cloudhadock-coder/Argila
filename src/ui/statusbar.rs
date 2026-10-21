//! Barra de status: arquivo, posicao, codificacao, quebra de linha, linguagem, projeto e compilador.
//! Codificacao e quebra de linha sao menus (reabrir/salvar com outra codificacao, trocar CRLF/LF).

use eframe::egui::{self, Align, Color32, Layout, RichText, Ui};

use crate::filesystem::encoding::{
    LEGACY_CHOICES, LineEnding, TextEncoding, TextFormat, encoding_for_label,
};
use crate::themes::Palette;

/// Estado do documento ativo mostrado na barra.
pub struct DocStatus<'a> {
    pub path: &'a str,
    pub line: usize,
    pub col: usize,
    pub format: TextFormat,
    pub mixed_line_endings: bool,
    pub language: &'a str,
    pub modified: bool,
}

#[derive(Default)]
pub struct StatusInfo<'a> {
    /// Mensagem temporaria (ex.: "Find in Files is not implemented yet") e sua cor.
    pub flash: Option<(&'a str, Color32)>,
    pub document: Option<DocStatus<'a>>,
    pub project: Option<&'a str>,
    pub compiler: Option<&'a str>,
}

pub enum StatusAction {
    /// Ler o arquivo de novo interpretando-o nesta codificacao.
    ReopenWith(TextEncoding),
    /// Gravar com outro formato no proximo salvamento.
    SaveWith(TextFormat),
}

pub fn show(ui: &mut Ui, p: &Palette, info: &StatusInfo) -> Option<StatusAction> {
    let mut action = None;
    // segmentos fixos da direita para a esquerda; o que sobra a esquerda e do caminho/mensagem
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        let weak = |t: &str| RichText::new(t).size(12.0).color(p.text_weak);
        ui.label(weak(info.compiler.unwrap_or("No compiler")));
        separator(ui);
        ui.label(weak(info.project.unwrap_or("No project")));
        if let Some(doc) = &info.document {
            separator(ui);
            ui.label(weak(doc.language));
            separator(ui);
            let eol = if doc.mixed_line_endings {
                format!("{} (mixed)", doc.format.line_ending.label())
            } else {
                doc.format.line_ending.label().to_owned()
            };
            ui.menu_button(weak(&eol), |ui| {
                for ending in [LineEnding::CrLf, LineEnding::Lf] {
                    if ui
                        .radio(doc.format.line_ending == ending, ending.label())
                        .clicked()
                    {
                        action = Some(StatusAction::SaveWith(TextFormat {
                            line_ending: ending,
                            ..doc.format
                        }));
                        ui.close_menu();
                    }
                }
            });
            separator(ui);
            ui.menu_button(weak(&doc.format.label()), |ui| {
                action = encoding_menu(ui, p, doc).or(action.take());
            });
            separator(ui);
            ui.label(weak(&format!("Ln {}, Col {}", doc.line, doc.col)));
            if doc.modified {
                separator(ui);
                ui.label(RichText::new("Modified").size(12.0).color(p.accent));
            }
        }
        ui.add_space(16.0);
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            let text = match (info.flash, &info.document) {
                (Some((text, color)), _) => RichText::new(text).color(color),
                (None, Some(doc)) => RichText::new(doc.path).size(12.0).color(p.text_weak),
                (None, None) => RichText::new("Ready").color(p.text_weak),
            };
            ui.add(egui::Label::new(text).truncate());
        });
    });
    action
}

fn separator(ui: &mut Ui) {
    ui.add(egui::Separator::default().vertical().spacing(10.0));
}

fn encoding_menu(ui: &mut Ui, p: &Palette, doc: &DocStatus) -> Option<StatusAction> {
    let mut action = None;
    let legacy: Vec<TextEncoding> = LEGACY_CHOICES
        .iter()
        .filter_map(|l| encoding_for_label(l))
        .map(TextEncoding::Legacy)
        .collect();
    ui.set_min_width(220.0);
    ui.label(
        RichText::new("Reopen with encoding")
            .size(11.0)
            .strong()
            .color(p.text_weak),
    );
    ui.add_enabled_ui(!doc.modified, |ui| {
        for enc in [
            TextEncoding::Utf8,
            TextEncoding::Utf16Le,
            TextEncoding::Utf16Be,
        ]
        .into_iter()
        .chain(legacy.iter().copied())
        {
            let response = ui.button(enc.label());
            if response.clicked() {
                action = Some(StatusAction::ReopenWith(enc));
                ui.close_menu();
            }
        }
    })
    .response
    .on_disabled_hover_text("Save or undo your changes first: reopening discards the buffer.");
    ui.separator();
    ui.label(
        RichText::new("Save with encoding")
            .size(11.0)
            .strong()
            .color(p.text_weak),
    );
    let save_choices = [
        (TextEncoding::Utf8, false),
        (TextEncoding::Utf8, true),
        (TextEncoding::Utf16Le, true),
    ]
    .into_iter()
    .chain(legacy.iter().map(|e| (*e, false)));
    for (encoding, bom) in save_choices {
        let format = TextFormat {
            encoding,
            bom,
            line_ending: doc.format.line_ending,
        };
        if ui.radio(doc.format == format, format.label()).clicked() {
            action = Some(StatusAction::SaveWith(format));
            ui.close_menu();
        }
    }
    action
}
