//! DFM is data. Parsing never instantiates classes or evaluates event handlers.
pub mod components;
pub mod designer;
pub mod parser;

/// Each demo is an independent temporary copy; no project file is replaced.
pub fn demo_copy() -> std::io::Result<std::path::PathBuf> {
    use std::io::Write;
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let path =
        std::env::temp_dir().join(format!("Argila-demo-{}-{unique}.dfm", std::process::id()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    file.write_all(include_bytes!("../../tests/fixtures/preview.dfm"))?;
    Ok(path)
}

use parser::{Form, ParseError};
use std::sync::{
    Arc,
    mpsc::{self, Receiver},
};

/// One versioned worker per document; outdated results are never displayed.
#[derive(Default)]
pub struct Preview {
    pub design: bool,
    pub selected: String,
    pub zoom: f32,
    pub show_hidden: bool,
    pub size: Option<[f32; 2]>,
    pub result: Option<Result<Arc<Form>, ParseError>>,
    pub version: Option<u64>,
    pending: Option<(u64, Receiver<Result<Arc<Form>, ParseError>>)>,
}

impl Preview {
    pub fn update(
        &mut self,
        version: u64,
        text: impl FnOnce() -> String,
        ctx: &eframe::egui::Context,
    ) {
        if let Some((v, rx)) = &self.pending {
            if let Ok(result) = rx.try_recv() {
                if *v == version {
                    self.result = Some(result);
                    self.version = Some(*v);
                    ctx.request_repaint();
                }
                self.pending = None;
            }
        }
        if self.version != Some(version) {
            self.result = None;
        }
        if self.pending.is_none() && self.version != Some(version) {
            let source = text();
            let (tx, rx) = mpsc::channel();
            let ctx = ctx.clone();
            self.pending = Some((version, rx));
            std::thread::spawn(move || {
                let _ = tx.send(parser::parse(&source).map(Arc::new));
                ctx.request_repaint();
            });
        }
    }
}
