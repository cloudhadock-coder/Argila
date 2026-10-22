//! Estado da aplicacao e execucao de comandos. A interface (`ui`) so devolve pedidos; aqui eles
//! viram acoes, sempre passando pelo mesmo caminho (menu, barra, atalho ou botao).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::time::Instant;

use eframe::egui::{self, Color32, FontId, Id, Margin, RichText, Stroke, ViewportCommand};
use encoding_rs::Encoding;

use crate::diagnostics::{Level, MessageLog};
use crate::editor::document::{Document, SaveError, SaveOptions};
use crate::editor::view::{EditorStyle, EditorView};
use crate::filesystem::{self, encoding};
use crate::projects::files::{self, Plan};
use crate::projects::outline::{self, Outline};
use crate::projects::{Project, ProjectKind, related_file};
use crate::settings::{BottomTab, LeftTab, SessionFile, SessionSettings, Settings, SettingsStore};
use crate::shortcuts::{Availability, Command, KeyChord, Keymap, USER_KEYMAP_FILE};
use crate::syntax::Language;
use crate::themes::{self, EARTHY, Palette};
use crate::ui::explorer::{ExplorerAction, ExplorerState, StructureInfo};
use crate::ui::inspector::InspectorItem;
use crate::ui::{self, console, dialogs, explorer, inspector, statusbar, toolbar, welcome};

pub struct EditorTab {
    pub id: u64,
    pub doc: Document,
    pub view: EditorView,
    pub dfm: crate::dfm::Preview,
    outline: Option<Arc<Outline>>,
    outline_version: Option<u64>,
    outline_busy: bool,
    seen_version: u64,
    changed_at: f64,
}

/// Abas da area central. Cada DFM compartilha o documento entre Text e Design.
pub enum Tab {
    Welcome,
    Editor(Box<EditorTab>),
}

impl Tab {
    fn title(&self) -> String {
        match self {
            Tab::Welcome => "Welcome".to_owned(),
            Tab::Editor(e) if e.doc.is_modified() => format!("{} ●", e.doc.file_name()),
            Tab::Editor(e) => e.doc.file_name(),
        }
    }

    fn is_modified(&self) -> bool {
        matches!(self, Tab::Editor(e) if e.doc.is_modified())
    }

    fn path(&self) -> Option<&Path> {
        match self {
            Tab::Editor(e) => Some(&e.doc.path),
            Tab::Welcome => None,
        }
    }
}

/// O que continuar depois que um salvamento pedido por um dialogo der certo.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Then {
    Nothing,
    CloseTab,
    Exit,
    CloseProject,
}

/// Dialogos modais: nada e descartado, sobrescrito ou alterado sem o usuario escolher.
enum Prompt {
    CloseModified(usize),
    ExitModified,
    CloseProjectModified,
    Unencodable {
        tab: usize,
        message: String,
        then: Then,
    },
    ChangedOnDisk {
        tab: usize,
        then: Then,
    },
    Lossy {
        tab: usize,
        then: Then,
    },
    GotoLine(String),
    UnitList {
        forms: bool,
        filter: String,
    },
    NewUnit {
        name: String,
        add: bool,
    },
    Rename {
        pas: PathBuf,
        name: String,
    },
    Confirm(Box<Plan>),
}

enum SaveResult {
    Saved,
    /// Um dialogo foi aberto para o usuario decidir.
    Pending,
    Failed,
}

struct Flash {
    text: String,
    level: Level,
    until: f64,
}

/// Projeto sendo lido em segundo plano.
struct Loading {
    path: PathBuf,
    name: String,
    rx: Receiver<Result<Project, String>>,
}

/// Estatistica de tempo de quadro (ARGILA_PERF=1 imprime no stderr a cada 2 s).
#[derive(Default)]
struct Perf {
    frames: u32,
    total: f32,
    max: f32,
    since: f64,
}

pub struct ArgilaApp {
    ctx: egui::Context,
    palette: Palette,
    settings: Settings,
    store: SettingsStore,
    keymap: Keymap,
    log: MessageLog,
    fonts: Vec<&'static str>,
    no_bom_default: &'static Encoding,
    tabs: Vec<Tab>,
    active: usize,
    next_id: u64,
    cursor: Option<(usize, usize)>,
    project: Option<Project>,
    loading: Option<Loading>,
    explorer: ExplorerState,
    outline_tx: Sender<(u64, u64, Outline)>,
    outline_rx: Receiver<(u64, u64, Outline)>,
    options: dialogs::OptionsState,
    show_options: bool,
    inspector_filter: String,
    binary_dfm: Option<PathBuf>,
    flash: Option<Flash>,
    pending: Vec<Command>,
    prompt: Option<Prompt>,
    allow_close: bool,
    /// Pedido de fechar a janela depois de salvar/descartar tudo.
    pending_close: bool,
    show_shortcuts: bool,
    show_about: bool,
    perf: Option<Perf>,
    /// Momento da ultima alteracao ainda nao gravada (gravacao com atraso para nao martelar o disco).
    dirty_since: Option<f64>,
}

fn is_project_path(path: &Path) -> bool {
    path.is_dir()
        || matches!(
            path.extension()
                .and_then(|e| e.to_str())
                .map(str::to_ascii_lowercase)
                .as_deref(),
            Some("dproj" | "dpr" | "dpk")
        )
}

fn same_path(a: &Path, b: &Path) -> bool {
    a.as_os_str().eq_ignore_ascii_case(b.as_os_str())
}

impl ArgilaApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        store: SettingsStore,
        settings: Settings,
        warning: Option<String>,
        files: Vec<PathBuf>,
    ) -> Self {
        let palette = EARTHY;
        themes::apply(&cc.egui_ctx, &palette);
        let fonts = themes::install_fonts(&cc.egui_ctx);

        let mut log = MessageLog::default();
        log.info(format!(
            "Argila {} started. Settings: {}",
            env!("CARGO_PKG_VERSION"),
            store.path().display()
        ));
        if let Some(w) = warning {
            log.warning(w);
        }
        let (keymap, keymap_warnings) = Keymap::load_user(&filesystem::config_dir());
        for w in keymap_warnings {
            log.warning(w);
        }
        for c in keymap.conflicts() {
            let names: Vec<_> = c.commands.iter().map(|c| c.label()).collect();
            log.warning(format!(
                "Shortcut conflict: {} is bound to {}; the first one wins.",
                c.chord,
                names.join(", ")
            ));
        }
        let configured = settings.editor.default_encoding.trim();
        let no_bom_default = if configured.eq_ignore_ascii_case("system") || configured.is_empty() {
            encoding::system_ansi_encoding()
        } else {
            encoding::encoding_for_label(configured).unwrap_or_else(|| {
                log.warning(format!(
                    "Unknown default_encoding '{configured}'; using the system ANSI code page."
                ));
                encoding::system_ansi_encoding()
            })
        };
        log.info(format!(
            "Files without BOM that are not UTF-8 open as {}.",
            encoding::TextEncoding::Legacy(no_bom_default).label()
        ));

        let (outline_tx, outline_rx) = channel();
        let session = settings.session.clone();
        let mut app = Self {
            ctx: cc.egui_ctx.clone(),
            palette,
            settings,
            store,
            keymap,
            log,
            fonts,
            no_bom_default,
            tabs: vec![Tab::Welcome],
            active: 0,
            next_id: 0,
            cursor: None,
            project: None,
            loading: None,
            explorer: ExplorerState::default(),
            outline_tx,
            outline_rx,
            options: dialogs::OptionsState::default(),
            show_options: false,
            inspector_filter: String::new(),
            binary_dfm: None,
            flash: None,
            pending: Vec::new(),
            prompt: None,
            allow_close: false,
            pending_close: false,
            show_shortcuts: false,
            show_about: false,
            perf: std::env::var_os("ARGILA_PERF").map(|_| Perf::default()),
            dirty_since: None,
        };
        app.restore_session(&session, files);
        app
    }

    /// Ultimo projeto, arquivos abertos (com a linha do cursor) e estado da arvore.
    /// Arquivos da linha de comando vem depois; um projeto na linha de comando substitui o da sessao.
    fn restore_session(&mut self, session: &SessionSettings, cli: Vec<PathBuf>) {
        self.explorer.selected = session.selected.clone();
        self.explorer.expanded = session.expanded.iter().cloned().collect();
        let cli_project = cli.iter().find(|p| is_project_path(p)).cloned();
        match cli_project.or_else(|| session.project.as_ref().map(PathBuf::from)) {
            Some(p) if p.exists() => self.load_project(p),
            Some(p) => self
                .log
                .warning(format!("Last project not found: {}", p.display())),
            None => {}
        }
        for f in &session.files {
            let path = Path::new(&f.path);
            if !path.is_file() {
                self.log.warning(format!(
                    "Previously open file not found: {}",
                    path.display()
                ));
                continue;
            }
            self.open_path(path, 0.0);
            if let Some(ed) = self.active_editor() {
                ed.view.goto(&mut ed.doc, f.line.saturating_sub(1), 0);
            }
        }
        if let Some(active) = &session.active {
            if let Some(i) = self
                .tabs
                .iter()
                .position(|t| t.path().is_some_and(|p| same_path(p, Path::new(active))))
            {
                self.active = i;
            }
        }
        for path in cli.iter().filter(|p| !is_project_path(p)) {
            self.open_path(path, 0.0);
        }
    }

    fn session_snapshot(&self) -> SessionSettings {
        let project = self
            .project
            .as_ref()
            .map(|p| &p.file)
            .or(self.loading.as_ref().map(|l| &l.path));
        let mut expanded: Vec<String> = self.explorer.expanded.iter().cloned().collect();
        expanded.sort();
        SessionSettings {
            project: project.map(|p| p.display().to_string()),
            files: self
                .tabs
                .iter()
                .filter_map(|t| match t {
                    Tab::Editor(e) => Some(SessionFile {
                        path: e.doc.path.display().to_string(),
                        line: e.doc.buffer.line_of(e.doc.buffer.selection.head) + 1,
                    }),
                    Tab::Welcome => None,
                })
                .collect(),
            active: self
                .tabs
                .get(self.active)
                .and_then(Tab::path)
                .map(|p| p.display().to_string()),
            selected: self.explorer.selected.clone(),
            expanded,
        }
    }

    fn keymap_path() -> PathBuf {
        filesystem::config_dir().join(USER_KEYMAP_FILE)
    }

    fn active_editor(&mut self) -> Option<&mut EditorTab> {
        match self.tabs.get_mut(self.active) {
            Some(Tab::Editor(e)) => Some(e),
            _ => None,
        }
    }

    fn active_path(&self) -> Option<&Path> {
        self.tabs.get(self.active).and_then(Tab::path)
    }

    fn has_editor(&self) -> bool {
        matches!(self.tabs.get(self.active), Some(Tab::Editor(_)))
    }

    /// Unit do Project Explorer selecionada (ou a do arquivo ativo).
    fn target_unit(&self) -> Option<&crate::projects::ProjectUnit> {
        let project = self.project.as_ref()?;
        let selected = self
            .explorer
            .selected
            .as_deref()
            .and_then(|s| s.strip_prefix("unit:").or_else(|| s.strip_prefix("dfm:")));
        match selected {
            Some(name) => project.units.iter().find(|u| u.name.to_lowercase() == name),
            None => self.active_path().and_then(|p| project.unit_for_path(p)),
        }
    }

    /// .pas alvo de "Rename Unit": a unit selecionada ou o .pas ativo.
    fn rename_target(&self) -> Option<PathBuf> {
        if let Some(u) = self.target_unit().filter(|u| u.exists) {
            return u.pas.clone();
        }
        self.active_path()
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pas")))
            .map(Path::to_path_buf)
    }

    /// Unico lugar que decide se um comando pode rodar.
    pub fn availability(&self, cmd: Command) -> Availability {
        use Command::*;
        let needs = |ok: bool, why: &str| {
            if ok {
                Availability::Ready
            } else {
                Availability::Unavailable(why.to_owned())
            }
        };
        let editable_project = self
            .project
            .as_ref()
            .is_some_and(|p| p.kind != ProjectKind::Folder);
        match cmd {
            Exit | ProjectManager | Structure | ObjectInspector | Messages | Welcome | ExportKeymap | OpenSettingsFolder
            | KeyboardShortcuts | About | OpenFile | OpenProject | OpenFolder | NewUnit => Availability::Ready,
            CloseTab => needs(!self.tabs.is_empty(), "No open tabs."),
            CloseProject | ViewUnits | ViewForms | ProjectOptions => needs(self.project.is_some(), "No project open (File ▸ Open Project, Ctrl+F11)."),
            AddToProject => needs(editable_project, "Open a .dproj, .dpr or .dpk project first."),
            RemoveFromProject => needs(editable_project && self.target_unit().is_some(), "Select a unit of the project first."),
            RenameUnit => needs(self.rename_target().is_some(), "Select a unit or open a .pas file first."),
            SaveAll => needs(self.tabs.iter().any(Tab::is_modified) || self.has_editor(), "Open a file first (File ▸ Open, Ctrl+O)."),
            Save | Undo | Redo | Find | Replace | SearchAgain | GotoLine | ToggleComment | ToggleFormUnit | DeclImplToggle
            | FindDeclaration => needs(self.has_editor(), "Open a file first (File ▸ Open, Ctrl+O)."),
            Compile | Build | Run | RunWithoutDebugging => Availability::Unavailable(
                "No compiler configured. DCC32/DCC64 integration is planned for phase 6 and needs a licensed Delphi installation."
                    .to_owned(),
            ),
            _ => Availability::NotImplemented,
        }
    }

    /// Fase do roteiro em que o comando passa a funcionar (para a mensagem de "nao implementado").
    fn planned_phase(cmd: Command) -> u8 {
        match cmd {
            Command::CodeCompletion | Command::ClassCompletion | Command::FindInFiles => 5,
            _ => 6,
        }
    }

    fn checked(&self, cmd: Command) -> Option<bool> {
        let l = &self.settings.layout;
        match cmd {
            Command::ProjectManager => Some(l.show_left && l.left_tab == LeftTab::ProjectExplorer),
            Command::Structure => Some(l.show_left && l.left_tab == LeftTab::Structure),
            Command::ObjectInspector => Some(l.show_right),
            Command::Messages => Some(l.show_bottom && l.bottom_tab == BottomTab::Messages),
            _ => None,
        }
    }

    fn notify(&mut self, level: Level, text: String, now: f64) {
        self.flash = Some(Flash {
            text: text.clone(),
            level,
            until: now + 4.0,
        });
        self.log.push(level, text);
    }

    fn mark_dirty(&mut self, now: f64) {
        self.dirty_since.get_or_insert(now);
    }

    fn run(&mut self, ctx: &egui::Context, cmd: Command) {
        let now = ctx.input(|i| i.time);
        match self.availability(cmd) {
            Availability::Ready => {}
            Availability::NotImplemented => {
                let text = format!(
                    "{}: not implemented yet (planned for phase {}).",
                    cmd.label(),
                    Self::planned_phase(cmd)
                );
                self.notify(Level::Hint, text, now);
                return;
            }
            Availability::Unavailable(reason) => {
                self.notify(Level::Warning, format!("{}: {reason}", cmd.label()), now);
                return;
            }
        }
        let layout = &mut self.settings.layout;
        match cmd {
            Command::Exit => ctx.send_viewport_cmd(ViewportCommand::Close),
            Command::OpenFile => self.open_dialog(now),
            Command::OpenProject => {
                let mut dialog = rfd::FileDialog::new()
                    .set_title("Open Project")
                    .add_filter("Delphi projects", &["dproj", "dpr", "dpk"])
                    .add_filter("All files", &["*"]);
                if let Some(dir) = self
                    .settings
                    .recent
                    .projects
                    .first()
                    .and_then(|p| Path::new(p).parent())
                    .filter(|d| d.is_dir())
                {
                    dialog = dialog.set_directory(dir);
                }
                if let Some(path) = dialog.pick_file() {
                    self.load_project(path);
                }
            }
            Command::OpenFolder => {
                if let Some(dir) = rfd::FileDialog::new()
                    .set_title("Open Folder")
                    .pick_folder()
                {
                    self.load_project(dir);
                }
            }
            Command::CloseProject => {
                if self.project_tabs_modified() {
                    self.prompt = Some(Prompt::CloseProjectModified);
                } else {
                    self.close_project_now(now);
                }
            }
            Command::Save => {
                self.save_tab(self.active, SaveOptions::default(), Then::Nothing, now);
            }
            Command::SaveAll => self.save_all(Then::Nothing, now),
            Command::CloseTab => self.request_close_tab(self.active),
            Command::Undo
            | Command::Redo
            | Command::Find
            | Command::Replace
            | Command::SearchAgain
            | Command::ToggleComment => {
                if let Some(ed) = self.active_editor() {
                    ed.dfm.design = false;
                    let (doc, view) = (&mut ed.doc, &mut ed.view);
                    match cmd {
                        Command::Undo => view.undo(doc),
                        Command::Redo => view.redo(doc),
                        Command::Find => view.open_find(doc, false),
                        Command::Replace => view.open_find(doc, true),
                        Command::SearchAgain => view.find_next(doc, false),
                        _ => view.toggle_comment(doc, now),
                    }
                }
            }
            Command::GotoLine => {
                if let Some(ed) = self.active_editor() {
                    ed.dfm.design = false;
                }
                self.prompt = Some(Prompt::GotoLine(String::new()));
            }
            Command::ToggleFormUnit => self.toggle_form_unit(now),
            Command::DeclImplToggle => self.toggle_decl_impl(now),
            Command::FindDeclaration => self.find_declaration(now),
            Command::ViewUnits => {
                self.prompt = Some(Prompt::UnitList {
                    forms: false,
                    filter: String::new(),
                })
            }
            Command::ViewForms => {
                self.prompt = Some(Prompt::UnitList {
                    forms: true,
                    filter: String::new(),
                })
            }
            Command::NewUnit => {
                let add = self
                    .project
                    .as_ref()
                    .is_some_and(|p| p.kind != ProjectKind::Folder);
                self.prompt = Some(Prompt::NewUnit {
                    name: self.next_unit_name(),
                    add,
                });
            }
            Command::AddToProject => {
                let dir = self.project.as_ref().map(|p| p.dir.clone());
                let mut dialog = rfd::FileDialog::new()
                    .set_title("Add to Project")
                    .add_filter("Delphi units", &["pas"]);
                if let Some(dir) = dir {
                    dialog = dialog.set_directory(dir);
                }
                if let (Some(pas), Some(project)) = (dialog.pick_file(), &self.project) {
                    let plan = files::plan_add_unit(project, &pas);
                    self.confirm_plan(plan, now);
                }
            }
            Command::RemoveFromProject => {
                if let (Some(project), Some(unit)) = (&self.project, self.target_unit()) {
                    let plan = files::plan_remove_unit(project, unit);
                    self.confirm_plan(plan, now);
                }
            }
            Command::RenameUnit => {
                if let Some(pas) = self.rename_target() {
                    let name = pas
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    self.prompt = Some(Prompt::Rename { pas, name });
                }
            }
            Command::ProjectOptions => self.show_options = true,
            Command::Welcome => match self.tabs.iter().position(|t| matches!(t, Tab::Welcome)) {
                Some(i) => self.active = i,
                None => {
                    self.tabs.push(Tab::Welcome);
                    self.active = self.tabs.len() - 1;
                }
            },
            Command::ProjectManager => toggle_left(layout, LeftTab::ProjectExplorer),
            Command::Structure => toggle_left(layout, LeftTab::Structure),
            Command::ObjectInspector => layout.show_right = !layout.show_right,
            Command::Messages => {
                if layout.show_bottom && layout.bottom_tab == BottomTab::Messages {
                    layout.show_bottom = false;
                } else {
                    layout.show_bottom = true;
                    layout.bottom_tab = BottomTab::Messages;
                }
            }
            Command::ExportKeymap => self.export_keymap(now),
            Command::OpenSettingsFolder => self.open_settings_folder(now),
            Command::KeyboardShortcuts => self.show_shortcuts = true,
            Command::About => self.show_about = true,
            other => self.notify(
                Level::Error,
                format!("{}: marked ready but has no action.", other.label()),
                now,
            ),
        }
    }

    // ------------------------------------------------------------------ projeto

    fn load_project(&mut self, path: PathBuf) {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let (tx, rx) = channel();
        let (ctx, target) = (self.ctx.clone(), path.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Project::load(&target));
            ctx.request_repaint();
        });
        self.loading = Some(Loading { path, name, rx });
    }

    fn poll_loading(&mut self, now: f64) {
        let Some(loading) = &self.loading else { return };
        let result = match loading.rx.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                Err("the project loader stopped unexpectedly".to_owned())
            }
        };
        let path = self.loading.take().map(|l| l.path).unwrap_or_default();
        match result {
            Ok(project) => {
                let forms = project.units.iter().filter(|u| u.dfm.is_some()).count();
                let text = format!(
                    "Opened project {} ({}): {} units, {forms} with form, {} missing, in {:.0} ms.",
                    project.name,
                    project.kind.label(),
                    project.units.len(),
                    project.missing_count(),
                    project.load_ms
                );
                self.notify(
                    if project.problems.is_empty() {
                        Level::Info
                    } else {
                        Level::Warning
                    },
                    text,
                    now,
                );
                for problem in project.problems.iter().take(40) {
                    self.log.warning(problem.clone());
                }
                if project.problems.len() > 40 {
                    self.log.warning(format!(
                        "… and {} more problems.",
                        project.problems.len() - 40
                    ));
                }
                if self.explorer.expanded.is_empty() {
                    self.explorer
                        .expanded
                        .extend(["project".to_owned(), "units".to_owned()]);
                }
                self.settings
                    .recent
                    .push_project(&project.file.display().to_string());
                self.project = Some(project);
                self.mark_dirty(now);
            }
            Err(e) => self.notify(
                Level::Error,
                format!("Could not open project {}: {e}", path.display()),
                now,
            ),
        }
    }

    fn belongs_to_project(project: &Project, path: &Path) -> bool {
        project.unit_for_path(path).is_some()
            || [&project.source, &project.dproj_path]
                .iter()
                .any(|p| p.as_deref().is_some_and(|p| same_path(p, path)))
    }

    fn project_tabs_modified(&self) -> bool {
        let Some(project) = &self.project else {
            return false;
        };
        self.tabs.iter().any(|t| {
            t.is_modified()
                && t.path()
                    .is_some_and(|p| Self::belongs_to_project(project, p))
        })
    }

    /// Fecha o projeto e as abas dos arquivos dele (a aplicacao continua aberta).
    fn close_project_now(&mut self, now: f64) {
        let Some(project) = self.project.take() else {
            return;
        };
        self.tabs.retain(|t| {
            !t.path()
                .is_some_and(|p| Self::belongs_to_project(&project, p))
        });
        self.active = self.active.min(self.tabs.len().saturating_sub(1));
        self.explorer.selected = None;
        self.show_options = false;
        self.notify(
            Level::Info,
            format!("Closed project {}.", project.name),
            now,
        );
        self.mark_dirty(now);
    }

    fn next_unit_name(&self) -> String {
        let dir = self.new_unit_dir();
        (1..1000)
            .map(|i| format!("Unit{i}"))
            .find(|n| {
                let free_on_disk = dir
                    .as_ref()
                    .is_none_or(|d| !d.join(format!("{n}.pas")).exists());
                let free_in_project = self
                    .project
                    .as_ref()
                    .is_none_or(|p| !p.units.iter().any(|u| u.name.eq_ignore_ascii_case(n)));
                free_on_disk && free_in_project
            })
            .unwrap_or_else(|| "UnitNova".to_owned())
    }

    fn new_unit_dir(&self) -> Option<PathBuf> {
        self.project.as_ref().map(|p| p.dir.clone()).or_else(|| {
            self.active_path()
                .and_then(Path::parent)
                .map(Path::to_path_buf)
        })
    }

    fn confirm_plan(&mut self, plan: Result<Plan, String>, now: f64) {
        match plan {
            Ok(plan) => self.prompt = Some(Prompt::Confirm(Box::new(plan))),
            Err(e) => self.notify(Level::Error, e, now),
        }
    }

    /// Grava um plano confirmado. Abas abertas acompanham os arquivos (recarregados ou renomeados).
    fn apply_plan(&mut self, plan: &Plan, now: f64) {
        let touched: Vec<&Path> = plan
            .edits
            .iter()
            .map(|e| e.path.as_path())
            .chain(plan.moves.iter().map(|m| m.0.as_path()))
            .collect();
        if let Some(t) = self.tabs.iter().find(|t| {
            t.is_modified()
                && t.path()
                    .is_some_and(|p| touched.iter().any(|x| same_path(x, p)))
        }) {
            let text = format!(
                "Save or close {} first: it has unsaved changes and would be changed by \"{}\".",
                t.title(),
                plan.title
            );
            self.notify(Level::Warning, text, now);
            return;
        }
        match files::apply(plan, &filesystem::config_dir().join("backups")) {
            Ok(backup) => {
                self.log.info(format!("{}:", plan.title));
                for line in plan.describe() {
                    self.log.info(format!("  {line}"));
                }
                if let Some(dir) = backup {
                    self.log.info(format!(
                        "  Backup of the previous versions: {}",
                        dir.display()
                    ));
                }
                let no_bom = self.no_bom_default;
                for tab in &mut self.tabs {
                    let Tab::Editor(e) = tab else { continue };
                    let moved = plan
                        .moves
                        .iter()
                        .find(|(from, _)| same_path(from, &e.doc.path))
                        .map(|m| m.1.clone());
                    let edited = plan.edits.iter().any(|x| same_path(&x.path, &e.doc.path));
                    if moved.is_some() || edited {
                        let path = moved.unwrap_or_else(|| e.doc.path.clone());
                        if let Ok(doc) = Document::open(&path, no_bom) {
                            e.doc = doc;
                            e.view.reset();
                            e.outline_version = None;
                        }
                    }
                }
                if let Some((created, _)) = plan.creates.first() {
                    let created = created.clone();
                    self.open_path(&created, now);
                }
                self.notify(Level::Info, format!("{}: done.", plan.title), now);
                if let Some(project) = &self.project {
                    let file = project.file.clone();
                    self.load_project(file);
                }
            }
            Err(e) => self.notify(Level::Error, format!("{}: {e}", plan.title), now),
        }
    }

    // ------------------------------------------------------------------ navegacao

    fn toggle_form_unit(&mut self, now: f64) {
        if let Some(ed) = self.active_editor() {
            if ed.doc.language == Language::Dfm && !ed.dfm.design {
                ed.dfm.design = true;
                return;
            }
            if ed.doc.language == Language::Dfm && related_file(&ed.doc.path).is_none() {
                ed.dfm.design = false;
                return;
            }
        }
        let Some(path) = self.active_path().map(Path::to_path_buf) else {
            return;
        };
        match related_file(&path) {
            Some(other) => {
                self.open_path(&other, now);
                if let Some(ed) = self.active_editor() {
                    if ed.doc.language == Language::Dfm {
                        ed.dfm.design = true;
                    }
                }
            }
            None => {
                let what = if path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("dfm"))
                {
                    ".pas"
                } else {
                    ".dfm"
                };
                self.notify(
                    Level::Hint,
                    format!("{} has no {what} next to it.", path.display()),
                    now,
                );
            }
        }
    }

    /// Outline do editor ativo; se estiver desatualizado, calcula agora (acao explicita do usuario).
    fn fresh_outline(&mut self) -> Option<Arc<Outline>> {
        let ed = self.active_editor()?;
        let version = ed.doc.buffer.version();
        if ed.outline_version != Some(version) || ed.outline.is_none() {
            let text = ed.doc.buffer.text();
            let o = if ed.doc.language == Language::Dfm {
                outline::dfm(&text)
            } else {
                outline::pascal(&text)
            };
            ed.outline = Some(Arc::new(o));
            ed.outline_version = Some(version);
        }
        ed.outline.clone()
    }

    fn toggle_decl_impl(&mut self, now: f64) {
        let Some(o) = self.fresh_outline() else {
            return;
        };
        let Some(ed) = self.active_editor() else {
            return;
        };
        let line = ed.doc.buffer.line_of(ed.doc.buffer.selection.head);
        match o.decl_impl_target(line) {
            Some((l, c)) => ed.view.goto(&mut ed.doc, l, c),
            None => self.notify(
                Level::Hint,
                "No method declaration or implementation at the cursor.".to_owned(),
                now,
            ),
        }
    }

    /// Alt+Up / Ctrl+clique: unit do uses abre o arquivo; tipo ou metodo desta unit vai ate a declaracao.
    fn find_declaration(&mut self, now: f64) {
        let Some(o) = self.fresh_outline() else {
            return;
        };
        let Some(ed) = self.active_editor() else {
            return;
        };
        let Some((word, line, col)) = EditorView::word_at_caret(&ed.doc) else {
            return;
        };
        let dir = ed
            .doc
            .path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let unit_name = o.used_unit_at(line, col).map(str::to_owned).or_else(|| {
            self.project
                .as_ref()
                .and_then(|p| p.units.iter().find(|u| u.name.eq_ignore_ascii_case(&word)))
                .map(|u| u.name.clone())
        });
        if let Some(name) = unit_name {
            let file = self
                .project
                .as_ref()
                .and_then(|p| p.resolve_unit(&name))
                .or_else(|| Some(dir.join(format!("{name}.pas"))).filter(|p| p.is_file()));
            match file {
                Some(f) => self.open_path(&f, now),
                None => self.notify(
                    Level::Hint,
                    format!("{name} is not part of the project or its search path (library unit)."),
                    now,
                ),
            }
            return;
        }
        let short = word.rsplit('.').next().unwrap_or(&word).to_owned();
        match o.find_symbol(&word).or_else(|| o.find_symbol(&short)) {
            Some((l, c)) => {
                if let Some(ed) = self.active_editor() {
                    ed.view.goto(&mut ed.doc, l, c);
                }
            }
            None => self.notify(Level::Hint, format!("Declaration of {word} not found in this unit (project-wide symbols come in phase 5)."), now),
        }
    }

    /// Outline da aba ativa em segundo plano, 0,4 s depois da ultima edicao.
    fn update_outline(&mut self, now: f64) {
        while let Ok((id, version, outline)) = self.outline_rx.try_recv() {
            for tab in &mut self.tabs {
                if let Tab::Editor(e) = tab {
                    if e.id == id {
                        e.outline = Some(Arc::new(outline));
                        e.outline_version = Some(version);
                        e.outline_busy = false;
                        break;
                    }
                }
            }
        }
        let (ctx, tx) = (self.ctx.clone(), self.outline_tx.clone());
        let Some(Tab::Editor(e)) = self.tabs.get_mut(self.active) else {
            return;
        };
        let version = e.doc.buffer.version();
        if version != e.seen_version {
            e.seen_version = version;
            e.changed_at = now;
        }
        if e.outline_busy
            || e.outline_version == Some(version)
            || e.doc.language == Language::PlainText
        {
            return;
        }
        if e.outline.is_some() && now - e.changed_at < 0.4 {
            ctx.request_repaint_after(std::time::Duration::from_millis(450));
            return;
        }
        let (text, language, id) = (e.doc.buffer.text(), e.doc.language, e.id);
        e.outline_busy = true;
        std::thread::spawn(move || {
            let o = if language == Language::Dfm {
                outline::dfm(&text)
            } else {
                outline::pascal(&text)
            };
            let _ = tx.send((id, version, o));
            ctx.request_repaint();
        });
    }

    fn inspector_item(&self) -> Option<InspectorItem> {
        if let Some(Tab::Editor(ed)) = self.tabs.get(self.active) {
            if ed.doc.language == Language::Dfm {
                if ed.dfm.version != Some(ed.doc.buffer.version()) {
                    return None;
                }
                if let Some(Ok(form)) = &ed.dfm.result {
                    let c = form.root.find(&ed.dfm.selected).unwrap_or(&form.root);
                    let mut rows = vec![
                        ("Hierarchy".into(), ed.dfm.selected.clone()),
                        ("Declaration".into(), format!("{:?}", c.kind)),
                    ];
                    if c.kind == crate::dfm::parser::ObjectKind::Inherited {
                        rows.push((
                            "Inheritance".into(),
                            "Stored overrides only; base properties are not resolved.".into(),
                        ));
                    }
                    rows.extend(c.properties.iter().map(|p| {
                        let value = if matches!(p.value, crate::dfm::parser::Value::Binary) {
                            format!(
                                "Binary data ({} hex digits); inspect full value in Text.",
                                p.raw.chars().filter(char::is_ascii_hexdigit).count()
                            )
                        } else {
                            let mut s: String = p.raw.chars().take(1000).collect();
                            if p.raw.chars().nth(1000).is_some() {
                                s.push_str("… [full value in Text]");
                            }
                            s
                        };
                        (p.name.clone(), value)
                    }));
                    return Some(InspectorItem {
                        title: c.name.clone(),
                        class: c.class.clone(),
                        rows,
                    });
                }
                return None;
            }
        }
        let project = self.project.as_ref()?;
        let sel = self.explorer.selected.as_deref()?;
        let yes = |b: bool| if b { "Yes" } else { "No" }.to_owned();
        let path = |p: &Option<PathBuf>| {
            p.as_ref()
                .map_or("—".to_owned(), |p| p.display().to_string())
        };
        if sel == "project" {
            let d = project.dproj.as_ref();
            return Some(InspectorItem {
                title: project.name.clone(),
                class: project.kind.label().to_owned(),
                rows: vec![
                    ("File".into(), project.file.display().to_string()),
                    ("Folder".into(), project.dir.display().to_string()),
                    ("Main source".into(), path(&project.source)),
                    (".dproj".into(), path(&project.dproj_path)),
                    ("Units".into(), project.units.len().to_string()),
                    (
                        "With form".into(),
                        project
                            .units
                            .iter()
                            .filter(|u| u.dfm.is_some())
                            .count()
                            .to_string(),
                    ),
                    ("Missing".into(), project.missing_count().to_string()),
                    (
                        "Library units".into(),
                        project
                            .units
                            .iter()
                            .filter(|u| u.is_library())
                            .count()
                            .to_string(),
                    ),
                    (
                        "Configurations".into(),
                        d.map(|d| d.config_names().join(", ")).unwrap_or_default(),
                    ),
                    (
                        "Platforms".into(),
                        d.map(|d| d.platform_names().join(", ")).unwrap_or_default(),
                    ),
                    (
                        "ProjectVersion".into(),
                        d.and_then(|d| d.global("ProjectVersion"))
                            .unwrap_or("—")
                            .to_owned(),
                    ),
                    ("Problems".into(), project.problems.len().to_string()),
                    ("Load time".into(), format!("{:.0} ms", project.load_ms)),
                ],
            });
        }
        if let Some(name) = sel
            .strip_prefix("unit:")
            .or_else(|| sel.strip_prefix("dfm:"))
        {
            let u = project
                .units
                .iter()
                .find(|u| u.name.to_lowercase() == name)?;
            let status = if u.is_missing() {
                "Missing"
            } else if u.is_library() {
                "Library unit (outside the project)"
            } else {
                "OK"
            };
            return Some(InspectorItem {
                title: u.name.clone(),
                class: if sel.starts_with("dfm:") {
                    "Form file".into()
                } else {
                    "Unit".into()
                },
                rows: vec![
                    ("Status".into(), status.to_owned()),
                    ("File".into(), path(&u.pas)),
                    (
                        "Declared path".into(),
                        u.declared_path.clone().unwrap_or_else(|| "—".into()),
                    ),
                    ("Form".into(), u.form.clone().unwrap_or_else(|| "—".into())),
                    (
                        "Form type".into(),
                        u.form_type.clone().unwrap_or_else(|| "—".into()),
                    ),
                    ("DFM".into(), path(&u.dfm)),
                    ("In uses clause".into(), yes(u.in_source)),
                    ("In .dproj".into(), yes(u.in_dproj)),
                    ("Conditional ($IF)".into(), yes(u.conditional)),
                ],
            });
        }
        if let Some(p) = sel
            .strip_prefix("other:")
            .or_else(|| sel.strip_prefix("file:"))
        {
            let file = PathBuf::from(p);
            let size = std::fs::metadata(&file)
                .map(|m| format!("{} bytes", m.len()))
                .unwrap_or_else(|_| "—".into());
            let role = project
                .others
                .iter()
                .find(|o| same_path(&o.path, &file))
                .map_or("project file", |o| o.role);
            return Some(InspectorItem {
                title: file
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                class: role.to_owned(),
                rows: vec![
                    ("File".into(), p.to_owned()),
                    ("Exists".into(), yes(file.exists())),
                    ("Size".into(), size),
                ],
            });
        }
        let d = project.dproj.as_ref()?;
        if let Some(name) = sel.strip_prefix("configs:") {
            let c = d.configurations.iter().find(|c| c.name == name)?;
            return Some(InspectorItem {
                title: c.name.clone(),
                class: "Build configuration".into(),
                rows: vec![
                    ("Key".into(), c.key.clone()),
                    (
                        "Parent".into(),
                        c.parent.clone().unwrap_or_else(|| "—".into()),
                    ),
                ],
            });
        }
        let name = sel.strip_prefix("platforms:")?;
        Some(InspectorItem {
            title: name.to_owned(),
            class: "Target platform".into(),
            rows: vec![("Active".into(), "Yes".into())],
        })
    }

    // ------------------------------------------------------------------ arquivos

    fn open_dialog(&mut self, now: f64) {
        let mut dialog = rfd::FileDialog::new()
            .set_title("Open")
            .add_filter(
                "Delphi files",
                &["pas", "dfm", "dpr", "dpk", "inc", "dproj"],
            )
            .add_filter("All files", &["*"]);
        if let Some(dir) = self
            .settings
            .recent
            .files
            .first()
            .and_then(|f| Path::new(f).parent())
        {
            if dir.is_dir() {
                dialog = dialog.set_directory(dir);
            }
        }
        for path in dialog.pick_files().unwrap_or_default() {
            self.open_path(&path, now);
        }
    }

    /// Projeto (.dproj/.dpr/.dpk/pasta) abre como projeto; o resto abre no editor.
    fn open_any(&mut self, path: &Path, now: f64) {
        if is_project_path(path) {
            self.load_project(path.to_path_buf());
        } else {
            self.open_path(path, now);
        }
    }

    pub fn open_path(&mut self, path: &Path, now: f64) {
        let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        if let Some(i) = self
            .tabs
            .iter()
            .position(|t| t.path().is_some_and(|p| same_path(p, &path)))
        {
            self.active = i;
            return;
        }
        let started = Instant::now();
        match Document::open(&path, self.no_bom_default) {
            Ok(doc) => {
                let ms = started.elapsed().as_secs_f64() * 1000.0;
                self.log.info(format!(
                    "Opened {} ({} lines, {}, {}) in {ms:.1} ms.",
                    path.display(),
                    doc.buffer.line_count(),
                    doc.format.label(),
                    doc.format.line_ending.label()
                ));
                if doc.mixed_line_endings {
                    self.log.warning(format!(
                        "{} mixes CRLF and LF; it will be saved with {} only.",
                        doc.file_name(),
                        doc.format.line_ending.label()
                    ));
                }
                if !doc.lossless {
                    self.log.warning(format!(
                        "{} could not be decoded exactly as {}; check the encoding in the status bar before saving.",
                        doc.file_name(),
                        doc.format.label()
                    ));
                }
                self.next_id += 1;
                let view = EditorView::new(Id::new(("argila_editor", self.next_id)));
                self.tabs.push(Tab::Editor(Box::new(EditorTab {
                    id: self.next_id,
                    doc,
                    view,
                    dfm: crate::dfm::Preview::default(),
                    outline: None,
                    outline_version: None,
                    outline_busy: false,
                    seen_version: 0,
                    changed_at: now,
                })));
                self.active = self.tabs.len() - 1;
                self.settings.recent.push_file(&path.to_string_lossy());
                self.mark_dirty(now);
            }
            Err(crate::editor::document::OpenError::Binary("binary DFM (TPF0)")) => {
                self.binary_dfm = Some(path.clone());
            }
            Err(e) => self.notify(
                Level::Error,
                format!("Could not open {}: {e}", path.display()),
                now,
            ),
        }
    }

    fn save_tab(&mut self, index: usize, options: SaveOptions, then: Then, now: f64) -> SaveResult {
        let Some(Tab::Editor(ed)) = self.tabs.get_mut(index) else {
            return SaveResult::Saved;
        };
        let result = ed.doc.save(options);
        let (name, path, format) = (
            ed.doc.file_name(),
            ed.doc.path.display().to_string(),
            ed.doc.format,
        );
        match result {
            Ok(()) => {
                let text = format!(
                    "Saved {path} ({}, {}).",
                    format.label(),
                    format.line_ending.label()
                );
                self.notify(Level::Info, text, now);
                SaveResult::Saved
            }
            Err(e @ SaveError::Unencodable { .. }) => {
                let message = format!("{name} is saved as {}, but {e}.", format.label());
                self.prompt = Some(Prompt::Unencodable {
                    tab: index,
                    message,
                    then,
                });
                SaveResult::Pending
            }
            Err(SaveError::ChangedOnDisk) => {
                self.prompt = Some(Prompt::ChangedOnDisk { tab: index, then });
                SaveResult::Pending
            }
            Err(SaveError::Lossy) => {
                self.prompt = Some(Prompt::Lossy { tab: index, then });
                SaveResult::Pending
            }
            Err(SaveError::Io(e)) => {
                self.notify(Level::Error, format!("Could not save {path}: {e}"), now);
                SaveResult::Failed
            }
        }
    }

    /// Salva todos os modificados; para no primeiro que precisar de decisao do usuario.
    fn save_all(&mut self, then: Then, now: f64) {
        for i in 0..self.tabs.len() {
            if self.tabs[i].is_modified() {
                match self.save_tab(i, SaveOptions::default(), then, now) {
                    SaveResult::Saved => {}
                    SaveResult::Pending | SaveResult::Failed => return,
                }
            }
        }
        match then {
            Then::Exit => {
                self.allow_close = true;
                self.pending_close = true;
            }
            Then::CloseProject => self.close_project_now(now),
            Then::Nothing | Then::CloseTab => {}
        }
    }

    fn continue_after(&mut self, then: Then, tab: usize, now: f64) {
        match then {
            Then::Nothing => {}
            Then::CloseTab => self.close_tab_now(tab),
            Then::Exit | Then::CloseProject => self.save_all(then, now),
        }
    }

    fn request_close_tab(&mut self, index: usize) {
        if self.tabs.get(index).is_some_and(Tab::is_modified) {
            self.active = index;
            self.prompt = Some(Prompt::CloseModified(index));
        } else {
            self.close_tab_now(index);
        }
    }

    fn close_tab_now(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.tabs.remove(index);
            if self.active > index || self.active >= self.tabs.len() {
                self.active = self.active.saturating_sub(1);
            }
        }
    }

    fn export_keymap(&mut self, now: f64) {
        let path = filesystem::config_dir().join("keymap-export.json");
        match filesystem::atomic_write(&path, self.keymap.to_json().as_bytes()) {
            Ok(()) => self.notify(
                Level::Info,
                format!(
                    "Keymap exported to {}. Edit it and save as {} to customize.",
                    path.display(),
                    USER_KEYMAP_FILE
                ),
                now,
            ),
            Err(e) => self.notify(
                Level::Error,
                format!("Could not export keymap to {}: {e}", path.display()),
                now,
            ),
        }
    }

    fn open_settings_folder(&mut self, now: f64) {
        let dir = filesystem::config_dir();
        let result = std::fs::create_dir_all(&dir).and_then(|_| open_in_file_manager(&dir));
        if let Err(e) = result {
            self.notify(
                Level::Error,
                format!("Could not open {}: {e}", dir.display()),
                now,
            );
        }
    }

    // ------------------------------------------------------------------ entrada e janela

    /// Atalhos globais: casamento exato de modificadores (Ctrl+S nao dispara com Shift+Ctrl+S).
    fn collect_shortcuts(&mut self, ctx: &egui::Context) {
        if self.prompt.is_some() {
            return;
        }
        let keymap = &self.keymap;
        let pending = &mut self.pending;
        ctx.input_mut(|i| {
            i.events.retain(|event| {
                if let egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } = event
                {
                    if let Some(cmd) = keymap.lookup(KeyChord::from_input(*key, *modifiers)) {
                        pending.push(cmd);
                        return false;
                    }
                }
                true
            });
        });
    }

    /// Guarda tamanho/estado da janela para a proxima sessao.
    fn track_window(&mut self, ctx: &egui::Context, now: f64) {
        let (maximized, size) = ctx.input(|i| {
            let vp = i.viewport();
            (
                vp.maximized.unwrap_or(false),
                vp.inner_rect.map(|r| r.size()),
            )
        });
        let w = &mut self.settings.window;
        let mut changed = maximized != w.maximized;
        w.maximized = maximized;
        if let (false, Some(size)) = (maximized, size) {
            if (size.x - w.width).abs() > 1.0 || (size.y - w.height).abs() > 1.0 {
                w.width = size.x;
                w.height = size.y;
                changed = true;
            }
        }
        if changed {
            self.mark_dirty(now);
        }
    }

    /// Fechar a janela com arquivos modificados abre o dialogo em vez de perder as alteracoes.
    fn guard_close(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested())
            && !self.allow_close
            && self.tabs.iter().any(Tab::is_modified)
        {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.prompt = Some(Prompt::ExitModified);
        }
    }

    fn save_settings(&mut self) {
        self.settings.session = self.session_snapshot();
        self.dirty_since = None;
        if let Err(e) = self.store.save(&self.settings) {
            self.log.error(format!(
                "Could not save settings to {}: {e}",
                self.store.path().display()
            ));
        }
    }

    /// A sessao e salva quando muda o que esta aberto; a linha do cursor vai junto no proximo salvamento.
    fn track_session(&mut self, now: f64) {
        let snap = self.session_snapshot();
        let without_lines = |s: &SessionSettings| {
            let mut s = s.clone();
            s.files.iter_mut().for_each(|f| f.line = 0);
            s
        };
        if without_lines(&snap) != without_lines(&self.settings.session) {
            self.mark_dirty(now);
        }
        self.settings.session = snap;
    }

    fn track_perf(&mut self, frame: &eframe::Frame, now: f64) {
        let Some(perf) = &mut self.perf else { return };
        if let Some(cpu) = frame.info().cpu_usage {
            perf.frames += 1;
            perf.total += cpu;
            perf.max = perf.max.max(cpu);
        }
        if now - perf.since >= 2.0 && perf.frames > 0 {
            eprintln!(
                "perf: {} frames, avg {:.2} ms, max {:.2} ms",
                perf.frames,
                perf.total * 1000.0 / perf.frames as f32,
                perf.max * 1000.0
            );
            *perf = Perf {
                since: now,
                ..Perf::default()
            };
        }
    }

    // ------------------------------------------------------------------ desenho

    fn central(&mut self, ui: &mut egui::Ui, now: f64) {
        let p = self.palette;
        let mut close = None;
        egui::Frame::new()
            .fill(p.bg_panel)
            .inner_margin(Margin::symmetric(6, 3))
            .stroke(Stroke::new(1.0_f32, p.border))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    for (i, tab) in self.tabs.iter().enumerate() {
                        let mut response = ui::tab_button(ui, &p, i == self.active, &tab.title());
                        if let Tab::Editor(e) = tab {
                            response = response.on_hover_text(e.doc.path.display().to_string());
                        }
                        if response.clicked() {
                            self.active = i;
                        }
                        if response.middle_clicked() || ui::close_button(ui, "Close (Ctrl+F4)") {
                            close = Some(i);
                        }
                        ui.add_space(6.0);
                    }
                });
            });
        if let Some(i) = close {
            self.request_close_tab(i);
        }
        let style = EditorStyle {
            font: FontId::monospace(self.settings.editor.font_size),
            tab_size: self.settings.editor.tab_size,
            palette: p,
        };
        let interactive = self.prompt.is_none();
        self.cursor = None;
        match self.tabs.get_mut(self.active) {
            Some(Tab::Welcome) => {
                match welcome::show(ui, &p, &self.keymap, &self.settings.recent) {
                    Some(welcome::WelcomeAction::Command(cmd)) => self.pending.push(cmd),
                    Some(welcome::WelcomeAction::OpenRecent(path)) => {
                        self.open_any(Path::new(&path), now)
                    }
                    Some(welcome::WelcomeAction::OpenForm) => {
                        if let Some(path) = rfd::FileDialog::new()
                            .set_title("Open DFM in Design")
                            .add_filter("Delphi form", &["dfm"])
                            .pick_file()
                        {
                            self.open_path(&path, now);
                            if let Some(ed) = self.active_editor() {
                                if ed.doc.language == Language::Dfm {
                                    ed.dfm.design = true;
                                }
                            }
                        }
                    }
                    Some(welcome::WelcomeAction::DemoForm) => match crate::dfm::demo_copy() {
                        Ok(path) => {
                            self.open_path(&path, now);
                            if let Some(ed) = self.active_editor() {
                                ed.dfm.design = true;
                            }
                        }
                        Err(error) => self.notify(
                            Level::Error,
                            format!("Could not open designer demo: {error}"),
                            now,
                        ),
                    },
                    None => {}
                }
            }
            Some(Tab::Editor(ed)) => {
                if ed.doc.language == Language::Dfm {
                    let version = ed.doc.buffer.version();
                    ed.dfm.update(version, || ed.doc.buffer.text(), &self.ctx);
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut ed.dfm.design, false, "Text");
                        ui.selectable_value(&mut ed.dfm.design, true, "Design");
                    });
                    if ed.dfm.design {
                        match ed.dfm.result.clone() {
                            Some(Ok(form)) => {
                                if let Some(line) =
                                    crate::dfm::designer::show(ui, &form, &mut ed.dfm)
                                {
                                    ed.dfm.design = false;
                                    ed.view.goto(&mut ed.doc, line, 0);
                                }
                            }
                            Some(Err(error)) => {
                                ui.colored_label(p.error, error.to_string());
                                if ui.button("Go to error in text").clicked() {
                                    ed.dfm.design = false;
                                    ed.view.goto(&mut ed.doc, error.line, error.column);
                                }
                            }
                            None => {
                                ui.spinner();
                                ui.label("Parsing DFM…");
                            }
                        }
                        return;
                    }
                }
                let out = ed.view.show(ui, &mut ed.doc, &style, now, interactive);
                self.cursor = Some((out.line, out.col));
                if let Some(Ok(form)) = &ed.dfm.result {
                    ed.dfm.selected = form.root.path_at_line(out.line.saturating_sub(1));
                }
                if std::mem::take(&mut ed.view.ctrl_clicked) {
                    self.pending.push(Command::FindDeclaration);
                }
            }
            None => ui::empty_state(
                ui,
                &p,
                "No open files",
                "File ▸ Open (Ctrl+O), drop files here, or View ▸ Welcome Page.",
            ),
        }
    }

    fn status_bar(&mut self, ctx: &egui::Context, now: f64) {
        let p = self.palette;
        let flash = self.flash.as_ref().filter(|f| f.until > now).map(|f| {
            let color: Color32 = match f.level {
                Level::Error => p.error,
                Level::Warning => p.warning,
                Level::Hint => p.accent,
                Level::Info => p.text,
            };
            (f.text.as_str(), color)
        });
        let path;
        let document = match (self.tabs.get(self.active), self.cursor) {
            (Some(Tab::Editor(e)), Some((line, col))) => {
                path = e.doc.path.display().to_string();
                Some(statusbar::DocStatus {
                    path: &path,
                    line,
                    col,
                    format: e.doc.format,
                    mixed_line_endings: e.doc.mixed_line_endings,
                    language: e.doc.language.name(),
                    modified: e.doc.is_modified(),
                })
            }
            _ => None,
        };
        let project = match (&self.project, &self.loading) {
            (_, Some(l)) => Some(format!("Loading {}…", l.name)),
            (Some(p), None) => Some(format!("{} · {} units", p.name, p.units.len())),
            (None, None) => None,
        };
        let info = statusbar::StatusInfo {
            flash,
            document,
            project: project.as_deref(),
            ..Default::default()
        };
        let action = egui::TopBottomPanel::bottom("status_bar")
            .frame(
                egui::Frame::new()
                    .fill(p.bg_top)
                    .inner_margin(Margin::symmetric(10, 3)),
            )
            .show(ctx, |ui| statusbar::show(ui, &p, &info))
            .inner;
        if let Some(f) = &self.flash {
            if f.until > now {
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(f.until - now));
            }
        }
        let Some(action) = action else { return };
        let Some(ed) = self.active_editor() else {
            return;
        };
        let message = match action {
            statusbar::StatusAction::ReopenWith(enc) => match ed.doc.reopen_as(enc) {
                Ok(()) => {
                    ed.view.reset();
                    ed.outline_version = None;
                    let lossy = if ed.doc.lossless {
                        ""
                    } else {
                        " Some bytes could not be decoded exactly."
                    };
                    (
                        Level::Info,
                        format!("Reopened {} as {}.{lossy}", ed.doc.file_name(), enc.label()),
                    )
                }
                Err(e) => (
                    Level::Error,
                    format!("Could not reopen {}: {e}", ed.doc.file_name()),
                ),
            },
            statusbar::StatusAction::SaveWith(format) => {
                ed.doc.set_format(format);
                let text = format!(
                    "{} will be saved as {}, {}.",
                    ed.doc.file_name(),
                    format.label(),
                    format.line_ending.label()
                );
                (Level::Info, text)
            }
        };
        self.notify(message.0, message.1, now);
    }

    fn show_prompt(&mut self, ctx: &egui::Context, now: f64) {
        let Some(mut prompt) = self.prompt.take() else {
            return;
        };
        let p = self.palette;
        let title_of = |tab: usize, tabs: &[Tab]| tabs.get(tab).map(Tab::title).unwrap_or_default();
        let modified: Vec<String> = self
            .tabs
            .iter()
            .filter(|t| t.is_modified())
            .map(Tab::title)
            .collect();
        let project_name = self
            .project
            .as_ref()
            .map(|p| p.name.clone())
            .unwrap_or_default();
        let mut choice = None;
        let mut goto = None;
        let mut open_unit: Option<PathBuf> = None;
        let units: Vec<(String, String, PathBuf)> = match &prompt {
            Prompt::UnitList { forms, filter } => {
                let needle = filter.trim().to_lowercase();
                self.project
                    .iter()
                    .flat_map(|p| &p.units)
                    .filter(|u| u.exists && (!forms || u.dfm.is_some()))
                    .filter(|u| {
                        needle.is_empty()
                            || u.name.to_lowercase().contains(&needle)
                            || u.form
                                .as_deref()
                                .is_some_and(|f| f.to_lowercase().contains(&needle))
                    })
                    .map(|u| {
                        let file = if *forms {
                            u.dfm.clone().unwrap_or_default()
                        } else {
                            u.pas.clone().unwrap_or_default()
                        };
                        (u.name.clone(), u.form.clone().unwrap_or_default(), file)
                    })
                    .collect()
            }
            _ => Vec::new(),
        };
        let modal = egui::Modal::new(Id::new("argila_prompt")).show(ctx, |ui| {
            ui.set_width(480.0);
            let heading = |ui: &mut egui::Ui, t: &str| {
                ui.label(RichText::new(t).strong().size(15.0).color(p.text));
                ui.add_space(4.0);
            };
            let buttons = |ui: &mut egui::Ui, labels: &[&str], choice: &mut Option<usize>| {
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    for (i, label) in labels.iter().enumerate() {
                        if ui.button(*label).clicked() {
                            *choice = Some(i);
                        }
                    }
                });
            };
            let body = |ui: &mut egui::Ui, t: &str| {
                ui.label(RichText::new(t).color(p.text_weak));
            };
            let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
            match &mut prompt {
                Prompt::CloseModified(tab) => {
                    heading(ui, "Save changes?");
                    body(ui, &format!("{} has unsaved changes.", title_of(*tab, &self.tabs)));
                    buttons(ui, &["Save", "Don't Save", "Cancel"], &mut choice);
                }
                Prompt::ExitModified | Prompt::CloseProjectModified => {
                    let exit = matches!(prompt, Prompt::ExitModified);
                    heading(ui, if exit { "Exit Argila?" } else { "Close project?" });
                    body(ui, &format!("These files have unsaved changes:\n{}", modified.join("\n")));
                    let labels: &[&str] = if exit {
                        &["Save All and Exit", "Exit Without Saving", "Cancel"]
                    } else {
                        &["Save All and Close", "Close Without Saving", "Cancel"]
                    };
                    buttons(ui, labels, &mut choice);
                }
                Prompt::Unencodable { message, .. } => {
                    heading(ui, "Character not representable");
                    body(ui, &format!("{message}\n\nNothing was written. Save as UTF-8 with BOM instead (Delphi 2009+ reads it)?"));
                    buttons(ui, &["Save as UTF-8 BOM", "Cancel"], &mut choice);
                }
                Prompt::ChangedOnDisk { tab, .. } => {
                    heading(ui, "File changed on disk");
                    body(ui, &format!("{} was changed by another program since it was opened. Overwrite it?", title_of(*tab, &self.tabs)));
                    buttons(ui, &["Overwrite", "Cancel"], &mut choice);
                }
                Prompt::Lossy { tab, .. } => {
                    heading(ui, "File was read with replaced bytes");
                    body(
                        ui,
                        &format!(
                            "{} contains bytes that could not be decoded in its current encoding. Saving writes the replaced \
                             characters. Use the encoding menu in the status bar to reopen it correctly first.",
                            title_of(*tab, &self.tabs)
                        ),
                    );
                    buttons(ui, &["Save Anyway", "Cancel"], &mut choice);
                }
                Prompt::GotoLine(input) => {
                    heading(ui, "Go to Line Number");
                    ui.add(egui::TextEdit::singleline(input).hint_text("Line number").desired_width(f32::INFINITY)).request_focus();
                    if enter {
                        goto = input.trim().parse::<usize>().ok();
                        choice = Some(if goto.is_some() { 0 } else { 1 });
                    }
                    buttons(ui, &["Go", "Cancel"], &mut choice);
                    if choice == Some(0) {
                        goto = input.trim().parse::<usize>().ok();
                    }
                }
                Prompt::UnitList { forms, filter } => {
                    heading(ui, if *forms { "View Form" } else { "View Unit" });
                    ui.add(egui::TextEdit::singleline(filter).hint_text("Type to filter").desired_width(f32::INFINITY)).request_focus();
                    ui.label(RichText::new(format!("{} of {}", units.len(), project_name)).size(11.0).color(p.text_weak));
                    egui::ScrollArea::vertical().max_height(340.0).show(ui, |ui| {
                        for (name, form, file) in units.iter().take(500) {
                            ui.horizontal(|ui| {
                                if ui.selectable_label(false, name).clicked() {
                                    open_unit = Some(file.clone());
                                }
                                ui.label(RichText::new(form).size(11.5).color(p.text_weak));
                            });
                        }
                    });
                    if enter {
                        open_unit = units.first().map(|u| u.2.clone());
                    }
                    buttons(ui, &["Cancel"], &mut choice);
                }
                Prompt::NewUnit { name, add } => {
                    heading(ui, "New Unit");
                    ui.add(egui::TextEdit::singleline(name).hint_text("Unit name").desired_width(f32::INFINITY)).request_focus();
                    if !project_name.is_empty() {
                        let can_add = self.project.as_ref().is_some_and(|p| p.kind != ProjectKind::Folder);
                        ui.add_enabled(can_add, egui::Checkbox::new(add, format!("Add to {project_name} (.dpr uses and .dproj)")));
                    }
                    let dir = self.new_unit_dir().map(|d| d.display().to_string()).unwrap_or_else(|| "(choose a folder)".into());
                    body(ui, &format!("Folder: {dir}"));
                    buttons(ui, &["Create", "Cancel"], &mut choice);
                    if enter {
                        choice = Some(0);
                    }
                }
                Prompt::Rename { pas, name } => {
                    heading(ui, "Rename Unit");
                    body(ui, &pas.display().to_string());
                    ui.add(egui::TextEdit::singleline(name).hint_text("New unit name").desired_width(f32::INFINITY)).request_focus();
                    buttons(ui, &["Continue", "Cancel"], &mut choice);
                    if enter {
                        choice = Some(0);
                    }
                }
                Prompt::Confirm(plan) => {
                    heading(ui, &plan.title);
                    body(ui, "The following changes will be made (a backup of every edited file is kept):");
                    for line in plan.describe() {
                        ui.label(RichText::new(format!("• {line}")).size(12.5));
                    }
                    buttons(ui, &["Apply", "Cancel"], &mut choice);
                }
            }
        });
        if modal.should_close() && choice.is_none() && open_unit.is_none() {
            choice = Some(usize::MAX); // Esc / clique fora = cancelar
        }
        if let Some(file) = open_unit {
            self.open_path(&file, now);
            return;
        }
        let Some(choice) = choice else {
            self.prompt = Some(prompt);
            return;
        };
        let overwrite = SaveOptions {
            overwrite_external_changes: true,
            ..SaveOptions::default()
        };
        let lossy = SaveOptions {
            accept_lossy: true,
            ..SaveOptions::default()
        };
        match (prompt, choice) {
            (Prompt::CloseModified(tab), 0) => {
                if let SaveResult::Saved =
                    self.save_tab(tab, SaveOptions::default(), Then::CloseTab, now)
                {
                    self.close_tab_now(tab);
                }
            }
            (Prompt::CloseModified(tab), 1) => self.close_tab_now(tab),
            (Prompt::ExitModified, 0) => self.save_all(Then::Exit, now),
            (Prompt::ExitModified, 1) => {
                self.allow_close = true;
                self.pending_close = true;
            }
            (Prompt::CloseProjectModified, 0) => self.save_all(Then::CloseProject, now),
            (Prompt::CloseProjectModified, 1) => self.close_project_now(now),
            (Prompt::Unencodable { tab, then, .. }, 0) => {
                if let Some(Tab::Editor(e)) = self.tabs.get_mut(tab) {
                    let line_ending = e.doc.format.line_ending;
                    e.doc.set_format(encoding::TextFormat {
                        encoding: encoding::TextEncoding::Utf8,
                        bom: true,
                        line_ending,
                    });
                }
                if let SaveResult::Saved = self.save_tab(tab, SaveOptions::default(), then, now) {
                    self.continue_after(then, tab, now);
                }
            }
            (Prompt::ChangedOnDisk { tab, then }, 0) => {
                if let SaveResult::Saved = self.save_tab(tab, overwrite, then, now) {
                    self.continue_after(then, tab, now);
                }
            }
            (Prompt::Lossy { tab, then }, 0) => {
                if let SaveResult::Saved = self.save_tab(tab, lossy, then, now) {
                    self.continue_after(then, tab, now);
                }
            }
            (Prompt::GotoLine(_), 0) => {
                if let (Some(line), Some(ed)) = (goto, self.active_editor()) {
                    ed.view.goto_line(&mut ed.doc, line);
                }
            }
            (Prompt::NewUnit { name, add }, 0) => {
                let dir = self.new_unit_dir().or_else(|| {
                    rfd::FileDialog::new()
                        .set_title("Folder for the new unit")
                        .pick_folder()
                });
                if let Some(dir) = dir {
                    let project = self
                        .project
                        .as_ref()
                        .filter(|p| add && p.kind != ProjectKind::Folder);
                    match files::plan_new_unit(&dir, name.trim(), project) {
                        Ok(plan) if plan.edits.is_empty() => self.apply_plan(&plan, now),
                        other => self.confirm_plan(other, now),
                    }
                }
            }
            (Prompt::Rename { pas, name }, 0) => {
                let plan = files::plan_rename_unit(self.project.as_ref(), &pas, name.trim());
                self.confirm_plan(plan, now);
            }
            (Prompt::Confirm(plan), 0) => self.apply_plan(&plan, now),
            _ => {}
        }
    }
}

fn toggle_left(layout: &mut crate::settings::LayoutSettings, tab: LeftTab) {
    if layout.show_left && layout.left_tab == tab {
        layout.show_left = false;
    } else {
        layout.show_left = true;
        layout.left_tab = tab;
    }
}

fn open_in_file_manager(dir: &Path) -> std::io::Result<()> {
    let program = if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    std::process::Command::new(program)
        .arg(dir)
        .spawn()
        .map(|_| ())
}

impl eframe::App for ArgilaApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let now = ctx.input(|i| i.time);
        let p = self.palette;
        self.track_perf(frame, now);
        self.poll_loading(now);
        self.update_outline(now);
        self.guard_close(ctx);
        self.collect_shortcuts(ctx);
        self.track_window(ctx, now);
        if self.prompt.is_none() {
            let dropped: Vec<PathBuf> = ctx.input(|i| {
                i.raw
                    .dropped_files
                    .iter()
                    .filter_map(|f| f.path.clone())
                    .collect()
            });
            for path in dropped {
                self.open_any(&path, now);
            }
        }

        let fired = {
            let availability = |c| self.availability(c);
            let checked = |c| self.checked(c);
            let mc = toolbar::MenuContext {
                keymap: &self.keymap,
                availability: &availability,
                checked: &checked,
            };
            let top = egui::Frame::new()
                .fill(p.bg_top)
                .inner_margin(Margin::symmetric(8, 2));
            let menu = egui::TopBottomPanel::top("menu_bar")
                .frame(top)
                .show(ctx, |ui| toolbar::menu_bar(ui, &mc))
                .inner;
            let strip = egui::TopBottomPanel::top("tool_strip")
                .frame(
                    top.fill(themes::mix(p.bg_top, p.bg_panel, 0.5))
                        .stroke(Stroke::new(1.0_f32, p.border)),
                )
                .show(ctx, |ui| toolbar::tool_strip(ui, &p, &mc))
                .inner;
            menu.or(strip)
        };
        if self.prompt.is_none() {
            self.pending.extend(fired);
        }

        self.status_bar(ctx, now);

        let dock = egui::Frame::new().fill(p.bg_panel);
        let before = self.settings.layout.clone();
        // so memoriza larguras durante o arraste (na animacao de abrir/fechar o tamanho e parcial)
        let resizing = ctx.input(|i| i.pointer.any_down() || i.pointer.any_released());
        let item = self.inspector_item();
        let layout = &mut self.settings.layout;

        let bottom = egui::TopBottomPanel::bottom("dock_bottom")
            .resizable(true)
            .default_height(layout.bottom_height)
            .height_range(70.0..=700.0)
            .frame(dock)
            .show_animated(ctx, layout.show_bottom, |ui| {
                console::show(ui, &p, &mut layout.bottom_tab, &mut self.log)
            });
        if let Some(r) = bottom {
            layout.show_bottom &= !r.inner.close;
            remember(
                resizing,
                &mut layout.bottom_height,
                r.response.rect.height(),
            );
        }

        let (structure_title, structure_outline, structure_busy) = match self.tabs.get(self.active)
        {
            Some(Tab::Editor(e)) => (e.doc.file_name(), e.outline.clone(), e.outline_busy),
            _ => (String::new(), None, false),
        };
        let structure = StructureInfo {
            title: &structure_title,
            outline: structure_outline.as_deref(),
            busy: structure_busy,
        };
        let loading = self.loading.as_ref().map(|l| l.name.clone());
        let left = egui::SidePanel::left("dock_left")
            .resizable(true)
            .default_width(layout.left_width)
            .width_range(160.0..=700.0)
            .frame(dock)
            .show_animated(ctx, layout.show_left, |ui| {
                explorer::show(
                    ui,
                    &p,
                    &mut layout.left_tab,
                    &self.keymap,
                    self.project.as_ref(),
                    loading.as_deref(),
                    &mut self.explorer,
                    &structure,
                )
            });
        let mut explorer_actions = Vec::new();
        if let Some(r) = left {
            layout.show_left &= !r.inner.close;
            remember(resizing, &mut layout.left_width, r.response.rect.width());
            explorer_actions = r.inner.actions;
        }

        let right = egui::SidePanel::right("dock_right")
            .resizable(true)
            .default_width(layout.right_width)
            .width_range(220.0..=700.0)
            .frame(dock)
            .show_animated(ctx, layout.show_right, |ui| {
                inspector::show(
                    ui,
                    &p,
                    &mut layout.inspector_tab,
                    &mut self.inspector_filter,
                    item.as_ref(),
                )
            });
        if let Some(r) = right {
            layout.show_right &= !r.inner.close;
            remember(resizing, &mut layout.right_width, r.response.rect.width());
        }

        for action in explorer_actions {
            match action {
                ExplorerAction::Command(cmd) => self.pending.push(cmd),
                ExplorerAction::Open(path) => self.open_path(&path, now),
                ExplorerAction::Remove(unit) => {
                    if let Some(project) = &self.project {
                        if let Some(u) = project.units.iter().find(|u| u.name == unit) {
                            let plan = files::plan_remove_unit(project, u);
                            self.confirm_plan(plan, now);
                        }
                    }
                }
                ExplorerAction::Rename(pas) => {
                    let name = pas
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    self.prompt = Some(Prompt::Rename { pas, name });
                }
                ExplorerAction::Navigate(line, col) => {
                    if let Some(ed) = self.active_editor() {
                        if let Some(Ok(form)) = &ed.dfm.result {
                            ed.dfm.selected = form.root.path_at_line(line);
                        }
                        ed.view.goto(&mut ed.doc, line, col);
                    }
                }
            }
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(p.bg_editor))
            .show(ctx, |ui| self.central(ui, now));

        let keymap_path = Self::keymap_path();
        let mut show_shortcuts = self.show_shortcuts;
        let availability = |c| self.availability(c);
        dialogs::shortcuts_window(
            ctx,
            &mut show_shortcuts,
            &p,
            &self.keymap,
            &keymap_path,
            &availability,
        );
        self.show_shortcuts = show_shortcuts;
        dialogs::about_window(
            ctx,
            &mut self.show_about,
            &p,
            self.store.path(),
            &self.fonts,
        );
        if let Some(project) = &self.project {
            dialogs::project_options(ctx, &mut self.show_options, &p, project, &mut self.options);
        }

        self.show_prompt(ctx, now);
        if let Some(path) = self.binary_dfm.clone() {
            let mut open = true;
            egui::Window::new("Binary DFM — original preserved")
                .open(&mut open).resizable(true).show(ctx, |ui| {
                    ui.label(path.display().to_string());
                    ui.label("This file has the TPF0 binary signature. Text editing and saving are disabled.");
                    ui.label("Safe conversion: use Delphi System.Classes.ObjectBinaryToText with separate input/output streams, writing a NEW text .dfm copy. Do not use ReadComponent or instantiate the form. Open the converted copy here; keep the binary original unchanged.");
                    ui.hyperlink_to("Official ObjectBinaryToText documentation", "https://docwiki.embarcadero.com/Libraries/Alexandria/en/System.Classes.ObjectBinaryToText");
                    ui.label("Argila does not run external converters automatically.");
                    if ui.button("Open converted text copy…").clicked() { self.pending.push(Command::OpenFile); }
                });
            if !open {
                self.binary_dfm = None;
            }
        }
        for cmd in std::mem::take(&mut self.pending) {
            self.run(ctx, cmd);
        }
        if std::mem::take(&mut self.pending_close) {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
        if self.settings.layout != before {
            self.mark_dirty(now);
        }
        self.track_session(now);
        if self.dirty_since.is_some_and(|t| now - t > 1.5) {
            self.save_settings();
        } else if self.dirty_since.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(1600));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_settings();
    }
}

/// Atualiza a medida guardada quando o usuario redimensiona um painel.
fn remember(resizing: bool, stored: &mut f32, actual: f32) {
    if resizing && (actual - *stored).abs() > 0.5 {
        *stored = actual;
    }
}
