//! Indice de simbolos do projeto: a estrutura de cada unit, lida em paralelo e em segundo plano.
//!
//! Serve o Go to Symbol, o Find Declaration entre units e o autocompletar de membros. Arquivos
//! sem alteracao (mesmo tamanho e data) sao reaproveitados da indexacao anterior; arquivos
//! abertos no editor entram com o texto em memoria.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime};

use super::Location;
use crate::projects::outline::{self, DeclKind, Outline, Symbol, SymbolKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EntryKind {
    Class,
    Record,
    Interface,
    Type,
    Unit,
    Routine,
    Method,
    Property,
    Field,
    Constant,
    Variable,
    EnumValue,
}

impl EntryKind {
    pub fn label(self) -> &'static str {
        match self {
            EntryKind::Unit => "unit",
            EntryKind::Class => "class",
            EntryKind::Record => "record",
            EntryKind::Interface => "interface",
            EntryKind::Type => "type",
            EntryKind::Method => "method",
            EntryKind::Routine => "routine",
            EntryKind::Property => "property",
            EntryKind::Field => "field",
            EntryKind::Variable => "var",
            EntryKind::Constant => "const",
            EntryKind::EnumValue => "enum",
        }
    }
}

/// Simbolo achatado para busca rapida.
#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub low: String,
    /// Classe/record dono (`TForm1`), vazio no nivel da unit.
    pub container: String,
    pub kind: EntryKind,
    pub detail: String,
    pub line: usize,
    pub col: usize,
}

#[derive(Debug)]
pub struct FileSymbols {
    pub path: PathBuf,
    pub unit: String,
    pub outline: Arc<Outline>,
    pub entries: Vec<Entry>,
    /// Tamanho e data do arquivo lido; None quando veio de um buffer aberto.
    stamp: Option<(u64, SystemTime)>,
}

fn stamp(path: &Path) -> Option<(u64, SystemTime)> {
    let m = std::fs::metadata(path).ok()?;
    Some((m.len(), m.modified().ok()?))
}

impl FileSymbols {
    /// A partir de um outline ja calculado (arquivo aberto no editor).
    pub fn new(path: PathBuf, outline: Arc<Outline>) -> Self {
        let unit = outline.unit_name().map(str::to_owned).unwrap_or_else(|| {
            path.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
        let entries = entries(&outline);
        Self {
            path,
            unit,
            outline,
            entries,
            stamp: None,
        }
    }

    fn read(path: &Path) -> Result<Self, String> {
        let before = stamp(path);
        let text = crate::projects::read_text(path)?;
        let mut f = Self::new(path.to_path_buf(), Arc::new(outline::symbols_only(&text)));
        f.stamp = before;
        Ok(f)
    }
}

/// Sufixo/prefixo de tipo que nao importa para achar a declaracao: `Vcl.Forms.TForm`,
/// `TList<TItem>`, `^TNode`.
pub fn base_type(name: &str) -> &str {
    let name = name.trim().trim_start_matches('^');
    let name = name.split('<').next().unwrap_or(name);
    name.rsplit('.').next().unwrap_or(name).trim()
}

#[derive(Debug, Default, Clone)]
pub struct SymbolIndex {
    pub files: Vec<Arc<FileSymbols>>,
    pub build_ms: f64,
    pub parsed: usize,
    pub reused: usize,
    pub failed: Vec<String>,
}

/// Resultado do Go to Symbol, com o que a lista precisa mostrar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    pub name: String,
    pub container: String,
    pub kind: EntryKind,
    pub detail: String,
    pub unit: String,
    pub location: Location,
    pub score: u32,
}

impl SymbolIndex {
    /// Le e analisa os fontes Pascal em paralelo. `previous` evita reler arquivos inalterados.
    pub fn build(paths: &[PathBuf], previous: Option<&SymbolIndex>, cancel: &AtomicBool) -> Self {
        let started = Instant::now();
        let prev: HashMap<String, &Arc<FileSymbols>> = previous
            .map(|p| {
                p.files
                    .iter()
                    .filter(|f| f.stamp.is_some())
                    .map(|f| (key(&f.path), f))
                    .collect()
            })
            .unwrap_or_default();
        let mut seen = HashSet::new();
        let paths: Vec<&PathBuf> = paths.iter().filter(|p| seen.insert(key(p))).collect();
        let next = AtomicUsize::new(0);
        let results = Mutex::new(Vec::with_capacity(paths.len()));
        let workers = std::thread::available_parallelism()
            .map_or(2, |n| n.get())
            .clamp(1, 8)
            .min(paths.len().max(1));
        std::thread::scope(|s| {
            for _ in 0..workers {
                s.spawn(|| {
                    let mut local = Vec::new();
                    loop {
                        if cancel.load(Ordering::Relaxed) {
                            break;
                        }
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some(path) = paths.get(i) else { break };
                        let reused = prev
                            .get(&key(path))
                            .filter(|f| f.stamp.is_some() && f.stamp == stamp(path))
                            .map(|f| Arc::clone(f));
                        let result = match reused {
                            Some(f) => Ok((f, true)),
                            None => FileSymbols::read(path).map(|f| (Arc::new(f), false)),
                        };
                        local.push((i, result));
                    }
                    results.lock().expect("sem panico").extend(local);
                });
            }
        });
        let mut results = results.into_inner().expect("sem panico");
        results.sort_by_key(|r| r.0);
        let mut index = SymbolIndex::default();
        for (_, r) in results {
            match r {
                Ok((f, reused)) => {
                    if reused {
                        index.reused += 1;
                    } else {
                        index.parsed += 1;
                    }
                    index.files.push(f);
                }
                Err(e) => index.failed.push(e),
            }
        }
        index.build_ms = started.elapsed().as_secs_f64() * 1000.0;
        index
    }

    /// Copia do indice com um arquivo substituido (ou acrescentado): buffers abertos no editor.
    pub fn with_file(&self, file: FileSymbols) -> SymbolIndex {
        let mut out = self.clone();
        let k = key(&file.path);
        let file = Arc::new(file);
        match out.files.iter().position(|f| key(&f.path) == k) {
            Some(i) => out.files[i] = file,
            None => out.files.push(file),
        }
        out
    }

    pub fn file(&self, path: &Path) -> Option<&Arc<FileSymbols>> {
        let k = key(path);
        self.files.iter().find(|f| key(&f.path) == k)
    }

    pub fn unit(&self, name: &str) -> Option<&Arc<FileSymbols>> {
        self.files
            .iter()
            .find(|f| f.unit.eq_ignore_ascii_case(name))
    }

    pub fn symbol_count(&self) -> usize {
        self.files.iter().map(|f| f.entries.len()).sum()
    }

    /// Arquivos na ordem de preferencia: as units citadas primeiro, depois o resto.
    fn ordered<'a>(&'a self, prefer: &[String]) -> Vec<&'a Arc<FileSymbols>> {
        let mut out: Vec<&Arc<FileSymbols>> = prefer.iter().filter_map(|u| self.unit(u)).collect();
        for f in &self.files {
            if !out.iter().any(|o| Arc::ptr_eq(o, f)) {
                out.push(f);
            }
        }
        out
    }

    /// Classe/record/interface pelo nome, procurando primeiro nas units preferidas.
    pub fn find_type(&self, name: &str, prefer: &[String]) -> Option<(&Arc<FileSymbols>, &Symbol)> {
        let name = base_type(name);
        if name.is_empty() {
            return None;
        }
        self.ordered(prefer)
            .into_iter()
            .find_map(|f| f.outline.find_type(name).map(|s| (f, s)))
    }

    /// Declaracao de tipo, rotina, constante ou variavel global com este nome.
    pub fn find_declaration(&self, name: &str, prefer: &[String]) -> Option<Location> {
        let name = base_type(name);
        for f in self.ordered(prefer) {
            let o = &f.outline;
            let found = o
                .find_type(name)
                .map(|s| (s.line, s.col))
                .or_else(|| {
                    o.declarations
                        .iter()
                        .find(|d| d.name.eq_ignore_ascii_case(name))
                        .map(|d| (d.line, d.col))
                })
                .or_else(|| {
                    o.routines
                        .iter()
                        .find(|r| r.key.is_none() && r.name.eq_ignore_ascii_case(name))
                        .map(|r| (r.line, r.col))
                });
            if let Some((line, col)) = found {
                return Some(Location {
                    path: f.path.clone(),
                    line,
                    col,
                });
            }
        }
        None
    }

    /// Go to Symbol: nome exato, prefixo, iniciais (`bc` = `BotaoClick`), trecho ou subsequencia.
    /// `TForm1.Bot` filtra pela classe.
    pub fn search(&self, query: &str, limit: usize) -> Vec<Hit> {
        let q = query.trim().to_lowercase();
        let (container_q, name_q) = match q.rsplit_once('.') {
            Some((c, n)) => (Some(c.to_owned()), n.to_owned()),
            None => (None, q.clone()),
        };
        let mut hits: Vec<(u32, EntryKind, usize, &FileSymbols, &Entry)> = Vec::new();
        for f in &self.files {
            for e in &f.entries {
                if let Some(c) = &container_q {
                    if !e.container.to_lowercase().ends_with(c.as_str()) {
                        continue;
                    }
                }
                if let Some(score) = match_score(&name_q, &e.low, &e.name) {
                    hits.push((score, e.kind, e.name.len(), f, e));
                }
            }
        }
        hits.sort_by(|a, b| {
            (a.0, a.1, a.2)
                .cmp(&(b.0, b.1, b.2))
                .then_with(|| a.4.name.cmp(&b.4.name))
        });
        hits.into_iter()
            .take(limit)
            .map(|(score, _, _, f, e)| Hit {
                name: e.name.clone(),
                container: e.container.clone(),
                kind: e.kind,
                detail: e.detail.clone(),
                unit: f.unit.clone(),
                location: Location {
                    path: f.path.clone(),
                    line: e.line,
                    col: e.col,
                },
                score,
            })
            .collect()
    }
}

fn key(path: &Path) -> String {
    path.to_string_lossy().to_lowercase()
}

/// Iniciais de um identificador: `TForm_Principal` -> `tfp`, `BotaoClick` -> `bc`.
fn initials(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let mut out = String::new();
    for (i, &c) in chars.iter().enumerate() {
        let start = match i.checked_sub(1).map(|p| chars[p]) {
            None => true,
            Some('_') => c != '_',
            // `TForm`: o F conta porque inicia "Form", mesmo depois de outra maiuscula
            Some(p) => {
                c.is_uppercase()
                    && (!p.is_uppercase() || chars.get(i + 1).is_some_and(|n| n.is_lowercase()))
            }
        };
        if start && c != '_' {
            out.extend(c.to_lowercase());
        }
    }
    out
}

fn is_subsequence(needle: &str, hay: &str) -> bool {
    let mut it = hay.chars();
    needle.chars().all(|c| it.any(|h| h == c))
}

/// Qualidade de uma correspondencia (menor e melhor); None se nao combina.
pub fn match_score(q: &str, low: &str, name: &str) -> Option<u32> {
    if q.is_empty() {
        return Some(5);
    }
    if low == q {
        Some(0)
    } else if low.starts_with(q) {
        Some(1)
    } else if initials(name).starts_with(q) {
        Some(2)
    } else if low.contains(q) {
        Some(3)
    } else if q.chars().count() >= 3 && is_subsequence(q, low) {
        Some(4)
    } else {
        None
    }
}

fn detail_of(s: &Symbol) -> String {
    match s.kind {
        SymbolKind::Class | SymbolKind::Record | SymbolKind::Interface if !s.detail.is_empty() => {
            format!("({})", s.detail)
        }
        SymbolKind::Method | SymbolKind::Routine => {
            if s.ty.is_empty() {
                s.params.clone()
            } else {
                format!("{}: {}", s.params, s.ty)
            }
        }
        SymbolKind::Field | SymbolKind::Property if !s.ty.is_empty() => format!(": {}", s.ty),
        _ => String::new(),
    }
}

fn entries(o: &Outline) -> Vec<Entry> {
    let declared: HashSet<String> = o
        .declarations
        .iter()
        .filter(|d| d.kind == DeclKind::Routine)
        .map(|d| d.name.to_lowercase())
        .collect();
    let mut out = Vec::new();
    fn walk(list: &[Symbol], container: &str, declared: &HashSet<String>, out: &mut Vec<Entry>) {
        for s in list {
            let kind = match s.kind {
                SymbolKind::Class => Some(EntryKind::Class),
                SymbolKind::Record => Some(EntryKind::Record),
                SymbolKind::Interface => Some(EntryKind::Interface),
                SymbolKind::Method => Some(EntryKind::Method),
                SymbolKind::Property => Some(EntryKind::Property),
                SymbolKind::Field => Some(EntryKind::Field),
                SymbolKind::Unit => Some(EntryKind::Unit),
                SymbolKind::Routine
                    if !s.name.contains('.') && !declared.contains(&s.name.to_lowercase()) =>
                {
                    Some(EntryKind::Routine)
                }
                _ => None,
            };
            if let Some(kind) = kind {
                out.push(Entry {
                    name: s.name.clone(),
                    low: s.name.to_lowercase(),
                    container: container.to_owned(),
                    kind,
                    detail: detail_of(s),
                    line: s.line,
                    col: s.col,
                });
            }
            match s.kind {
                SymbolKind::Class | SymbolKind::Record | SymbolKind::Interface => {
                    let path = if container.is_empty() {
                        s.name.clone()
                    } else {
                        format!("{container}.{}", s.name)
                    };
                    walk(&s.children, &path, declared, out);
                }
                SymbolKind::Group => walk(&s.children, container, declared, out),
                _ => {}
            }
        }
    }
    walk(&o.symbols, "", &declared, &mut out);
    for d in &o.declarations {
        let kind = match d.kind {
            DeclKind::Type => EntryKind::Type,
            DeclKind::Variable => EntryKind::Variable,
            DeclKind::Constant => EntryKind::Constant,
            DeclKind::EnumValue => EntryKind::EnumValue,
            DeclKind::Routine => EntryKind::Routine,
        };
        let detail = match d.kind {
            DeclKind::Routine if d.ty.is_empty() => d.params.clone(),
            DeclKind::Routine => format!("{}: {}", d.params, d.ty),
            DeclKind::Type if !d.ty.is_empty() => format!("= {}", d.ty),
            _ if !d.ty.is_empty() => format!(": {}", d.ty),
            _ => String::new(),
        };
        out.push(Entry {
            name: d.name.clone(),
            low: d.name.to_lowercase(),
            container: String::new(),
            kind,
            detail,
            line: d.line,
            col: d.col,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scores_prefer_exact_prefix_and_initials() {
        assert_eq!(match_score("tform1", "tform1", "TForm1"), Some(0));
        assert_eq!(match_score("bot", "botaoclick", "BotaoClick"), Some(1));
        assert_eq!(match_score("bc", "botaoclick", "BotaoClick"), Some(2));
        assert_eq!(match_score("click", "botaoclick", "BotaoClick"), Some(3));
        assert_eq!(match_score("bck", "botaoclick", "BotaoClick"), Some(4));
        assert_eq!(match_score("xyz", "botaoclick", "BotaoClick"), None);
        assert_eq!(initials("TForm_Fl_Funcionario"), "tfff");
        assert_eq!(base_type("Vcl.Forms.TForm"), "TForm");
        assert_eq!(base_type("TList<TItem>"), "TList");
        assert_eq!(base_type("^TNode"), "TNode");
    }
}
