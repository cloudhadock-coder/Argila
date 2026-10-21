//! Janelas auxiliares: lista de atalhos (com conflitos) e Sobre.

use std::path::Path;

use eframe::egui::{self, RichText};

use crate::shortcuts::{Availability, Command, Keymap};
use crate::themes::Palette;

pub fn shortcuts_window(
    ctx: &egui::Context,
    open: &mut bool,
    p: &Palette,
    keymap: &Keymap,
    keymap_path: &Path,
    availability: &dyn Fn(Command) -> Availability,
) {
    egui::Window::new("Keyboard Shortcuts")
        .open(open)
        .default_size([560.0, 520.0])
        .collapsible(false)
        .show(ctx, |ui| {
            ui.label(RichText::new(format!("Profile: {}", keymap.profile)).strong());
            ui.label(
                RichText::new(format!(
                    "To customize, export the keymap (Tools ▸ Export Keymap) and save your changes as {}.",
                    keymap_path.display()
                ))
                .size(12.0)
                .color(p.text_weak),
            );
            let conflicts = keymap.conflicts();
            for c in &conflicts {
                let names: Vec<_> = c.commands.iter().map(|c| c.label()).collect();
                ui.label(RichText::new(format!("Conflict: {} → {}", c.chord, names.join(", "))).color(p.error));
            }
            ui.separator();
            egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
                egui::Grid::new("shortcuts_grid").striped(true).num_columns(3).spacing([18.0, 4.0]).show(ui, |ui| {
                    let mut group = "";
                    for cmd in Command::ALL {
                        if cmd.group() != group {
                            group = cmd.group();
                            ui.label(RichText::new(group.to_uppercase()).size(11.0).strong().color(p.accent));
                            ui.end_row();
                        }
                        ui.label(cmd.label());
                        let chords: Vec<String> = keymap.chords(*cmd).map(|k| k.to_string()).collect();
                        ui.label(RichText::new(chords.join("  /  ")).monospace().color(p.text));
                        let (status, color) = match availability(*cmd) {
                            Availability::Ready => ("ready", p.success),
                            Availability::NotImplemented => ("not implemented yet", p.text_weak),
                            Availability::Unavailable(_) => ("unavailable", p.warning),
                        };
                        ui.label(RichText::new(status).size(12.0).color(color));
                        ui.end_row();
                    }
                });
            });
        });
}

pub fn about_window(
    ctx: &egui::Context,
    open: &mut bool,
    p: &Palette,
    settings_path: &Path,
    fonts: &[&str],
) {
    egui::Window::new("About Argila")
        .open(open)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.label(RichText::new("Argila").size(24.0).strong().color(p.accent));
            ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
            ui.label(RichText::new(env!("CARGO_PKG_DESCRIPTION")).color(p.text_weak));
            ui.separator();
            ui.label(format!("Settings: {}", settings_path.display()));
            let fonts = if fonts.is_empty() { "built-in".to_owned() } else { fonts.join(", ") };
            ui.label(format!("Fonts: {fonts}"));
            ui.label(
                RichText::new(
                    "No Embarcadero files are bundled. DCC32/DCC64 will be used only from a licensed Delphi installation.",
                )
                .size(12.0)
                .color(p.text_weak),
            );
        });
}
