//! Buffer de texto (rope) com selecao e historico de desfazer/refazer.
//!
//! Posicoes sao indices de caractere. O texto usa so `\n` como quebra (a quebra original do
//! arquivo fica no `TextFormat` do documento).

use std::ops::Range;

use ropey::Rope;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    pub anchor: usize,
    pub head: usize,
}

impl Selection {
    pub fn caret(pos: usize) -> Self {
        Self {
            anchor: pos,
            head: pos,
        }
    }

    pub fn new(anchor: usize, head: usize) -> Self {
        Self { anchor, head }
    }

    pub fn range(&self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }
}

/// Troca `removed` (a partir de `at`) por `inserted`. Numa transacao as edicoes sao aplicadas em
/// sequencia: cada `at` vale para o texto ja com as edicoes anteriores.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    pub at: usize,
    pub removed: String,
    pub inserted: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditKind {
    /// Digitacao: teclas seguidas viram um unico desfazer (por palavra).
    Typing,
    /// Backspace/Delete seguidos tambem se agrupam.
    Deleting,
    Other,
}

#[derive(Debug)]
struct Transaction {
    edits: Vec<Edit>,
    before: Selection,
    after: Selection,
    kind: EditKind,
    time: f64,
}

#[derive(Debug)]
pub struct TextBuffer {
    rope: Rope,
    pub selection: Selection,
    undo: Vec<Transaction>,
    redo: Vec<Transaction>,
    /// Tamanho da pilha de desfazer quando o arquivo foi salvo (None: estado salvo inalcancavel).
    saved_at: Option<usize>,
    /// A ultima acao foi uma edicao que pode ser estendida pela proxima tecla.
    can_extend: bool,
    version: u64,
    dirty_line: Option<usize>,
}

impl TextBuffer {
    /// Pausa maxima entre teclas para continuar o mesmo grupo de desfazer.
    pub const GROUP_SECONDS: f64 = 1.5;

    pub fn new(text: &str) -> Self {
        Self {
            rope: Rope::from_str(text),
            selection: Selection::default(),
            undo: Vec::new(),
            redo: Vec::new(),
            saved_at: Some(0),
            can_extend: false,
            version: 0,
            dirty_line: None,
        }
    }

    pub fn rope(&self) -> &Rope {
        &self.rope
    }

    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    pub fn len_chars(&self) -> usize {
        self.rope.len_chars()
    }

    pub fn line_count(&self) -> usize {
        self.rope.len_lines()
    }

    /// Muda a cada alteracao de texto (chave de caches).
    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn line_start(&self, line: usize) -> usize {
        self.rope.line_to_char(line.min(self.rope.len_lines()))
    }

    /// Comprimento da linha sem a quebra.
    pub fn line_len(&self, line: usize) -> usize {
        let slice = self.rope.line(line);
        let n = slice.len_chars();
        if n > 0 && slice.char(n - 1) == '\n' {
            n - 1
        } else {
            n
        }
    }

    pub fn line_of(&self, pos: usize) -> usize {
        self.rope.char_to_line(pos.min(self.rope.len_chars()))
    }

    /// (linha, coluna) base 0.
    pub fn line_col(&self, pos: usize) -> (usize, usize) {
        let line = self.line_of(pos);
        (line, pos.min(self.rope.len_chars()) - self.line_start(line))
    }

    pub fn pos(&self, line: usize, col: usize) -> usize {
        let line = line.min(self.line_count() - 1);
        self.line_start(line) + col.min(self.line_len(line))
    }

    pub fn slice(&self, range: Range<usize>) -> String {
        let end = range.end.min(self.rope.len_chars());
        self.rope.slice(range.start.min(end)..end).to_string()
    }

    pub fn char_at(&self, pos: usize) -> Option<char> {
        (pos < self.rope.len_chars()).then(|| self.rope.char(pos))
    }

    pub fn set_selection(&mut self, selection: Selection) {
        let len = self.rope.len_chars();
        self.selection = Selection::new(selection.anchor.min(len), selection.head.min(len));
        self.can_extend = false;
    }

    pub fn is_modified(&self) -> bool {
        self.saved_at != Some(self.undo.len())
    }

    pub fn mark_saved(&mut self) {
        self.saved_at = Some(self.undo.len());
        self.can_extend = false;
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Primeira linha alterada desde a ultima consulta (para invalidar o realce).
    pub fn take_dirty_line(&mut self) -> Option<usize> {
        self.dirty_line.take()
    }

    /// Troca `range` por `text` e poe o cursor no fim do que foi inserido.
    pub fn replace(&mut self, range: Range<usize>, text: &str, kind: EditKind, now: f64) {
        let after = Selection::caret(range.start + text.chars().count());
        let edit = Edit {
            at: range.start,
            removed: self.slice(range),
            inserted: text.to_owned(),
        };
        self.transact(vec![edit], after, kind, now);
    }

    /// Substitui a selecao (ou insere no cursor).
    pub fn insert(&mut self, text: &str, kind: EditKind, now: f64) {
        self.replace(self.selection.range(), text, kind, now);
    }

    pub fn transact(&mut self, edits: Vec<Edit>, after: Selection, kind: EditKind, now: f64) {
        if edits
            .iter()
            .all(|e| e.removed.is_empty() && e.inserted.is_empty())
        {
            self.set_selection(after);
            return;
        }
        let before = self.selection;
        for edit in &edits {
            self.apply(edit);
        }
        let len = self.rope.len_chars();
        self.selection = Selection::new(after.anchor.min(len), after.head.min(len));
        self.redo.clear();
        if self.saved_at.is_some_and(|s| s > self.undo.len()) {
            self.saved_at = None;
        }
        if self.can_merge(kind, before, &edits, now) {
            let last = self
                .undo
                .last_mut()
                .expect("can_merge garante uma transacao anterior");
            last.edits.extend(edits);
            last.after = self.selection;
            last.time = now;
        } else {
            self.undo.push(Transaction {
                edits,
                before,
                after: self.selection,
                kind,
                time: now,
            });
        }
        self.can_extend = true;
    }

    fn can_merge(&self, kind: EditKind, before: Selection, edits: &[Edit], now: f64) -> bool {
        let Some(last) = self.undo.last() else {
            return false;
        };
        let mergeable = self.can_extend
            && kind != EditKind::Other
            && last.kind == kind
            && last.after == before
            && now - last.time <= Self::GROUP_SECONDS
            && self.saved_at != Some(self.undo.len());
        if !mergeable || kind == EditKind::Deleting {
            return mergeable;
        }
        // digitacao: um novo grupo a cada palavra e a cada quebra de linha
        let prev = last.edits.last().and_then(|e| e.inserted.chars().last());
        let next = edits.first().and_then(|e| e.inserted.chars().next());
        match (prev, next) {
            (Some(p), Some(n)) => {
                n != '\n' && p != '\n' && !(p.is_whitespace() && !n.is_whitespace())
            }
            _ => false,
        }
    }

    fn apply(&mut self, edit: &Edit) {
        let line = self.line_of(edit.at);
        self.dirty_line = Some(self.dirty_line.map_or(line, |d| d.min(line)));
        let removed = edit.removed.chars().count();
        if removed > 0 {
            self.rope.remove(edit.at..edit.at + removed);
        }
        if !edit.inserted.is_empty() {
            self.rope.insert(edit.at, &edit.inserted);
        }
        self.version += 1;
    }

    pub fn undo(&mut self) -> bool {
        let Some(tx) = self.undo.pop() else {
            return false;
        };
        for e in tx.edits.iter().rev() {
            self.apply(&Edit {
                at: e.at,
                removed: e.inserted.clone(),
                inserted: e.removed.clone(),
            });
        }
        self.selection = tx.before;
        self.redo.push(tx);
        self.can_extend = false;
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(tx) = self.redo.pop() else {
            return false;
        };
        for e in &tx.edits {
            self.apply(e);
        }
        self.selection = tx.after;
        self.undo.push(tx);
        self.can_extend = false;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn type_str(b: &mut TextBuffer, s: &str, t: &mut f64) {
        for c in s.chars() {
            *t += 0.1;
            b.insert(&c.to_string(), EditKind::Typing, *t);
        }
    }

    #[test]
    fn typing_groups_by_word_and_undo_restores() {
        let mut b = TextBuffer::new("");
        let mut t = 0.0;
        type_str(&mut b, "begin end", &mut t);
        assert_eq!(b.text(), "begin end");
        assert!(b.undo());
        assert_eq!(b.text(), "begin ");
        assert!(b.undo());
        assert_eq!(b.text(), "");
        assert!(!b.is_modified());
        assert!(b.redo() && b.redo());
        assert_eq!(b.text(), "begin end");
    }

    #[test]
    fn pause_and_cursor_move_break_groups() {
        let mut b = TextBuffer::new("");
        b.insert("a", EditKind::Typing, 0.0);
        b.insert("b", EditKind::Typing, 5.0);
        b.undo();
        assert_eq!(b.text(), "a");
        b.redo();
        b.set_selection(Selection::caret(0));
        b.insert("x", EditKind::Typing, 5.1);
        b.undo();
        assert_eq!(b.text(), "ab");
    }

    #[test]
    fn modified_flag_follows_save_point() {
        let mut b = TextBuffer::new("x");
        let mut t = 0.0;
        b.set_selection(Selection::caret(1));
        type_str(&mut b, "yz", &mut t);
        b.mark_saved();
        assert!(!b.is_modified());
        type_str(&mut b, "w", &mut t); // nao pode se juntar ao grupo de antes do salvamento
        assert!(b.is_modified());
        b.undo();
        assert_eq!(b.text(), "xyz");
        assert!(!b.is_modified());
        b.undo();
        assert!(b.is_modified());
        b.insert("q", EditKind::Other, 9.0); // estado salvo ficou no refazer descartado
        assert!(b.is_modified());
        assert!(!b.can_redo());
    }

    #[test]
    fn multi_edit_transaction_is_one_undo() {
        let mut b = TextBuffer::new("a a a");
        let edits = vec![
            Edit {
                at: 4,
                removed: "a".into(),
                inserted: "bb".into(),
            },
            Edit {
                at: 2,
                removed: "a".into(),
                inserted: "bb".into(),
            },
            Edit {
                at: 0,
                removed: "a".into(),
                inserted: "bb".into(),
            },
        ];
        b.transact(edits, Selection::caret(0), EditKind::Other, 0.0);
        assert_eq!(b.text(), "bb bb bb");
        b.undo();
        assert_eq!(b.text(), "a a a");
        assert_eq!(b.take_dirty_line(), Some(0));
    }

    #[test]
    fn line_helpers() {
        let b = TextBuffer::new("ab\ncde\n");
        assert_eq!(b.line_count(), 3);
        assert_eq!(b.line_len(1), 3);
        assert_eq!(b.line_col(4), (1, 1));
        assert_eq!(b.pos(1, 99), 6);
        assert_eq!(b.pos(9, 0), 7);
    }
}
