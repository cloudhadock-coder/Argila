//! Realce de sintaxe por linha, com estado entre linhas (comentarios e strings de varias linhas).
//!
//! O editor so pede tokens das linhas visiveis; `LineStates` guarda o estado no inicio de cada
//! linha ja vista e e invalidado a partir da linha editada, entao digitar no meio de um arquivo
//! de 10 mil linhas nao reprocessa o arquivo todo.

pub mod dfm;
pub mod pascal;

use std::path::Path;

use ropey::Rope;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    Pascal,
    Dfm,
    PlainText,
}

impl Language {
    pub fn from_path(path: &Path) -> Self {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        match ext.as_str() {
            "pas" | "dpr" | "dpk" | "inc" | "lpr" | "pp" => Language::Pascal,
            "dfm" | "fmx" | "lfm" | "xfm" => Language::Dfm,
            _ => Language::PlainText,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Language::Pascal => "Object Pascal",
            Language::Dfm => "DFM",
            Language::PlainText => "Plain Text",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind {
    Text,
    Keyword,
    Identifier,
    Number,
    String,
    Comment,
    Directive,
    Symbol,
    /// Nome de classe num `object X: TClasse` do DFM.
    TypeName,
    /// Bloco `{ ... }` hexadecimal do DFM.
    Binary,
}

/// Faixa `[start, end)` em caracteres dentro da linha.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    pub start: usize,
    pub end: usize,
    pub kind: TokenKind,
}

/// Estado no inicio de uma linha.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LexState {
    #[default]
    Normal,
    BraceComment,
    ParenComment,
    BraceDirective,
    ParenDirective,
    /// String multilinha do Delphi 12 (`'''`).
    MultiLineString,
    DfmBinary,
}

/// Tokeniza uma linha (sem a quebra). Os tokens cobrem a linha inteira, sem buracos.
pub fn lex_line(
    language: Language,
    line: &[char],
    state: LexState,
    out: &mut Vec<Token>,
) -> LexState {
    out.clear();
    match language {
        Language::Pascal => pascal::lex_line(line, state, out),
        Language::Dfm => dfm::lex_line(line, state, out),
        Language::PlainText => {
            if !line.is_empty() {
                out.push(Token {
                    start: 0,
                    end: line.len(),
                    kind: TokenKind::Text,
                });
            }
            LexState::Normal
        }
    }
}

/// Junta tokens vizinhos do mesmo tipo e garante cobertura total da linha.
pub(crate) struct Emitter<'a> {
    pub out: &'a mut Vec<Token>,
}

impl Emitter<'_> {
    pub fn push(&mut self, start: usize, end: usize, kind: TokenKind) {
        if start >= end {
            return;
        }
        if let Some(last) = self.out.last_mut() {
            if last.kind == kind && last.end == start {
                last.end = end;
                return;
            }
        }
        self.out.push(Token { start, end, kind });
    }
}

pub(crate) fn is_ident_start(c: char) -> bool {
    c == '_' || c.is_alphabetic()
}

pub(crate) fn is_ident_char(c: char) -> bool {
    c == '_' || c.is_alphanumeric()
}

pub(crate) fn find_seq(line: &[char], from: usize, seq: &[char]) -> Option<usize> {
    (from..line.len().saturating_sub(seq.len() - 1)).find(|&i| line[i..].starts_with(seq))
}

/// Linha `i` do rope sem a quebra final.
pub fn line_chars(rope: &Rope, i: usize) -> Vec<char> {
    let mut chars: Vec<char> = rope.line(i).chars().collect();
    if chars.last() == Some(&'\n') {
        chars.pop();
    }
    chars
}

/// Estado no inicio de cada linha, calculado sob demanda e invalidado a partir da linha editada.
#[derive(Default)]
pub struct LineStates {
    states: Vec<LexState>,
}

impl LineStates {
    /// O estado da linha `line` depende so das anteriores, que nao mudaram.
    pub fn invalidate_from(&mut self, line: usize) {
        self.states.truncate(line + 1);
    }

    pub fn clear(&mut self) {
        self.states.clear();
    }

    pub fn state_at(&mut self, language: Language, rope: &Rope, line: usize) -> LexState {
        if self.states.is_empty() {
            self.states.push(LexState::Normal);
        }
        let mut scratch = Vec::new();
        while self.states.len() <= line && self.states.len() <= rope.len_lines() {
            let i = self.states.len() - 1;
            let next = lex_line(language, &line_chars(rope, i), self.states[i], &mut scratch);
            self.states.push(next);
        }
        self.states[line.min(self.states.len() - 1)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_are_computed_lazily_and_invalidated() {
        let rope = Rope::from_str("a\n{ comentario\nainda }\nb\n");
        let mut s = LineStates::default();
        assert_eq!(
            s.state_at(Language::Pascal, &rope, 2),
            LexState::BraceComment
        );
        assert_eq!(s.state_at(Language::Pascal, &rope, 3), LexState::Normal);
        s.invalidate_from(1);
        let rope = Rope::from_str("a\nb\nainda }\nb\n");
        assert_eq!(s.state_at(Language::Pascal, &rope, 2), LexState::Normal);
    }

    #[test]
    fn language_from_extension() {
        assert_eq!(
            Language::from_path(Path::new("Unit1.PAS")),
            Language::Pascal
        );
        assert_eq!(Language::from_path(Path::new("Unit1.dfm")), Language::Dfm);
        assert_eq!(
            Language::from_path(Path::new("notas.txt")),
            Language::PlainText
        );
    }
}
