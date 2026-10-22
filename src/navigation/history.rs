//! Historico de navegacao: Alt+Left volta para onde o cursor estava antes de um salto
//! (Find Declaration, Ctrl+clique, declaracao/implementacao, Go to Symbol, resultados de busca)
//! e Alt+Right refaz o salto.

use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Location {
    pub path: PathBuf,
    /// Linha e coluna base 0.
    pub line: usize,
    pub col: usize,
}

#[derive(Debug, Default)]
pub struct History {
    back: Vec<Location>,
    forward: Vec<Location>,
}

impl History {
    const MAX: usize = 100;

    /// Guarda a posicao de antes de um salto. Um novo salto descarta o "avancar".
    pub fn record(&mut self, from: Location) {
        if self.back.last() != Some(&from) {
            self.back.push(from);
            if self.back.len() > Self::MAX {
                self.back.remove(0);
            }
        }
        self.forward.clear();
    }

    pub fn back(&mut self, current: Location) -> Option<Location> {
        let to = self.back.pop()?;
        self.forward.push(current);
        Some(to)
    }

    pub fn forward(&mut self, current: Location) -> Option<Location> {
        let to = self.forward.pop()?;
        self.back.push(current);
        Some(to)
    }

    pub fn can_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub fn can_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    /// Arquivo fechado ou renomeado: as posicoes dele deixam de valer.
    pub fn forget(&mut self, path: &std::path::Path) {
        let keep = |l: &Location| !l.path.as_os_str().eq_ignore_ascii_case(path.as_os_str());
        self.back.retain(keep);
        self.forward.retain(keep);
    }
}
