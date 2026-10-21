//! Painel esquerdo: Project Explorer e Structure.

use eframe::egui::{self, Margin, Ui};

use super::{PanelOutput, command_button, empty_state, panel_tabs};
use crate::settings::LeftTab;
use crate::shortcuts::{Command, Keymap};
use crate::themes::Palette;

pub fn show(ui: &mut Ui, p: &Palette, tab: &mut LeftTab, keymap: &Keymap) -> PanelOutput {
    let mut out = PanelOutput {
        close: panel_tabs(
            ui,
            p,
            tab,
            &[
                (LeftTab::ProjectExplorer, "Project Explorer".into()),
                (LeftTab::Structure, "Structure".into()),
            ],
            |_| {},
        ),
        command: None,
    };
    egui::Frame::new()
        .inner_margin(Margin::same(10))
        .show(ui, |ui| match *tab {
            LeftTab::ProjectExplorer => {
                empty_state(
                    ui,
                    p,
                    "No project open",
                    "Open a .dproj / .dpr or a folder with Delphi units.",
                );
                ui.add_space(10.0);
                ui.vertical_centered(|ui| {
                    for cmd in [Command::OpenProject, Command::OpenFolder] {
                        if command_button(ui, cmd.label(), &keymap.shortcut_text(cmd)) {
                            out.command = Some(cmd);
                        }
                    }
                });
            }
            LeftTab::Structure => {
                empty_state(
                    ui,
                    p,
                    "No unit open",
                    "Classes, methods and form components of the active unit appear here.",
                );
            }
        });
    out
}
