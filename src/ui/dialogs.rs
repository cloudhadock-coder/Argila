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

#[derive(Default)]
pub struct OptionsState {
    pub config: String,
    pub platform: String,
    pub filter: String,
}

/// Opcoes do projeto, somente leitura: geral, valores efetivos e PropertyGroups como escritos.
pub fn project_options(
    ctx: &egui::Context,
    open: &mut bool,
    p: &Palette,
    project: &crate::projects::Project,
    st: &mut OptionsState,
) {
    egui::Window::new(format!("Project Options — {}", project.name))
        .id(egui::Id::new("project_options"))
        .open(open)
        .default_size([680.0, 560.0])
        .show(ctx, |ui| {
            ui.label(
                RichText::new("Read-only: nothing here changes the project files.")
                    .size(12.0)
                    .color(p.text_weak),
            );
            ui.separator();
            let d = project.dproj.as_ref();
            let general: Vec<(&str, String)> = vec![
                ("Kind", project.kind.label().to_owned()),
                ("Opened file", project.file.display().to_string()),
                (
                    "Main source",
                    project
                        .source
                        .as_ref()
                        .map(|s| s.display().to_string())
                        .unwrap_or_else(|| "—".into()),
                ),
                (
                    ".dproj",
                    project
                        .dproj_path
                        .as_ref()
                        .map(|s| s.display().to_string())
                        .unwrap_or_else(|| "—".into()),
                ),
                (
                    "ProjectGuid",
                    d.and_then(|d| d.global("ProjectGuid"))
                        .unwrap_or("—")
                        .to_owned(),
                ),
                (
                    "ProjectVersion",
                    d.and_then(|d| d.global("ProjectVersion"))
                        .unwrap_or("—")
                        .to_owned(),
                ),
                (
                    "FrameworkType",
                    d.and_then(|d| d.global("FrameworkType"))
                        .unwrap_or("—")
                        .to_owned(),
                ),
                (
                    "AppType",
                    d.and_then(|d| d.global("AppType"))
                        .unwrap_or("—")
                        .to_owned(),
                ),
                (
                    "Units",
                    format!(
                        "{} ({} missing)",
                        project.units.len(),
                        project.missing_count()
                    ),
                ),
                (
                    "Requires",
                    if project.requires.is_empty() {
                        "—".into()
                    } else {
                        project.requires.join(", ")
                    },
                ),
            ];
            egui::Grid::new("po_general")
                .num_columns(2)
                .spacing([12.0, 3.0])
                .show(ui, |ui| {
                    for (k, v) in &general {
                        ui.label(RichText::new(*k).color(p.text_weak));
                        ui.add(egui::Label::new(v.as_str()).wrap());
                        ui.end_row();
                    }
                });
            let Some(d) = d else {
                ui.add_space(8.0);
                ui.label(
                    RichText::new("No .dproj: build configurations are not available.")
                        .color(p.text_weak),
                );
                return;
            };
            let configs = d.config_names();
            let platforms = d.platform_names();
            if st.config.is_empty() || !configs.contains(&st.config) {
                st.config = d
                    .global("Config")
                    .map(str::to_owned)
                    .filter(|c| configs.contains(c))
                    .or_else(|| configs.first().cloned())
                    .unwrap_or_default();
            }
            if st.platform.is_empty() || !platforms.contains(&st.platform) {
                st.platform = platforms
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "Win32".to_owned());
            }
            ui.separator();
            ui.horizontal(|ui| {
                egui::ComboBox::from_label("Configuration")
                    .selected_text(&st.config)
                    .show_ui(ui, |ui| {
                        for c in &configs {
                            ui.selectable_value(&mut st.config, c.clone(), c);
                        }
                    });
                egui::ComboBox::from_label("Platform")
                    .selected_text(&st.platform)
                    .show_ui(ui, |ui| {
                        for pl in &platforms {
                            ui.selectable_value(&mut st.platform, pl.clone(), pl);
                        }
                    });
                ui.add(
                    egui::TextEdit::singleline(&mut st.filter)
                        .hint_text("Filter")
                        .desired_width(160.0),
                );
            });
            let effective = d.effective(&st.config, &st.platform);
            const KEY: &[&str] = &[
                "DCC_UnitSearchPath",
                "DCC_Define",
                "DCC_ExeOutput",
                "DCC_DcuOutput",
                "DCC_Namespace",
                "DCC_BplOutput",
                "DCC_DcpOutput",
                "DCC_UsePackage",
            ];
            let needle = st.filter.trim().to_lowercase();
            let shown = |k: &str, v: &str| {
                needle.is_empty()
                    || k.to_lowercase().contains(&needle)
                    || v.to_lowercase().contains(&needle)
            };
            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(format!(
                            "EFFECTIVE VALUES · {} · {}",
                            st.config, st.platform
                        ))
                        .size(11.0)
                        .strong()
                        .color(p.accent),
                    );
                    egui::Grid::new("po_effective")
                        .striped(true)
                        .num_columns(2)
                        .spacing([12.0, 3.0])
                        .show(ui, |ui| {
                            let ordered = KEY
                                .iter()
                                .filter_map(|k| effective.get_key_value(*k))
                                .chain(
                                    effective.iter().filter(|(k, _)| !KEY.contains(&k.as_str())),
                                );
                            for (k, v) in ordered.filter(|(k, v)| {
                                shown(k, v) && k.as_str() != "Config" && k.as_str() != "Platform"
                            }) {
                                let strong = KEY.contains(&k.as_str());
                                ui.label(RichText::new(k).color(if strong {
                                    p.text
                                } else {
                                    p.text_weak
                                }));
                                ui.add(egui::Label::new(RichText::new(v).monospace()).wrap());
                                ui.end_row();
                            }
                        });
                    ui.add_space(8.0);
                    egui::CollapsingHeader::new("Property groups as written in the .dproj")
                        .default_open(false)
                        .show(ui, |ui| {
                            for (i, g) in d.groups.iter().enumerate() {
                                let label = match (&g.scope, &g.condition) {
                                    (Some(s), _) => d.scope_label(s),
                                    (None, Some(c)) => c.clone(),
                                    (None, None) => "Project".to_owned(),
                                };
                                egui::CollapsingHeader::new(format!(
                                    "{label} ({})",
                                    g.properties.len()
                                ))
                                .id_salt(("po_group", i))
                                .show(ui, |ui| {
                                    if let Some(c) = &g.condition {
                                        ui.label(
                                            RichText::new(c)
                                                .monospace()
                                                .size(11.0)
                                                .color(p.text_weak),
                                        );
                                    }
                                    for (k, v, _) in
                                        g.properties.iter().filter(|(k, v, _)| shown(k, v))
                                    {
                                        ui.label(format!("{k} = {v}"));
                                    }
                                });
                            }
                            if !d.unknown_elements.is_empty() {
                                ui.label(
                                    RichText::new(format!(
                                        "Kept but not interpreted: {}",
                                        d.unknown_elements.join(", ")
                                    ))
                                    .color(p.text_weak),
                                );
                            }
                        });
                });
        });
}
