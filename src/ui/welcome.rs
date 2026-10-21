//! Pagina inicial (aba Welcome): acoes rapidas, recentes e perfil de atalhos.

use eframe::egui::{self, RichText, Ui};

use super::command_button;
use crate::settings::RecentSettings;
use crate::shortcuts::{Command, Keymap};
use crate::themes::Palette;

pub fn show(ui: &mut Ui, p: &Palette, keymap: &Keymap, recent: &RecentSettings) -> Option<Command> {
    let mut fired = None;
    egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            ui.set_max_width(620.0);
            ui.label(RichText::new("Argila").size(34.0).strong().color(p.accent));
            ui.label(RichText::new("Object Pascal IDE · units, forms and projects without the Delphi IDE").color(p.text_weak));
            ui.add_space(26.0);

            section(ui, p, "Start");
            ui.horizontal_wrapped(|ui| {
                for cmd in [Command::OpenFile, Command::OpenProject, Command::OpenFolder, Command::NewUnit] {
                    if command_button(ui, cmd.label(), &keymap.shortcut_text(cmd)) {
                        fired = Some(cmd);
                    }
                }
            });

            ui.add_space(18.0);
            section(ui, p, "Recent");
            let items: Vec<&String> = recent.projects.iter().chain(&recent.files).take(RecentSettings::MAX).collect();
            if items.is_empty() {
                ui.label(RichText::new("Nothing opened yet.").color(p.text_weak));
            }
            for path in items {
                ui.label(RichText::new(path).monospace().color(p.text));
            }

            ui.add_space(18.0);
            section(ui, p, "Keyboard");
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("Profile: {}", keymap.profile)).color(p.text));
                if ui.link("View all shortcuts").clicked() {
                    fired = Some(Command::KeyboardShortcuts);
                }
            });

            ui.add_space(18.0);
            section(ui, p, "Status");
            ui.label(
                RichText::new(
                    "Phase 1 (foundation): window, earthy theme, menus, panels, tabs, persistent settings \
                     and the Delphi keymap. Commands that are not implemented yet say so instead of pretending.",
                )
                .color(p.text_weak),
            );
        });
    });
    fired
}

fn section(ui: &mut Ui, p: &Palette, title: &str) {
    ui.label(
        RichText::new(title.to_uppercase())
            .size(11.0)
            .strong()
            .color(p.text_weak),
    );
    ui.add_space(4.0);
}
