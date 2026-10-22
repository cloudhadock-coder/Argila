//! Argila: IDE Object Pascal/Delphi em Rust.
//!
//! A logica (configuracoes, atalhos, arquivos, mensagens) fica separada da interface (`ui`) para
//! poder ser testada sem janela. DFM possui parser e layout independentes do canvas; compiler
//! entra em uma fase posterior.

pub mod app;
pub mod dfm;
pub mod diagnostics;
pub mod editor;
pub mod filesystem;
pub mod projects;
pub mod settings;
pub mod shortcuts;
pub mod syntax;
pub mod themes;
pub mod ui;
