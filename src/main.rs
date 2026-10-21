// sem janela de console no build de release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use argila::app::ArgilaApp;
use argila::settings::SettingsStore;
use eframe::egui;

fn main() -> eframe::Result<()> {
    let store = SettingsStore::default_location();
    let (settings, warning) = store.load();
    let window = &settings.window;
    let viewport = egui::ViewportBuilder::default()
        .with_title("Argila")
        .with_inner_size([window.width, window.height])
        .with_min_inner_size([800.0, 500.0])
        .with_maximized(window.maximized);
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "Argila",
        options,
        Box::new(move |cc| Ok(Box::new(ArgilaApp::new(cc, store, settings, warning)))),
    )
}
