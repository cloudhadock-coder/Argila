//! Argila: IDE Object Pascal/Delphi em Rust.
//!
//! A logica (configuracoes, atalhos, arquivos, mensagens) fica separada da interface (`ui`) para
//! poder ser testada sem janela. Modulos das proximas fases: editor, syntax, projects, dfm, compiler.

pub mod app;
pub mod diagnostics;
pub mod filesystem;
pub mod settings;
pub mod shortcuts;
pub mod themes;
pub mod ui;
