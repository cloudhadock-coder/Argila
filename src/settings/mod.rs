//! Configuracoes persistentes: layout dos paineis, janela, editor e arquivos recentes.
//!
//! Ficam em `%APPDATA%\Argila\settings.toml`. Arquivo ausente = padroes; arquivo invalido =
//! padroes + copia `.bak` do original (nunca sobrescrevemos sem guardar o que o usuario tinha).

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::filesystem;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub window: WindowSettings,
    pub layout: LayoutSettings,
    pub editor: EditorSettings,
    pub recent: RecentSettings,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowSettings {
    pub width: f32,
    pub height: f32,
    pub maximized: bool,
}

impl Default for WindowSettings {
    fn default() -> Self {
        Self {
            width: 1400.0,
            height: 860.0,
            maximized: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LayoutSettings {
    pub show_left: bool,
    pub show_right: bool,
    pub show_bottom: bool,
    pub left_width: f32,
    pub right_width: f32,
    pub bottom_height: f32,
    pub left_tab: LeftTab,
    pub inspector_tab: InspectorTab,
    pub bottom_tab: BottomTab,
}

impl Default for LayoutSettings {
    fn default() -> Self {
        Self {
            show_left: true,
            show_right: true,
            show_bottom: true,
            left_width: 270.0,
            right_width: 330.0,
            bottom_height: 170.0,
            left_tab: LeftTab::default(),
            inspector_tab: InspectorTab::default(),
            bottom_tab: BottomTab::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LeftTab {
    #[default]
    ProjectExplorer,
    Structure,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum InspectorTab {
    #[default]
    Properties,
    Events,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BottomTab {
    #[default]
    Messages,
    SearchResults,
    Diagnostics,
    BuildOutput,
    Console,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EditorSettings {
    pub font_size: f32,
    /// Delphi usa 2 espacos por nivel por padrao.
    pub tab_size: usize,
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            font_size: 14.0,
            tab_size: 2,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecentSettings {
    pub files: Vec<String>,
    pub projects: Vec<String>,
}

impl RecentSettings {
    pub const MAX: usize = 12;

    pub fn push_file(&mut self, path: &str) {
        push_recent(&mut self.files, path);
    }

    pub fn push_project(&mut self, path: &str) {
        push_recent(&mut self.projects, path);
    }
}

/// Mais recente primeiro, sem duplicatas (comparacao sem diferenciar maiusculas, como no Windows).
fn push_recent(list: &mut Vec<String>, path: &str) {
    list.retain(|p| !p.eq_ignore_ascii_case(path));
    list.insert(0, path.to_owned());
    list.truncate(RecentSettings::MAX);
}

impl Settings {
    /// Corrige valores fora de faixa (arquivo editado a mao, monitor diferente...).
    pub fn sanitized(mut self) -> Self {
        let w = &mut self.window;
        w.width = clamp_or(w.width, 640.0, 10_000.0, 1400.0);
        w.height = clamp_or(w.height, 400.0, 10_000.0, 860.0);
        let l = &mut self.layout;
        l.left_width = clamp_or(l.left_width, 150.0, 900.0, 270.0);
        l.right_width = clamp_or(l.right_width, 200.0, 900.0, 330.0);
        l.bottom_height = clamp_or(l.bottom_height, 60.0, 900.0, 170.0);
        self.editor.font_size = clamp_or(self.editor.font_size, 8.0, 48.0, 14.0);
        self.editor.tab_size = self.editor.tab_size.clamp(1, 16);
        self.recent.files.truncate(RecentSettings::MAX);
        self.recent.projects.truncate(RecentSettings::MAX);
        self
    }
}

fn clamp_or(v: f32, min: f32, max: f32, fallback: f32) -> f32 {
    if v.is_finite() {
        v.clamp(min, max)
    } else {
        fallback
    }
}

pub struct SettingsStore {
    path: PathBuf,
}

impl SettingsStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn default_location() -> Self {
        Self::new(filesystem::config_dir().join("settings.toml"))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Nunca falha: devolve as configuracoes possiveis e, se for o caso, um aviso para o usuario.
    pub fn load(&self) -> (Settings, Option<String>) {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return (Settings::default(), None),
            Err(e) => {
                let msg = format!(
                    "Could not read {}: {e}. Using defaults.",
                    self.path.display()
                );
                return (Settings::default(), Some(msg));
            }
        };
        match toml::from_str::<Settings>(&text) {
            Ok(settings) => (settings.sanitized(), None),
            Err(e) => {
                let backup = self.path.with_extension("toml.bak");
                let saved = std::fs::copy(&self.path, &backup).is_ok();
                let msg = format!(
                    "Invalid settings file {} ({}). Using defaults{}.",
                    self.path.display(),
                    e.message(),
                    if saved {
                        format!("; original kept as {}", backup.display())
                    } else {
                        String::new()
                    }
                );
                (Settings::default(), Some(msg))
            }
        }
    }

    pub fn save(&self, settings: &Settings) -> io::Result<()> {
        let text = toml::to_string_pretty(settings).map_err(io::Error::other)?;
        filesystem::atomic_write(&self.path, text.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_files_dedupe_and_cap() {
        let mut r = RecentSettings::default();
        for i in 0..20 {
            r.push_file(&format!("C:\\p\\u{i}.pas"));
        }
        r.push_file("c:\\P\\U5.PAS");
        assert_eq!(r.files.len(), RecentSettings::MAX);
        assert_eq!(r.files[0], "c:\\P\\U5.PAS");
        assert_eq!(
            r.files
                .iter()
                .filter(|f| f.eq_ignore_ascii_case("C:\\p\\u5.pas"))
                .count(),
            1
        );
    }

    #[test]
    fn sanitize_fixes_out_of_range_values() {
        let mut s = Settings::default();
        s.window.width = f32::NAN;
        s.layout.left_width = 5.0;
        s.editor.font_size = 400.0;
        let s = s.sanitized();
        assert_eq!(s.window.width, 1400.0);
        assert_eq!(s.layout.left_width, 150.0);
        assert_eq!(s.editor.font_size, 48.0);
    }
}
