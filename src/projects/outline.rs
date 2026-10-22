//! Estrutura de uma unit (painel Structure, navegacao, autocompletar e Class Completion).
//!
//! Usa o lexer da Fase 2, entao comentarios, strings e diretivas nunca confundem a analise.
//! Nao e um compilador: e um leitor tolerante que pula o que nao entende.

use crate::syntax::folding::{self, FoldRange};
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
    /// Tipo do campo/propriedade ou retorno da funcao (vazio quando nao se aplica).
    pub ty: String,
    /// Parametros como escritos, `(Sender: TObject)` (vazio sem parametros).
    pub params: String,
    /// Linha e coluna (base 0) do nome.
    pub line: usize,
    pub col: usize,
    /// Ultima linha da declaracao (o `end` de classes e o fim do corpo de rotinas).
    pub end_line: usize,
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
    /// Classe com a grafia original e nome do metodo.
    pub class_path: String,
    pub name: String,
    pub params: String,
    pub ty: String,
    /// Do `class`/`procedure` ate depois do `;` do cabecalho (sem as diretivas).
    pub header: ((usize, usize), (usize, usize)),
    /// Precisa de corpo na implementacao (nao e abstract/external nem de interface).
    pub needs_body: bool,
}

/// Rotina com corpo na implementacao.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Routine {
    pub key: Option<String>,
    pub name: String,
    pub params: String,
    pub ty: String,
    pub line: usize,
    pub col: usize,
    pub end_line: usize,
    /// Do `class`/`procedure` ate depois do `;` do cabecalho.
    pub header: ((usize, usize), (usize, usize)),
}

/// Declaracao no nivel da unit fora de classes: tipos, variaveis, constantes, valores de
/// enumeracao e rotinas da interface. Alimenta o autocompletar e a busca de simbolos.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeclKind {
    Type,
    Variable,
    Constant,
    EnumValue,
    Routine,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Declaration {
    pub kind: DeclKind,
    pub name: String,
    pub ty: String,
    pub params: String,
    pub line: usize,
    pub col: usize,
    /// Declarada na secao interface (visivel para outras units).
    pub interface: bool,
}

/// Secao de visibilidade dentro de uma classe (`private`, `strict private`, `public`...).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClassSection {
    pub visibility: String,
    pub line: usize,
}

/// Onde fica cada parte de uma classe/record, para o Class Completion inserir codigo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClassInfo {
    /// Caminho com a grafia original (`TExterna.TInterna`).
    pub path: String,
    pub kind: SymbolKind,
    /// Linha do nome e linha/coluna do `end` que fecha a declaracao.
    pub line: usize,
    pub end_line: usize,
    pub end_col: usize,
    pub sections: Vec<ClassSection>,
}

/// `property` declarada numa classe (Class Completion completa `read`/`write`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PropertyDecl {
    pub class_path: String,
    pub name: String,
    pub ty: String,
    pub indexed: bool,
    pub has_read: bool,
    pub has_write: bool,
    pub line: usize,
    pub col: usize,
    /// Linha/coluna do `;` que fecha a declaracao.
    pub semi: (usize, usize),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Outline {
    pub symbols: Vec<Symbol>,
    pub used_units: Vec<UsedUnit>,
    pub decls: Vec<MethodDecl>,
    pub routines: Vec<Routine>,
    pub declarations: Vec<Declaration>,
    pub classes: Vec<ClassInfo>,
    pub properties: Vec<PropertyDecl>,
    /// Linha da palavra `implementation` e do que termina a secao (`initialization`,
    /// `finalization` ou o `end.` final).
    pub implementation_line: Option<usize>,
    pub implementation_end: Option<usize>,
    /// Regioes que o editor pode recolher.
    pub folds: Vec<FoldRange>,
}

/// Token significativo (palavra, simbolo, numero ou string) com posicao.
#[derive(Clone, Debug)]
pub(crate) struct T {
    pub low: String,
    pub text: String,
    pub line: usize,
    pub col: usize,
    pub kind: TokenKind,
}

pub(crate) fn tokens(text: &str) -> Vec<T> {
    let mut out = Vec::new();
    let mut state = LexState::Normal;
    let mut toks = Vec::new();
    for (line_no, line) in text.split('\n').enumerate() {
        let chars: Vec<char> = line.trim_end_matches('\r').chars().collect();
        state = lex_line(Language::Pascal, &chars, state, &mut toks);
        push_tokens(&chars, line_no, &toks, &mut out);
    }
    out
}

pub(crate) fn push_tokens(
    chars: &[char],
    line_no: usize,
    toks: &[crate::syntax::Token],
    out: &mut Vec<T>,
) {
    for t in toks {
        match t.kind {
            // simbolos sao de um caractere; o lexer junta vizinhos do mesmo tipo
            TokenKind::Symbol => {
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
            }
            TokenKind::Keyword | TokenKind::Identifier | TokenKind::Number | TokenKind::String => {
                let text: String = chars[t.start..t.end].iter().collect();
                out.push(T {
                    low: text.to_lowercase(),
                    text,
                    line: line_no,
                    col: t.start,
                    kind: t.kind,
                });
            }
            _ => {}
        }
    }
}

/// Texto legivel de uma sequencia de tokens: `(const S: string; A, B: Integer = 0)`.
pub(crate) fn join(toks: &[T]) -> String {
    let mut s = String::new();
    for (k, t) in toks.iter().enumerate() {
        if k > 0 {
            let p = &toks[k - 1];
            let tight_before = matches!(t.low.as_str(), "," | ";" | ":" | ")" | "]" | "." | ">")
                || (matches!(t.low.as_str(), "(" | "[" | "<") && p.kind != TokenKind::Symbol);
            let tight_after = matches!(p.low.as_str(), "(" | "[" | "." | "<" | "@" | "^");
            if !tight_before && !tight_after {
                s.push(' ');
            }
        }
        s.push_str(&t.text);
    }
    s
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

    fn pos(&self, i: usize) -> (usize, usize) {
        self.t.get(i).map_or((0, 0), |t| (t.line, t.col))
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

    /// Em `(`: texto dos parametros, avancando para depois do `)` correspondente.
    fn params(&mut self) -> String {
        if self.low(self.i) != "(" {
            return String::new();
        }
        let start = self.i;
        let mut depth = 0i32;
        while self.i < self.t.len() {
            match self.low(self.i) {
                "(" | "[" => depth += 1,
                ")" | "]" => {
                    depth -= 1;
                    if depth == 0 {
                        self.i += 1;
                        break;
                    }
                }
                _ => {}
            }
            self.i += 1;
        }
        join(&self.t[start..self.i])
    }

    /// Texto de um tipo que comeca em `i` (sem avancar): ate `;`, `=` ou um especificador.
    fn type_at(&self, i: usize) -> String {
        let start = i;
        let mut j = i;
        let mut depth = 0i32;
        while j < self.t.len() && j - start < 48 {
            match self.low(j) {
                "(" | "[" | "<" => depth += 1,
                ")" | "]" | ">" => {
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                }
                ";" | "=" if depth == 0 => break,
                "read" | "write" | "default" | "stored" | "index" | "implements" | "nodefault"
                | "absolute"
                    if depth == 0 && j > start =>
                {
                    break;
                }
                "end" | "record" | "class" | "object" | "interface" | "packed" if depth == 0 => {
                    if j == start {
                        return self.t[j].text.clone();
                    }
                    break;
                }
                _ => {}
            }
            j += 1;
        }
        join(&self.t[start..j])
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

    /// Pula diretivas; devolve (forward ou external, abstract).
    fn directives(&mut self) -> (bool, bool) {
        let (mut forward, mut abstract_) = (false, false);
        while DIRECTIVES.contains(&self.low(self.i)) {
            forward |= matches!(self.low(self.i), "forward" | "external");
            abstract_ |= self.low(self.i) == "abstract";
            self.skip_to_semicolon();
        }
        (forward, abstract_)
    }

    fn skip_directives(&mut self) -> bool {
        self.directives().0
    }

    /// Fim (exclusivo) do cabecalho cujo `;` acabou de ser consumido.
    fn header_end(&self, fallback: (usize, usize)) -> (usize, usize) {
        let k = self.i.wrapping_sub(1);
        if self.low(k) == ";" {
            let (l, c) = self.pos(k);
            (l, c + 1)
        } else {
            fallback
        }
    }

    fn run(&mut self) {
        let mut impl_routines = Vec::new();
        while self.i < self.t.len() {
            let low = self.low(self.i).to_owned();
            let line = self.pos(self.i).0;
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
                    self.out.implementation_line.get_or_insert(line);
                    self.i += 1;
                }
                "initialization" | "finalization" => {
                    self.out.implementation_end.get_or_insert(line);
                    self.i += 1;
                }
                "end" if self.low(self.i + 1) == "." => {
                    self.out.implementation_end.get_or_insert(line);
                    self.i += 1;
                }
                "uses" | "contains" => {
                    let label = if self.interface {
                        "Uses"
                    } else {
                        "Uses (implementation)"
                    };
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
                "var" | "threadvar" => {
                    self.i += 1;
                    self.decl_section(DeclKind::Variable);
                }
                "const" | "resourcestring" => {
                    self.i += 1;
                    self.decl_section(DeclKind::Constant);
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

    /// `A, B: Tipo;` ou `C = valor;` de secoes var/const no nivel da unit.
    fn decl_section(&mut self, kind: DeclKind) {
        while self.is_ident(self.i) {
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
            let ty = match self.low(self.i) {
                ":" => self.type_at(self.i + 1),
                "=" => String::new(),
                _ => return,
            };
            self.skip_to_semicolon();
            for n in names {
                self.out.declarations.push(Declaration {
                    kind,
                    name: n.text,
                    ty: ty.clone(),
                    params: String::new(),
                    line: n.line,
                    col: n.col,
                    interface: self.interface,
                });
            }
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
                    if prefix.is_empty() {
                        self.simple_type(&name_tok, j);
                    }
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
                let sections = self.members(&mut s, &path, kind);
                let end = self.i.wrapping_sub(1);
                if self.low(end) == "end" {
                    let (end_line, end_col) = self.pos(end);
                    s.end_line = end_line;
                    self.out.classes.push(ClassInfo {
                        path,
                        kind,
                        line: name_tok.line,
                        end_line,
                        end_col,
                        sections,
                    });
                }
                if self.low(self.i) == ";" {
                    self.i += 1;
                }
            }
            found.push(s);
        }
        found
    }

    /// Tipo que nao e classe/record/interface: alias, enumeracao, conjunto, procedural...
    fn simple_type(&mut self, name: &T, j: usize) {
        let ty = self.type_at(j);
        self.out.declarations.push(Declaration {
            kind: DeclKind::Type,
            name: name.text.clone(),
            ty,
            params: String::new(),
            line: name.line,
            col: name.col,
            interface: self.interface,
        });
        if self.low(j) != "(" {
            return;
        }
        let mut k = j + 1;
        while k < self.t.len() && self.low(k) != ")" && self.low(k) != ";" {
            if self.is_ident(k) && matches!(self.low(k + 1), "," | ")" | "=") {
                let v = &self.t[k];
                self.out.declarations.push(Declaration {
                    kind: DeclKind::EnumValue,
                    name: v.text.clone(),
                    ty: name.text.clone(),
                    params: String::new(),
                    line: v.line,
                    col: v.col,
                    interface: self.interface,
                });
            }
            k += 1;
        }
    }

    fn members(&mut self, owner: &mut Symbol, path: &str, kind: SymbolKind) -> Vec<ClassSection> {
        let mut visibility = String::new();
        let mut sections = Vec::new();
        while self.i < self.t.len() {
            let low = self.low(self.i).to_owned();
            match low.as_str() {
                "end" => {
                    self.i += 1;
                    return sections;
                }
                "private" | "protected" | "public" | "published" | "automated" => {
                    visibility = if self.low(self.i.wrapping_sub(1)) == "strict" {
                        format!("strict {low}")
                    } else {
                        low
                    };
                    sections.push(ClassSection {
                        visibility: visibility.clone(),
                        line: self.pos(self.i).0,
                    });
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
                    let kw = if self.low(self.i.wrapping_sub(1)) == "class" {
                        self.pos(self.i - 1)
                    } else {
                        self.pos(self.i)
                    };
                    self.i += 1;
                    if !self.is_ident(self.i) {
                        continue;
                    }
                    let t = self.t[self.i].clone();
                    let (name, next) = self.dotted(self.i);
                    self.i = next;
                    let params = self.params();
                    let ty = if self.low(self.i) == ":" {
                        self.type_at(self.i + 1)
                    } else {
                        String::new()
                    };
                    self.skip_to_semicolon();
                    let header = (kw, self.header_end((t.line, t.col)));
                    let (external, abstract_) = self.directives();
                    self.out.decls.push(MethodDecl {
                        key: format!("{path}.{name}").to_lowercase(),
                        line: t.line,
                        col: t.col,
                        class_path: path.to_owned(),
                        name: name.clone(),
                        params: params.clone(),
                        ty: ty.clone(),
                        header,
                        needs_body: kind != SymbolKind::Interface
                            && !abstract_
                            && !external
                            && !name.contains('.'),
                    });
                    let mut s = sym(
                        SymbolKind::Method,
                        name,
                        &format!("{low} {visibility}"),
                        t.line,
                        t.col,
                    );
                    s.params = params;
                    s.ty = ty;
                    owner.children.push(s);
                }
                "property" => {
                    self.i += 1;
                    if !self.is_ident(self.i) {
                        continue;
                    }
                    let t = self.t[self.i].clone();
                    let mut ty = String::new();
                    let (mut has_read, mut has_write, mut indexed) = (false, false, false);
                    self.i += 1;
                    let mut depth = 0;
                    while self.i < self.t.len() {
                        match self.low(self.i) {
                            "[" => {
                                depth += 1;
                                indexed |= ty.is_empty();
                            }
                            "]" => depth -= 1,
                            ":" if depth == 0 && ty.is_empty() => ty = self.type_at(self.i + 1),
                            "read" if depth == 0 => has_read = true,
                            "write" if depth == 0 => has_write = true,
                            ";" if depth == 0 => break,
                            "end" => break,
                            _ => {}
                        }
                        self.i += 1;
                    }
                    let semi = self.pos(self.i);
                    if self.low(self.i) == ";" {
                        self.i += 1;
                    }
                    if self.low(self.i) == "default" && self.low(self.i + 1) == ";" {
                        self.i += 2;
                    }
                    self.out.properties.push(PropertyDecl {
                        class_path: path.to_owned(),
                        name: t.text.clone(),
                        ty: ty.clone(),
                        indexed,
                        has_read,
                        has_write,
                        line: t.line,
                        col: t.col,
                        semi,
                    });
                    let mut s = sym(SymbolKind::Property, t.text, &ty, t.line, t.col);
                    s.ty = ty;
                    owner.children.push(s);
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
                    let ty = self.type_at(self.i + 1);
                    self.skip_to_semicolon();
                    for n in names {
                        let mut s = sym(SymbolKind::Field, n.text, &ty, n.line, n.col);
                        s.ty = ty.clone();
                        owner.children.push(s);
                    }
                }
                _ => self.i += 1,
            }
        }
        sections
    }

    /// Cabecalho de rotina fora de classe. Na implementacao tambem pula o corpo.
    fn routine(&mut self) -> Option<Symbol> {
        let kw = self.t[self.i].clone();
        let kw_pos = if self.low(self.i.wrapping_sub(1)) == "class" {
            self.pos(self.i - 1)
        } else {
            (kw.line, kw.col)
        };
        self.i += 1;
        if !self.is_ident(self.i) {
            return None; // metodo anonimo
        }
        let name_tok = self.t[self.i].clone();
        let (name, next) = self.dotted(self.i);
        self.i = next;
        let params = self.params();
        let ty = if self.low(self.i) == ":" {
            self.type_at(self.i + 1)
        } else {
            String::new()
        };
        self.skip_to_semicolon();
        let header = (kw_pos, self.header_end((name_tok.line, name_tok.col)));
        let forward = self.skip_directives();
        if self.interface {
            if !name.contains('.') {
                self.out.declarations.push(Declaration {
                    kind: DeclKind::Routine,
                    name,
                    ty,
                    params,
                    line: name_tok.line,
                    col: name_tok.col,
                    interface: true,
                });
            }
            return None;
        }
        if forward {
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
            params: params.clone(),
            ty: ty.clone(),
            line: name_tok.line,
            col: name_tok.col,
            end_line,
            header,
        });
        let mut s = sym(
            SymbolKind::Routine,
            name,
            &kw.low,
            name_tok.line,
            name_tok.col,
        );
        s.params = params;
        s.ty = ty;
        s.end_line = end_line;
        Some(s)
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
        ty: String::new(),
        params: String::new(),
        line,
        col,
        end_line: line,
        children: Vec::new(),
    }
}

/// Outline de um fonte Pascal (.pas, .dpr, .dpk, .inc), com as regioes de folding.
pub fn pascal(text: &str) -> Outline {
    let mut o = symbols_only(text);
    o.folds = folding::pascal(text);
    o
}

/// Outline sem as regioes de folding (indice do projeto, que nao precisa delas).
pub fn symbols_only(text: &str) -> Outline {
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
            if let Some(Some(mut done)) = stack.pop() {
                done.end_line = line_no;
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
        folds: folding::dfm(text),
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

    /// Declaracao de tipo/metodo/rotina/constante com este nome (sem diferenciar maiusculas).
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
        walk(&self.symbols, name).or_else(|| {
            self.declarations
                .iter()
                .find(|d| d.name.eq_ignore_ascii_case(name))
                .map(|d| (d.line, d.col))
        })
    }

    /// Nome da unit (ou programa) declarado no cabecalho.
    pub fn unit_name(&self) -> Option<&str> {
        self.symbols
            .first()
            .filter(|s| s.kind == SymbolKind::Unit)
            .map(|s| s.name.as_str())
    }

    /// Classe/record/interface declarada nesta unit (nome simples ou caminho `TA.TB`).
    pub fn find_type(&self, name: &str) -> Option<&Symbol> {
        fn walk<'a>(list: &'a [Symbol], name: &str) -> Option<&'a Symbol> {
            for s in list {
                if matches!(
                    s.kind,
                    SymbolKind::Class | SymbolKind::Record | SymbolKind::Interface
                ) {
                    if s.name.eq_ignore_ascii_case(name) {
                        return Some(s);
                    }
                    if let Some(found) = walk(&s.children, name) {
                        return Some(found);
                    }
                }
            }
            None
        }
        let last = name.rsplit('.').next().unwrap_or(name);
        walk(&self.symbols, last)
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
