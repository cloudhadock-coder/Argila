//! Lexer do DFM textual. Mesmas regras do tokenizer do dfm2html.py: strings com `''` e `#nn`,
//! blocos binarios `{ hex }` que atravessam linhas, conjuntos `[ ]`, colecoes `< item ... end >`.

use super::{Emitter, LexState, Token, TokenKind, is_ident_char, is_ident_start};

const KEYWORDS: &[&str] = &[
    "object",
    "inherited",
    "inline",
    "end",
    "item",
    "true",
    "false",
];

pub fn lex_line(line: &[char], state: LexState, out: &mut Vec<Token>) -> LexState {
    let mut e = Emitter { out };
    let n = line.len();
    let mut i = 0;
    if state == LexState::DfmBinary {
        match line.iter().position(|c| *c == '}') {
            Some(p) => {
                e.push(0, p + 1, TokenKind::Binary);
                i = p + 1;
            }
            None => {
                e.push(0, n, TokenKind::Binary);
                return LexState::DfmBinary;
            }
        }
    }
    let mut after_colon = false;
    while i < n {
        let c = line[i];
        if c == '{' {
            match line[i..].iter().position(|c| *c == '}') {
                Some(p) => {
                    e.push(i, i + p + 1, TokenKind::Binary);
                    i += p + 1;
                    continue;
                }
                None => {
                    e.push(i, n, TokenKind::Binary);
                    return LexState::DfmBinary;
                }
            }
        }
        if c == '\''
            || (c == '#'
                && line
                    .get(i + 1)
                    .is_some_and(|d| d.is_ascii_digit() || *d == '$'))
        {
            let start = i;
            while i < n && (line[i] == '\'' || line[i] == '#') {
                if line[i] == '\'' {
                    i += 1;
                    while i < n {
                        if line[i] == '\'' {
                            if line.get(i + 1) == Some(&'\'') {
                                i += 2;
                                continue;
                            }
                            i += 1;
                            break;
                        }
                        i += 1;
                    }
                } else {
                    let s = i;
                    i += 1;
                    if line.get(i) == Some(&'$') {
                        i += 1;
                    }
                    while i < n && line[i].is_ascii_hexdigit() {
                        i += 1;
                    }
                    if i == s + 1 {
                        break;
                    }
                }
            }
            e.push(start, i.max(start + 1), TokenKind::String);
            i = i.max(start + 1);
            continue;
        }
        if c.is_ascii_digit()
            || ((c == '-' || c == '$') && line.get(i + 1).is_some_and(char::is_ascii_hexdigit))
        {
            let start = i;
            i += 1;
            while i < n && (line[i].is_ascii_alphanumeric() || line[i] == '.' || line[i] == '$') {
                i += 1;
            }
            e.push(start, i, TokenKind::Number);
            continue;
        }
        if is_ident_start(c) {
            let start = i;
            while i < n && (is_ident_char(line[i]) || line[i] == '.') {
                i += 1;
            }
            let word: String = line[start..i]
                .iter()
                .collect::<String>()
                .to_ascii_lowercase();
            let kind = if after_colon {
                TokenKind::TypeName
            } else if KEYWORDS.contains(&word.as_str()) {
                TokenKind::Keyword
            } else {
                TokenKind::Identifier
            };
            after_colon = false;
            e.push(start, i, kind);
            continue;
        }
        if c == ':' {
            after_colon = true;
        }
        let kind = if c.is_whitespace() {
            TokenKind::Text
        } else {
            TokenKind::Symbol
        };
        e.push(i, i + 1, kind);
        i += 1;
    }
    LexState::Normal
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex(src: &str, state: LexState) -> (Vec<(String, TokenKind)>, LexState) {
        let chars: Vec<char> = src.chars().collect();
        let mut out = Vec::new();
        let st = lex_line(&chars, state, &mut out);
        let toks = out
            .iter()
            .filter(|t| t.kind != TokenKind::Text)
            .map(|t| (chars[t.start..t.end].iter().collect(), t.kind))
            .collect();
        (toks, st)
    }

    #[test]
    fn object_header_and_values() {
        let (t, _) = lex("  object btnSalvar: TBitBtn", LexState::Normal);
        assert_eq!(t[0], ("object".into(), TokenKind::Keyword));
        assert_eq!(t[1], ("btnSalvar".into(), TokenKind::Identifier));
        assert_eq!(t[3], ("TBitBtn".into(), TokenKind::TypeName));
        let (t, _) = lex("    Caption = 'Fun'#231#227'o'", LexState::Normal);
        assert_eq!(t[0], ("Caption".into(), TokenKind::Identifier));
        assert_eq!(t[2], ("'Fun'#231#227'o'".into(), TokenKind::String));
        let (t, _) = lex("    Font.Height = -11", LexState::Normal);
        assert_eq!(t[0], ("Font.Height".into(), TokenKind::Identifier));
        assert_eq!(t[2], ("-11".into(), TokenKind::Number));
    }

    #[test]
    fn binary_blocks_span_lines() {
        let (_, st) = lex("    Glyph.Data = {", LexState::Normal);
        assert_eq!(st, LexState::DfmBinary);
        let (t, st) = lex("      0001AABBCC", LexState::DfmBinary);
        assert_eq!((t[0].1, st), (TokenKind::Binary, LexState::DfmBinary));
        let (t, st) = lex("      FF00}", LexState::DfmBinary);
        assert_eq!((t[0].1, st), (TokenKind::Binary, LexState::Normal));
    }
}
