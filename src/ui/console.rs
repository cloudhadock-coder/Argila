//! Painel inferior: Messages, Search Results, Diagnostics, Build Output e Console.

use std::path::PathBuf;

use eframe::egui::{self, Margin, RichText, Sense, Ui, text::LayoutJob};

use super::{PanelOutput, empty_state, panel_tabs};
use crate::diagnostics::{Level, MessageLog};
use crate::settings::BottomTab;
use crate::themes::Palette;

/// Uma linha da lista de Search Results: cabecalho de arquivo ou ocorrencia.
pub struct SearchRow {
    pub text: String,
    pub is_file: bool,
    /// Trecho encontrado dentro de `text` (colunas em caracteres).
    pub highlight: Option<(usize, usize)>,
    /// Arquivo, linha e colunas da ocorrencia (base 0).
    pub target: Option<(PathBuf, usize, usize, usize)>,
}

pub struct SearchView<'a> {
    pub title: &'a str,
    pub status: &'a str,
    pub running: bool,
    pub rows: &'a [SearchRow],
    pub selected: Option<usize>,
}

pub enum ConsoleAction {
    OpenResult(usize),
    CancelSearch,
}

pub fn show(
    ui: &mut Ui,
    p: &Palette,
    tab: &mut BottomTab,
    log: &mut MessageLog,
    search: Option<&SearchView>,
) -> (PanelOutput, Option<ConsoleAction>) {
    let problems = log.count(Level::Error) + log.count(Level::Warning);
    let messages = if problems > 0 {
        format!("Messages ({problems})")
    } else {
        "Messages".to_owned()
    };
    let results = match search {
        Some(s) if s.running => "Search Results (…)".to_owned(),
        Some(s) => format!(
            "Search Results ({})",
            s.rows.iter().filter(|r| !r.is_file).count()
        ),
        None => "Search Results".to_owned(),
    };
    let tabs = [
        (BottomTab::Messages, messages),
        (BottomTab::SearchResults, results),
        (BottomTab::Diagnostics, "Diagnostics".to_owned()),
        (BottomTab::BuildOutput, "Build Output".to_owned()),
        (BottomTab::Console, "Console".to_owned()),
    ];
    let mut clear = false;
    let mut action = None;
    let showing_messages = *tab == BottomTab::Messages;
    let showing_running = *tab == BottomTab::SearchResults && search.is_some_and(|s| s.running);
    let close = panel_tabs(ui, p, tab, &tabs, |ui| {
        if showing_messages && ui.small_button("Clear").clicked() {
            clear = true;
        }
        if showing_running && ui.small_button("Cancel").clicked() {
            action = Some(ConsoleAction::CancelSearch);
        }
    });
    if clear {
        log.clear();
    }
    egui::Frame::new()
        .inner_margin(Margin::symmetric(10, 6))
        .show(ui, |ui| match *tab {
            BottomTab::Messages => messages_list(ui, p, log),
            BottomTab::SearchResults => match search {
                Some(s) => {
                    if let Some(a) = search_list(ui, p, s) {
                        action = Some(a);
                    }
                }
                None => empty_state(
                    ui,
                    p,
                    "No search results",
                    "Search ▸ Find in Files (Shift+Ctrl+F) lists every match here.",
                ),
            },
            BottomTab::Diagnostics => empty_state(
                ui,
                p,
                "No diagnostics",
                "Syntax problems found while editing will appear here.",
            ),
            BottomTab::BuildOutput => empty_state(
                ui,
                p,
                "No build output",
                "No compiler configured: DCC32/DCC64 support is planned for phase 6.",
            ),
            BottomTab::Console => empty_state(
                ui,
                p,
                "Console not available yet",
                "Output of external tools will appear here.",
            ),
        });
    (
        PanelOutput {
            close,
            command: None,
        },
        action,
    )
}

/// Resultados do Find in Files: virtualizados (milhares de linhas nao pesam), clique abre.
fn search_list(ui: &mut Ui, p: &Palette, s: &SearchView) -> Option<ConsoleAction> {
    let mut action = None;
    ui.horizontal(|ui| {
        if s.running {
            ui.spinner();
        }
        ui.label(RichText::new(s.title).strong().color(p.text));
        ui.label(RichText::new(s.status).size(12.0).color(p.text_weak));
    });
    ui.add_space(2.0);
    let font = egui::FontId::monospace(12.5);
    let row_h = (ui.fonts(|f| f.row_height(&font)) * 1.3).round();
    egui::ScrollArea::both()
        .id_salt("search_results")
        .auto_shrink([false; 2])
        .show_rows(ui, row_h, s.rows.len(), |ui, range| {
            for i in range {
                let row = &s.rows[i];
                let mut job = LayoutJob::default();
                let fmt = |color| egui::TextFormat::simple(font.clone(), color);
                if row.is_file {
                    job.append(&row.text, 0.0, fmt(p.accent));
                } else {
                    let chars: Vec<char> = row.text.chars().collect();
                    let (a, b) = row.highlight.unwrap_or((0, 0));
                    let (a, b) = (a.min(chars.len()), b.min(chars.len()));
                    let part = |r: std::ops::Range<usize>| chars[r].iter().collect::<String>();
                    job.append(&part(0..a), 0.0, fmt(p.text_weak));
                    let mut hit = fmt(p.text);
                    hit.background = p.accent.gamma_multiply(0.35);
                    job.append(&part(a..b), 0.0, hit);
                    job.append(&part(b..chars.len()), 0.0, fmt(p.text_weak));
                }
                let galley = ui.fonts(|f| f.layout_job(job));
                let width = galley.size().x.max(ui.available_width());
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, row_h), Sense::click());
                resp.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, row.text.trim())
                });
                if s.selected == Some(i) {
                    ui.painter()
                        .rect_filled(rect, egui::CornerRadius::ZERO, p.selection);
                } else if resp.hovered() {
                    ui.painter()
                        .rect_filled(rect, egui::CornerRadius::ZERO, p.hover());
                }
                ui.painter().galley(
                    egui::pos2(rect.min.x + 2.0, rect.center().y - galley.size().y / 2.0),
                    galley,
                    p.text,
                );
                if resp.clicked() && row.target.is_some() {
                    action = Some(ConsoleAction::OpenResult(i));
                }
            }
        });
    action
}

fn messages_list(ui: &mut Ui, p: &Palette, log: &MessageLog) {
    if log.entries().is_empty() {
        empty_state(ui, p, "No messages", "");
        return;
    }
    egui::ScrollArea::vertical()
        .auto_shrink([false; 2])
        .stick_to_bottom(true)
        .show(ui, |ui| {
            for m in log.entries() {
                let (icon, color) = match m.level {
                    Level::Error => ("✖", p.error),
                    Level::Warning => ("⚠", p.warning),
                    Level::Info => ("ℹ", p.text_weak),
                    Level::Hint => ("•", p.accent),
                };
                ui.horizontal(|ui| {
                    ui.label(RichText::new(icon).color(color));
                    ui.add(
                        egui::Label::new(RichText::new(&m.text).monospace().color(p.text)).wrap(),
                    );
                });
            }
        });
}
