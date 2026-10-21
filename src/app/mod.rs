//! Estado da aplicacao e execucao de comandos. A interface (`ui`) so devolve pedidos; aqui eles
//! viram acoes, sempre passando pelo mesmo caminho (menu, barra, atalho ou botao).

use std::path::{Path, PathBuf};
use std::time::Instant;

use eframe::egui::{self, Color32, FontId, Id, Margin, RichText, Stroke, ViewportCommand};
use encoding_rs::Encoding;

use crate::diagnostics::{Level, MessageLog};
use crate::editor::document::{Document, SaveError, SaveOptions};
use crate::editor::view::{EditorStyle, EditorView};
use crate::filesystem::{self, encoding};
use crate::settings::{BottomTab, LeftTab, Settings, SettingsStore};
use crate::shortcuts::{Availability, Command, KeyChord, Keymap, USER_KEYMAP_FILE};
use crate::themes::{self, EARTHY, Palette};
use crate::ui::{self, console, dialogs, explorer, inspector, statusbar, toolbar, welcome};

pub struct EditorTab {
    pub doc: Document,
    pub view: EditorView,
}

/// Abas da area central. O designer de formularios entra na Fase 4.
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
}

/// O que continuar depois que um salvamento pedido por um dialogo der certo.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Then {
    Nothing,
    CloseTab,
    Exit,
}

/// Dialogos modais: nada e descartado ou sobrescrito sem o usuario escolher.
enum Prompt {
    CloseModified(usize),
    ExitModified,
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

/// Estatistica de tempo de quadro (ARGILA_PERF=1 imprime no stderr a cada 2 s).
#[derive(Default)]
struct Perf {
    frames: u32,
    total: f32,
    max: f32,
    since: f64,
}

pub struct ArgilaApp {
    palette: Palette,
    settings: Settings,
    store: SettingsStore,
    keymap: Keymap,
    log: MessageLog,
    fonts: Vec<&'static str>,
    no_bom_default: &'static Encoding,
    tabs: Vec<Tab>,
    active: usize,
    next_view: u64,
    cursor: Option<(usize, usize)>,
    inspector_filter: String,
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

        let mut app = Self {
            palette,
            settings,
            store,
            keymap,
            log,
            fonts,
            no_bom_default,
            tabs: vec![Tab::Welcome],
            active: 0,
            next_view: 0,
            cursor: None,
            inspector_filter: String::new(),
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
        for file in files {
            app.open_path(&file, 0.0);
        }
        app
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

    fn has_editor(&self) -> bool {
        matches!(self.tabs.get(self.active), Some(Tab::Editor(_)))
    }

    /// Unico lugar que decide se um comando pode rodar.
    pub fn availability(&self, cmd: Command) -> Availability {
        use Command::*;
        match cmd {
            Exit | ProjectManager | Structure | ObjectInspector | Messages | Welcome | ExportKeymap
            | OpenSettingsFolder | KeyboardShortcuts | About | OpenFile => Availability::Ready,
            CloseTab if !self.tabs.is_empty() => Availability::Ready,
            Save | SaveAll | Undo | Redo | Find | Replace | SearchAgain | GotoLine | ToggleComment | CloseTab => {
                if self.has_editor() || (cmd == SaveAll && self.tabs.iter().any(Tab::is_modified)) {
                    Availability::Ready
                } else {
                    Availability::Unavailable("Open a file first (File ▸ Open, Ctrl+O).".to_owned())
                }
            }
            Compile | Build | Run | RunWithoutDebugging => Availability::Unavailable(
                "No compiler configured. DCC32/DCC64 integration is planned for phase 6 and needs a licensed Delphi installation."
                    .to_owned(),
            ),
            _ => Availability::NotImplemented,
        }
    }

    /// Fase do roteiro em que o comando passa a funcionar (para a mensagem de "nao implementado").
    fn planned_phase(cmd: Command) -> u8 {
        use Command::*;
        match cmd {
            NewUnit | OpenProject | OpenFolder => 3,
            ToggleFormUnit => 4,
            CodeCompletion | ClassCompletion | DeclImplToggle | FindInFiles | FindDeclaration => 5,
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
            Command::GotoLine => self.prompt = Some(Prompt::GotoLine(String::new())),
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

    pub fn open_path(&mut self, path: &Path, now: f64) {
        let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        let same = |t: &Tab| matches!(t, Tab::Editor(e) if e.doc.path.as_os_str().eq_ignore_ascii_case(path.as_os_str()));
        if let Some(i) = self.tabs.iter().position(same) {
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
                self.next_view += 1;
                let view = EditorView::new(Id::new(("argila_editor", self.next_view)));
                self.tabs
                    .push(Tab::Editor(Box::new(EditorTab { doc, view })));
                self.active = self.tabs.len() - 1;
                self.settings.recent.push_file(&path.to_string_lossy());
                self.mark_dirty(now);
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
        if then == Then::Exit {
            self.allow_close = true;
            self.pending_close = true;
        }
    }

    fn continue_after(&mut self, then: Then, tab: usize, now: f64) {
        match then {
            Then::Nothing => {}
            Then::CloseTab => self.close_tab_now(tab),
            Then::Exit => self.save_all(Then::Exit, now),
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
        self.dirty_since = None;
        if let Err(e) = self.store.save(&self.settings) {
            self.log.error(format!(
                "Could not save settings to {}: {e}",
                self.store.path().display()
            ));
        }
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
                ui.horizontal(|ui| {
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
                        self.open_path(Path::new(&path), now)
                    }
                    None => {}
                }
            }
            Some(Tab::Editor(ed)) => {
                let out = ed.view.show(ui, &mut ed.doc, &style, now, interactive);
                self.cursor = Some((out.line, out.col));
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
        let info = statusbar::StatusInfo {
            flash,
            document,
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
        let title_of = |tab: usize, tabs: &[Tab]| match tabs.get(tab) {
            Some(Tab::Editor(e)) => e.doc.file_name(),
            _ => String::new(),
        };
        let modified: Vec<String> = self
            .tabs
            .iter()
            .filter(|t| t.is_modified())
            .map(|t| t.title())
            .collect();
        let mut choice = None;
        let mut goto = None;
        let modal = egui::Modal::new(Id::new("argila_prompt")).show(ctx, |ui| {
            ui.set_width(460.0);
            let (title, body, buttons): (String, String, &[&str]) = match &mut prompt {
                Prompt::CloseModified(tab) => (
                    "Save changes?".into(),
                    format!("{} has unsaved changes.", title_of(*tab, &self.tabs)),
                    &["Save", "Don't Save", "Cancel"],
                ),
                Prompt::ExitModified => (
                    "Exit Argila?".into(),
                    format!("These files have unsaved changes:\n{}", modified.join("\n")),
                    &["Save All and Exit", "Exit Without Saving", "Cancel"],
                ),
                Prompt::Unencodable { message, .. } => (
                    "Character not representable".into(),
                    format!("{message}\n\nNothing was written. Save as UTF-8 with BOM instead (Delphi 2009+ reads it)?"),
                    &["Save as UTF-8 BOM", "Cancel"],
                ),
                Prompt::ChangedOnDisk { tab, .. } => (
                    "File changed on disk".into(),
                    format!("{} was changed by another program since it was opened. Overwrite it?", title_of(*tab, &self.tabs)),
                    &["Overwrite", "Cancel"],
                ),
                Prompt::Lossy { tab, .. } => (
                    "File was read with replaced bytes".into(),
                    format!(
                        "{} contains bytes that could not be decoded in its current encoding. Saving writes the replaced \
                         characters. Use the encoding menu in the status bar to reopen it correctly first.",
                        title_of(*tab, &self.tabs)
                    ),
                    &["Save Anyway", "Cancel"],
                ),
                Prompt::GotoLine(input) => {
                    ui.label(RichText::new("Go to Line Number").strong().size(15.0));
                    let field = ui.add(egui::TextEdit::singleline(input).hint_text("Line number").desired_width(f32::INFINITY));
                    field.request_focus();
                    if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        goto = input.trim().parse::<usize>().ok();
                        choice = Some(if goto.is_some() { 0 } else { 1 });
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Go").clicked() {
                            goto = input.trim().parse::<usize>().ok();
                            choice = Some(0);
                        }
                        if ui.button("Cancel").clicked() {
                            choice = Some(1);
                        }
                    });
                    return;
                }
            };
            ui.label(RichText::new(title).strong().size(15.0).color(p.text));
            ui.add_space(4.0);
            ui.label(RichText::new(body).color(p.text_weak));
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                for (i, label) in buttons.iter().enumerate() {
                    if ui.button(*label).clicked() {
                        choice = Some(i);
                    }
                }
            });
        });
        if modal.should_close() && choice.is_none() {
            choice = Some(usize::MAX); // Esc / clique fora = cancelar
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
                self.open_path(&path, now);
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

        let left = egui::SidePanel::left("dock_left")
            .resizable(true)
            .default_width(layout.left_width)
            .width_range(160.0..=700.0)
            .frame(dock)
            .show_animated(ctx, layout.show_left, |ui| {
                explorer::show(ui, &p, &mut layout.left_tab, &self.keymap)
            });
        if let Some(r) = left {
            layout.show_left &= !r.inner.close;
            remember(resizing, &mut layout.left_width, r.response.rect.width());
            self.pending.extend(r.inner.command);
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
                )
            });
        if let Some(r) = right {
            layout.show_right &= !r.inner.close;
            remember(resizing, &mut layout.right_width, r.response.rect.width());
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

        self.show_prompt(ctx, now);
        for cmd in std::mem::take(&mut self.pending) {
            self.run(ctx, cmd);
        }
        if std::mem::take(&mut self.pending_close) {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
        if self.settings.layout != before {
            self.mark_dirty(now);
        }
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
