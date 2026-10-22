//! Find in Files (Shift+Ctrl+F): busca em todos os arquivos do projeto, nos abertos ou numa pasta.
//!
//! Roda em threads, manda os resultados aos poucos e pode ser cancelada. Cada arquivo e lido na
//! codificacao detectada (BOM, UTF-8 ou ANSI), o mesmo criterio do editor; arquivos abertos usam
//! o texto em memoria, inclusive alteracoes nao salvas. Binarios (DFM TPF0, executaveis) sao
//! pulados. Nada e alterado em disco.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use encoding_rs::Encoding;
use regex::{Regex, RegexBuilder};

use crate::filesystem::encoding;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FindOptions {
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub regex: bool,
}

/// Uma ocorrencia: linha base 0 e colunas em caracteres, mais o texto da linha para a lista.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineMatch {
    pub line: usize,
    pub start: usize,
    pub end: usize,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileMatches {
    /// Posicao do arquivo na lista pedida (os resultados chegam fora de ordem).
    pub order: usize,
    pub path: PathBuf,
    pub matches: Vec<LineMatch>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Progress {
    File(FileMatches),
    Done(Summary),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub files_searched: usize,
    pub skipped: Vec<String>,
    pub cancelled: bool,
    pub truncated: bool,
    pub elapsed_ms: u64,
}

pub struct Request {
    pub query: String,
    pub options: FindOptions,
    pub files: Vec<PathBuf>,
    /// Textos de arquivos abertos no editor (valem no lugar do disco).
    pub buffers: Vec<(PathBuf, String)>,
    pub no_bom_default: &'static Encoding,
}

/// Linhas longas aparecem cortadas na lista (a ocorrencia continua visivel).
const PREVIEW_CHARS: usize = 400;
pub const MAX_MATCHES: usize = 50_000;
const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;

pub struct Matcher {
    re: Regex,
}

impl Matcher {
    /// Texto simples vira expressao escapada; "palavra inteira" usa limites de palavra Unicode.
    pub fn new(query: &str, options: FindOptions) -> Result<Self, String> {
        if query.is_empty() {
            return Err("Type something to search for.".to_owned());
        }
        let mut pattern = if options.regex {
            query.to_owned()
        } else {
            regex::escape(query)
        };
        if options.whole_word {
            pattern = format!(r"\b(?:{pattern})\b");
        }
        let re = RegexBuilder::new(&pattern)
            .case_insensitive(!options.case_sensitive)
            .multi_line(true)
            .build()
            .map_err(|e| format!("Invalid regular expression: {e}"))?;
        Ok(Self { re })
    }

    /// Ocorrencias por linha (uma expressao nunca atravessa linhas).
    pub fn search(&self, text: &str, limit: usize) -> Vec<LineMatch> {
        let mut out = Vec::new();
        for (line_no, raw) in text.split('\n').enumerate() {
            let line = raw.strip_suffix('\r').unwrap_or(raw);
            let mut found = self.re.find_iter(line).filter(|m| !m.is_empty()).peekable();
            if found.peek().is_none() {
                continue;
            }
            let preview: String = line.chars().take(PREVIEW_CHARS).collect();
            for m in found {
                let start = line[..m.start()].chars().count();
                let end = start + line[m.start()..m.end()].chars().count();
                out.push(LineMatch {
                    line: line_no,
                    start,
                    end,
                    text: preview.clone(),
                });
                if out.len() >= limit {
                    return out;
                }
            }
        }
        out
    }
}

/// Le um arquivo como texto, pulando binarios e arquivos enormes.
pub fn read_text(path: &Path, no_bom_default: &'static Encoding) -> Result<String, String> {
    let size = std::fs::metadata(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .len();
    if size > MAX_FILE_BYTES {
        return Err(format!("{}: larger than 32 MB, skipped", path.display()));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if let Some(kind) = encoding::binary_kind(&bytes) {
        return Err(format!("{}: {kind}, skipped", path.display()));
    }
    Ok(encoding::decode(&bytes, no_bom_default).text)
}

/// Pastas que nunca tem fonte do projeto (historico do Delphi, saidas de build, controle de versao).
const SKIP_DIRS: &[&str] = &[
    "__history",
    "__recovery",
    ".git",
    ".svn",
    ".hg",
    "win32",
    "win64",
    "dcu",
    "target",
    "node_modules",
];

/// `*.pas;*.dfm` -> combina pelo nome do arquivo, sem diferenciar maiusculas.
pub fn mask_matches(masks: &[String], name: &str) -> bool {
    fn glob(p: &[char], s: &[char]) -> bool {
        match (p.first(), s.first()) {
            (None, None) => true,
            (Some('*'), _) => glob(&p[1..], s) || (!s.is_empty() && glob(p, &s[1..])),
            (Some('?'), Some(_)) => glob(&p[1..], &s[1..]),
            (Some(a), Some(b)) => a == b && glob(&p[1..], &s[1..]),
            _ => false,
        }
    }
    let name: Vec<char> = name.to_lowercase().chars().collect();
    masks.iter().any(|m| {
        let p: Vec<char> = m.trim().to_lowercase().chars().collect();
        !p.is_empty() && glob(&p, &name)
    })
}

pub fn parse_masks(text: &str) -> Vec<String> {
    text.split([';', ','])
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Arquivos de uma pasta que combinam com as mascaras, em ordem alfabetica.
pub fn list_directory(
    dir: &Path,
    masks: &[String],
    recursive: bool,
    cancel: &AtomicBool,
) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(d) = pending.pop() {
        if cancel.load(Ordering::Relaxed) || out.len() > 200_000 {
            break;
        }
        let Ok(read) = std::fs::read_dir(&d) else {
            continue;
        };
        let mut entries: Vec<_> = read.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let name = e.file_name().to_string_lossy().into_owned();
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                if recursive && !SKIP_DIRS.contains(&name.to_lowercase().as_str()) {
                    pending.push(e.path());
                }
            } else if mask_matches(masks, &name) {
                out.push(e.path());
            }
        }
    }
    out.sort();
    out
}

fn same(a: &Path, b: &Path) -> bool {
    a.as_os_str().eq_ignore_ascii_case(b.as_os_str())
}

/// Executa a busca (bloqueante). `send` recebe cada arquivo com ocorrencias e o resumo final.
pub fn run(request: &Request, cancel: &AtomicBool, send: &(dyn Fn(Progress) + Sync)) {
    let started = Instant::now();
    let matcher = match Matcher::new(&request.query, request.options) {
        Ok(m) => m,
        Err(e) => {
            send(Progress::Done(Summary {
                skipped: vec![e],
                ..Summary::default()
            }));
            return;
        }
    };
    let next = AtomicUsize::new(0);
    let total = AtomicUsize::new(0);
    let searched = AtomicUsize::new(0);
    let skipped = Mutex::new(Vec::new());
    let workers = std::thread::available_parallelism()
        .map_or(2, |n| n.get())
        .clamp(1, 8);
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| {
                loop {
                    if cancel.load(Ordering::Relaxed)
                        || total.load(Ordering::Relaxed) >= MAX_MATCHES
                    {
                        break;
                    }
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(path) = request.files.get(i) else {
                        break;
                    };
                    let text = match request.buffers.iter().find(|(p, _)| same(p, path)) {
                        Some((_, t)) => Ok(t.clone()),
                        None => read_text(path, request.no_bom_default),
                    };
                    match text {
                        Ok(text) => {
                            searched.fetch_add(1, Ordering::Relaxed);
                            let room = MAX_MATCHES.saturating_sub(total.load(Ordering::Relaxed));
                            let matches = matcher.search(&text, room.max(1));
                            if !matches.is_empty() {
                                total.fetch_add(matches.len(), Ordering::Relaxed);
                                send(Progress::File(FileMatches {
                                    order: i,
                                    path: path.clone(),
                                    matches,
                                }));
                            }
                        }
                        Err(e) => skipped.lock().expect("sem panico").push(e),
                    }
                }
            });
        }
    });
    send(Progress::Done(Summary {
        files_searched: searched.load(Ordering::Relaxed),
        skipped: skipped.into_inner().expect("sem panico"),
        cancelled: cancel.load(Ordering::Relaxed),
        truncated: total.load(Ordering::Relaxed) >= MAX_MATCHES,
        elapsed_ms: started.elapsed().as_millis() as u64,
    }));
}

/// Busca numa thread; `notify` e chamado a cada resultado (para redesenhar a janela).
pub fn spawn(
    request: Request,
    cancel: Arc<AtomicBool>,
    notify: impl Fn() + Send + Sync + 'static,
) -> Receiver<Progress> {
    let (tx, rx): (Sender<Progress>, Receiver<Progress>) = channel();
    std::thread::spawn(move || {
        let tx = Mutex::new(tx);
        run(&request, &cancel, &|p| {
            let _ = tx.lock().expect("sem panico").send(p);
            notify();
        });
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matcher_columns_are_characters() {
        let m = Matcher::new("ação", FindOptions::default()).unwrap();
        let r = m.search("x := 'Ação';\r\n\tAÇÃO ação\n", 10);
        assert_eq!(
            r.iter()
                .map(|m| (m.line, m.start, m.end))
                .collect::<Vec<_>>(),
            vec![(0, 6, 10), (1, 1, 5), (1, 6, 10)]
        );
        assert_eq!(r[0].text, "x := 'Ação';");
        let w = Matcher::new(
            "Close",
            FindOptions {
                case_sensitive: true,
                whole_word: true,
                regex: false,
            },
        )
        .unwrap();
        assert_eq!(w.search("TbClose Close close Close1", 10).len(), 1);
        let re = Matcher::new(
            r"Tb\w+\.Open",
            FindOptions {
                regex: true,
                ..FindOptions::default()
            },
        )
        .unwrap();
        assert_eq!(re.search("tbClientes.Open; Tb.Open", 10).len(), 1);
        assert!(
            Matcher::new(
                "(",
                FindOptions {
                    regex: true,
                    ..FindOptions::default()
                }
            )
            .is_err()
        );
        assert_eq!(
            Matcher::new("(", FindOptions::default())
                .unwrap()
                .search("f(x)", 5)
                .len(),
            1
        );
    }

    #[test]
    fn masks() {
        let m = parse_masks("*.pas; *.DFM;Unit?.inc");
        assert!(mask_matches(&m, "Unit_Teste.pas"));
        assert!(mask_matches(&m, "form.dfm"));
        assert!(mask_matches(&m, "Unit1.inc"));
        assert!(!mask_matches(&m, "Unit12.inc"));
        assert!(!mask_matches(&m, "x.dproj"));
    }
}
