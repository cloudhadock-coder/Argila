//! Regioes recolhiveis (code folding) de fontes Pascal e DFM.
//!
//! Calculadas pelo mesmo lexer do realce, entao `begin` dentro de string ou comentario nunca abre
//! bloco. A primeira linha de uma regiao continua visivel; as seguintes ate `end` sao escondidas.

use super::{Language, LexState, TokenKind, lex_line};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FoldKind {
    /// Corpo de procedure/function/metodo (com secoes var/const locais).
    Routine,
    /// Declaracao de classe, record ou interface.
    Type,
    /// `begin`/`try`/`case`/`asm`/`repeat` dentro do codigo.
    Block,
    /// `{$REGION}` ... `{$ENDREGION}`.
    Region,
    /// Comentario de varias linhas.
    Comment,
    /// Clausula `uses`/`contains`/`requires`.
    Uses,
    /// interface, implementation, initialization, finalization.
    Section,
    /// `object`/`item` de um DFM.
    Object,
    /// Valor de varias linhas de um DFM (lista de strings, dados binarios, colecao).
    Value,
}

/// Linhas base 0: `start` fica visivel, `start + 1 ..= end` somem quando recolhido.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoldRange {
    pub start: usize,
    pub end: usize,
    pub kind: FoldKind,
}

pub fn for_language(language: Language, text: &str) -> Vec<FoldRange> {
    match language {
        Language::Pascal => pascal(text),
        Language::Dfm => dfm(text),
        Language::PlainText => Vec::new(),
    }
}

struct Word {
    low: String,
    line: usize,
    symbol: bool,
}

const ROUTINE: &[&str] = &[
    "procedure",
    "function",
    "constructor",
    "destructor",
    "operator",
];
const DIRECTIVES: &[&str] = &[
    "virtual",
    "override",
    "abstract",
    "overload",
    "reintroduce",
    "dynamic",
    "message",
    "static",
    "inline",
    "cdecl",
    "stdcall",
    "safecall",
    "register",
    "pascal",
    "deprecated",
    "platform",
    "experimental",
    "final",
    "forward",
    "external",
    "assembler",
    "export",
    "varargs",
    "winapi",
    "unsafe",
];

/// Regioes de um fonte Pascal, ordenadas por linha inicial (a maior primeiro).
pub fn pascal(text: &str) -> Vec<FoldRange> {
    let mut out = Vec::new();
    let mut words: Vec<Word> = Vec::new();
    let mut regions: Vec<usize> = Vec::new();
    let mut comment_start: Option<usize> = None;
    let mut state = LexState::Normal;
    let mut toks = Vec::new();
    for (line_no, line) in text.split('\n').enumerate() {
        let chars: Vec<char> = line.trim_end_matches('\r').chars().collect();
        let began = state;
        state = lex_line(Language::Pascal, &chars, began, &mut toks);
        let in_comment = |s: LexState| matches!(s, LexState::BraceComment | LexState::ParenComment);
        if in_comment(state) && !in_comment(began) {
            comment_start = Some(line_no);
        }
        if in_comment(began) && !in_comment(state) {
            if let Some(start) = comment_start.take() {
                push(&mut out, start, line_no, FoldKind::Comment);
            }
        }
        for t in &toks {
            let text: String = chars[t.start..t.end].iter().collect();
            match t.kind {
                TokenKind::Directive => {
                    let low = text.to_ascii_lowercase();
                    let body = low.trim_start_matches(['{', '(', '*', '$']);
                    if body.starts_with("region") {
                        regions.push(line_no);
                    } else if body.starts_with("endregion") {
                        if let Some(start) = regions.pop() {
                            push(&mut out, start, line_no, FoldKind::Region);
                        }
                    }
                }
                TokenKind::Keyword | TokenKind::Identifier => words.push(Word {
                    low: text.to_lowercase(),
                    line: line_no,
                    symbol: false,
                }),
                TokenKind::Symbol => {
                    for c in text.chars().filter(|c| !c.is_whitespace()) {
                        words.push(Word {
                            low: c.to_string(),
                            line: line_no,
                            symbol: true,
                        });
                    }
                }
                _ => {}
            }
        }
    }
    blocks(&words, &mut out);
    out.sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));
    out.dedup();
    out
}

fn push(out: &mut Vec<FoldRange>, start: usize, end: usize, kind: FoldKind) {
    if end > start {
        out.push(FoldRange { start, end, kind });
    }
}

struct Frame {
    line: usize,
    word: &'static str,
    kind: FoldKind,
    record: bool,
}

fn frame(line: usize, word: &'static str, kind: FoldKind) -> Frame {
    Frame {
        line,
        word,
        kind,
        record: false,
    }
}

struct PendingRoutine {
    line: usize,
    depth: usize,
    body: bool,
}

fn blocks(w: &[Word], out: &mut Vec<FoldRange>) {
    let low = |i: usize| w.get(i).map_or("", |x| x.low.as_str());
    let mut stack: Vec<Frame> = Vec::new();
    let mut routines: Vec<PendingRoutine> = Vec::new();
    let mut section: Option<usize> = None;
    let mut in_interface = false;
    let mut uses: Option<usize> = None;
    let mut i = 0;
    while i < w.len() {
        let word = &w[i];
        let (cur, line) = (word.low.as_str(), word.line);
        let prev = if i > 0 { low(i - 1) } else { "" };
        if word.symbol {
            if cur == ";" {
                if let Some(start) = uses.take() {
                    push(out, start, line, FoldKind::Uses);
                }
            }
            i += 1;
            continue;
        }
        match cur {
            "uses" | "contains" | "requires" if stack.is_empty() => uses = Some(line),
            "interface" if stack.is_empty() && prev != "=" => {
                in_interface = true;
                section = Some(line);
            }
            "implementation" | "initialization" | "finalization" if stack.is_empty() => {
                if let Some(start) = section.replace(line) {
                    push(out, start, line.saturating_sub(1), FoldKind::Section);
                }
                in_interface = false;
                routines.clear();
            }
            "begin" | "asm" => {
                if let Some(r) = routines.last_mut() {
                    if r.depth == stack.len() && !r.body {
                        r.body = true;
                    }
                }
                stack.push(frame(line, "end", FoldKind::Block));
            }
            "try" => stack.push(frame(line, "end", FoldKind::Block)),
            "case" => {
                // parte variante de record nao tem `end` proprio
                if !stack.last().is_some_and(|f| f.record) {
                    stack.push(frame(line, "end", FoldKind::Block));
                }
            }
            "repeat" => stack.push(frame(line, "until", FoldKind::Block)),
            "until" => {
                if let Some(f) = stack.pop_if(|f| f.word == "until") {
                    push(out, f.line, line, f.kind);
                }
            }
            "record" => {
                let typed = prev == "=" || (prev == "packed" && i >= 2 && low(i - 2) == "=");
                let kind = if typed {
                    FoldKind::Type
                } else {
                    FoldKind::Block
                };
                stack.push(Frame {
                    record: true,
                    ..frame(line, "end", kind)
                });
            }
            "class" | "object" | "interface" | "dispinterface" => {
                let next = low(i + 1);
                let typed = prev == "=" || (prev == "packed" && i >= 2 && low(i - 2) == "=");
                let opens = typed
                    && !matches!(
                        next,
                        "of" | ";" | "var" | "property" | "threadvar" | "const"
                    )
                    && !ROUTINE.contains(&next);
                if opens {
                    stack.push(frame(line, "end", FoldKind::Type));
                }
            }
            "end" => {
                while stack.last().is_some_and(|f| f.word == "until") {
                    stack.pop();
                }
                if let Some(f) = stack.pop() {
                    push(out, f.line, line, f.kind);
                }
                let depth = stack.len();
                if let Some(r) = routines.pop_if(|r| r.body && r.depth == depth) {
                    push(out, r.line, line, FoldKind::Routine);
                }
                if stack.is_empty() && low(i + 1) == "." {
                    if let Some(start) = section.take() {
                        push(out, start, line.saturating_sub(1), FoldKind::Section);
                    }
                }
            }
            r if ROUTINE.contains(&r) => {
                let in_type = stack.iter().any(|f| f.kind == FoldKind::Type);
                let named = w
                    .get(i + 1)
                    .is_some_and(|x| !x.symbol && !matches!(x.low.as_str(), "begin" | "of"));
                let declared = !matches!(prev, "=" | ":" | "to" | "(" | "," | "of");
                if named && declared && !in_type && !in_interface {
                    if let Some(end) = header_end(w, i + 1) {
                        let (forward, after) = directives(w, end);
                        if !forward {
                            routines.push(PendingRoutine {
                                line,
                                depth: stack.len(),
                                body: false,
                            });
                        }
                        i = after;
                        continue;
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
}

/// Indice logo depois do `;` que fecha o cabecalho (nivel 0 de parenteses).
fn header_end(w: &[Word], mut i: usize) -> Option<usize> {
    let mut depth = 0i32;
    while i < w.len() {
        match w[i].low.as_str() {
            "(" | "[" => depth += 1,
            ")" | "]" => depth -= 1,
            ";" if depth <= 0 => return Some(i + 1),
            "begin" | "end" | "var" | "const" if depth <= 0 => return None,
            _ => {}
        }
        i += 1;
    }
    None
}

/// Pula diretivas (`overload; virtual;`); devolve se e `forward`/`external` e o proximo indice.
fn directives(w: &[Word], mut i: usize) -> (bool, usize) {
    let mut forward = false;
    while i < w.len() && DIRECTIVES.contains(&w[i].low.as_str()) {
        forward |= matches!(w[i].low.as_str(), "forward" | "external");
        while i < w.len() && w[i].low != ";" {
            i += 1;
        }
        i += 1;
    }
    (forward, i)
}

/// Regioes de um DFM textual: objetos, itens de colecao e valores de varias linhas.
pub fn dfm(text: &str) -> Vec<FoldRange> {
    #[derive(PartialEq)]
    enum Open {
        Object,
        Collection,
        Paren,
        Brace,
    }
    let mut out = Vec::new();
    let mut stack: Vec<(Open, usize)> = Vec::new();
    for (line_no, raw) in text.split('\n').enumerate() {
        let t = raw.trim_end_matches('\r').trim();
        let low = t.to_ascii_lowercase();
        match stack.last() {
            Some((Open::Paren, start)) => {
                if t.ends_with(')') {
                    push(&mut out, *start, line_no, FoldKind::Value);
                    stack.pop();
                }
                continue;
            }
            Some((Open::Brace, start)) => {
                if t.ends_with('}') {
                    push(&mut out, *start, line_no, FoldKind::Value);
                    stack.pop();
                }
                continue;
            }
            _ => {}
        }
        if ["object ", "inherited ", "inline "]
            .iter()
            .any(|k| low.starts_with(k))
            || low == "item"
        {
            stack.push((Open::Object, line_no));
        } else if low == "end" || low == "end>" {
            if let Some((Open::Object, start)) = stack.last() {
                push(&mut out, *start, line_no, FoldKind::Object);
                stack.pop();
            }
            if low == "end>" {
                if let Some((Open::Collection, start)) = stack.last() {
                    push(&mut out, *start, line_no, FoldKind::Value);
                    stack.pop();
                }
            }
        } else if t.ends_with("= <") {
            stack.push((Open::Collection, line_no));
        } else if t.ends_with("= (") {
            stack.push((Open::Paren, line_no));
        } else if t.ends_with("= {") {
            stack.push((Open::Brace, line_no));
        }
    }
    out.sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find(f: &[FoldRange], start: usize) -> Vec<(usize, FoldKind)> {
        f.iter()
            .filter(|r| r.start == start)
            .map(|r| (r.end, r.kind))
            .collect()
    }

    #[test]
    fn pascal_routines_types_blocks_regions_and_comments() {
        let src = "unit U;\n\
interface\n\
uses\n  A,\n  B;\n\
type\n  TX = class(TObject)\n    procedure P;\n    property Q: Integer read F;\n  end;\n\
  TR = record\n    case K: Integer of\n      0: (A: Integer);\n  end;\n\
implementation\n\
{$REGION 'Rotinas'}\n\
procedure Fwd; forward;\n\
procedure TX.P;\n\
var\n  S: string;\n\
  procedure Inner;\n  begin\n  end;\n\
begin\n  S := 'begin';\n  try\n    Inner;\n  finally\n  end;\n\
end;\n\
{$ENDREGION}\n\
{ comentario\n  longo }\n\
procedure Fwd;\nbegin\nend;\n\
end.\n";
        let lines: Vec<&str> = src.lines().collect();
        let at = |needle: &str| lines.iter().position(|l| l.contains(needle)).unwrap();
        let f = pascal(src);
        assert_eq!(find(&f, at("uses")), [(at("  B;"), FoldKind::Uses)]);
        assert_eq!(find(&f, at("TX = class")), [(at("  end;"), FoldKind::Type)]);
        assert_eq!(
            find(&f, at("TR = record")),
            [(at("0: (A") + 1, FoldKind::Type)]
        );
        assert_eq!(
            find(&f, at("procedure TX.P")),
            [(at("finally") + 2, FoldKind::Routine)]
        );
        assert_eq!(
            find(&f, at("procedure Inner")),
            [(at("procedure Inner") + 2, FoldKind::Routine)]
        );
        assert_eq!(
            find(&f, at("  try")),
            [(at("finally") + 1, FoldKind::Block)]
        );
        assert_eq!(
            find(&f, at("{$REGION")),
            [(at("{$ENDREGION"), FoldKind::Region)]
        );
        assert_eq!(
            find(&f, at("{ comentario")),
            [(at("longo }"), FoldKind::Comment)]
        );
        assert_eq!(find(&f, at("procedure Fwd;")), Vec::new());
        assert!(
            find(&f, at("interface"))
                .iter()
                .any(|r| r.1 == FoldKind::Section)
        );
        // a string 'begin' nao abriu bloco: o corpo de TX.P fecha no end certo
        assert!(f.iter().all(|r| r.end > r.start));
    }

    #[test]
    fn dfm_objects_collections_and_values() {
        let src = "object F: TF\n  Lines.Strings = (\n    'a'\n    'b)')\n  object G: TDBGrid\n    Columns = <\n      item\n        X = 1\n      end\n      item\n      end>\n    Data = {\n      0A0B}\n  end\nend\n";
        let f = dfm(src);
        assert_eq!(find(&f, 0), [(14, FoldKind::Object)]);
        assert_eq!(find(&f, 1), [(3, FoldKind::Value)]);
        assert_eq!(find(&f, 4), [(13, FoldKind::Object)]);
        assert_eq!(find(&f, 5), [(10, FoldKind::Value)]);
        assert_eq!(find(&f, 6), [(8, FoldKind::Object)]);
        assert_eq!(find(&f, 11), [(12, FoldKind::Value)]);
    }
}
