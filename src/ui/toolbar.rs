//! Barra de menus (File, Edit, Search, View, Project, Run, Tools, Help) e faixa de ferramentas.

use eframe::egui::{self, Button, RichText, Ui};

use crate::shortcuts::{Availability, Command, Keymap};
use crate::themes::Palette;

pub struct MenuContext<'a> {
    pub keymap: &'a Keymap,
    pub availability: &'a dyn Fn(Command) -> Availability,
    /// Estado marcado para itens de alternancia (View); None para itens comuns.
    pub checked: &'a dyn Fn(Command) -> Option<bool>,
}

pub fn menu_bar(ui: &mut Ui, mc: &MenuContext) -> Option<Command> {
    use Command::*;
    let mut fired = None;
    egui::menu::bar(ui, |ui| {
        menu(
            ui,
            mc,
            &mut fired,
            "File",
            &[
                &[NewUnit, OpenFile, OpenProject, OpenFolder],
                &[Save, SaveAll, CloseTab, CloseProject],
                &[Exit],
            ],
        );
        menu(
            ui,
            mc,
            &mut fired,
            "Edit",
            &[
                &[Undo, Redo],
                &[
                    ToggleComment,
                    CodeCompletion,
                    ClassCompletion,
                    DeclImplToggle,
                ],
                &[
                    FoldToggle,
                    FoldExpandAll,
                    FoldMethods,
                    FoldClasses,
                    FoldRegions,
                    CodeFolding,
                ],
            ],
        );
        menu(
            ui,
            mc,
            &mut fired,
            "Search",
            &[
                &[Find, Replace, FindInFiles, SearchAgain],
                &[GotoLine, FindDeclaration, GoToSymbol],
                &[NavigateBack, NavigateForward],
            ],
        );
        menu(
            ui,
            mc,
            &mut fired,
            "View",
            &[
                &[
                    ProjectManager,
                    Structure,
                    ObjectInspector,
                    Messages,
                    SearchResults,
                ],
                &[ToggleFormUnit, ViewUnits, ViewForms],
                &[SplitVertical, SplitHorizontal, SwitchPane, Unsplit],
                &[Welcome],
            ],
        );
        menu(
            ui,
            mc,
            &mut fired,
            "Project",
            &[
                &[AddToProject, RemoveFromProject, RenameUnit],
                &[Compile, Build],
                &[ProjectOptions],
            ],
        );
        menu(ui, mc, &mut fired, "Run", &[&[Run, RunWithoutDebugging]]);
        menu(
            ui,
            mc,
            &mut fired,
            "Tools",
            &[&[ExportKeymap, OpenSettingsFolder]],
        );
        menu(
            ui,
            mc,
            &mut fired,
            "Help",
            &[&[KeyboardShortcuts], &[About]],
        );
    });
    fired
}

fn menu(
    ui: &mut Ui,
    mc: &MenuContext,
    fired: &mut Option<Command>,
    title: &str,
    groups: &[&[Command]],
) {
    ui.menu_button(title, |ui| {
        ui.set_min_width(250.0);
        for (i, group) in groups.iter().enumerate() {
            if i > 0 {
                ui.separator();
            }
            for cmd in *group {
                if menu_item(ui, mc, *cmd) {
                    *fired = Some(*cmd);
                    ui.close_menu();
                }
            }
        }
    });
}

fn menu_item(ui: &mut Ui, mc: &MenuContext, cmd: Command) -> bool {
    let availability = (mc.availability)(cmd);
    let mut text = RichText::new(cmd.label());
    if availability != Availability::Ready {
        text = text.color(ui.visuals().weak_text_color());
    }
    let mut button = Button::new(text).shortcut_text(mc.keymap.shortcut_text(cmd));
    if let Some(on) = (mc.checked)(cmd) {
        button = button.selected(on);
    }
    let response = ui.add(button);
    let response = match &availability {
        Availability::Ready => response,
        Availability::NotImplemented => response.on_hover_text("Not implemented yet"),
        Availability::Unavailable(reason) => response.on_hover_text(reason.as_str()),
    };
    response.clicked()
}

/// Faixa de icones logo abaixo do menu.
pub fn tool_strip(ui: &mut Ui, p: &Palette, mc: &MenuContext) -> Option<Command> {
    use Command::*;
    let mut fired = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for group in [
            &[OpenFile, OpenProject][..],
            &[Save, SaveAll],
            &[Find, FindInFiles],
            &[Compile, Run],
        ] {
            for cmd in group {
                let icon = match cmd {
                    OpenFile => "📄",
                    OpenProject => "📂",
                    Save => "💾",
                    SaveAll => "🗐",
                    Find => "🔍",
                    FindInFiles => "🔎",
                    Compile => "⚙",
                    _ => "▶",
                };
                let ready = (mc.availability)(*cmd) == Availability::Ready;
                let color = if ready { p.text } else { p.text_weak };
                let shortcut = mc.keymap.shortcut_text(*cmd);
                let tip = if shortcut.is_empty() {
                    cmd.label().to_owned()
                } else {
                    format!("{}  ({shortcut})", cmd.label())
                };
                let button = Button::new(RichText::new(icon).size(15.0).color(color)).frame(false);
                if ui.add(button).on_hover_text(tip).clicked() {
                    fired = Some(*cmd);
                }
            }
            ui.add_space(6.0);
            ui.separator();
            ui.add_space(4.0);
        }
    });
    fired
}
