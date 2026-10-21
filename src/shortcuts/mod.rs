//! Comandos da IDE e atalhos de teclado.
//!
//! Todo atalho passa por aqui. O perfil padrao segue o keymap "Default" do Delphi 12; o usuario
//! pode sobrescrever comandos num `keymap.json` (importar/exportar) e conflitos sao detectados.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use eframe::egui::{Key, Modifiers};
use serde::{Deserialize, Serialize};

macro_rules! commands {
    ($($variant:ident => $id:literal, $label:literal;)*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum Command { $($variant),* }

        impl Command {
            pub const ALL: &'static [Command] = &[$(Command::$variant),*];

            /// Identificador estavel usado no keymap.json.
            pub fn id(self) -> &'static str {
                match self { $(Command::$variant => $id),* }
            }

            pub fn label(self) -> &'static str {
                match self { $(Command::$variant => $label),* }
            }
        }
    };
}

commands! {
    NewUnit => "file.new_unit", "New Unit";
    OpenFile => "file.open", "Open...";
    OpenProject => "file.open_project", "Open Project...";
    OpenFolder => "file.open_folder", "Open Folder...";
    Save => "file.save", "Save";
    SaveAll => "file.save_all", "Save All";
    CloseTab => "file.close", "Close";
    CloseProject => "file.close_project", "Close Project";
    Exit => "file.exit", "Exit";
    Undo => "edit.undo", "Undo";
    Redo => "edit.redo", "Redo";
    ToggleComment => "edit.toggle_comment", "Toggle Comment";
    CodeCompletion => "edit.code_completion", "Code Completion";
    ClassCompletion => "edit.class_completion", "Complete Class at Cursor";
    DeclImplToggle => "edit.decl_impl", "Toggle Declaration/Implementation";
    Find => "search.find", "Find...";
    Replace => "search.replace", "Replace...";
    FindInFiles => "search.find_in_files", "Find in Files...";
    SearchAgain => "search.again", "Search Again";
    GotoLine => "search.goto_line", "Go to Line Number...";
    FindDeclaration => "search.find_declaration", "Find Declaration";
    ProjectManager => "view.project_manager", "Project Manager";
    Structure => "view.structure", "Structure";
    ObjectInspector => "view.object_inspector", "Object Inspector";
    Messages => "view.messages", "Messages";
    ToggleFormUnit => "view.toggle_form_unit", "Toggle Form/Unit";
    ViewUnits => "view.units", "Units...";
    ViewForms => "view.forms", "Forms...";
    Welcome => "view.welcome", "Welcome Page";
    AddToProject => "project.add", "Add to Project...";
    RemoveFromProject => "project.remove", "Remove from Project...";
    RenameUnit => "project.rename", "Rename Unit...";
    ProjectOptions => "project.options", "Options...";
    Compile => "project.compile", "Compile";
    Build => "project.build", "Build";
    Run => "run.run", "Run";
    RunWithoutDebugging => "run.run_without_debugging", "Run Without Debugging";
    ExportKeymap => "tools.export_keymap", "Export Keymap...";
    OpenSettingsFolder => "tools.settings_folder", "Open Settings Folder";
    KeyboardShortcuts => "help.shortcuts", "Keyboard Shortcuts";
    About => "help.about", "About Argila";
}

impl Command {
    pub fn from_id(id: &str) -> Option<Command> {
        Self::ALL.iter().copied().find(|c| c.id() == id)
    }

    /// Grupo do comando ("file", "edit", ...), igual ao prefixo do id.
    pub fn group(self) -> &'static str {
        self.id().split('.').next().unwrap_or_default()
    }
}

/// Se um comando pode rodar agora. Comandos ainda nao implementados nunca simulam execucao:
/// a IDE mostra uma indicacao discreta e registra no painel Messages.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Availability {
    Ready,
    NotImplemented,
    Unavailable(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct KeyChord {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: Key,
}

impl KeyChord {
    pub const fn new(key: Key) -> Self {
        Self {
            ctrl: false,
            shift: false,
            alt: false,
            key,
        }
    }

    pub const fn ctrl(mut self) -> Self {
        self.ctrl = true;
        self
    }

    pub const fn shift(mut self) -> Self {
        self.shift = true;
        self
    }

    pub const fn alt(mut self) -> Self {
        self.alt = true;
        self
    }

    /// Converte um evento de tecla do egui (no Windows, `command` == Ctrl).
    pub fn from_input(key: Key, modifiers: Modifiers) -> Self {
        Self {
            ctrl: modifiers.ctrl || modifiers.command,
            shift: modifiers.shift,
            alt: modifiers.alt,
            key,
        }
    }

    /// Aceita "Shift+Ctrl+S", "ctrl+shift+s", "Ctrl+Alt+F11", "Ctrl+/", "Ctrl++" etc.
    pub fn parse(text: &str) -> Result<Self, String> {
        let text = text.trim();
        let (mods, key_name) = match text.strip_suffix("++") {
            Some(rest) => (rest, "+"),
            None => text.rsplit_once('+').unwrap_or(("", text)),
        };
        let key = parse_key(key_name.trim())
            .ok_or_else(|| format!("unknown key '{}' in '{text}'", key_name.trim()))?;
        let mut chord = KeyChord::new(key);
        for m in mods.split('+').map(str::trim).filter(|m| !m.is_empty()) {
            match m.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => chord.ctrl = true,
                "shift" => chord.shift = true,
                "alt" => chord.alt = true,
                other => return Err(format!("unknown modifier '{other}' in '{text}'")),
            }
        }
        Ok(chord)
    }
}

fn parse_key(name: &str) -> Option<Key> {
    if let Some(key) = Key::from_name(name) {
        return Some(key);
    }
    let upper = name.to_ascii_uppercase();
    Key::from_name(&upper).or(match upper.as_str() {
        "UP" => Some(Key::ArrowUp),
        "DOWN" => Some(Key::ArrowDown),
        "LEFT" => Some(Key::ArrowLeft),
        "RIGHT" => Some(Key::ArrowRight),
        "SPACE" => Some(Key::Space),
        "ENTER" | "RETURN" => Some(Key::Enter),
        "ESC" | "ESCAPE" => Some(Key::Escape),
        "BKSP" | "BACKSPACE" => Some(Key::Backspace),
        "DEL" | "DELETE" => Some(Key::Delete),
        "INS" | "INSERT" => Some(Key::Insert),
        "PGUP" | "PAGEUP" => Some(Key::PageUp),
        "PGDN" | "PAGEDOWN" => Some(Key::PageDown),
        "HOME" => Some(Key::Home),
        "END" => Some(Key::End),
        "TAB" => Some(Key::Tab),
        _ => None,
    })
}

fn key_label(key: Key) -> &'static str {
    match key {
        Key::ArrowUp => "Up",
        Key::ArrowDown => "Down",
        Key::ArrowLeft => "Left",
        Key::ArrowRight => "Right",
        // o egui nomeia pontuacao por extenso ("Slash"); menus do Delphi mostram o simbolo
        Key::Slash => "/",
        Key::Backslash => "\\",
        Key::Minus => "-",
        Key::Plus => "+",
        Key::Equals => "=",
        Key::Comma => ",",
        Key::Period => ".",
        Key::Semicolon => ";",
        Key::OpenBracket => "[",
        Key::CloseBracket => "]",
        other => other.name(),
    }
}

impl fmt::Display for KeyChord {
    /// Mesma ordem dos menus do Delphi: Shift+Ctrl+Alt+Tecla.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.shift {
            f.write_str("Shift+")?;
        }
        if self.ctrl {
            f.write_str("Ctrl+")?;
        }
        if self.alt {
            f.write_str("Alt+")?;
        }
        f.write_str(key_label(self.key))
    }
}

pub const DEFAULT_PROFILE: &str = "Delphi 12 (Default keymap)";
pub const USER_KEYMAP_FILE: &str = "keymap.json";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conflict {
    pub chord: KeyChord,
    pub commands: Vec<Command>,
}

#[derive(Clone, Debug)]
pub struct Keymap {
    pub profile: String,
    bindings: Vec<(Command, KeyChord)>,
}

/// Formato do keymap.json: id do comando -> lista de atalhos ([] remove o atalho).
#[derive(Debug, Default, Serialize, Deserialize)]
struct KeymapFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    profile: Option<String>,
    #[serde(default)]
    bindings: BTreeMap<String, Vec<String>>,
}

impl Keymap {
    /// Perfil padrao. Confirmados no docwiki da Embarcadero: Replace (Ctrl+R / Ctrl+H) e
    /// Open Project (Ctrl+F11). Os demais seguem o keymap Default do Delphi; Ctrl+O para
    /// abrir arquivo vem da especificacao (no Delphi, Ctrl+O e prefixo de combinacoes).
    pub fn delphi12() -> Self {
        use Command::*;
        let k = KeyChord::new;
        let bindings = vec![
            (OpenFile, k(Key::O).ctrl()),
            (OpenProject, k(Key::F11).ctrl()),
            (Save, k(Key::S).ctrl()),
            (SaveAll, k(Key::S).ctrl().shift()),
            (CloseTab, k(Key::F4).ctrl()),
            (Undo, k(Key::Z).ctrl()),
            (Undo, k(Key::Backspace).alt()),
            (Redo, k(Key::Z).ctrl().shift()),
            (ToggleComment, k(Key::Slash).ctrl()),
            (CodeCompletion, k(Key::Space).ctrl()),
            (ClassCompletion, k(Key::C).ctrl().shift()),
            (DeclImplToggle, k(Key::ArrowUp).ctrl().shift()),
            (DeclImplToggle, k(Key::ArrowDown).ctrl().shift()),
            (Find, k(Key::F).ctrl()),
            (Replace, k(Key::R).ctrl()),
            (Replace, k(Key::H).ctrl()),
            (FindInFiles, k(Key::F).ctrl().shift()),
            (SearchAgain, k(Key::F3)),
            (GotoLine, k(Key::G).alt()),
            (FindDeclaration, k(Key::ArrowUp).alt()),
            (ProjectManager, k(Key::F11).ctrl().alt()),
            (Structure, k(Key::F11).shift().alt()),
            (ObjectInspector, k(Key::F11)),
            (ToggleFormUnit, k(Key::F12)),
            (ViewUnits, k(Key::F12).ctrl()),
            (ViewForms, k(Key::F12).shift()),
            (AddToProject, k(Key::F11).shift()),
            (ProjectOptions, k(Key::F11).ctrl().shift()),
            (Compile, k(Key::F9).ctrl()),
            (Build, k(Key::F9).shift()),
            (Run, k(Key::F9)),
            (RunWithoutDebugging, k(Key::F9).ctrl().shift()),
        ];
        Self {
            profile: DEFAULT_PROFILE.to_owned(),
            bindings,
        }
    }

    pub fn bindings(&self) -> &[(Command, KeyChord)] {
        &self.bindings
    }

    pub fn chords(&self, cmd: Command) -> impl Iterator<Item = KeyChord> + '_ {
        self.bindings
            .iter()
            .filter(move |(c, _)| *c == cmd)
            .map(|(_, k)| *k)
    }

    /// Texto do primeiro atalho, para menus e dicas ("" se nao houver).
    pub fn shortcut_text(&self, cmd: Command) -> String {
        self.chords(cmd)
            .next()
            .map(|k| k.to_string())
            .unwrap_or_default()
    }

    pub fn lookup(&self, chord: KeyChord) -> Option<Command> {
        self.bindings
            .iter()
            .find(|(_, k)| *k == chord)
            .map(|(c, _)| *c)
    }

    /// Substitui todos os atalhos de um comando (lista vazia = sem atalho).
    pub fn set(&mut self, cmd: Command, chords: impl IntoIterator<Item = KeyChord>) {
        self.bindings.retain(|(c, _)| *c != cmd);
        self.bindings.extend(chords.into_iter().map(|k| (cmd, k)));
    }

    /// Atalhos ligados a mais de um comando, em ordem estavel.
    pub fn conflicts(&self) -> Vec<Conflict> {
        let mut by_chord: BTreeMap<String, Conflict> = BTreeMap::new();
        for (cmd, chord) in &self.bindings {
            let entry = by_chord
                .entry(chord.to_string())
                .or_insert_with(|| Conflict {
                    chord: *chord,
                    commands: Vec::new(),
                });
            if !entry.commands.contains(cmd) {
                entry.commands.push(*cmd);
            }
        }
        by_chord
            .into_values()
            .filter(|c| c.commands.len() > 1)
            .collect()
    }

    /// Exporta o keymap completo (todos os comandos, inclusive os sem atalho).
    pub fn to_json(&self) -> String {
        let bindings = Command::ALL
            .iter()
            .map(|c| {
                (
                    c.id().to_owned(),
                    self.chords(*c).map(|k| k.to_string()).collect(),
                )
            })
            .collect();
        let file = KeymapFile {
            profile: Some(self.profile.clone()),
            bindings,
        };
        serde_json::to_string_pretty(&file).expect("keymap sempre serializa")
    }

    /// Aplica um keymap.json por cima deste: so os comandos listados mudam.
    /// Devolve avisos (comando/atalho desconhecido) sem abortar o restante.
    pub fn apply_json(&mut self, json: &str) -> Result<Vec<String>, String> {
        let file: KeymapFile =
            serde_json::from_str(json).map_err(|e| format!("invalid keymap JSON: {e}"))?;
        let mut warnings = Vec::new();
        if let Some(profile) = file.profile {
            self.profile = profile;
        }
        for (id, keys) in file.bindings {
            let Some(cmd) = Command::from_id(&id) else {
                warnings.push(format!("keymap: unknown command '{id}' ignored"));
                continue;
            };
            let mut chords = Vec::new();
            for text in keys {
                match KeyChord::parse(&text) {
                    Ok(chord) => chords.push(chord),
                    Err(e) => warnings.push(format!("keymap: {e} (command '{id}')")),
                }
            }
            self.set(cmd, chords);
        }
        Ok(warnings)
    }

    /// Perfil padrao + `keymap.json` da pasta de configuracao, se existir.
    pub fn load_user(config_dir: &Path) -> (Keymap, Vec<String>) {
        let mut keymap = Keymap::delphi12();
        let path = config_dir.join(USER_KEYMAP_FILE);
        let json = match std::fs::read_to_string(&path) {
            Ok(json) => json,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (keymap, Vec::new()),
            Err(e) => {
                return (
                    keymap,
                    vec![format!("Could not read {}: {e}", path.display())],
                );
            }
        };
        match keymap.apply_json(&json) {
            Ok(warnings) => (keymap, warnings),
            Err(e) => (
                Keymap::delphi12(),
                vec![format!("{} ignored: {e}", path.display())],
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_ids_are_unique_and_resolvable() {
        for c in Command::ALL {
            assert_eq!(Command::from_id(c.id()), Some(*c));
        }
        let mut ids: Vec<_> = Command::ALL.iter().map(|c| c.id()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), Command::ALL.len());
    }

    #[test]
    fn chords_parse_and_print_like_delphi() {
        for text in [
            "Shift+Ctrl+S",
            "Ctrl+Alt+F11",
            "Shift+Alt+F11",
            "Shift+Ctrl+Up",
            "Ctrl+Space",
            "Alt+G",
            "F12",
            "Ctrl+/",
        ] {
            let chord = KeyChord::parse(text).unwrap();
            assert_eq!(chord.to_string(), text);
        }
        assert_eq!(
            KeyChord::parse("ctrl+shift+s"),
            KeyChord::parse("Shift+Ctrl+S")
        );
        assert_eq!(
            KeyChord::parse("Ctrl+ArrowDown").unwrap(),
            KeyChord::new(Key::ArrowDown).ctrl()
        );
        assert!(KeyChord::parse("Hyper+S").is_err());
        assert!(KeyChord::parse("Ctrl+Banana").is_err());
    }
}
