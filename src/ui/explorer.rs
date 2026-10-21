//! Painel esquerdo: Project Explorer (arvore do projeto) e Structure (simbolos da unit ativa).
//!
//! As duas arvores sao achatadas em linhas a cada quadro e desenhadas com `show_rows`, entao so
//! as linhas visiveis custam algo, mesmo com centenas de units.

use std::collections::HashSet;
use std::path::PathBuf;

use eframe::egui::{self, Color32, EventFilter, Id, Key, Margin, Modifiers, RichText, Sense, Ui};

use super::{command_button, empty_state, panel_tabs};
use crate::projects::outline::{Outline, Symbol, SymbolKind};
use crate::projects::{Project, ProjectKind};
use crate::settings::LeftTab;
use crate::shortcuts::{Command, Keymap};
use crate::themes::Palette;

#[derive(Default)]
pub struct ExplorerState {
    pub selected: Option<String>,
    pub expanded: HashSet<String>,
    pub filter: String,
    /// Nos do Structure que o usuario inverteu (o padrao: classes abertas, uses e implementacao recolhidos).
    pub structure_toggled: HashSet<String>,
    scroll_to: Option<f32>,
}

pub enum ExplorerAction {
    Command(Command),
    Open(PathBuf),
    Remove(String),
    Rename(PathBuf),
    /// Linha/coluna (base 0) no editor ativo.
    Navigate(usize, usize),
}

pub struct ExplorerOutput {
    pub close: bool,
    pub actions: Vec<ExplorerAction>,
}

struct Row {
    id: String,
    depth: usize,
    label: String,
    detail: String,
    icon: &'static str,
    color: Option<Color32>,
    children: bool,
    /// Aberto (so usado no Structure; no projeto vale `ExplorerState::expanded`).
    expanded: bool,
    open: Option<PathBuf>,
    unit: Option<String>,
    target: Option<(usize, usize)>,
}

impl Row {
    fn new(
        id: impl Into<String>,
        depth: usize,
        icon: &'static str,
        label: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            depth,
            label: label.into(),
            detail: String::new(),
            icon,
            color: None,
            children: false,
            expanded: false,
            open: None,
            unit: None,
            target: None,
        }
    }
}

pub struct StructureInfo<'a> {
    pub title: &'a str,
    pub outline: Option<&'a Outline>,
    pub busy: bool,
}

#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut Ui,
    p: &Palette,
    tab: &mut LeftTab,
    keymap: &Keymap,
    project: Option<&Project>,
    loading: Option<&str>,
    state: &mut ExplorerState,
    structure: &StructureInfo,
) -> ExplorerOutput {
    let close = panel_tabs(
        ui,
        p,
        tab,
        &[
            (LeftTab::ProjectExplorer, "Project Explorer".into()),
            (LeftTab::Structure, "Structure".into()),
        ],
        |_| {},
    );
    let mut actions = Vec::new();
    egui::Frame::new()
        .inner_margin(Margin::symmetric(6, 6))
        .show(ui, |ui| match *tab {
            LeftTab::ProjectExplorer => {
                project_panel(ui, p, keymap, project, loading, state, &mut actions)
            }
            LeftTab::Structure => structure_panel(ui, p, structure, state, &mut actions),
        });
    ExplorerOutput { close, actions }
}

fn project_panel(
    ui: &mut Ui,
    p: &Palette,
    keymap: &Keymap,
    project: Option<&Project>,
    loading: Option<&str>,
    state: &mut ExplorerState,
    actions: &mut Vec<ExplorerAction>,
) {
    if let Some(name) = loading {
        empty_state(
            ui,
            p,
            &format!("Loading {name}…"),
            "Reading the project files in the background.",
        );
        return;
    }
    let Some(project) = project else {
        empty_state(
            ui,
            p,
            "No project open",
            "Open a .dproj / .dpr / .dpk or a folder with Delphi units.",
        );
        ui.add_space(10.0);
        ui.vertical_centered(|ui| {
            for cmd in [Command::OpenProject, Command::OpenFolder] {
                if command_button(ui, cmd.label(), &keymap.shortcut_text(cmd)) {
                    actions.push(ExplorerAction::Command(cmd));
                }
            }
        });
        return;
    };
    ui.add(
        egui::TextEdit::singleline(&mut state.filter)
            .hint_text("Filter units and forms")
            .desired_width(f32::INFINITY),
    );
    ui.add_space(4.0);
    let rows = project_rows(project, state);
    tree(ui, p, Id::new("project_tree"), &rows, state, actions, true);
}

fn project_rows(project: &Project, state: &ExplorerState) -> Vec<Row> {
    let mut rows = Vec::new();
    let open = |id: &str| state.expanded.contains(id);
    let mut root = Row::new(
        "project",
        0,
        if project.kind == ProjectKind::Package {
            "📦"
        } else {
            "📁"
        },
        &project.name,
    );
    root.detail = format!("{} · {} units", project.kind.label(), project.units.len());
    root.children = true;
    root.open = project
        .source
        .clone()
        .or_else(|| project.dproj_path.clone());
    rows.push(root);
    let filter = state.filter.trim().to_lowercase();
    if !filter.is_empty() {
        for u in &project.units {
            let hit = u.name.to_lowercase().contains(&filter)
                || u.form
                    .as_deref()
                    .is_some_and(|f| f.to_lowercase().contains(&filter));
            if hit {
                rows.push(unit_row(u, 1));
            }
        }
        return rows;
    }
    if !open("project") {
        return rows;
    }
    for (path, icon) in [(&project.source, "📄"), (&project.dproj_path, "⚙")] {
        if let Some(path) = path {
            let mut r = Row::new(format!("file:{}", path.display()), 1, icon, file_name(path));
            r.open = Some(path.clone());
            rows.push(r);
        }
    }
    if let Some(d) = &project.dproj {
        let configs = d.config_names();
        let platforms = d.platform_names();
        for (id, label, items, icon) in [
            ("configs", "Build Configurations", configs, "⚙"),
            ("platforms", "Target Platforms", platforms, "🖥"),
        ] {
            let mut r = Row::new(id, 1, "📁", label);
            r.detail = items.join(", ");
            r.children = !items.is_empty();
            rows.push(r);
            if open(id) {
                for item in items {
                    rows.push(Row::new(format!("{id}:{item}"), 2, icon, item));
                }
            }
        }
    }
    let mut units = Row::new("units", 1, "📁", "Units");
    let missing = project.missing_count();
    units.detail = if missing > 0 {
        format!("{} · {missing} missing", project.units.len())
    } else {
        project.units.len().to_string()
    };
    units.children = !project.units.is_empty();
    rows.push(units);
    if open("units") {
        for u in &project.units {
            let row = unit_row(u, 2);
            let expanded = open(&row.id);
            rows.push(row);
            if let (Some(dfm), true) = (&u.dfm, expanded) {
                let mut r = Row::new(
                    format!("dfm:{}", u.name.to_lowercase()),
                    3,
                    "🎨",
                    file_name(dfm),
                );
                r.open = Some(dfm.clone());
                r.unit = Some(u.name.clone());
                rows.push(r);
            }
        }
    }
    if !project.requires.is_empty() {
        let mut r = Row::new("requires", 1, "📁", "Requires");
        r.detail = project.requires.len().to_string();
        r.children = true;
        rows.push(r);
        if open("requires") {
            for pkg in &project.requires {
                rows.push(Row::new(format!("pkg:{pkg}"), 2, "📦", pkg));
            }
        }
    }
    if !project.others.is_empty() {
        let mut r = Row::new("others", 1, "📁", "Other Files");
        r.detail = project.others.len().to_string();
        r.children = true;
        rows.push(r);
        if open("others") {
            for o in &project.others {
                let mut r = Row::new(
                    format!("other:{}", o.path.display()),
                    2,
                    "📄",
                    file_name(&o.path),
                );
                r.detail = o.role.to_owned();
                if !o.exists {
                    r.color = Some(Color32::from_rgb(0xC5, 0x6B, 0x5D));
                    r.detail.push_str(" · missing");
                } else if o.role != "resource" {
                    r.open = Some(o.path.clone());
                }
                rows.push(r);
            }
        }
    }
    rows
}

fn unit_row(u: &crate::projects::ProjectUnit, depth: usize) -> Row {
    let mut r = Row::new(
        format!("unit:{}", u.name.to_lowercase()),
        depth,
        if u.is_missing() { "⚠" } else { "📄" },
        format!("{}.pas", u.name),
    );
    r.unit = Some(u.name.clone());
    r.children = u.dfm.is_some();
    r.detail = match (&u.form, u.is_missing(), u.is_library()) {
        (_, true, _) => "missing".to_owned(),
        (_, _, true) => "library unit".to_owned(),
        (Some(f), _, _) => f.clone(),
        _ => String::new(),
    };
    if u.conditional {
        r.detail.push_str(" · {$IF}");
    }
    if u.is_missing() {
        r.color = Some(Color32::from_rgb(0xC5, 0x6B, 0x5D));
    } else if u.is_library() {
        r.color = Some(Color32::from_rgb(0x8C, 0x81, 0x70));
    }
    r.open = u.pas.clone().filter(|_| u.exists);
    r
}

fn file_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn structure_panel(
    ui: &mut Ui,
    p: &Palette,
    info: &StructureInfo,
    state: &mut ExplorerState,
    actions: &mut Vec<ExplorerAction>,
) {
    let Some(outline) = info.outline else {
        let hint = if info.busy {
            "Reading the unit structure…"
        } else {
            "Classes, methods and form components of the active unit appear here."
        };
        empty_state(
            ui,
            p,
            if info.busy {
                "Analyzing…"
            } else {
                "No unit open"
            },
            hint,
        );
        return;
    };
    ui.label(RichText::new(info.title).strong().color(p.text));
    ui.add_space(4.0);
    if outline.symbols.is_empty() {
        empty_state(ui, p, "No symbols", "");
        return;
    }
    let mut rows = Vec::new();
    fn walk(
        list: &[Symbol],
        prefix: &str,
        depth: usize,
        collapsed: &HashSet<String>,
        rows: &mut Vec<Row>,
    ) {
        for (i, s) in list.iter().enumerate() {
            let id = format!("{prefix}/{i}");
            let icon = match s.kind {
                SymbolKind::Unit => "📦",
                SymbolKind::Uses | SymbolKind::Group => "📁",
                SymbolKind::UsedUnit => "📄",
                SymbolKind::Class | SymbolKind::Interface | SymbolKind::Object => "🔷",
                SymbolKind::Record => "🔶",
                SymbolKind::Field => "·",
                SymbolKind::Method | SymbolKind::Routine => "ƒ",
                SymbolKind::Property => "🔹",
            };
            let mut r = Row::new(id.clone(), depth, icon, &s.name);
            r.detail = if s.kind == SymbolKind::Group || s.kind == SymbolKind::Uses {
                s.children.len().to_string()
            } else {
                s.detail.clone()
            };
            r.children = !s.children.is_empty();
            r.target = Some((s.line, s.col));
            let default_open = !matches!(s.kind, SymbolKind::Uses | SymbolKind::Group);
            r.expanded = default_open != collapsed.contains(&id);
            let open = r.expanded;
            rows.push(r);
            if !s.children.is_empty() && open {
                walk(&s.children, &id, depth + 1, collapsed, rows);
            }
        }
    }
    walk(
        &outline.symbols,
        "s",
        0,
        &state.structure_toggled,
        &mut rows,
    );
    tree(
        ui,
        p,
        Id::new("structure_tree"),
        &rows,
        state,
        actions,
        false,
    );
}

/// Arvore generica: selecao, expandir/recolher, duplo clique, teclado e menu de contexto.
fn tree(
    ui: &mut Ui,
    p: &Palette,
    id: Id,
    rows: &[Row],
    state: &mut ExplorerState,
    actions: &mut Vec<ExplorerAction>,
    project: bool,
) {
    let row_h = 21.0;
    let area = ui.available_rect_before_wrap();
    let focus = ui.interact(area, id, Sense::click());
    if focus.clicked() {
        ui.memory_mut(|m| m.request_focus(id));
    }
    let focused = ui.memory(|m| m.has_focus(id));
    let selected_idx = rows
        .iter()
        .position(|r| Some(&r.id) == state.selected.as_ref());
    let mut toggle: Option<String> = None;
    let mut activate: Option<usize> = None;

    if focused && project {
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                id,
                EventFilter {
                    tab: false,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: false,
                },
            )
        });
        // as teclas da arvore sao consumidas: nao chegam ao editor que o Enter vai abrir
        let (up, down, left, right, enter) = ui.input_mut(|i| {
            let none = Modifiers::NONE;
            (
                i.consume_key(none, Key::ArrowUp),
                i.consume_key(none, Key::ArrowDown),
                i.consume_key(none, Key::ArrowLeft),
                i.consume_key(none, Key::ArrowRight),
                i.consume_key(none, Key::Enter),
            )
        });
        let cur = selected_idx.unwrap_or(0);
        let mut moved = None;
        if down && cur + 1 < rows.len() {
            moved = Some(cur + 1);
        }
        if up && cur > 0 {
            moved = Some(cur - 1);
        }
        if let Some(r) = rows.get(cur) {
            let is_open = state.expanded.contains(&r.id);
            if right && r.children && !is_open {
                toggle = Some(r.id.clone());
            }
            if left {
                if r.children && is_open {
                    toggle = Some(r.id.clone());
                } else if let Some(parent) = rows[..cur].iter().rposition(|x| x.depth < r.depth) {
                    moved = Some(parent);
                }
            }
            if enter {
                activate = Some(cur);
            }
        }
        if let Some(i) = moved {
            state.selected = Some(rows[i].id.clone());
            state.scroll_to = Some(i as f32 * row_h);
        }
    }

    let mut scroll = egui::ScrollArea::vertical()
        .id_salt(id.with("scroll"))
        .auto_shrink([false; 2]);
    if let Some(offset) = state.scroll_to.take() {
        scroll = scroll.vertical_scroll_offset((offset - 3.0 * row_h).max(0.0));
    }
    scroll.show_rows(ui, row_h, rows.len(), |ui, range| {
        for i in range {
            let r = &rows[i];
            ui.horizontal(|ui| {
                ui.set_height(row_h);
                ui.add_space(r.depth as f32 * 14.0);
                let is_open = if project {
                    state.expanded.contains(&r.id)
                } else {
                    r.expanded
                };
                if r.children {
                    let arrow = RichText::new(if is_open { "▼" } else { "▶" })
                        .size(9.0)
                        .color(p.text_weak);
                    if ui
                        .add(
                            egui::Button::new(arrow)
                                .frame(false)
                                .min_size(egui::vec2(14.0, 14.0)),
                        )
                        .clicked()
                    {
                        toggle = Some(r.id.clone());
                    }
                } else {
                    ui.add_space(18.0);
                }
                let color = r.color.unwrap_or(p.text);
                let selected = state.selected.as_ref() == Some(&r.id);
                let response = ui.selectable_label(
                    selected,
                    RichText::new(format!("{} {}", r.icon, r.label)).color(color),
                );
                if !r.detail.is_empty() {
                    ui.label(RichText::new(&r.detail).size(11.5).color(p.text_weak));
                }
                if response.clicked() {
                    state.selected = Some(r.id.clone());
                    ui.memory_mut(|m| m.request_focus(id));
                    if !project {
                        activate = Some(i);
                    }
                }
                if response.double_clicked() && project {
                    activate = Some(i);
                }
                if project {
                    response.context_menu(|ui| {
                        if let Some(path) = &r.open {
                            if ui.button("Open").clicked() {
                                actions.push(ExplorerAction::Open(path.clone()));
                                ui.close_menu();
                            }
                        }
                        if let Some(unit) = r.unit.as_ref().filter(|_| r.id.starts_with("unit:")) {
                            if let Some(pas) = &r.open {
                                if ui.button("Rename…").clicked() {
                                    actions.push(ExplorerAction::Rename(pas.clone()));
                                    ui.close_menu();
                                }
                            }
                            if ui.button("Remove from Project…").clicked() {
                                actions.push(ExplorerAction::Remove(unit.clone()));
                                ui.close_menu();
                            }
                        }
                        if ui.button("Add Existing Unit…").clicked() {
                            actions.push(ExplorerAction::Command(Command::AddToProject));
                            ui.close_menu();
                        }
                    });
                }
            });
        }
    });

    if let Some(i) = activate {
        let r = &rows[i];
        match (&r.open, r.target) {
            (_, Some((line, col))) => actions.push(ExplorerAction::Navigate(line, col)),
            (Some(path), _) => actions.push(ExplorerAction::Open(path.clone())),
            (None, _) if r.children => toggle = Some(r.id.clone()),
            _ => {}
        }
    }
    if let Some(t) = toggle {
        let set = if project {
            &mut state.expanded
        } else {
            &mut state.structure_toggled
        };
        if !set.remove(&t) {
            set.insert(t);
        }
    }
}
