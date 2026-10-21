//! Painel inferior: Messages, Search Results, Diagnostics, Build Output e Console.

use eframe::egui::{self, Margin, RichText, Ui};

use super::{PanelOutput, empty_state, panel_tabs};
use crate::diagnostics::{Level, MessageLog};
use crate::settings::BottomTab;
use crate::themes::Palette;

pub fn show(ui: &mut Ui, p: &Palette, tab: &mut BottomTab, log: &mut MessageLog) -> PanelOutput {
    let problems = log.count(Level::Error) + log.count(Level::Warning);
    let messages = if problems > 0 {
        format!("Messages ({problems})")
    } else {
        "Messages".to_owned()
    };
    let tabs = [
        (BottomTab::Messages, messages),
        (BottomTab::SearchResults, "Search Results".to_owned()),
        (BottomTab::Diagnostics, "Diagnostics".to_owned()),
        (BottomTab::BuildOutput, "Build Output".to_owned()),
        (BottomTab::Console, "Console".to_owned()),
    ];
    let mut clear = false;
    let showing_messages = *tab == BottomTab::Messages;
    let close = panel_tabs(ui, p, tab, &tabs, |ui| {
        if showing_messages && ui.small_button("Clear").clicked() {
            clear = true;
        }
    });
    if clear {
        log.clear();
    }
    egui::Frame::new()
        .inner_margin(Margin::symmetric(10, 6))
        .show(ui, |ui| match *tab {
            BottomTab::Messages => messages_list(ui, p, log),
            BottomTab::SearchResults => empty_state(
                ui,
                p,
                "No search results",
                "Find in Files results will be listed here.",
            ),
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
    PanelOutput {
        close,
        command: None,
    }
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
