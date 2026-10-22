//! Pagina inicial (aba Welcome): acoes rapidas, recentes e perfil de atalhos.

use eframe::egui::{self, RichText, Ui};

use super::command_button;
use crate::settings::RecentSettings;
use crate::shortcuts::{Command, Keymap};
use crate::themes::Palette;

pub enum WelcomeAction {
    Command(Command),
    OpenRecent(String),
    OpenForm,
    DemoForm,
}

pub fn show(
    ui: &mut Ui,
    p: &Palette,
    keymap: &Keymap,
    recent: &RecentSettings,
) -> Option<WelcomeAction> {
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
                        fired = Some(WelcomeAction::Command(cmd));
                    }
                }
            });

            ui.add_space(18.0);
            section(ui, p, "Form Designer");
            ui.label("Open a .dfm to see its form, component tree and properties. From a Pascal unit, press F12.");
            ui.horizontal(|ui| {
                if ui.button("Open DFM in Design…").clicked() { fired = Some(WelcomeAction::OpenForm); }
                if ui.button("Try designer demo").clicked() { fired = Some(WelcomeAction::DemoForm); }
            });

            ui.add_space(18.0);
            section(ui, p, "Recent");
            let items: Vec<&String> = recent.projects.iter().chain(&recent.files).take(RecentSettings::MAX).collect();
            if items.is_empty() {
                ui.label(RichText::new("Nothing opened yet. You can also drop files on the window.").color(p.text_weak));
            }
            for path in items {
                if ui.link(RichText::new(path).monospace()).clicked() {
                    fired = Some(WelcomeAction::OpenRecent(path.clone()));
                }
            }

            ui.add_space(18.0);
            section(ui, p, "Keyboard");
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("Profile: {}", keymap.profile)).color(p.text));
                if ui.link("View all shortcuts").clicked() {
                    fired = Some(WelcomeAction::Command(Command::KeyboardShortcuts));
                }
            });

            ui.add_space(18.0);
            section(ui, p, "Status");
            ui.label(
                RichText::new(
                    "Phase 5: code completion (Ctrl+Space, after a dot), Class Completion (Shift+Ctrl+C), \
                     Go to Symbol (Shift+Alt+S), project-wide Find Declaration with Alt+Left/Right history, \
                     code folding (Shift+Ctrl+K, …), Find in Files (Shift+Ctrl+F) and split editors (View menu). \
                     DFM preview, Delphi projects and PAS editing remain available.",
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
