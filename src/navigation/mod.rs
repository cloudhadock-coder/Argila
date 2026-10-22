//! Navegacao entre simbolos e arquivos: indice do projeto (Go to Symbol, Find Declaration entre
//! units), historico de saltos (Alt+Left/Alt+Right) e Find in Files.

pub mod find_in_files;
pub mod history;
pub mod index;

pub use history::{History, Location};
pub use index::SymbolIndex;
