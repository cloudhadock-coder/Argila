//! Estado da aplicacao e execucao de comandos. A interface (`ui`) so devolve pedidos; aqui eles
//! viram acoes, sempre passando pelo mesmo caminho (menu, barra, atalho ou botao).

use std::path::PathBuf;

use eframe::egui::{self, Color32, Margin, Stroke, ViewportCommand};

use crate::diagnostics::{Level, MessageLog};
use crate::filesystem;
use crate::settings::{BottomTab, LeftTab, Settings, SettingsStore};
use crate::shortcuts::{Availability, Command, KeyChord, Keymap, USER_KEYMAP_FILE};
use crate::themes::{self, EARTHY, Palette};
use crate::ui::{self, console, dialogs, explorer, inspector, statusbar, toolbar, welcome};

/// Documentos abertos na area central. Editor de codigo e designer entram nas Fases 2 e 4.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Document {
    Welcome,
}

impl Document {
    pub fn title(&self) -> &str {
        match self {
            Document::Welcome => "Welcome",
        }
    }
}

struct Flash {
    text: String,
    level: Level,
    until: f64,
}

pub struct ArgilaApp {
    palette: Palette,
    settings: Settings,
    store: SettingsStore,
    keymap: Keymap,
    log: MessageLog,
    fonts: Vec<&'static str>,
    documents: Vec<Document>,
    active: usize,
    inspector_filter: String,
    flash: Option<Flash>,
    pending: Vec<Command>,
    show_shortcuts: bool,
    show_about: bool,
    /// Momento da ultima alteracao ainda nao gravada (gravacao com atraso para nao martelar o disco).
    dirty_since: Option<f64>,
}

impl ArgilaApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        store: SettingsStore,
        settings: Settings,
        warning: Option<String>,
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

        Self {
            palette,
            settings,
            store,
            keymap,
            log,
            fonts,
            documents: vec![Document::Welcome],
            active: 0,
            inspector_filter: String::new(),
            flash: None,
            pending: Vec::new(),
            show_shortcuts: false,
            show_about: false,
            dirty_since: None,
        }
    }

    fn keymap_path() -> PathBuf {
        filesystem::config_dir().join(USER_KEYMAP_FILE)
    }

    /// Unico lugar que decide se um comando pode rodar.
    pub fn availability(&self, cmd: Command) -> Availability {
        use Command::*;
        match cmd {
            Exit | CloseTab | ProjectManager | Structure | ObjectInspector | Messages | Welcome | ExportKeymap
            | OpenSettingsFolder | KeyboardShortcuts | About => Availability::Ready,
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
            Compile | Build | Run | RunWithoutDebugging => 6,
            _ => 2,
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
            Command::CloseTab => {
                if !self.documents.is_empty() {
                    self.documents.remove(self.active);
                    self.active = self.active.min(self.documents.len().saturating_sub(1));
                }
            }
            Command::Welcome => self.focus_or_open(Document::Welcome),
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

    fn focus_or_open(&mut self, doc: Document) {
        match self.documents.iter().position(|d| *d == doc) {
            Some(i) => self.active = i,
            None => {
                self.documents.push(doc);
                self.active = self.documents.len() - 1;
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

    /// Atalhos globais: casamento exato de modificadores (Ctrl+S nao dispara com Shift+Ctrl+S).
    fn collect_shortcuts(&mut self, ctx: &egui::Context) {
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

    fn save_settings(&mut self) {
        self.dirty_since = None;
        if let Err(e) = self.store.save(&self.settings) {
            self.log.error(format!(
                "Could not save settings to {}: {e}",
                self.store.path().display()
            ));
        }
    }

    fn central(&mut self, ui: &mut egui::Ui) {
        let p = self.palette;
        let mut close = None;
        egui::Frame::new()
            .fill(p.bg_panel)
            .inner_margin(Margin::symmetric(6, 3))
            .stroke(Stroke::new(1.0_f32, p.border))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    for (i, doc) in self.documents.iter().enumerate() {
                        let response = ui::tab_button(ui, &p, i == self.active, doc.title());
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
            self.active = i;
            self.pending.push(Command::CloseTab);
        }
        match self.documents.get(self.active) {
            Some(Document::Welcome) => {
                if let Some(cmd) = welcome::show(ui, &p, &self.keymap, &self.settings.recent) {
                    self.pending.push(cmd);
                }
            }
            None => ui::empty_state(
                ui,
                &p,
                "No open files",
                "File ▸ Open (Ctrl+O), or View ▸ Welcome Page.",
            ),
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

fn open_in_file_manager(dir: &std::path::Path) -> std::io::Result<()> {
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
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = ctx.input(|i| i.time);
        let p = self.palette;
        self.collect_shortcuts(ctx);
        self.track_window(ctx, now);

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
        self.pending.extend(fired);

        let flash = self.flash.as_ref().filter(|f| f.until > now).map(|f| {
            let color: Color32 = match f.level {
                Level::Error => p.error,
                Level::Warning => p.warning,
                Level::Hint => p.accent,
                Level::Info => p.text,
            };
            (f.text.as_str(), color)
        });
        egui::TopBottomPanel::bottom("status_bar")
            .frame(
                egui::Frame::new()
                    .fill(p.bg_top)
                    .inner_margin(Margin::symmetric(10, 3)),
            )
            .show(ctx, |ui| {
                statusbar::show(
                    ui,
                    &p,
                    &statusbar::StatusInfo {
                        flash,
                        ..Default::default()
                    },
                )
            });
        if let Some(f) = &self.flash {
            if f.until > now {
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(f.until - now));
            }
        }

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
            .show(ctx, |ui| self.central(ui));

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

        for cmd in std::mem::take(&mut self.pending) {
            self.run(ctx, cmd);
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
