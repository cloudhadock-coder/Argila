//! O que esta em volta do cursor: prefixo digitado, expressao antes do ponto, clausula uses,
//! comentario/string e as declaracoes locais da rotina (parametros, var, const, Result, Self).

use crate::projects::outline::{Outline, T, push_tokens};
use crate::syntax::{Language, LexState, TokenKind, is_ident_char, lex_line};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Context {
    /// Cursor (linha e coluna em caracteres, base 0).
    pub line: usize,
    pub col: usize,
    /// Parte do identificador ja digitada e onde ela comeca.
    pub prefix: String,
    pub start_col: usize,
    /// `Self.Edit1.Te|` -> ["Self", "Edit1"]. Vazio sem ponto antes do prefixo.
    pub qualifier: Vec<String>,
    /// Dentro de `uses`/`contains`/`requires`.
    pub in_uses: bool,
    /// Dentro de comentario, string ou diretiva: nada a completar.
    pub suppressed: bool,
    pub scope: Scope,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalKind {
    Parameter,
    Variable,
    Constant,
    Result,
    Routine,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Local {
    pub name: String,
    pub ty: String,
    pub kind: LocalKind,
    pub line: usize,
    pub col: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scope {
    pub locals: Vec<Local>,
    /// Classe do metodo onde o cursor esta (`TForm1` em `procedure TForm1.Botao...`).
    pub self_class: Option<String>,
    pub routine: Option<String>,
}

impl Scope {
    pub fn local(&self, name: &str) -> Option<&Local> {
        // a ultima declaracao vence (var inline depois de um parametro de mesmo nome)
        self.locals
            .iter()
            .rev()
            .find(|l| l.name.eq_ignore_ascii_case(name))
    }
}

/// Linhas de contexto guardadas como tokens antes do cursor (o resto so passa pelo lexer).
const CONTEXT_LINES: usize = 300;

pub fn analyze(text: &str, outline: &Outline, line: usize, col: usize) -> Context {
    let routine = outline
        .routines
        .iter()
        .filter(|r| r.line <= line && line <= r.end_line)
        .max_by_key(|r| r.line);
    let keep_from = routine
        .map_or(line, |r| r.line.saturating_sub(2))
        .min(line.saturating_sub(CONTEXT_LINES));
    let mut before = Vec::new();
    let mut state = LexState::Normal;
    let mut toks = Vec::new();
    let mut caret_chars: Vec<char> = Vec::new();
    let mut suppressed = false;
    for (n, raw) in text.split('\n').enumerate().take(line + 1) {
        let chars: Vec<char> = raw.trim_end_matches('\r').chars().collect();
        if n < line {
            state = lex_line(Language::Pascal, &chars, state, &mut toks);
            if n >= keep_from {
                push_tokens(&chars, n, &toks, &mut before);
            }
            continue;
        }
        let upto = &chars[..col.min(chars.len())];
        let end_state = lex_line(Language::Pascal, upto, state, &mut toks);
        suppressed = end_state != LexState::Normal
            || toks.last().is_some_and(|t| {
                t.end == upto.len()
                    && matches!(
                        t.kind,
                        TokenKind::Comment | TokenKind::String | TokenKind::Directive
                    )
            });
        push_tokens(upto, n, &toks, &mut before);
        caret_chars = chars;
    }
    let col = col.min(caret_chars.len());
    let mut start = col;
    while start > 0 && is_ident_char(caret_chars[start - 1]) {
        start -= 1;
    }
    let prefix: String = caret_chars[start..col].iter().collect();
    let qualifier = qualifier_before(&caret_chars, start);
    let in_uses = before
        .iter()
        .rev()
        .take_while(|t| t.low != ";")
        .any(|t| matches!(t.low.as_str(), "uses" | "contains" | "requires"));
    let scope = match routine {
        Some(r) => scope_of(&before, r.line, r.col, (line, col)),
        None => Scope::default(),
    };
    Context {
        line,
        col,
        prefix,
        start_col: start,
        qualifier,
        in_uses,
        suppressed,
        scope,
    }
}

/// Identificadores separados por ponto antes de `start` (`Lista[0].Item(1).` vira
/// ["Lista", "Item"]: indices e chamadas valem pelo tipo do membro).
fn qualifier_before(chars: &[char], start: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = start;
    loop {
        let mut j = i;
        while j > 0 && chars[j - 1] == ' ' {
            j -= 1;
        }
        if j == 0 || chars[j - 1] != '.' {
            break;
        }
        j -= 1;
        while j > 0 && chars[j - 1] == ' ' {
            j -= 1;
        }
        // pula chamadas e indices balanceados
        while j > 0 && matches!(chars[j - 1], ')' | ']') {
            let (open, close) = if chars[j - 1] == ')' {
                ('(', ')')
            } else {
                ('[', ']')
            };
            let mut depth = 0;
            while j > 0 {
                j -= 1;
                if chars[j] == close {
                    depth += 1;
                } else if chars[j] == open {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
            }
        }
        let end = j;
        while j > 0 && is_ident_char(chars[j - 1]) {
            j -= 1;
        }
        if j == end {
            break;
        }
        out.push(chars[j..end].iter().collect());
        i = j;
    }
    out.reverse();
    out
}

fn low(t: &[T], i: usize) -> &str {
    t.get(i).map_or("", |x| x.low.as_str())
}

fn is_name(t: &[T], i: usize) -> bool {
    t.get(i).is_some_and(|x| {
        x.kind == TokenKind::Identifier
            || (x.kind == TokenKind::Keyword && matches!(x.low.as_str(), "string" | "result"))
    })
}

/// Tipo a partir de `i` (sem avancar) ate `;`, `)` de fechamento, `=` ou `:=`.
fn type_from(t: &[T], i: usize) -> String {
    let mut j = i;
    let mut depth = 0i32;
    while j < t.len() && j - i < 32 {
        match low(t, j) {
            "(" | "[" | "<" => depth += 1,
            ")" | "]" | ">" => {
                if depth == 0 {
                    break;
                }
                depth -= 1;
            }
            ";" | "=" if depth == 0 => break,
            ":" if depth == 0 && low(t, j + 1) == "=" => break,
            _ => {}
        }
        j += 1;
    }
    crate::projects::outline::join(&t[i..j])
}

/// Parametros, Result, Self e secoes locais da rotina cujo nome esta em (line, col).
fn scope_of(t: &[T], line: usize, col: usize, caret: (usize, usize)) -> Scope {
    let mut scope = Scope::default();
    let Some(mut i) = t.iter().position(|x| x.line == line && x.col == col) else {
        return scope;
    };
    let mut name = t[i].text.clone();
    i += 1;
    while low(t, i) == "." && is_name(t, i + 1) {
        name.push('.');
        name.push_str(&t[i + 1].text);
        i += 2;
    }
    if let Some((class, _)) = name.rsplit_once('.') {
        scope.self_class = Some(class.to_owned());
    }
    scope.routine = Some(name);
    let push = |scope: &mut Scope, x: &T, ty: String, kind: LocalKind| {
        scope.locals.push(Local {
            name: x.text.clone(),
            ty,
            kind,
            line: x.line,
            col: x.col,
        });
    };
    if low(t, i) == "(" {
        i += 1;
        let mut names: Vec<usize> = Vec::new();
        while i < t.len() && low(t, i) != ")" {
            match low(t, i) {
                "const" | "var" | "out" | "constref" => {}
                ":" => {
                    let ty = type_from(t, i + 1);
                    for &n in &names {
                        push(&mut scope, &t[n], ty.clone(), LocalKind::Parameter);
                    }
                    names.clear();
                    while i < t.len() && !matches!(low(t, i), ";" | ")") {
                        if low(t, i) == "(" {
                            break;
                        }
                        i += 1;
                    }
                    continue;
                }
                ";" => names.clear(),
                _ if is_name(t, i) => names.push(i),
                _ => {}
            }
            i += 1;
        }
        // parametros sem tipo (`var Buffer`)
        for &n in &names {
            push(&mut scope, &t[n], String::new(), LocalKind::Parameter);
        }
        i += 1;
    }
    if low(t, i) == ":" {
        let ty = type_from(t, i + 1);
        scope.locals.push(Local {
            name: "Result".to_owned(),
            ty,
            kind: LocalKind::Result,
            line,
            col,
        });
    }
    while i < t.len() && low(t, i) != ";" {
        i += 1;
    }
    // secoes locais e corpo
    let mut depth = 0i32;
    let mut mode: Option<LocalKind> = None;
    while i < t.len() {
        let x = &t[i];
        if (x.line, x.col) >= caret {
            break;
        }
        match x.low.as_str() {
            "var" | "threadvar" if depth == 0 => mode = Some(LocalKind::Variable),
            "const" if depth == 0 => mode = Some(LocalKind::Constant),
            "type" | "label" if depth == 0 => mode = None,
            "procedure" | "function" if depth == 0 && is_name(t, i + 1) => {
                push(&mut scope, &t[i + 1], String::new(), LocalKind::Routine);
                mode = None;
            }
            "begin" | "try" | "case" | "asm" | "record" => {
                depth += 1;
                if x.low == "begin" {
                    mode = None;
                }
            }
            "end" => depth -= 1,
            // var inline (Delphi 10.3+): `var X: T` ou `var X := TFoo.Create`
            "var" | "const" if depth > 0 && is_name(t, i + 1) => {
                let n = i + 1;
                let ty = if low(t, n + 1) == ":" && low(t, n + 2) != "=" {
                    type_from(t, n + 2)
                } else if low(t, n + 1) == ":"
                    && low(t, n + 2) == "="
                    && is_name(t, n + 3)
                    && low(t, n + 4) == "."
                    && low(t, n + 5) == "create"
                {
                    t[n + 3].text.clone()
                } else {
                    String::new()
                };
                let kind = if x.low == "var" {
                    LocalKind::Variable
                } else {
                    LocalKind::Constant
                };
                push(&mut scope, &t[n], ty, kind);
            }
            _ if depth == 0 && mode.is_some() && is_name(t, i) => {
                let kind = mode.expect("verificado");
                let mut names = vec![i];
                let mut j = i + 1;
                while low(t, j) == "," && is_name(t, j + 1) {
                    names.push(j + 1);
                    j += 2;
                }
                if matches!(low(t, j), ":" | "=") {
                    let ty = if low(t, j) == ":" {
                        type_from(t, j + 1)
                    } else {
                        String::new()
                    };
                    for n in names {
                        push(&mut scope, &t[n], ty.clone(), kind);
                    }
                    let mut d = 0i32;
                    while j < t.len() {
                        match low(t, j) {
                            "(" | "[" => d += 1,
                            ")" | "]" => d -= 1,
                            "record" => depth += 1,
                            "end" if depth > 0 => depth -= 1,
                            ";" if d <= 0 && depth == 0 => break,
                            _ => {}
                        }
                        j += 1;
                    }
                    i = j;
                }
            }
            _ => {}
        }
        i += 1;
    }
    scope
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qualifier_chain_skips_calls_and_indexes() {
        let chars: Vec<char> = "  Self.Lista[I].Itens(0).Nom".chars().collect();
        assert_eq!(
            qualifier_before(&chars, chars.len() - 3),
            ["Self", "Lista", "Itens"]
        );
        let chars: Vec<char> = "x := 3 + Foo".chars().collect();
        assert!(qualifier_before(&chars, 9).is_empty());
    }
}
