//! Estado de code folding de uma vista: quais regioes estao recolhidas e o mapa entre linhas do
//! documento e linhas visiveis.
//!
//! As regioes vem da analise em segundo plano (`syntax::folding`). Entre uma edicao e a proxima
//! analise elas sao deslocadas pelo registro de alteracoes do buffer, entao recolher continua
//! certo enquanto se digita acima de um bloco fechado.

use std::collections::BTreeSet;

use super::buffer::{LineChange, TextBuffer};
use crate::syntax::folding::{FoldKind, FoldRange};

#[derive(Debug)]
pub struct Folding {
    ranges: Vec<FoldRange>,
    /// Linhas iniciais das regioes recolhidas.
    folded: BTreeSet<usize>,
    /// Intervalos escondidos (inclusivos), ordenados e sem sobreposicao.
    hidden: Vec<(usize, usize)>,
    /// prefix[i] = linhas escondidas antes do intervalo i (tamanho = hidden.len() + 1).
    prefix: Vec<usize>,
    /// Versao do buffer a que as regioes se referem.
    version: u64,
    pub disabled: bool,
}

impl Default for Folding {
    /// Sem regioes: toda linha e visivel (o mapa ja vale antes da primeira analise).
    fn default() -> Self {
        Self {
            ranges: Vec::new(),
            folded: BTreeSet::new(),
            hidden: Vec::new(),
            prefix: vec![0],
            version: 0,
            disabled: false,
        }
    }
}

impl Folding {
    /// Esquece regioes e recolhimentos (documento recarregado); mantem ligado/desligado.
    pub fn clear(&mut self) {
        *self = Folding {
            disabled: self.disabled,
            ..Folding::default()
        };
    }

    /// Regioes recalculadas para a versao `version` do buffer. Regioes recolhidas que ainda
    /// existem continuam recolhidas.
    pub fn set_ranges(&mut self, ranges: &[FoldRange], version: u64) {
        if version != self.version && !self.ranges.is_empty() {
            return; // analise de uma versao antiga: as regioes deslocadas valem mais
        }
        self.ranges = ranges.to_vec();
        let starts: BTreeSet<usize> = self.ranges.iter().map(|r| r.start).collect();
        self.folded.retain(|s| starts.contains(s));
        self.rebuild();
    }

    /// Acompanha as alteracoes do buffer desde a ultima chamada.
    pub fn sync(&mut self, buf: &TextBuffer) {
        let version = buf.version();
        if version == self.version {
            return;
        }
        match buf.changes_since(self.version) {
            Some(changes) => {
                for c in changes {
                    self.shift(c);
                }
            }
            None => {
                self.ranges.clear();
                self.folded.clear();
            }
        }
        self.version = version;
        self.rebuild();
    }

    /// Conferido pelo app: regioes calculadas para a versao atual do buffer.
    pub fn version(&self) -> u64 {
        self.version
    }

    fn shift(&mut self, c: LineChange) {
        if c.delta == 0 {
            return;
        }
        let at = c.line;
        let removed_to = if c.delta < 0 {
            at + c.delta.unsigned_abs()
        } else {
            at
        };
        let move_line = |l: usize| -> Option<usize> {
            if l <= at {
                Some(l)
            } else if c.delta < 0 && l <= removed_to {
                None // a linha sumiu junto com o trecho apagado
            } else {
                Some(l.saturating_add_signed(c.delta))
            }
        };
        self.ranges = std::mem::take(&mut self.ranges)
            .into_iter()
            .filter_map(|r| {
                let start = move_line(r.start)?;
                let end = if r.end <= at {
                    r.end
                } else if c.delta < 0 && r.end <= removed_to {
                    at
                } else {
                    r.end.saturating_add_signed(c.delta)
                };
                (end > start).then_some(FoldRange { start, end, ..r })
            })
            .collect();
        self.folded = std::mem::take(&mut self.folded)
            .into_iter()
            .filter_map(move_line)
            .collect();
    }

    fn rebuild(&mut self) {
        let mut spans: Vec<(usize, usize)> = self
            .folded
            .iter()
            .filter_map(|&s| self.range_at(s).map(|r| (r.start + 1, r.end)))
            .collect();
        spans.sort();
        self.hidden.clear();
        for (s, e) in spans {
            match self.hidden.last_mut() {
                Some(last) if s <= last.1 + 1 => last.1 = last.1.max(e),
                _ => self.hidden.push((s, e)),
            }
        }
        self.prefix = Vec::with_capacity(self.hidden.len() + 1);
        self.prefix.push(0);
        for &(s, e) in &self.hidden {
            let last = *self.prefix.last().expect("comeca com 0");
            self.prefix.push(last + e - s + 1);
        }
    }

    pub fn ranges(&self) -> &[FoldRange] {
        &self.ranges
    }

    /// A maior regiao que comeca nesta linha.
    pub fn range_at(&self, line: usize) -> Option<&FoldRange> {
        let i = self.ranges.partition_point(|r| r.start < line);
        self.ranges.get(i).filter(|r| r.start == line)
    }

    pub fn is_folded(&self, start: usize) -> bool {
        self.folded.contains(&start)
    }

    pub fn has_folds(&self) -> bool {
        !self.folded.is_empty()
    }

    pub fn is_hidden(&self, line: usize) -> bool {
        let i = self.hidden.partition_point(|h| h.1 < line);
        self.hidden.get(i).is_some_and(|h| h.0 <= line)
    }

    /// Linhas visiveis de um documento com `line_count` linhas.
    pub fn visible_rows(&self, line_count: usize) -> usize {
        line_count.saturating_sub(*self.prefix.last().unwrap_or(&0))
    }

    /// Linha visivel (base 0) de uma linha do documento; escondida conta como a regiao.
    pub fn row_of(&self, line: usize) -> usize {
        let i = self.hidden.partition_point(|h| h.1 < line);
        match self.hidden.get(i) {
            Some(&(s, _)) if s <= line => s - 1 - self.prefix[i],
            _ => line - self.prefix[i],
        }
    }

    /// Linha do documento mostrada na linha visivel `row`.
    pub fn line_of(&self, row: usize) -> usize {
        let k = self
            .hidden
            .iter()
            .enumerate()
            .take_while(|(i, h)| h.0 - self.prefix[*i] <= row)
            .count();
        row + self.prefix[k]
    }

    /// Proxima linha visivel depois de `line`.
    pub fn next_visible(&self, line: usize) -> usize {
        let next = line + 1;
        let i = self.hidden.partition_point(|h| h.1 < next);
        match self.hidden.get(i) {
            Some(&(s, e)) if s <= next => e + 1,
            _ => next,
        }
    }

    /// A regiao mais interna que contem a linha (ou comeca nela).
    pub fn innermost(&self, line: usize) -> Option<&FoldRange> {
        self.ranges
            .iter()
            .filter(|r| r.start <= line && line <= r.end)
            .min_by_key(|r| r.end - r.start)
    }

    pub fn fold(&mut self, start: usize) {
        if self.range_at(start).is_some() {
            self.folded.insert(start);
            self.rebuild();
        }
    }

    pub fn unfold(&mut self, start: usize) {
        if self.folded.remove(&start) {
            self.rebuild();
        }
    }

    /// Clique na marca da margem ou Shift+Ctrl+K, T.
    pub fn toggle(&mut self, start: usize) -> bool {
        if self.folded.contains(&start) {
            self.unfold(start);
            true
        } else if self.range_at(start).is_some() {
            self.fold(start);
            true
        } else {
            false
        }
    }

    /// Garante que a linha (cursor, resultado de busca) esteja visivel.
    pub fn reveal(&mut self, line: usize) -> bool {
        let before = self.folded.len();
        let ranges = &self.ranges;
        self.folded.retain(|&s| {
            !ranges
                .iter()
                .any(|r| r.start == s && s < line && line <= r.end)
        });
        let changed = self.folded.len() != before;
        if changed {
            self.rebuild();
        }
        changed
    }

    pub fn expand_all(&mut self) {
        self.folded.clear();
        self.rebuild();
    }

    /// Recolhe todas as regioes do tipo (metodos, classes, regioes, secoes).
    pub fn collapse_kind(&mut self, kind: FoldKind, keep_visible: usize) -> usize {
        let mut n = 0;
        for r in &self.ranges {
            if r.kind == kind && !(r.start < keep_visible && keep_visible <= r.end) {
                n += usize::from(self.folded.insert(r.start));
            }
        }
        self.rebuild();
        n
    }

    /// Rotinas declaradas dentro de outra rotina.
    pub fn collapse_nested(&mut self, keep_visible: usize) -> usize {
        let routines: Vec<&FoldRange> = self
            .ranges
            .iter()
            .filter(|r| r.kind == FoldKind::Routine)
            .collect();
        let mut n = 0;
        for r in &routines {
            let nested = routines.iter().any(|o| o.start < r.start && r.end <= o.end);
            if nested && !(r.start < keep_visible && keep_visible <= r.end) {
                n += usize::from(self.folded.insert(r.start));
            }
        }
        self.rebuild();
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::buffer::{EditKind, Selection};

    fn r(start: usize, end: usize) -> FoldRange {
        FoldRange {
            start,
            end,
            kind: FoldKind::Block,
        }
    }

    #[test]
    fn new_state_maps_lines_one_to_one() {
        let f = Folding::default();
        assert_eq!((f.row_of(7), f.line_of(7), f.next_visible(7)), (7, 7, 8));
        assert_eq!(f.visible_rows(10), 10);
        assert!(!f.is_hidden(3));
        let mut b = TextBuffer::new("a\nb\n");
        let mut f = Folding::default();
        b.insert("x\n", EditKind::Other, 0.0);
        f.sync(&b);
        assert_eq!(f.row_of(2), 2);
    }

    #[test]
    fn rows_and_lines_skip_folded_regions() {
        let mut f = Folding::default();
        f.set_ranges(&[r(2, 5), r(3, 4), r(8, 9)], 0);
        f.fold(3);
        f.fold(2);
        f.fold(8);
        assert_eq!(f.hidden, vec![(3, 5), (9, 9)]);
        assert_eq!(f.visible_rows(12), 8);
        let visible: Vec<usize> = (0..8).map(|row| f.line_of(row)).collect();
        assert_eq!(visible, [0, 1, 2, 6, 7, 8, 10, 11]);
        for (row, line) in visible.iter().enumerate() {
            assert_eq!(f.row_of(*line), row);
        }
        assert_eq!(f.row_of(4), 2); // escondida: conta como a linha da regiao
        assert!(f.is_hidden(4) && !f.is_hidden(6));
        assert_eq!(f.next_visible(2), 6);
        f.unfold(2); // a interna continua recolhida
        assert_eq!(f.hidden, vec![(4, 4), (9, 9)]);
        assert!(f.reveal(4));
        assert!(!f.is_hidden(4));
        assert_eq!(f.innermost(4).map(|r| r.start), Some(3));
    }

    #[test]
    fn edits_shift_folded_regions() {
        let mut b = TextBuffer::new("a\nb\nc\nd\ne\nf\ng\n");
        let mut f = Folding::default();
        f.sync(&b);
        f.set_ranges(&[r(3, 5)], b.version());
        f.fold(3);
        b.set_selection(Selection::caret(0));
        b.insert("x\ny\n", EditKind::Other, 0.0); // duas linhas acima
        f.sync(&b);
        assert!(f.is_folded(5));
        assert_eq!(f.hidden, vec![(6, 7)]);
        // analise antiga nao desfaz o deslocamento
        f.set_ranges(&[r(3, 5)], 0);
        assert!(f.is_folded(5));
        // apagar a linha inicial some com a regiao
        let start = b.line_start(4);
        b.replace(start..b.line_start(6), "", EditKind::Other, 1.0);
        f.sync(&b);
        assert!(!f.has_folds());
    }

    #[test]
    fn collapse_by_kind_keeps_the_cursor_visible() {
        let mut f = Folding::default();
        let routine = |s, e| FoldRange {
            start: s,
            end: e,
            kind: FoldKind::Routine,
        };
        f.set_ranges(&[routine(0, 10), routine(2, 4), routine(12, 15)], 0);
        assert_eq!(f.collapse_kind(FoldKind::Routine, 13), 2);
        assert!(f.is_folded(0) && !f.is_folded(12));
        f.expand_all();
        assert_eq!(f.collapse_nested(0), 1);
        assert!(f.is_folded(2));
    }
}
