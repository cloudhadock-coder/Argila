//! Autocompletar (Ctrl+Space e depois de `.`) e Class Completion (Shift+Ctrl+C).
//!
//! As sugestoes vem de provedores independentes (`CompletionProvider`): palavras-chave e tipos
//! do System, declaracoes locais da rotina, membros da classe, simbolos da unit, das units do
//! `uses` (pelo indice do projeto) e palavras do proprio arquivo. Um provedor futuro pode usar
//! um compilador ou servidor de linguagem sem mudar o editor.

pub mod class_completion;
pub mod context;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::navigation::index::{FileSymbols, SymbolIndex, base_type};
use crate::projects::outline::{DeclKind, Outline, Symbol, SymbolKind};
pub use context::{Context, Local, LocalKind, Scope};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ItemKind {
    Keyword,
    Unit,
    Class,
    Record,
    Interface,
    Type,
    Procedure,
    Function,
    Constructor,
    Property,
    Field,
    Variable,
    Parameter,
    Constant,
    EnumValue,
    Word,
}

impl ItemKind {
    pub fn label(self) -> &'static str {
        match self {
            ItemKind::Keyword => "keyword",
            ItemKind::Unit => "unit",
            ItemKind::Class => "class",
            ItemKind::Record => "record",
            ItemKind::Interface => "interface",
            ItemKind::Type => "type",
            ItemKind::Procedure => "procedure",
            ItemKind::Function => "function",
            ItemKind::Constructor => "constructor",
            ItemKind::Property => "property",
            ItemKind::Field => "field",
            ItemKind::Variable => "var",
            ItemKind::Parameter => "param",
            ItemKind::Constant => "const",
            ItemKind::EnumValue => "enum",
            ItemKind::Word => "word",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompletionItem {
    pub label: String,
    pub kind: ItemKind,
    /// Assinatura ou tipo (`(Sender: TObject)`, `: string`).
    pub detail: String,
    /// De onde veio (unit, classe ou "local").
    pub source: String,
    /// Menor = mais relevante (locais antes de membros, membros antes da unit...).
    pub priority: u8,
}

/// Lista pronta para o editor: substitui de `start_col` ate o cursor.
#[derive(Clone, Debug, Default)]
pub struct Completion {
    pub context: Context,
    pub items: Vec<CompletionItem>,
}

/// O que os provedores consultam alem do texto.
pub struct World<'a> {
    pub outline: &'a Outline,
    pub index: Option<&'a SymbolIndex>,
    /// Units do uses do arquivo atual (preferidas ao procurar tipos).
    pub uses: Vec<String>,
    pub unit: String,
}

/// Tipo de uma expressao `A.B.C`: classe/record, unit ou enumeracao.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Type(String),
    Unit(String),
    Enum(String),
}

pub struct Member<'a> {
    pub symbol: &'a Symbol,
    pub owner: String,
}

impl<'a> World<'a> {
    pub fn new(outline: &'a Outline, index: Option<&'a SymbolIndex>, unit: &str) -> Self {
        Self {
            outline,
            index,
            uses: outline.used_units.iter().map(|u| u.name.clone()).collect(),
            unit: unit.to_owned(),
        }
    }

    /// Classe/record/interface: primeiro nesta unit, depois nas do uses, depois no projeto.
    pub fn find_type(&self, name: &str) -> Option<&'a Symbol> {
        let base = base_type(name);
        if base.is_empty() {
            return None;
        }
        self.outline.find_type(base).or_else(|| {
            self.index
                .and_then(|i| i.find_type(base, &self.uses))
                .map(|(_, s)| s)
        })
    }

    /// Membros com os herdados de ancestrais conhecidos (no projeto). O mais especifico vem antes.
    pub fn members(&self, type_name: &str) -> Vec<Member<'a>> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        let mut current = self.find_type(type_name);
        while let Some(t) = current {
            if !seen.insert(t.name.to_lowercase()) || seen.len() > 16 {
                break;
            }
            for c in &t.children {
                out.push(Member {
                    symbol: c,
                    owner: t.name.clone(),
                });
            }
            let ancestor = t.detail.split(',').next().unwrap_or("").trim();
            current = if ancestor.is_empty() {
                None
            } else {
                self.find_type(ancestor)
            };
        }
        out
    }

    /// Classe/record com o arquivo onde foi declarada (None = esta unit).
    fn find_type_file(&self, name: &str) -> Option<(&'a Symbol, Option<&'a Path>)> {
        let base = base_type(name);
        if base.is_empty() {
            return None;
        }
        if let Some(s) = self.outline.find_type(base) {
            return Some((s, None));
        }
        self.index
            .and_then(|i| i.find_type(base, &self.uses))
            .map(|(f, s)| (s, Some(f.path.as_path())))
    }

    /// Declaracao de um membro, subindo pelos ancestrais (pode estar em outra unit).
    fn member_location(
        &self,
        type_name: &str,
        name: &str,
    ) -> Option<(Option<PathBuf>, usize, usize)> {
        let mut seen = HashSet::new();
        let mut current = self.find_type_file(type_name);
        while let Some((t, file)) = current {
            if !seen.insert(t.name.to_lowercase()) || seen.len() > 16 {
                break;
            }
            if let Some(m) = t
                .children
                .iter()
                .find(|c| c.name.eq_ignore_ascii_case(name))
            {
                return Some((file.map(Path::to_path_buf), m.line, m.col));
            }
            let ancestor = t.detail.split(',').next().unwrap_or("").trim();
            current = if ancestor.is_empty() {
                None
            } else {
                self.find_type_file(ancestor)
            };
        }
        None
    }

    /// Find Declaration: onde `name` (depois de `chain`, se houver) foi declarado.
    /// Arquivo None = a unit atual. Locais, membros (com heranca), a unit e o projeto.
    pub fn locate(
        &self,
        scope: &Scope,
        chain: &[String],
        name: &str,
    ) -> Option<(Option<PathBuf>, usize, usize)> {
        if chain.is_empty() {
            if let Some(l) = scope.local(name) {
                return Some((None, l.line, l.col));
            }
            if let Some(class) = &scope.self_class {
                if let Some(found) = self.member_location(class, name) {
                    return Some(found);
                }
            }
            if let Some((l, c)) = self.outline.find_symbol(name) {
                return Some((None, l, c));
            }
            let loc = self.index?.find_declaration(name, &self.uses)?;
            return Some((Some(loc.path), loc.line, loc.col));
        }
        match self.resolve(scope, chain)? {
            Target::Type(t) => self.member_location(&t, name),
            Target::Unit(u) => {
                let f = self.unit_file(&u)?;
                let o = &f.outline;
                o.find_type(name)
                    .map(|s| (s.line, s.col))
                    .or_else(|| {
                        o.declarations
                            .iter()
                            .find(|d| d.interface && d.name.eq_ignore_ascii_case(name))
                            .map(|d| (d.line, d.col))
                    })
                    .map(|(l, c)| (Some(f.path.clone()), l, c))
            }
            Target::Enum(_) => {
                let find = |o: &Outline| {
                    o.declarations
                        .iter()
                        .find(|d| {
                            d.kind == DeclKind::EnumValue && d.name.eq_ignore_ascii_case(name)
                        })
                        .map(|d| (d.line, d.col))
                };
                if let Some((l, c)) = find(self.outline) {
                    return Some((None, l, c));
                }
                self.index?
                    .files
                    .iter()
                    .find_map(|f| find(&f.outline).map(|(l, c)| (Some(f.path.clone()), l, c)))
            }
        }
    }

    pub fn member(&self, type_name: &str, name: &str) -> Option<Member<'a>> {
        self.members(type_name)
            .into_iter()
            .find(|m| m.symbol.name.eq_ignore_ascii_case(name))
    }

    fn unit_file(&self, name: &str) -> Option<&'a FileSymbols> {
        self.index?.unit(name).map(|f| f.as_ref())
    }

    fn is_enum(&self, name: &str) -> bool {
        let is = |o: &Outline| {
            o.declarations.iter().any(|d| {
                d.kind == DeclKind::Type
                    && d.name.eq_ignore_ascii_case(name)
                    && d.ty.starts_with('(')
            })
        };
        is(self.outline)
            || self
                .index
                .is_some_and(|i| i.files.iter().any(|f| is(&f.outline)))
    }

    /// Declaracao global (var/const) visivel: desta unit ou da interface de outra.
    fn global(&self, name: &str) -> Option<String> {
        let find = |o: &Outline, own: bool| {
            o.declarations
                .iter()
                .find(|d| {
                    (own || d.interface)
                        && matches!(d.kind, DeclKind::Variable | DeclKind::Constant)
                        && d.name.eq_ignore_ascii_case(name)
                })
                .map(|d| d.ty.clone())
        };
        find(self.outline, true).or_else(|| {
            let index = self.index?;
            self.uses
                .iter()
                .filter_map(|u| index.unit(u))
                .find_map(|f| find(&f.outline, false))
        })
    }

    /// Resolve `A.B.C` (o que vem antes do ponto).
    pub fn resolve(&self, scope: &Scope, chain: &[String]) -> Option<Target> {
        let (first, rest) = chain.split_first()?;
        let mut target = self.resolve_first(scope, first, chain)?;
        let skip = match &target {
            Target::Unit(u) => u.split('.').count().saturating_sub(1),
            _ => 0,
        };
        for seg in rest.iter().skip(skip) {
            target = match target {
                Target::Type(t) => {
                    let m = self.member(&t, seg)?;
                    match m.symbol.kind {
                        SymbolKind::Class | SymbolKind::Record | SymbolKind::Interface => {
                            Target::Type(m.symbol.name.clone())
                        }
                        _ if m.symbol.ty.is_empty() => return None,
                        _ => Target::Type(m.symbol.ty.clone()),
                    }
                }
                Target::Unit(u) => {
                    let f = self.unit_file(&u)?;
                    if f.outline.find_type(seg).is_some() {
                        Target::Type(seg.clone())
                    } else {
                        let d = f
                            .outline
                            .declarations
                            .iter()
                            .find(|d| d.interface && d.name.eq_ignore_ascii_case(seg))?;
                        match d.kind {
                            DeclKind::Type if d.ty.starts_with('(') => Target::Enum(d.name.clone()),
                            DeclKind::Type => Target::Type(d.name.clone()),
                            _ => Target::Type(d.ty.clone()),
                        }
                    }
                }
                Target::Enum(_) => return None,
            };
        }
        Some(target)
    }

    fn resolve_first(&self, scope: &Scope, first: &str, chain: &[String]) -> Option<Target> {
        if first.eq_ignore_ascii_case("self") {
            return scope.self_class.clone().map(Target::Type);
        }
        if let Some(l) = scope.local(first) {
            return (!l.ty.is_empty()).then(|| Target::Type(l.ty.clone()));
        }
        if let Some(class) = &scope.self_class {
            if let Some(m) = self.member(class, first) {
                if !m.symbol.ty.is_empty() {
                    return Some(Target::Type(m.symbol.ty.clone()));
                }
            }
        }
        if let Some(ty) = self.global(first) {
            return (!ty.is_empty()).then_some(Target::Type(ty));
        }
        if self.find_type(first).is_some() {
            return Some(Target::Type(first.to_owned()));
        }
        if self.is_enum(first) {
            return Some(Target::Enum(first.to_owned()));
        }
        // unit, inclusive com namespace (`Vcl.Forms.`)
        for k in (1..=chain.len()).rev() {
            let name = chain[..k].join(".");
            if self.unit_file(&name).is_some() {
                return Some(Target::Unit(name));
            }
        }
        None
    }
}

fn method_kind(s: &Symbol) -> ItemKind {
    match s.detail.split_whitespace().next().unwrap_or_default() {
        "function" | "operator" => ItemKind::Function,
        "constructor" | "destructor" => ItemKind::Constructor,
        _ if !s.ty.is_empty() => ItemKind::Function,
        _ => ItemKind::Procedure,
    }
}

fn symbol_item(s: &Symbol, source: &str, priority: u8) -> Option<CompletionItem> {
    let (kind, detail) = match s.kind {
        SymbolKind::Class => (ItemKind::Class, paren(&s.detail)),
        SymbolKind::Record => (ItemKind::Record, String::new()),
        SymbolKind::Interface => (ItemKind::Interface, paren(&s.detail)),
        SymbolKind::Method | SymbolKind::Routine => {
            let name = s.name.rsplit('.').next().unwrap_or(&s.name);
            if name.len() != s.name.len() {
                return None; // implementacao de metodo: a declaracao ja aparece pela classe
            }
            (method_kind(s), signature(&s.params, &s.ty))
        }
        SymbolKind::Property => (ItemKind::Property, typed(&s.ty)),
        SymbolKind::Field => (ItemKind::Field, typed(&s.ty)),
        _ => return None,
    };
    Some(CompletionItem {
        label: s.name.clone(),
        kind,
        detail,
        source: source.to_owned(),
        priority,
    })
}

fn paren(s: &str) -> String {
    if s.is_empty() {
        String::new()
    } else {
        format!("({s})")
    }
}

fn typed(ty: &str) -> String {
    if ty.is_empty() {
        String::new()
    } else {
        format!(": {ty}")
    }
}

fn signature(params: &str, ty: &str) -> String {
    format!("{params}{}", typed(ty))
}

fn decl_item(
    d: &crate::projects::outline::Declaration,
    source: &str,
    priority: u8,
) -> CompletionItem {
    let (kind, detail) = match d.kind {
        DeclKind::Type => (ItemKind::Type, String::new()),
        DeclKind::Variable => (ItemKind::Variable, typed(&d.ty)),
        DeclKind::Constant => (ItemKind::Constant, typed(&d.ty)),
        DeclKind::EnumValue => (ItemKind::EnumValue, typed(&d.ty)),
        DeclKind::Routine => (
            if d.ty.is_empty() {
                ItemKind::Procedure
            } else {
                ItemKind::Function
            },
            signature(&d.params, &d.ty),
        ),
    };
    CompletionItem {
        label: d.name.clone(),
        kind,
        detail,
        source: source.to_owned(),
        priority,
    }
}

/// Fonte de sugestoes. Recebe o contexto ja analisado e acrescenta itens em `out`.
pub trait CompletionProvider: Send + Sync {
    fn name(&self) -> &'static str;
    fn complete(&self, cx: &Context, world: &World, out: &mut Vec<CompletionItem>);
}

/// Palavras reservadas e identificadores do System (tipos e rotinas embutidas).
pub struct KeywordProvider;

const SYSTEM_TYPES: &[&str] = &[
    "Integer",
    "Cardinal",
    "ShortInt",
    "SmallInt",
    "LongInt",
    "Int64",
    "UInt64",
    "Byte",
    "Word",
    "LongWord",
    "NativeInt",
    "NativeUInt",
    "Boolean",
    "Char",
    "AnsiChar",
    "WideChar",
    "AnsiString",
    "WideString",
    "UnicodeString",
    "RawByteString",
    "Single",
    "Double",
    "Extended",
    "Real",
    "Currency",
    "TDateTime",
    "TDate",
    "TTime",
    "Variant",
    "OleVariant",
    "Pointer",
    "PChar",
    "TObject",
    "TClass",
    "IInterface",
    "IUnknown",
    "TInterfacedObject",
    "TArray",
    "TBytes",
];
const SYSTEM_ROUTINES: &[&str] = &[
    "Abs",
    "Assert",
    "Assigned",
    "Break",
    "Chr",
    "Concat",
    "Continue",
    "Copy",
    "Dec",
    "Delete",
    "Dispose",
    "Exclude",
    "Exit",
    "FillChar",
    "FreeMem",
    "GetMem",
    "Halt",
    "High",
    "Inc",
    "Include",
    "Insert",
    "Length",
    "Low",
    "Move",
    "New",
    "Odd",
    "Ord",
    "Pos",
    "Pred",
    "Round",
    "SetLength",
    "SizeOf",
    "Sqr",
    "Sqrt",
    "Succ",
    "Trunc",
];

impl CompletionProvider for KeywordProvider {
    fn name(&self) -> &'static str {
        "keywords"
    }

    fn complete(&self, cx: &Context, _: &World, out: &mut Vec<CompletionItem>) {
        if !cx.qualifier.is_empty() || cx.in_uses {
            return;
        }
        for k in crate::syntax::pascal::KEYWORDS {
            out.push(CompletionItem {
                label: (*k).to_owned(),
                kind: ItemKind::Keyword,
                detail: String::new(),
                source: String::new(),
                priority: 5,
            });
        }
        for t in SYSTEM_TYPES {
            out.push(CompletionItem {
                label: (*t).to_owned(),
                kind: ItemKind::Type,
                detail: String::new(),
                source: "System".to_owned(),
                priority: 4,
            });
        }
        for r in SYSTEM_ROUTINES {
            out.push(CompletionItem {
                label: (*r).to_owned(),
                kind: ItemKind::Procedure,
                detail: String::new(),
                source: "System".to_owned(),
                priority: 4,
            });
        }
    }
}

/// Parametros, variaveis locais, Result e membros da classe do metodo atual.
pub struct ScopeProvider;

impl CompletionProvider for ScopeProvider {
    fn name(&self) -> &'static str {
        "scope"
    }

    fn complete(&self, cx: &Context, world: &World, out: &mut Vec<CompletionItem>) {
        if !cx.qualifier.is_empty() || cx.in_uses {
            return;
        }
        for l in &cx.scope.locals {
            let kind = match l.kind {
                LocalKind::Parameter => ItemKind::Parameter,
                LocalKind::Variable | LocalKind::Result => ItemKind::Variable,
                LocalKind::Constant => ItemKind::Constant,
                LocalKind::Routine => ItemKind::Procedure,
            };
            out.push(CompletionItem {
                label: l.name.clone(),
                kind,
                detail: typed(&l.ty),
                source: "local".to_owned(),
                priority: 0,
            });
        }
        if let Some(class) = &cx.scope.self_class {
            out.push(CompletionItem {
                label: "Self".to_owned(),
                kind: ItemKind::Variable,
                detail: typed(class),
                source: "local".to_owned(),
                priority: 0,
            });
            for m in world.members(class) {
                if let Some(item) = symbol_item(m.symbol, &m.owner, 1) {
                    out.push(item);
                }
            }
        }
    }
}

/// Simbolos da propria unit e das units do uses (so o que esta na interface delas).
pub struct UnitProvider;

impl CompletionProvider for UnitProvider {
    fn name(&self) -> &'static str {
        "units"
    }

    fn complete(&self, cx: &Context, world: &World, out: &mut Vec<CompletionItem>) {
        if !cx.qualifier.is_empty() {
            return;
        }
        if cx.in_uses {
            let mut names: Vec<String> = world
                .index
                .map(|i| i.files.iter().map(|f| f.unit.clone()).collect())
                .unwrap_or_default();
            names.sort_by_key(|n| n.to_lowercase());
            for n in names {
                if !n.eq_ignore_ascii_case(&world.unit) {
                    out.push(CompletionItem {
                        label: n,
                        kind: ItemKind::Unit,
                        detail: String::new(),
                        source: "project".to_owned(),
                        priority: 2,
                    });
                }
            }
            return;
        }
        add_outline(world.outline, &world.unit, true, 2, out);
        if let Some(index) = world.index {
            for u in &world.uses {
                if let Some(f) = index.unit(u) {
                    out.push(CompletionItem {
                        label: f.unit.clone(),
                        kind: ItemKind::Unit,
                        detail: String::new(),
                        source: "uses".to_owned(),
                        priority: 3,
                    });
                    add_outline(&f.outline, &f.unit, false, 3, out);
                }
            }
        }
    }
}

/// Tipos, rotinas e declaracoes de uma unit (`own`: inclusive o que esta so na implementacao).
fn add_outline(o: &Outline, unit: &str, own: bool, priority: u8, out: &mut Vec<CompletionItem>) {
    let interface_end = o.implementation_line.unwrap_or(usize::MAX);
    for s in &o.symbols {
        match s.kind {
            SymbolKind::Class | SymbolKind::Record | SymbolKind::Interface
                if own || s.line < interface_end =>
            {
                out.extend(symbol_item(s, unit, priority));
            }
            SymbolKind::Group if own => {
                for r in &s.children {
                    out.extend(symbol_item(r, unit, priority));
                }
            }
            _ => {}
        }
    }
    for d in &o.declarations {
        if own || d.interface {
            out.push(decl_item(d, unit, priority));
        }
    }
}

/// Depois de `.`: membros da classe/record, declaracoes de uma unit ou valores de enumeracao.
pub struct MemberProvider;

impl CompletionProvider for MemberProvider {
    fn name(&self) -> &'static str {
        "members"
    }

    fn complete(&self, cx: &Context, world: &World, out: &mut Vec<CompletionItem>) {
        if cx.qualifier.is_empty() || cx.in_uses {
            return;
        }
        match world.resolve(&cx.scope, &cx.qualifier) {
            Some(Target::Type(t)) => {
                for m in world.members(&t) {
                    out.extend(symbol_item(m.symbol, &m.owner, 1));
                }
            }
            Some(Target::Unit(u)) => {
                if let Some(f) = world.unit_file(&u) {
                    add_outline(&f.outline, &f.unit, false, 1, out);
                }
            }
            Some(Target::Enum(e)) => {
                let mut add = |o: &Outline, unit: &str| {
                    for d in &o.declarations {
                        if d.kind == DeclKind::EnumValue && d.ty.eq_ignore_ascii_case(&e) {
                            out.push(decl_item(d, unit, 1));
                        }
                    }
                };
                add(world.outline, &world.unit);
                if let Some(index) = world.index {
                    for f in &index.files {
                        add(&f.outline, &f.unit);
                    }
                }
            }
            None => {}
        }
    }
}

/// Identificadores do proprio arquivo (o ultimo recurso quando nada mais conhece o nome).
pub struct WordProvider {
    pub words: Vec<String>,
}

impl WordProvider {
    pub fn from_text(text: &str) -> Self {
        let mut seen = HashSet::new();
        let mut words = Vec::new();
        for w in text.split(|c: char| !crate::syntax::is_ident_char(c)) {
            if w.chars().count() >= 3
                && w.chars()
                    .next()
                    .is_some_and(|c| c.is_alphabetic() || c == '_')
                && seen.insert(w.to_lowercase())
            {
                words.push(w.to_owned());
                if words.len() >= 20_000 {
                    break;
                }
            }
        }
        Self { words }
    }
}

impl CompletionProvider for WordProvider {
    fn name(&self) -> &'static str {
        "words"
    }

    fn complete(&self, cx: &Context, _: &World, out: &mut Vec<CompletionItem>) {
        if !cx.qualifier.is_empty() || cx.in_uses || cx.prefix.is_empty() {
            return;
        }
        let p = cx.prefix.to_lowercase();
        for w in &self.words {
            if w.to_lowercase().starts_with(&p) && !w.eq_ignore_ascii_case(&cx.prefix) {
                out.push(CompletionItem {
                    label: w.clone(),
                    kind: ItemKind::Word,
                    detail: String::new(),
                    source: "file".to_owned(),
                    priority: 6,
                });
            }
        }
    }
}

/// Junta os provedores, remove repetidos (fica o mais relevante) e ordena.
pub struct Engine {
    providers: Vec<Box<dyn CompletionProvider>>,
}

impl Default for Engine {
    fn default() -> Self {
        Self {
            providers: vec![
                Box::new(ScopeProvider),
                Box::new(MemberProvider),
                Box::new(UnitProvider),
                Box::new(KeywordProvider),
            ],
        }
    }
}

impl Engine {
    pub fn add(&mut self, provider: Box<dyn CompletionProvider>) {
        self.providers.push(provider);
    }

    pub fn provider_names(&self) -> Vec<&'static str> {
        self.providers.iter().map(|p| p.name()).collect()
    }

    /// `extra` entra so nesta chamada (ex.: palavras do arquivo, que dependem do texto atual).
    pub fn complete(
        &self,
        cx: Context,
        world: &World,
        extra: &[&dyn CompletionProvider],
    ) -> Completion {
        if cx.suppressed {
            return Completion {
                context: cx,
                items: Vec::new(),
            };
        }
        let mut items = Vec::new();
        for p in &self.providers {
            p.complete(&cx, world, &mut items);
        }
        for p in extra {
            p.complete(&cx, world, &mut items);
        }
        items.sort_by(|a, b| {
            a.priority
                .cmp(&b.priority)
                .then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase()))
        });
        let mut seen = HashSet::new();
        items.retain(|i| seen.insert(i.label.to_lowercase()));
        items.sort_by(|a, b| {
            a.label
                .to_lowercase()
                .cmp(&b.label.to_lowercase())
                .then(a.priority.cmp(&b.priority))
        });
        // relevancia primeiro, alfabetica dentro do mesmo nivel
        items.sort_by_key(|i| i.priority.min(4));
        Completion { context: cx, items }
    }
}

/// Filtro do popup enquanto o usuario digita: prefixo primeiro; sem prefixo, trecho.
pub fn filter(items: &[CompletionItem], typed: &str) -> Vec<usize> {
    let t = typed.to_lowercase();
    let prefixed: Vec<usize> = items
        .iter()
        .enumerate()
        .filter(|(_, i)| i.label.to_lowercase().starts_with(&t))
        .map(|(n, _)| n)
        .collect();
    if !prefixed.is_empty() || t.is_empty() {
        return prefixed;
    }
    items
        .iter()
        .enumerate()
        .filter(|(_, i)| i.label.to_lowercase().contains(&t))
        .map(|(n, _)| n)
        .collect()
}
