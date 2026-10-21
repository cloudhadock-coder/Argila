//! Argila: IDE Object Pascal/Delphi em Rust.
//!
//! A logica (configuracoes, atalhos, arquivos, mensagens) fica separada da interface (`ui`) para
//! poder ser testada sem janela. Modulos das proximas fases: projects, dfm, compiler.

pub mod app;
pub mod diagnostics;
pub mod editor;
pub mod filesystem;
pub mod settings;
pub mod shortcuts;
pub mod syntax;
pub mod themes;
pub mod ui;
