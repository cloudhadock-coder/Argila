//! Lexer de Object Pascal. Mesmas regras do realce do dfm2html (`hiPascal`): comentarios `//`,
//! `{ }` e `(* *)`, diretivas `{$ }`, strings com `''` e codigos `#13`/`#$0D`, numeros `$FF`.

use super::{Emitter, LexState, Token, TokenKind, find_seq, is_ident_char, is_ident_start};

/// Palavras reservadas e as diretivas que o Delphi realca fora de contexto. Diretivas que
/// tambem sao nomes comuns (Name, Index, Read, Write, Default, Message...) ficam de fora.
pub(crate) const KEYWORDS: &[&str] = &[
    "and",
    "array",
    "as",
    "asm",
    "begin",
    "case",
    "class",
    "const",
    "constructor",
    "destructor",
    "dispinterface",
    "div",
    "do",
    "downto",
    "else",
    "end",
    "except",
    "exports",
    "file",
    "finalization",
    "finally",
    "for",
    "function",
    "goto",
    "if",
    "implementation",
    "in",
    "inherited",
    "initialization",
    "inline",
    "interface",
    "is",
    "label",
    "library",
    "mod",
    "nil",
    "not",
    "object",
    "of",
    "or",
    "out",
    "packed",
    "procedure",
    "program",
    "property",
    "raise",
    "record",
    "repeat",
    "resourcestring",
    "set",
    "shl",
    "shr",
    "string",
    "then",
    "threadvar",
    "to",
    "try",
    "type",
    "unit",
    "until",
    "uses",
    "var",
    "while",
    "with",
    "xor",
    "abstract",
    "cdecl",
    "deprecated",
    "dynamic",
    "external",
    "forward",
    "overload",
    "override",
    "private",
    "protected",
    "public",
    "published",
    "reintroduce",
    "safecall",
    "sealed",
    "static",
    "stdcall",
    "strict",
    "virtual",
    "operator",
    "self",
    "result",
    "true",
    "false",
];

pub fn is_keyword(word: &[char]) -> bool {
    if word.len() > 14 || !word.iter().all(char::is_ascii_alphabetic) {
        return false;
    }
    let lower: String = word.iter().map(|c| c.to_ascii_lowercase()).collect();
    KEYWORDS.contains(&lower.as_str())
}

pub fn lex_line(line: &[char], mut state: LexState, out: &mut Vec<Token>) -> LexState {
    let mut e = Emitter { out };
    let n = line.len();
    let mut i = 0;
    while i < n {
        match state {
            LexState::BraceComment | LexState::BraceDirective => {
                let kind = if state == LexState::BraceComment {
                    TokenKind::Comment
                } else {
                    TokenKind::Directive
                };
                match line[i..].iter().position(|c| *c == '}') {
                    Some(p) => {
                        e.push(i, i + p + 1, kind);
                        i += p + 1;
                        state = LexState::Normal;
                    }
                    None => {
                        e.push(i, n, kind);
                        return state;
                    }
                }
                continue;
            }
            LexState::ParenComment | LexState::ParenDirective => {
                let kind = if state == LexState::ParenComment {
                    TokenKind::Comment
                } else {
                    TokenKind::Directive
                };
                match find_seq(line, i, &['*', ')']) {
                    Some(p) => {
                        e.push(i, p + 2, kind);
                        i = p + 2;
                        state = LexState::Normal;
                    }
                    None => {
                        e.push(i, n, kind);
                        return state;
                    }
                }
                continue;
            }
            LexState::MultiLineString => {
                // fecha numa linha que comeca (apos espacos) com '''
                let lead = line.iter().take_while(|c| c.is_whitespace()).count();
                if i == 0 && line[lead..].starts_with(&['\'', '\'', '\'']) {
                    e.push(0, lead + 3, TokenKind::String);
                    i = lead + 3;
                    state = LexState::Normal;
                } else {
                    e.push(i, n, TokenKind::String);
                    return state;
                }
                continue;
            }
            LexState::Normal | LexState::DfmBinary => {}
        }
        let c = line[i];
        let rest = &line[i..];
        if rest.starts_with(&['/', '/']) {
            e.push(i, n, TokenKind::Comment);
            return LexState::Normal;
        }
        if c == '{' {
            state = if rest.get(1) == Some(&'$') {
                LexState::BraceDirective
            } else {
                LexState::BraceComment
            };
            continue;
        }
        if rest.starts_with(&['(', '*']) {
            state = if rest.get(2) == Some(&'$') {
                LexState::ParenDirective
            } else {
                LexState::ParenComment
            };
            // consome o "(*" antes de procurar o fechamento ("(*)" nao fecha)
            let kind = if state == LexState::ParenComment {
                TokenKind::Comment
            } else {
                TokenKind::Directive
            };
            e.push(i, i + 2, kind);
            i += 2;
            continue;
        }
        if rest.starts_with(&['\'', '\'', '\'']) && rest[3..].iter().all(|c| c.is_whitespace()) {
            e.push(i, n, TokenKind::String);
            return LexState::MultiLineString;
        }
        if c == '\'' || (c == '#' && rest.get(1).is_some_and(|d| d.is_ascii_digit() || *d == '$')) {
            let end = string_end(line, i);
            e.push(i, end, TokenKind::String);
            i = end;
            continue;
        }
        if c.is_ascii_digit() || (c == '$' && rest.get(1).is_some_and(char::is_ascii_hexdigit)) {
            let end = number_end(line, i);
            e.push(i, end, TokenKind::Number);
            i = end;
            continue;
        }
        if is_ident_start(c) || (c == '&' && rest.get(1).is_some_and(|d| is_ident_start(*d))) {
            let start = i;
            i += 1;
            while i < n && is_ident_char(line[i]) {
                i += 1;
            }
            let kind = if c != '&' && is_keyword(&line[start..i]) {
                TokenKind::Keyword
            } else {
                TokenKind::Identifier
            };
            e.push(start, i, kind);
            continue;
        }
        let kind = if c.is_whitespace() {
            TokenKind::Text
        } else {
            TokenKind::Symbol
        };
        e.push(i, i + 1, kind);
        i += 1;
    }
    state
}

/// Sequencia de partes de string: 'abc''d'#13#$0A'x'
fn string_end(line: &[char], mut i: usize) -> usize {
    let n = line.len();
    while i < n {
        if line[i] == '\'' {
            i += 1;
            loop {
                if i >= n {
                    return n;
                }
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
        } else if line[i] == '#' {
            let mut j = i + 1;
            if line.get(j) == Some(&'$') {
                j += 1;
                while j < n && line[j].is_ascii_hexdigit() {
                    j += 1;
                }
            } else {
                while j < n && line[j].is_ascii_digit() {
                    j += 1;
                }
            }
            if j == i + 1 {
                return i;
            }
            i = j;
        } else {
            break;
        }
    }
    i
}

fn number_end(line: &[char], mut i: usize) -> usize {
    let n = line.len();
    if line[i] == '$' {
        i += 1;
        while i < n && line[i].is_ascii_hexdigit() {
            i += 1;
        }
        return i;
    }
    while i < n && (line[i].is_ascii_digit() || line[i] == '_') {
        i += 1;
    }
    if i + 1 < n && line[i] == '.' && line[i + 1].is_ascii_digit() {
        i += 1;
        while i < n && line[i].is_ascii_digit() {
            i += 1;
        }
    }
    if i < n && (line[i] == 'e' || line[i] == 'E') {
        let mut j = i + 1;
        if j < n && (line[j] == '+' || line[j] == '-') {
            j += 1;
        }
        if j < n && line[j].is_ascii_digit() {
            i = j;
            while i < n && line[i].is_ascii_digit() {
                i += 1;
            }
        }
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str, state: LexState) -> (Vec<(String, TokenKind)>, LexState) {
        let chars: Vec<char> = src.chars().collect();
        let mut out = Vec::new();
        let end = lex_line(&chars, state, &mut out);
        let mut pos = 0;
        for t in &out {
            assert_eq!(
                t.start, pos,
                "tokens devem cobrir a linha sem buracos: {out:?}"
            );
            pos = t.end;
        }
        assert_eq!(pos, chars.len());
        let list = out
            .iter()
            .filter(|t| t.kind != TokenKind::Text)
            .map(|t| (chars[t.start..t.end].iter().collect(), t.kind))
            .collect();
        (list, end)
    }

    #[test]
    fn keywords_are_case_insensitive_and_names_are_not_keywords() {
        let (t, _) = kinds("  BEGIN Result := Self.Name; End;", LexState::Normal);
        assert_eq!(t[0], ("BEGIN".into(), TokenKind::Keyword));
        assert_eq!(t[1], ("Result".into(), TokenKind::Keyword));
        assert!(t.contains(&("Name".into(), TokenKind::Identifier)));
        assert!(t.contains(&("End".into(), TokenKind::Keyword)));
    }

    #[test]
    fn strings_with_quotes_and_char_codes() {
        let (t, _) = kinds("s := 'it''s' + #13#$0A'x'; // fim", LexState::Normal);
        assert!(t.contains(&("'it''s'".into(), TokenKind::String)));
        assert!(t.contains(&("#13#$0A'x'".into(), TokenKind::String)));
        assert_eq!(t.last().unwrap(), &("// fim".into(), TokenKind::Comment));
    }

    #[test]
    fn comments_and_directives_span_lines() {
        let (t, st) = kinds("x := 1; { comeca", LexState::Normal);
        assert_eq!(st, LexState::BraceComment);
        assert_eq!(t.last().unwrap().1, TokenKind::Comment);
        let (t, st) = kinds("termina } y", LexState::BraceComment);
        assert_eq!((t[0].1, st), (TokenKind::Comment, LexState::Normal));
        let (t, _) = kinds("{$IFDEF DEBUG} (*$R+*) (* c *)", LexState::Normal);
        assert_eq!(t[0], ("{$IFDEF DEBUG}".into(), TokenKind::Directive));
        assert_eq!(t[1], ("(*$R+*)".into(), TokenKind::Directive));
        assert_eq!(t[2], ("(* c *)".into(), TokenKind::Comment));
        let (_, st) = kinds("(* abre", LexState::Normal);
        assert_eq!(st, LexState::ParenComment);
    }

    #[test]
    fn numbers_and_delphi12_multiline_strings() {
        let (t, _) = kinds("a := $FF + 1.5e3 + 10;", LexState::Normal);
        assert!(t.contains(&("$FF".into(), TokenKind::Number)));
        assert!(t.contains(&("1.5e3".into(), TokenKind::Number)));
        let (_, st) = kinds("s := '''", LexState::Normal);
        assert_eq!(st, LexState::MultiLineString);
        let (t, st) = kinds("  ''';", LexState::MultiLineString);
        assert_eq!((t[0].1, st), (TokenKind::String, LexState::Normal));
    }
}
