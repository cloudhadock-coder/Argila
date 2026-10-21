//! Projetos Delphi (.dproj, .dpr, .dpk ou uma pasta), lidos sem executar nada e sem alterar
//! nenhum arquivo. Alteracoes ficam em `files` e so acontecem por acao explicita do usuario.

pub mod dpr;
pub mod dproj;
pub mod files;
pub mod outline;

use std::path::{Component, Path, PathBuf};
use std::time::Instant;

use crate::filesystem::encoding;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectKind {
    Program,
    Library,
    Package,
    Folder,
}

impl ProjectKind {
    pub fn label(self) -> &'static str {
        match self {
            ProjectKind::Program => "Program",
            ProjectKind::Library => "Library",
            ProjectKind::Package => "Package",
            ProjectKind::Folder => "Folder",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectUnit {
    pub name: String,
    /// Caminho como escrito no `in '...'` ou no `Include` do .dproj.
    pub declared_path: Option<String>,
    /// Caminho resolvido do .pas (existindo ou nao, quando declarado).
    pub pas: Option<PathBuf>,
    pub dfm: Option<PathBuf>,
    pub form: Option<String>,
    pub form_type: Option<String>,
    pub conditional: bool,
    pub in_source: bool,
    pub in_dproj: bool,
    pub exists: bool,
}

impl ProjectUnit {
    /// Unit sem caminho e fora das pastas do projeto (RTL/VCL/bibliotecas): nao e um problema.
    pub fn is_library(&self) -> bool {
        self.declared_path.is_none() && !self.exists
    }

    pub fn is_missing(&self) -> bool {
        self.declared_path.is_some() && !self.exists
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OtherFile {
    pub path: PathBuf,
    pub role: &'static str,
    pub exists: bool,
}

#[derive(Clone, Debug)]
pub struct Project {
    pub kind: ProjectKind,
    pub name: String,
    /// O que o usuario abriu (.dproj, .dpr, .dpk ou pasta).
    pub file: PathBuf,
    pub dir: PathBuf,
    pub source: Option<PathBuf>,
    pub dproj_path: Option<PathBuf>,
    pub dproj: Option<dproj::Dproj>,
    pub units: Vec<ProjectUnit>,
    pub requires: Vec<String>,
    pub others: Vec<OtherFile>,
    pub problems: Vec<String>,
    pub load_ms: f64,
}

/// Le um fonte de projeto na codificacao detectada (BOM / UTF-8 / ANSI do sistema).
pub fn read_text(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(encoding::decode(&bytes, encoding::system_ansi_encoding()).text)
}

/// Normaliza `.` e `..` sem tocar no disco (caminhos inexistentes tambem funcionam).
pub fn clean(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Caminho de projeto Delphi (com `\`) relativo a `dir`.
fn resolve(dir: &Path, declared: &str) -> PathBuf {
    let native: String = declared
        .chars()
        .map(|c| {
            if c == '\\' || c == '/' {
                std::path::MAIN_SEPARATOR
            } else {
                c
            }
        })
        .collect();
    clean(&dir.join(native))
}

/// O .dfm de um .pas e vice-versa, se existir ao lado.
pub fn related_file(path: &Path) -> Option<PathBuf> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    let other = match ext.as_str() {
        "pas" => "dfm",
        "dfm" => "pas",
        _ => return None,
    };
    let candidate = path.with_extension(other);
    candidate.is_file().then_some(candidate)
}

fn same_file(a: &Path, b: &Path) -> bool {
    a.as_os_str().eq_ignore_ascii_case(b.as_os_str())
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

impl Project {
    pub fn load(path: &Path) -> Result<Project, String> {
        let started = Instant::now();
        let path = clean(&std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf()));
        let mut project = if path.is_dir() {
            Self::folder(&path)
        } else {
            match path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase()
                .as_str()
            {
                "dproj" => Self::from_dproj(&path)?,
                "dpr" | "dpk" => Self::from_source(&path, None)?,
                other => {
                    return Err(format!(
                        "{}: .{other} is not a Delphi project file",
                        path.display()
                    ));
                }
            }
        };
        project.file = path;
        project.load_ms = started.elapsed().as_secs_f64() * 1000.0;
        Ok(project)
    }

    fn empty(kind: ProjectKind, name: String, dir: &Path) -> Self {
        Self {
            kind,
            name,
            file: PathBuf::new(),
            dir: dir.to_path_buf(),
            source: None,
            dproj_path: None,
            dproj: None,
            units: Vec::new(),
            requires: Vec::new(),
            others: Vec::new(),
            problems: Vec::new(),
            load_ms: 0.0,
        }
    }

    fn from_dproj(path: &Path) -> Result<Self, String> {
        let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let d = dproj::parse(&read_text(path)?).map_err(|e| format!("{}: {e}", path.display()))?;
        let main = d
            .global("MainSource")
            .map(|m| resolve(&dir, m))
            .or_else(|| {
                ["dpr", "dpk"]
                    .iter()
                    .map(|e| path.with_extension(e))
                    .find(|p| p.is_file())
            });
        match main {
            Some(m) if m.is_file() => Self::from_source(&m, Some((path.to_path_buf(), d))),
            other => {
                let mut p = Self::empty(ProjectKind::Program, stem(path), &dir);
                p.problems.push(match other {
                    Some(m) => format!("Main source not found: {}", m.display()),
                    None => "The .dproj has no MainSource".to_owned(),
                });
                p.dproj_path = Some(path.to_path_buf());
                p.dproj = Some(d);
                p.merge_dproj_references();
                Ok(p)
            }
        }
    }

    fn from_source(path: &Path, dproj: Option<(PathBuf, dproj::Dproj)>) -> Result<Self, String> {
        let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let src = dpr::parse(&read_text(path)?);
        let kind = match src.kind {
            Some(dpr::SourceKind::Library) => ProjectKind::Library,
            Some(dpr::SourceKind::Package) => ProjectKind::Package,
            _ => ProjectKind::Program,
        };
        let name = if src.name.is_empty() {
            stem(path)
        } else {
            src.name.clone()
        };
        let mut p = Self::empty(kind, name, &dir);
        if !matches!(
            src.kind,
            Some(dpr::SourceKind::Program | dpr::SourceKind::Library | dpr::SourceKind::Package)
        ) {
            p.problems.push(format!(
                "{} does not start with program, library or package",
                path.display()
            ));
        }
        p.source = Some(path.to_path_buf());
        p.requires = src.requires.clone();
        for entry in &src.uses {
            let pas = match &entry.path {
                Some(declared) => Some(resolve(&dir, declared)),
                None => Some(dir.join(format!("{}.pas", entry.name))).filter(|p| p.is_file()),
            };
            let exists = pas.as_ref().is_some_and(|p| p.is_file());
            p.units.push(ProjectUnit {
                name: entry.name.clone(),
                declared_path: entry.path.clone(),
                dfm: pas.as_deref().filter(|_| exists).and_then(related_file),
                pas,
                form: entry.form.clone(),
                form_type: entry.form_type.clone(),
                conditional: entry.conditional,
                in_source: true,
                in_dproj: false,
                exists,
            });
        }
        let source_stem = stem(path);
        for (list, role) in [(&src.resources, "resource"), (&src.includes, "include")] {
            for r in list {
                let file = resolve(&dir, &r.replace('*', &source_stem));
                let exists = file.is_file();
                if !exists && role == "include" {
                    p.problems
                        .push(format!("Include file not found: {}", file.display()));
                }
                p.others.push(OtherFile {
                    path: file,
                    role,
                    exists,
                });
            }
        }
        let dproj = dproj.or_else(|| {
            let sibling = path.with_extension("dproj");
            match read_text(&sibling).and_then(|t| dproj::parse(&t)) {
                Ok(d) => Some((sibling, d)),
                Err(e) if sibling.is_file() => {
                    p.problems.push(format!("{}: {e}", sibling.display()));
                    None
                }
                Err(_) => None,
            }
        });
        if let Some((dp, d)) = dproj {
            p.dproj_path = Some(dp);
            p.dproj = Some(d);
            p.merge_dproj_references();
        }
        for u in &p.units {
            if u.is_missing() {
                p.problems.push(format!(
                    "Unit {}: file not found ({})",
                    u.name,
                    u.declared_path.as_deref().unwrap_or_default()
                ));
            }
        }
        Ok(p)
    }

    /// Junta as `DCCReference` do .dproj com o uses do fonte.
    fn merge_dproj_references(&mut self) {
        let Some(d) = &self.dproj else { return };
        let has_source = self.source.is_some();
        for r in &d.references {
            let pas = resolve(&self.dir, &r.include);
            let name = stem(&pas);
            let known = self.units.iter_mut().find(|u| {
                u.pas.as_deref().is_some_and(|p| same_file(p, &pas))
                    || u.name.eq_ignore_ascii_case(&name)
            });
            match known {
                Some(u) => {
                    u.in_dproj = true;
                    if u.form.is_none() {
                        u.form = r.form.clone();
                    }
                    if u.form_type.is_none() {
                        u.form_type = r.design_class.clone();
                    }
                }
                None => {
                    let exists = pas.is_file();
                    if has_source {
                        self.problems.push(format!(
                            "{} is in the .dproj but not in the uses clause",
                            r.include
                        ));
                    }
                    self.units.push(ProjectUnit {
                        name,
                        declared_path: Some(r.include.clone()),
                        dfm: exists.then(|| related_file(&pas)).flatten(),
                        pas: Some(pas),
                        form: r.form.clone(),
                        form_type: r.design_class.clone(),
                        conditional: false,
                        in_source: false,
                        in_dproj: true,
                        exists,
                    });
                }
            }
        }
        for u in self
            .units
            .iter()
            .filter(|u| u.in_source && u.declared_path.is_some() && !u.in_dproj)
        {
            self.problems.push(format!(
                "{} is in the uses clause but not in the .dproj",
                u.name
            ));
        }
    }

    /// Pasta aberta como projeto: units e arquivos relacionados ate 5 niveis abaixo.
    fn folder(dir: &Path) -> Self {
        const SKIP: &[&str] = &[
            "__history",
            "__recovery",
            ".git",
            ".svn",
            "win32",
            "win64",
            "bin",
            "obj",
            "dcu",
            "target",
            "node_modules",
        ];
        let name = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| dir.display().to_string());
        let mut p = Self::empty(ProjectKind::Folder, name, dir);
        let mut stack = vec![(dir.to_path_buf(), 0)];
        let mut seen = 0usize;
        while let Some((d, depth)) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&d) else {
                continue;
            };
            let mut entries: Vec<_> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
            entries.sort();
            for path in entries {
                seen += 1;
                if seen > 20_000 {
                    p.problems
                        .push("Folder has more than 20,000 entries; the scan stopped".to_owned());
                    return p;
                }
                let file_name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                if path.is_dir() {
                    if depth < 5 && !SKIP.contains(&file_name.as_str()) {
                        stack.push((path, depth + 1));
                    }
                    continue;
                }
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                match ext.as_str() {
                    "pas" => p.units.push(ProjectUnit {
                        name: stem(&path),
                        declared_path: None,
                        dfm: related_file(&path),
                        pas: Some(path),
                        form: None,
                        form_type: None,
                        conditional: false,
                        in_source: false,
                        in_dproj: false,
                        exists: true,
                    }),
                    "dpr" | "dpk" | "dproj" | "inc" | "res" | "rc" => {
                        let role = match ext.as_str() {
                            "inc" => "include",
                            "res" | "rc" => "resource",
                            _ => "project",
                        };
                        p.others.push(OtherFile {
                            path,
                            role,
                            exists: true,
                        });
                    }
                    _ => {}
                }
            }
        }
        p.units.sort_by_key(|u| u.name.to_lowercase());
        p
    }

    pub fn missing_count(&self) -> usize {
        self.units.iter().filter(|u| u.is_missing()).count()
    }

    pub fn unit_for_path(&self, path: &Path) -> Option<&ProjectUnit> {
        self.units.iter().find(|u| {
            u.pas.as_deref().is_some_and(|p| same_file(p, path))
                || u.dfm.as_deref().is_some_and(|p| same_file(p, path))
        })
    }

    /// Pasta do projeto + DCC_UnitSearchPath efetivo (configuracao/plataforma padrao) que existir.
    pub fn search_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = vec![self.dir.clone()];
        if let Some(d) = &self.dproj {
            let config = d
                .global("Config")
                .map(str::to_owned)
                .or_else(|| d.config_names().first().cloned())
                .unwrap_or_default();
            let platform = d
                .platform_names()
                .first()
                .cloned()
                .unwrap_or_else(|| "Win32".to_owned());
            if let Some(paths) = d.effective(&config, &platform).get("DCC_UnitSearchPath") {
                for part in paths
                    .split(';')
                    .map(str::trim)
                    .filter(|s| !s.is_empty() && !s.contains("$("))
                {
                    let dir = resolve(&self.dir, part);
                    if dir.is_dir() && !dirs.iter().any(|d| same_file(d, &dir)) {
                        dirs.push(dir);
                    }
                }
            }
        }
        dirs
    }

    /// Arquivo de uma unit pelo nome: units do projeto e depois as pastas de busca.
    pub fn resolve_unit(&self, name: &str) -> Option<PathBuf> {
        if let Some(u) = self
            .units
            .iter()
            .find(|u| u.name.eq_ignore_ascii_case(name) && u.exists)
        {
            return u.pas.clone();
        }
        self.search_dirs()
            .into_iter()
            .map(|d| d.join(format!("{name}.pas")))
            .find(|p| p.is_file())
    }
}
