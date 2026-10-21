//! Painel direito: Object Inspector (abas Properties / Events e busca).
//!
//! Na Fase 4 recebe o componente selecionado no designer/Structure; por enquanto mostra o estado vazio.

use eframe::egui::{self, Margin, RichText, Ui};

use super::{PanelOutput, empty_state, panel_title, tab_button};
use crate::settings::InspectorTab;
use crate::themes::Palette;

pub fn show(ui: &mut Ui, p: &Palette, tab: &mut InspectorTab, filter: &mut String) -> PanelOutput {
    let close = panel_title(ui, p, "Object Inspector");
    egui::Frame::new()
        .inner_margin(Margin::same(8))
        .show(ui, |ui| {
            ui.add_enabled_ui(false, |ui| {
                egui::ComboBox::from_id_salt("oi_component")
                    .selected_text(RichText::new("(no component)").color(p.text_weak))
                    .width(ui.available_width())
                    .show_ui(ui, |_| {});
            });
            ui.add_space(6.0);
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
            empty_state(
                ui,
                p,
                "No component selected",
                "Select a component in the form designer or in Structure.",
            );
        });
    PanelOutput {
        close,
        command: None,
    }
}
