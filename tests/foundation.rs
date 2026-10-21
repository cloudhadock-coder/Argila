//! Testes de integracao da Fase 1: configuracoes persistentes e keymap.

use argila::settings::{BottomTab, LeftTab, Settings, SettingsStore};
use argila::shortcuts::{Command, KeyChord, Keymap, USER_KEYMAP_FILE};

#[test]
fn settings_roundtrip_through_disk() {
    let dir = tempfile::tempdir().unwrap();
    let store = SettingsStore::new(dir.path().join("Argila").join("settings.toml"));
    let mut s = Settings::default();
    s.layout.show_right = false;
    s.layout.left_tab = LeftTab::Structure;
    s.layout.bottom_tab = BottomTab::BuildOutput;
    s.layout.left_width = 333.0;
    s.window.maximized = true;
    s.recent
        .push_file("D:\\repositorios\\sistemahorus\\Unit_Fl_Funcionario.pas");
    store.save(&s).unwrap();
    let (loaded, warning) = store.load();
    assert_eq!(warning, None);
    assert_eq!(loaded, s);
}

#[test]
fn missing_settings_file_gives_defaults_silently() {
    let dir = tempfile::tempdir().unwrap();
    let (loaded, warning) = SettingsStore::new(dir.path().join("nada.toml")).load();
    assert_eq!(loaded, Settings::default());
    assert_eq!(warning, None);
}

#[test]
fn corrupt_settings_are_backed_up_not_lost() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.toml");
    std::fs::write(&path, "[layout\nshow_left = talvez").unwrap();
    let (loaded, warning) = SettingsStore::new(&path).load();
    assert_eq!(loaded, Settings::default());
    assert!(warning.unwrap().contains("Invalid settings file"));
    let backup = std::fs::read_to_string(dir.path().join("settings.toml.bak")).unwrap();
    assert_eq!(backup, "[layout\nshow_left = talvez");
}

#[test]
fn partial_settings_keep_defaults_for_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.toml");
    std::fs::write(&path, "[layout]\nshow_bottom = false\nchave_futura = 1\n").unwrap();
    let (loaded, warning) = SettingsStore::new(&path).load();
    assert_eq!(warning, None);
    assert!(!loaded.layout.show_bottom);
    assert_eq!(loaded.window, Settings::default().window);
}

#[test]
fn delphi_profile_has_no_conflicts_and_expected_bindings() {
    let km = Keymap::delphi12();
    assert!(km.conflicts().is_empty(), "{:?}", km.conflicts());
    let cmd = |s: &str| km.lookup(KeyChord::parse(s).unwrap());
    assert_eq!(cmd("Ctrl+S"), Some(Command::Save));
    assert_eq!(cmd("Shift+Ctrl+S"), Some(Command::SaveAll));
    assert_eq!(cmd("Ctrl+R"), Some(Command::Replace));
    assert_eq!(cmd("Ctrl+H"), Some(Command::Replace));
    assert_eq!(cmd("F12"), Some(Command::ToggleFormUnit));
    assert_eq!(cmd("F11"), Some(Command::ObjectInspector));
    assert_eq!(cmd("Ctrl+F11"), Some(Command::OpenProject));
    assert_eq!(cmd("Ctrl+Alt+F11"), Some(Command::ProjectManager));
    assert_eq!(cmd("Shift+Ctrl+Down"), Some(Command::DeclImplToggle));
    assert_eq!(cmd("F9"), Some(Command::Run));
    assert_eq!(cmd("Ctrl+F9"), Some(Command::Compile));
    assert_eq!(cmd("Shift+Ctrl+Alt+S"), None);
}

#[test]
fn user_keymap_overrides_only_listed_commands_and_reports_problems() {
    let dir = tempfile::tempdir().unwrap();
    let json = r#"{
        "bindings": {
            "file.save": ["F2"],
            "run.run": [],
            "comando.inexistente": ["Ctrl+Q"],
            "search.find": ["Ctrl+Banana"]
        }
    }"#;
    std::fs::write(dir.path().join(USER_KEYMAP_FILE), json).unwrap();
    let (km, warnings) = Keymap::load_user(dir.path());
    assert_eq!(km.shortcut_text(Command::Save), "F2");
    assert_eq!(km.shortcut_text(Command::Run), "");
    assert_eq!(km.shortcut_text(Command::Find), "");
    assert_eq!(km.shortcut_text(Command::SaveAll), "Shift+Ctrl+S");
    assert_eq!(warnings.len(), 2, "{warnings:?}");
}

#[test]
fn invalid_user_keymap_falls_back_to_default() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(USER_KEYMAP_FILE), "{ isto nao e json").unwrap();
    let (km, warnings) = Keymap::load_user(dir.path());
    assert_eq!(km.shortcut_text(Command::Save), "Ctrl+S");
    assert_eq!(warnings.len(), 1);
}

#[test]
fn conflicts_are_detected() {
    let mut km = Keymap::delphi12();
    km.set(Command::Save, [KeyChord::parse("F9").unwrap()]);
    let conflicts = km.conflicts();
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].chord.to_string(), "F9");
    assert!(
        conflicts[0].commands.contains(&Command::Save)
            && conflicts[0].commands.contains(&Command::Run)
    );
}

#[test]
fn keymap_export_import_roundtrip() {
    let mut original = Keymap::delphi12();
    original.set(Command::Messages, [KeyChord::parse("Shift+Alt+M").unwrap()]);
    let mut imported = Keymap::delphi12();
    let warnings = imported.apply_json(&original.to_json()).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    for cmd in Command::ALL {
        let a: Vec<_> = original.chords(*cmd).collect();
        let b: Vec<_> = imported.chords(*cmd).collect();
        assert_eq!(a, b, "{}", cmd.id());
    }
}
