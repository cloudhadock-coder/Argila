//! Painel direito: Object Inspector (abas Properties / Events e busca).
//!
//! Mostra propriedades do projeto ou do componente DFM selecionado, sempre somente leitura.

use eframe::egui::{self, Margin, RichText, Ui};

use super::{PanelOutput, empty_state, panel_title, tab_button};
use crate::settings::InspectorTab;
use crate::themes::Palette;

/// O que o inspetor mostra: titulo, "classe" e pares nome/valor.
pub struct InspectorItem {
    pub title: String,
    pub class: String,
    pub rows: Vec<(String, String)>,
}

pub fn show(
    ui: &mut Ui,
    p: &Palette,
    tab: &mut InspectorTab,
    filter: &mut String,
    item: Option<&InspectorItem>,
) -> PanelOutput {
    let close = panel_title(ui, p, "Object Inspector");
    egui::Frame::new()
        .inner_margin(Margin::same(8))
        .show(ui, |ui| {
            let selected = item.map_or("(no selection)".to_owned(), |i| {
                format!("{}: {}", i.title, i.class)
            });
            ui.add_enabled_ui(false, |ui| {
                egui::ComboBox::from_id_salt("oi_component")
                    .selected_text(RichText::new(selected).color(p.text_weak))
                    .width(ui.available_width())
                    .show_ui(ui, |_| {});
            });
            ui.add_space(6.0);
            ui.small("Read-only");
            ui.horizontal(|ui| {
                for (value, label) in [
                    (InspectorTab::Properties, "Properties"),
                    (InspectorTab::Events, "Events"),
                ] {
                    if tab_button(ui, p, *tab == value, label).clicked() {
                        *tab = value;
                    }
                }
            });
            ui.add_space(4.0);
            let hint = match tab {
                InspectorTab::Properties => "Search properties",
                InspectorTab::Events => "Search events",
            };
            ui.add(
                egui::TextEdit::singleline(filter)
                    .hint_text(hint)
                    .desired_width(f32::INFINITY),
            );
            match (item, *tab) {
                (None, _) => empty_state(
                    ui,
                    p,
                    "Nothing selected",
                    "Select a unit, form or the project in the Project Explorer.",
                ),
                (Some(item), selected_tab) => {
                    let needle = filter.trim().to_lowercase();
                    egui::ScrollArea::vertical()
                        .auto_shrink([false; 2])
                        .show(ui, |ui| {
                            egui::Grid::new("oi_grid")
                                .striped(true)
                                .num_columns(2)
                                .spacing([10.0, 4.0])
                                .show(ui, |ui| {
                                    for (name, value) in &item.rows {
                                        let event = name
                                            .rsplit('.')
                                            .next()
                                            .unwrap_or(name)
                                            .get(..2)
                                            .is_some_and(|prefix| {
                                                prefix.eq_ignore_ascii_case("On")
                                            });
                                        if event != (selected_tab == InspectorTab::Events) {
                                            continue;
                                        }
                                        if !needle.is_empty()
                                            && !name.to_lowercase().contains(&needle)
                                            && !value.to_lowercase().contains(&needle)
                                        {
                                            continue;
                                        }
                                        ui.label(RichText::new(name).color(p.text_weak));
                                        ui.add(
                                            egui::Label::new(RichText::new(value).color(p.text))
                                                .wrap(),
                                        );
                                        ui.end_row();
                                    }
                                });
                        });
                }
            }
        });
    PanelOutput {
        close,
        command: None,
    }
}
