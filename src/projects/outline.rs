//! Estrutura de uma unit (painel Structure e navegacao declaracao <-> implementacao).
//!
//! Usa o lexer da Fase 2, entao comentarios, strings e diretivas nunca confundem a analise.
//! Nao e um compilador: e um leitor tolerante que pula o que nao entende.

use crate::syntax::{Language, LexState, TokenKind, lex_line};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SymbolKind {
    Unit,
    Uses,
    UsedUnit,
    Class,
    Record,
    Interface,
    Field,
    Method,
    Property,
    Routine,
    Group,
    Object,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    pub kind: SymbolKind,
    pub name: String,
    pub detail: String,
    /// Linha e coluna (base 0) do nome.
    pub line: usize,
    pub col: usize,
    pub children: Vec<Symbol>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsedUnit {
    pub name: String,
    pub line: usize,
    pub col: usize,
    pub end_col: usize,
}

/// Declaracao de metodo dentro de uma classe (`tform1.botaoclick`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MethodDecl {
    pub key: String,
    pub line: usize,
    pub col: usize,
}

/// Rotina com corpo na implementacao.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Routine {
    pub key: Option<String>,
    pub name: String,
    pub line: usize,
    pub col: usize,
    pub end_line: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Outline {
    pub symbols: Vec<Symbol>,
    pub used_units: Vec<UsedUnit>,
    pub decls: Vec<MethodDecl>,
    pub routines: Vec<Routine>,
}

#[derive(Clone, Debug)]
struct T {
    low: String,
    text: String,
    line: usize,
    col: usize,
    kind: TokenKind,
}

fn tokens(text: &str) -> Vec<T> {
    let mut out = Vec::new();
    let mut state = LexState::Normal;
    let mut toks = Vec::new();
    for (line_no, line) in text.split('\n').enumerate() {
        let chars: Vec<char> = line.trim_end_matches('\r').chars().collect();
        state = lex_line(Language::Pascal, &chars, state, &mut toks);
        for t in &toks {
            if matches!(
                t.kind,
                TokenKind::Keyword | TokenKind::Identifier | TokenKind::Symbol
            ) {
                // simbolos sao de um caractere; o lexer junta vizinhos do mesmo tipo
                if t.kind == TokenKind::Symbol {
                    for (k, c) in chars[t.start..t.end].iter().enumerate() {
                        if !c.is_whitespace() {
                            out.push(T {
                                low: c.to_string(),
                                text: c.to_string(),
                                line: line_no,
                                col: t.start + k,
                                kind: t.kind,
                            });
                        }
                    }
                } else {
                    let text: String = chars[t.start..t.end].iter().collect();
                    out.push(T {
                        low: text.to_lowercase(),
                        text,
                        line: line_no,
                        col: t.start,
                        kind: t.kind,
                    });
                }
            }
        }
    }
    out
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
    "dispid",
    "final",
    "forward",
    "external",
    "assembler",
    "export",
    "varargs",
    "winapi",
    "unsafe",
    "library",
];
const DECL_SECTIONS: &[&str] = &[
    "var",
    "const",
    "type",
    "label",
    "resourcestring",
    "threadvar",
];

struct Parser {
    t: Vec<T>,
    i: usize,
    interface: bool,
    out: Outline,
}

impl Parser {
    fn low(&self, i: usize) -> &str {
        self.t.get(i).map_or("", |t| t.low.as_str())
    }

    fn is_ident(&self, i: usize) -> bool {
        self.t.get(i).is_some_and(|t| {
            t.kind == TokenKind::Identifier
                || (t.kind == TokenKind::Keyword
                    && matches!(t.low.as_str(), "result" | "self" | "string"))
        })
    }

    /// Nome com pontos (`Vcl.Forms`, `TForm1.Botao`): (texto, proximo indice).
    fn dotted(&self, mut i: usize) -> (String, usize) {
        let mut name = self.t[i].text.clone();
        i += 1;
        while self.low(i) == "."
            && self
                .t
                .get(i + 1)
                .is_some_and(|t| t.kind != TokenKind::Symbol)
        {
            name.push('.');
            name.push_str(&self.t[i + 1].text);
            i += 2;
        }
        (name, i)
    }

    /// O token abre um bloco terminado por `end`?
    fn opens_block(&self, i: usize) -> bool {
        match self.low(i) {
            "begin" | "try" | "case" | "asm" | "record" => true,
            "class" | "object" | "interface" | "dispinterface" => {
                let next = self.low(i + 1);
                // `class of`, `class;`, `class procedure`, `class var`, `of object` nao abrem bloco
                self.low(i.wrapping_sub(1)) != "of"
                    && !matches!(
                        next,
                        "of" | ";" | "var" | "property" | "threadvar" | "const"
                    )
                    && !ROUTINE.contains(&next)
            }
            _ => false,
        }
    }

    /// Pula ate o `;` no nivel 0 de parenteses/colchetes; blocos `record ... end` sao pulados inteiros.
    fn skip_to_semicolon(&mut self) {
        let mut depth = 0i32;
        while self.i < self.t.len() {
            match self.low(self.i) {
                "(" | "[" => depth += 1,
                ")" | "]" => depth -= 1,
                ";" if depth <= 0 => {
                    self.i += 1;
                    return;
                }
                "end" if depth <= 0 => return,
                "record" | "class" | "object" | "interface" | "dispinterface"
                    if depth <= 0 && self.opens_block(self.i) =>
                {
                    self.skip_block();
                    continue;
                }
                _ => {}
            }
            self.i += 1;
        }
    }

    /// Esta num abridor de bloco; avanca ate depois do `end` correspondente.
    /// `case` dentro de `record` (parte variante) nao tem `end` proprio.
    fn skip_block(&mut self) {
        let mut stack: Vec<String> = Vec::new();
        while self.i < self.t.len() {
            let low = self.low(self.i).to_owned();
            if low == "end" {
                stack.pop();
                self.i += 1;
                if stack.is_empty() {
                    return;
                }
                continue;
            }
            let variant_case = low == "case" && stack.last().is_some_and(|s| s == "record");
            if self.opens_block(self.i) && !variant_case {
                stack.push(low);
            }
            self.i += 1;
        }
    }

    fn skip_directives(&mut self) -> bool {
        let mut forward = false;
        while DIRECTIVES.contains(&self.low(self.i)) {
            forward |= matches!(self.low(self.i), "forward" | "external");
            self.skip_to_semicolon();
        }
        forward
    }

    fn run(&mut self) {
        let mut impl_routines = Vec::new();
        while self.i < self.t.len() {
            let low = self.low(self.i).to_owned();
            match low.as_str() {
                "unit" | "program" | "library" | "package"
                    if self.out.symbols.is_empty() && self.is_ident(self.i + 1) =>
                {
                    let (name, next) = self.dotted(self.i + 1);
                    let t = &self.t[self.i + 1];
                    self.out
                        .symbols
                        .push(sym(SymbolKind::Unit, name, &low, t.line, t.col));
                    self.i = next;
                }
                "interface" if self.low(self.i.wrapping_sub(1)) != "=" => {
                    self.interface = true;
                    self.i += 1;
                }
                "implementation" => {
                    self.interface = false;
                    self.i += 1;
                }
                "uses" | "contains" => {
                    let label = if self.interface {
                        "Uses"
                    } else {
                        "Uses (implementation)"
                    };
                    let line = self.t[self.i].line;
                    self.i += 1;
                    let mut group = sym(SymbolKind::Uses, label.to_owned(), "", line, 0);
                    while self.i < self.t.len() && self.low(self.i) != ";" {
                        if self.is_ident(self.i) && self.low(self.i) != "in" {
                            let t = self.t[self.i].clone();
                            let (name, next) = self.dotted(self.i);
                            let end_col =
                                self.t[next - 1].col + self.t[next - 1].text.chars().count();
                            self.out.used_units.push(UsedUnit {
                                name: name.clone(),
                                line: t.line,
                                col: t.col,
                                end_col,
                            });
                            group
                                .children
                                .push(sym(SymbolKind::UsedUnit, name, "", t.line, t.col));
                            self.i = next;
                        } else {
                            self.i += 1;
                        }
                    }
                    self.out.symbols.push(group);
                }
                "type" => {
                    self.i += 1;
                    let types = self.type_section("");
                    self.out.symbols.extend(types);
                }
                "class" if ROUTINE.contains(&self.low(self.i + 1)) => self.i += 1,
                r if ROUTINE.contains(&r) => {
                    if let Some(s) = self.routine() {
                        impl_routines.push(s);
                    }
                }
                _ => self.i += 1,
            }
        }
        if !impl_routines.is_empty() {
            let mut group = sym(
                SymbolKind::Group,
                "Implementation".to_owned(),
                "",
                impl_routines[0].line,
                0,
            );
            group.children = impl_routines;
            self.out.symbols.push(group);
        }
    }

    /// Declaracoes `Nome = ...` de uma secao type. `prefix` e o caminho da classe externa.
    fn type_section(&mut self, prefix: &str) -> Vec<Symbol> {
        let mut found = Vec::new();
        loop {
            if !self.is_ident(self.i) {
                break;
            }
            let mut j = self.i + 1;
            if self.low(j) == "<" {
                while j < self.t.len() && self.low(j) != ">" {
                    j += 1;
                }
                j += 1;
            }
            if self.low(j) != "=" {
                break;
            }
            let name_tok = self.t[self.i].clone();
            j += 1;
            while matches!(self.low(j), "type" | "packed") {
                j += 1;
            }
            let kind = match self.low(j) {
                "class" | "object" => SymbolKind::Class,
                "record" => SymbolKind::Record,
                "interface" | "dispinterface" => SymbolKind::Interface,
                _ => {
                    self.i = j;
                    self.skip_to_semicolon();
                    continue;
                }
            };
            let mut k = j + 1;
            if self.low(k) == ";" || self.low(k) == "of" {
                self.i = k;
                self.skip_to_semicolon();
                continue; // declaracao antecipada ou `class of`
            }
            if self.low(k) == "helper" {
                k += 1;
                while k < self.t.len() && self.low(k) != "for" {
                    k += 1;
                }
                k += 2;
            }
            let mut detail = String::new();
            if self.low(k) == "(" {
                k += 1;
                while k < self.t.len() && self.low(k) != ")" {
                    detail.push_str(&self.t[k].text);
                    k += 1;
                }
                k += 1;
            }
            let path = if prefix.is_empty() {
                name_tok.text.clone()
            } else {
                format!("{prefix}.{}", name_tok.text)
            };
            let mut s = sym(
                kind,
                name_tok.text.clone(),
                &detail,
                name_tok.line,
                name_tok.col,
            );
            self.i = k;
            if self.low(k) == ";" {
                self.i += 1; // TX = class(TY);
            } else {
                self.members(&mut s, &path);
                if self.low(self.i) == ";" {
                    self.i += 1;
                }
            }
            found.push(s);
        }
        found
    }

    fn members(&mut self, owner: &mut Symbol, path: &str) {
        let mut visibility = String::new();
        while self.i < self.t.len() {
            let low = self.low(self.i).to_owned();
            match low.as_str() {
                "end" => {
                    self.i += 1;
                    return;
                }
                "private" | "protected" | "public" | "published" | "automated" => {
                    visibility = low;
                    self.i += 1;
                }
                "strict" | "class" | "var" | "const" | "threadvar" => self.i += 1,
                "case" => {
                    while self.i < self.t.len() && self.low(self.i) != "of" {
                        self.i += 1;
                    }
                    self.i += 1;
                }
                "type" => {
                    self.i += 1;
                    let nested = self.type_section(path);
                    owner.children.extend(nested);
                }
                r if ROUTINE.contains(&r) => {
                    self.i += 1;
                    if !self.is_ident(self.i) {
                        continue;
                    }
                    let t = self.t[self.i].clone();
                    let (name, next) = self.dotted(self.i);
                    self.i = next;
                    self.skip_to_semicolon();
                    self.skip_directives();
                    self.out.decls.push(MethodDecl {
                        key: format!("{path}.{name}").to_lowercase(),
                        line: t.line,
                        col: t.col,
                    });
                    owner.children.push(sym(
                        SymbolKind::Method,
                        name,
                        &format!("{low} {visibility}"),
                        t.line,
                        t.col,
                    ));
                }
                "property" => {
                    self.i += 1;
                    if !self.is_ident(self.i) {
                        continue;
                    }
                    let t = self.t[self.i].clone();
                    let mut ty = String::new();
                    self.i += 1;
                    let mut depth = 0;
                    while self.i < self.t.len() {
                        match self.low(self.i) {
                            "[" => depth += 1,
                            "]" => depth -= 1,
                            ":" if depth == 0 && ty.is_empty() => {
                                if let Some(n) = self.t.get(self.i + 1) {
                                    ty = n.text.clone();
                                }
                            }
                            ";" if depth == 0 => break,
                            _ => {}
                        }
                        self.i += 1;
                    }
                    self.i += 1;
                    if self.low(self.i) == "default" && self.low(self.i + 1) == ";" {
                        self.i += 2;
                    }
                    owner
                        .children
                        .push(sym(SymbolKind::Property, t.text, &ty, t.line, t.col));
                }
                _ if self.is_ident(self.i) => {
                    let mut names = Vec::new();
                    while self.is_ident(self.i) {
                        names.push(self.t[self.i].clone());
                        self.i += 1;
                        if self.low(self.i) == "," {
                            self.i += 1;
                        } else {
                            break;
                        }
                    }
                    if self.low(self.i) != ":" {
                        continue;
                    }
                    let ty = self
                        .t
                        .get(self.i + 1)
                        .map(|t| t.text.clone())
                        .unwrap_or_default();
                    self.skip_to_semicolon();
                    for n in names {
                        owner
                            .children
                            .push(sym(SymbolKind::Field, n.text, &ty, n.line, n.col));
                    }
                }
                _ => self.i += 1,
            }
        }
    }

    /// Cabecalho de rotina fora de classe. Na implementacao tambem pula o corpo.
    fn routine(&mut self) -> Option<Symbol> {
        let kw = self.t[self.i].clone();
        self.i += 1;
        if !self.is_ident(self.i) {
            return None; // metodo anonimo
        }
        let name_tok = self.t[self.i].clone();
        let (name, next) = self.dotted(self.i);
        self.i = next;
        self.skip_to_semicolon();
        let forward = self.skip_directives();
        if self.interface || forward {
            return None;
        }
        self.skip_body();
        let end_line = self
            .t
            .get(self.i.saturating_sub(1))
            .map_or(kw.line, |t| t.line);
        let key = name.contains('.').then(|| name.to_lowercase());
        self.out.routines.push(Routine {
            key,
            name: name.clone(),
            line: name_tok.line,
            col: name_tok.col,
            end_line,
        });
        Some(sym(
            SymbolKind::Routine,
            name,
            &kw.low,
            name_tok.line,
            name_tok.col,
        ))
    }

    /// Secoes locais, rotinas aninhadas e o bloco begin..end de uma rotina.
    fn skip_body(&mut self) {
        while self.i < self.t.len() {
            let low = self.low(self.i).to_owned();
            match low.as_str() {
                "begin" | "asm" => {
                    self.skip_block();
                    if self.low(self.i) == ";" {
                        self.i += 1;
                    }
                    return;
                }
                l if ROUTINE.contains(&l) => {
                    self.i += 1;
                    if self.is_ident(self.i) {
                        self.i = self.dotted(self.i).1;
                    }
                    self.skip_to_semicolon();
                    if !self.skip_directives() {
                        self.skip_body();
                    }
                }
                l if DECL_SECTIONS.contains(&l) => {
                    self.i += 1;
                    while self.i < self.t.len() {
                        let l = self.low(self.i);
                        if matches!(l, "begin" | "asm")
                            || ROUTINE.contains(&l)
                            || DECL_SECTIONS.contains(&l)
                        {
                            break;
                        }
                        self.skip_to_semicolon();
                    }
                }
                "implementation" | "initialization" | "finalization" => return,
                _ => self.i += 1,
            }
        }
    }
}

fn sym(kind: SymbolKind, name: String, detail: &str, line: usize, col: usize) -> Symbol {
    Symbol {
        kind,
        name,
        detail: detail.trim().to_owned(),
        line,
        col,
        children: Vec::new(),
    }
}

/// Outline de um fonte Pascal (.pas, .dpr, .dpk, .inc).
pub fn pascal(text: &str) -> Outline {
    let mut p = Parser {
        t: tokens(text),
        i: 0,
        interface: false,
        out: Outline::default(),
    };
    p.run();
    p.out
}

/// Arvore de componentes de um .dfm textual (`object Nome: TClasse ... end`).
pub fn dfm(text: &str) -> Outline {
    let mut stack: Vec<Option<Symbol>> = Vec::new();
    let mut roots = Vec::new();
    for (line_no, raw) in text.split('\n').enumerate() {
        let line = raw.trim_end_matches('\r');
        let t = line.trim_start();
        let col = line.chars().count() - t.chars().count();
        let low = t.to_ascii_lowercase();
        let header = ["object ", "inherited ", "inline "]
            .iter()
            .find_map(|k| low.starts_with(k).then(|| &t[k.len()..]));
        if let Some(rest) = header {
            let rest = rest.split('[').next().unwrap_or_default().trim();
            let (name, class) = match rest.split_once(':') {
                Some((n, c)) => (n.trim().to_owned(), c.trim().to_owned()),
                None => (rest.to_owned(), String::new()),
            };
            let name_col =
                col + t.len() - t.trim_start_matches(|c: char| c.is_alphabetic()).len() + 1;
            stack.push(Some(sym(
                SymbolKind::Object,
                name,
                &class,
                line_no,
                name_col,
            )));
        } else if low == "item" {
            stack.push(None);
        } else if low == "end" || low == "end>" {
            if let Some(Some(done)) = stack.pop() {
                match stack.iter_mut().rev().find_map(|s| s.as_mut()) {
                    Some(parent) => parent.children.push(done),
                    None => roots.push(done),
                }
            }
        }
    }
    // arquivo truncado: o que ficou aberto ainda aparece
    while let Some(open) = stack.pop() {
        if let Some(done) = open {
            match stack.iter_mut().rev().find_map(|s| s.as_mut()) {
                Some(parent) => parent.children.push(done),
                None => roots.push(done),
            }
        }
    }
    Outline {
        symbols: roots,
        ..Outline::default()
    }
}

impl Outline {
    /// Ctrl+Shift+Up/Down: da implementacao para a declaracao na classe, e vice-versa.
    pub fn decl_impl_target(&self, line: usize) -> Option<(usize, usize)> {
        if let Some(r) = self
            .routines
            .iter()
            .find(|r| r.line <= line && line <= r.end_line)
        {
            let key = r.key.as_ref()?;
            let nth = self
                .routines
                .iter()
                .take_while(|x| !std::ptr::eq(*x, r))
                .filter(|x| x.key.as_ref() == Some(key))
                .count();
            let decls: Vec<_> = self.decls.iter().filter(|d| &d.key == key).collect();
            return decls.get(nth).or(decls.first()).map(|d| (d.line, d.col));
        }
        let d = self.decls.iter().find(|d| d.line == line)?;
        let nth = self
            .decls
            .iter()
            .take_while(|x| !std::ptr::eq(*x, d))
            .filter(|x| x.key == d.key)
            .count();
        let impls: Vec<_> = self
            .routines
            .iter()
            .filter(|r| r.key.as_deref() == Some(d.key.as_str()))
            .collect();
        impls.get(nth).or(impls.first()).map(|r| (r.line, r.col))
    }

    /// Nome de unit da clausula uses sob o cursor.
    pub fn used_unit_at(&self, line: usize, col: usize) -> Option<&str> {
        self.used_units
            .iter()
            .find(|u| u.line == line && u.col <= col && col <= u.end_col)
            .map(|u| u.name.as_str())
    }

    /// Declaracao de tipo/metodo/rotina com este nome (sem diferenciar maiusculas).
    pub fn find_symbol(&self, name: &str) -> Option<(usize, usize)> {
        fn walk(list: &[Symbol], name: &str) -> Option<(usize, usize)> {
            for s in list {
                let named = s.name.eq_ignore_ascii_case(name)
                    || s.name
                        .rsplit('.')
                        .next()
                        .is_some_and(|n| n.eq_ignore_ascii_case(name));
                if named
                    && !matches!(
                        s.kind,
                        SymbolKind::Uses | SymbolKind::UsedUnit | SymbolKind::Group
                    )
                {
                    return Some((s.line, s.col));
                }
                if let Some(found) = walk(&s.children, name) {
                    return Some(found);
                }
            }
            None
        }
        walk(&self.symbols, name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UNIT: &str = "unit Unit_Teste;

interface

uses
  Vcl.Forms, System.Classes; // { begin end }

type
  TModo = (mNovo, mEditar);
  TForm1 = class;
  TPonto = record
    X, Y: Integer;
    function Soma: Integer;
  end;

  TForm1 = class(TForm)
    Botao: TButton;
    procedure BotaoClick(Sender: TObject);
  private
    FModo: TModo;
    function Calcula(A: Integer): Integer; overload; virtual;
    function Calcula(const S: string): Integer; overload;
  public
    property Modo: TModo read FModo write FModo;
    property Itens[I: Integer]: string read GetItem; default;
  end;

implementation

uses
  System.SysUtils;

function TPonto.Soma: Integer;
begin
  Result := X + Y;
end;

procedure TForm1.BotaoClick(Sender: TObject);
  procedure Interna;
  var r: record a: Integer; end;
  begin
    try ShowMessage('end'); finally end;
  end;
begin
  case FModo of mNovo: Interna; end;
  TThread.Queue(nil, procedure begin Caption := 'x'; end);
end;

function TForm1.Calcula(A: Integer): Integer;
begin
  Result := A;
end;

function TForm1.Calcula(const S: string): Integer;
begin
  Result := Length(S);
end;

procedure Avulsa; forward;

procedure Avulsa;
begin
end;

initialization
  RegisterClass(TForm1);
end.
";

    fn names(list: &[Symbol]) -> Vec<String> {
        list.iter().map(|s| s.name.clone()).collect()
    }

    #[test]
    fn unit_structure_is_recognized() {
        let o = pascal(UNIT);
        assert_eq!(
            names(&o.symbols),
            [
                "Unit_Teste",
                "Uses",
                "TPonto",
                "TForm1",
                "Uses (implementation)",
                "Implementation"
            ]
        );
        assert_eq!(
            names(&o.symbols[1].children),
            ["Vcl.Forms", "System.Classes"]
        );
        let form = &o.symbols[3];
        assert_eq!(
            (form.kind, form.detail.as_str()),
            (SymbolKind::Class, "TForm")
        );
        assert_eq!(
            names(&form.children),
            [
                "Botao",
                "BotaoClick",
                "FModo",
                "Calcula",
                "Calcula",
                "Modo",
                "Itens"
            ]
        );
        assert_eq!(form.children[1].kind, SymbolKind::Method);
        assert_eq!(form.children[5].kind, SymbolKind::Property);
        assert_eq!(names(&o.symbols[2].children), ["X", "Y", "Soma"]);
        let impls = &o.symbols[5].children;
        assert_eq!(
            names(impls),
            [
                "TPonto.Soma",
                "TForm1.BotaoClick",
                "TForm1.Calcula",
                "TForm1.Calcula",
                "Avulsa"
            ]
        );
    }

    #[test]
    fn declaration_and_implementation_navigation() {
        let o = pascal(UNIT);
        let line_of = |needle: &str| UNIT.lines().position(|l| l.contains(needle)).unwrap();
        // da implementacao (dentro do corpo, inclusive rotina aninhada) para a declaracao
        let decl = o.decl_impl_target(line_of("try ShowMessage")).unwrap();
        assert_eq!(decl.0, line_of("procedure BotaoClick"));
        // e de volta
        assert_eq!(
            o.decl_impl_target(decl.0).unwrap().0,
            line_of("procedure TForm1.BotaoClick")
        );
        // sobrecargas na mesma ordem
        assert_eq!(
            o.decl_impl_target(line_of("Result := Length")).unwrap().0,
            line_of("Calcula(const S: string): Integer; overload;")
        );
        assert_eq!(o.used_unit_at(line_of("Vcl.Forms"), 4), Some("Vcl.Forms"));
        assert_eq!(
            o.find_symbol("tponto").map(|p| p.0),
            Some(line_of("TPonto = record"))
        );
    }

    #[test]
    fn dfm_object_tree_with_collections() {
        let src = "object Form1: TForm1\n  Caption = 'x'\n  object Grid: TDBGrid\n    Columns = <\n      item\n        Expanded = False\n      end\n      item\n      end>\n  end\n  inherited Painel: TPanel\n    object Botao: TButton\n    end\n  end\nend\n";
        let o = dfm(src);
        assert_eq!(names(&o.symbols), ["Form1"]);
        assert_eq!(names(&o.symbols[0].children), ["Grid", "Painel"]);
        assert_eq!(o.symbols[0].children[1].children[0].detail, "TButton");
        assert_eq!(o.symbols[0].children[0].line, 2);
    }
}
