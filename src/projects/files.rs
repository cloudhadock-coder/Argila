//! Alteracoes em arquivos de projeto, sempre por acao explicita do usuario.
//!
//! Um `Plan` descreve tudo antes de gravar (o usuario confirma). Ao aplicar: confere que nenhum
//! arquivo mudou desde a leitura, guarda copias em `<config>/backups/<data>`, move/grava de forma
//! atomica e mantem a codificacao e a quebra de linha de cada arquivo editado.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::{Project, ProjectUnit, dpr, dproj, read_text};
use crate::filesystem::atomic_write;
use crate::filesystem::encoding::{self, LineEnding, TextEncoding, TextFormat};

#[derive(Debug)]
pub struct FileEdit {
    pub path: PathBuf,
    pub summary: String,
    original: Vec<u8>,
    format: TextFormat,
    pub new_text: String,
}

impl FileEdit {
    fn prepare(
        path: &Path,
        summary: String,
        edit: impl FnOnce(&str) -> Result<String, String>,
    ) -> Result<Self, String> {
        let original = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let decoded = encoding::decode(&original, encoding::system_ansi_encoding());
        if !decoded.lossless {
            return Err(format!(
                "{} cannot be edited safely: it is not decoded exactly as {}",
                path.display(),
                decoded.format.label()
            ));
        }
        let new_text = edit(&decoded.text).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Self {
            path: path.to_path_buf(),
            summary,
            original,
            format: decoded.format,
            new_text,
        })
    }
}

#[derive(Debug, Default)]
pub struct Plan {
    pub title: String,
    pub edits: Vec<FileEdit>,
    /// (de, para): arquivos renomeados.
    pub moves: Vec<(PathBuf, PathBuf)>,
    /// Arquivos novos (UTF-8 com BOM e CRLF, o padrao do Delphi 12).
    pub creates: Vec<(PathBuf, String)>,
    /// Lembretes que nao viram alteracao (ex.: units que usam o nome antigo).
    pub notes: Vec<String>,
}

impl Plan {
    /// Linhas legiveis para o dialogo de confirmacao.
    pub fn describe(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.extend(
            self.creates
                .iter()
                .map(|(p, _)| format!("Create {}", p.display())),
        );
        out.extend(self.moves.iter().map(|(a, b)| {
            format!(
                "Rename {} → {}",
                a.display(),
                b.file_name().unwrap_or_default().to_string_lossy()
            )
        }));
        out.extend(
            self.edits
                .iter()
                .map(|e| format!("Edit {}: {}", e.path.display(), e.summary)),
        );
        out.extend(self.notes.iter().cloned());
        out
    }
}

pub fn is_valid_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c == '_' || c.is_alphabetic())
        && chars.all(|c| c == '_' || c.is_alphanumeric())
}

/// Caminho relativo no estilo Delphi (`..\Comum\Unit.pas`).
pub fn relative_path(from_dir: &Path, to: &Path) -> String {
    let a: Vec<_> = from_dir.components().collect();
    let b: Vec<_> = to.components().collect();
    let common = a
        .iter()
        .zip(&b)
        .take_while(|(x, y)| x.as_os_str().eq_ignore_ascii_case(y.as_os_str()))
        .count();
    if common == 0 {
        return to.display().to_string();
    }
    let mut parts: Vec<String> = vec!["..".to_owned(); a.len() - common];
    parts.extend(
        b[common..]
            .iter()
            .map(|c| c.as_os_str().to_string_lossy().into_owned()),
    );
    parts.join("\\")
}

/// Nome do form no cabecalho do .dfm (`object Form1: TForm1`).
pub fn form_of_dfm(dfm: &Path) -> Option<(String, String)> {
    let text = read_text(dfm).ok()?;
    let line = text.lines().map(str::trim).find(|l| !l.is_empty())?;
    let rest = line.split_once(char::is_whitespace)?.1;
    let (name, class) = rest.split_once(':')?;
    Some((name.trim().to_owned(), class.trim().to_owned()))
}

fn project_edits(
    project: &Project,
    mut source_edit: impl FnMut(&str) -> Result<String, String>,
    mut dproj_edit: impl FnMut(&str) -> Result<String, String>,
    what: &str,
) -> Result<Vec<FileEdit>, String> {
    let mut edits = Vec::new();
    if let Some(src) = &project.source {
        edits.push(FileEdit::prepare(
            src,
            format!("{what} (uses clause)"),
            &mut source_edit,
        )?);
    }
    if let Some(dp) = &project.dproj_path {
        edits.push(FileEdit::prepare(
            dp,
            format!("{what} (DCCReference)"),
            &mut dproj_edit,
        )?);
    }
    if edits.is_empty() {
        return Err("this project has no .dpr/.dpk or .dproj to edit".to_owned());
    }
    Ok(edits)
}

/// Adicionar uma unit existente ao projeto (.dpr/.dpk e .dproj).
pub fn plan_add_unit(project: &Project, pas: &Path) -> Result<Plan, String> {
    let name = pas
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    if !is_valid_identifier(&name) {
        return Err(format!("'{name}' is not a valid unit name"));
    }
    let rel = relative_path(&project.dir, pas);
    let form = pas
        .with_extension("dfm")
        .is_file()
        .then(|| form_of_dfm(&pas.with_extension("dfm")))
        .flatten();
    let form_name = form.as_ref().map(|f| f.0.as_str());
    let edits = project_edits(
        project,
        |t| dpr::add_unit(t, &name, &rel, form_name),
        |t| dproj::add_reference(t, &rel, form_name, None),
        &format!("add {name}"),
    )?;
    Ok(Plan {
        title: format!("Add {name} to {}", project.name),
        edits,
        ..Plan::default()
    })
}

/// Tirar a unit do projeto (o arquivo continua no disco).
pub fn plan_remove_unit(project: &Project, unit: &ProjectUnit) -> Result<Plan, String> {
    let include = unit
        .declared_path
        .clone()
        .unwrap_or_else(|| format!("{}.pas", unit.name));
    let edits = project_edits(
        project,
        |t| {
            if unit.in_source {
                dpr::remove_unit(t, &unit.name)
            } else {
                Ok(t.to_owned())
            }
        },
        |t| {
            if unit.in_dproj {
                dproj::remove_reference(t, &include)
            } else {
                Ok(t.to_owned())
            }
        },
        &format!("remove {}", unit.name),
    )?;
    let mut plan = Plan {
        title: format!("Remove {} from {}", unit.name, project.name),
        edits,
        ..Plan::default()
    };
    plan.notes
        .push("The unit files are not deleted.".to_owned());
    Ok(plan)
}

/// Renomear uma unit: arquivos .pas/.dfm, cabecalho `unit` e referencias do projeto.
pub fn plan_rename_unit(
    project: Option<&Project>,
    pas: &Path,
    new_name: &str,
) -> Result<Plan, String> {
    if !is_valid_identifier(new_name) {
        return Err(format!("'{new_name}' is not a valid unit name"));
    }
    let old_name = pas
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let new_pas = pas.with_file_name(format!("{new_name}.pas"));
    if new_pas.exists() && !new_pas.as_os_str().eq_ignore_ascii_case(pas.as_os_str()) {
        return Err(format!("{} already exists", new_pas.display()));
    }
    let mut plan = Plan {
        title: format!("Rename {old_name} to {new_name}"),
        ..Plan::default()
    };
    plan.edits.push(FileEdit::prepare(
        pas,
        format!("unit {old_name} → unit {new_name}"),
        |t| dpr::rename_unit_header(t, new_name),
    )?);
    plan.moves.push((pas.to_path_buf(), new_pas.clone()));
    let dfm = pas.with_extension("dfm");
    if dfm.is_file() {
        plan.moves.push((dfm, new_pas.with_extension("dfm")));
    }
    if let Some(project) = project.filter(|p| p.unit_for_path(pas).is_some()) {
        let unit = project.unit_for_path(pas).expect("filtrado acima");
        let old_include = unit
            .declared_path
            .clone()
            .unwrap_or_else(|| relative_path(&project.dir, pas));
        let new_include = relative_path(&project.dir, &new_pas);
        plan.edits.extend(project_edits(
            project,
            |t| {
                if unit.in_source {
                    dpr::rename_unit(t, &unit.name, new_name, Some(&new_include))
                } else {
                    Ok(t.to_owned())
                }
            },
            |t| {
                if unit.in_dproj {
                    dproj::rename_reference(t, &old_include, &new_include)
                } else {
                    Ok(t.to_owned())
                }
            },
            &format!("{old_name} → {new_name}"),
        )?);
    }
    plan.notes.push(format!(
        "Other units that use {old_name} must be updated by hand."
    ));
    Ok(plan)
}

/// Unit nova (opcionalmente ja adicionada ao projeto).
pub fn plan_new_unit(dir: &Path, name: &str, project: Option<&Project>) -> Result<Plan, String> {
    if !is_valid_identifier(name) {
        return Err(format!("'{name}' is not a valid unit name"));
    }
    let pas = dir.join(format!("{name}.pas"));
    if pas.exists() {
        return Err(format!("{} already exists", pas.display()));
    }
    let text = format!(
        "unit {name};\n\ninterface\n\nuses\n  System.SysUtils, System.Classes;\n\nimplementation\n\nend.\n"
    );
    let mut plan = Plan {
        title: format!("New unit {name}"),
        creates: vec![(pas.clone(), text)],
        ..Plan::default()
    };
    if let Some(project) = project {
        let rel = relative_path(&project.dir, &pas);
        plan.edits = project_edits(
            project,
            |t| dpr::add_unit(t, name, &rel, None),
            |t| dproj::add_reference(t, &rel, None, None),
            &format!("add {name}"),
        )?;
    }
    Ok(plan)
}

/// Aplica o plano. Devolve a pasta de backup (se algo foi editado).
pub fn apply(plan: &Plan, backup_root: &Path) -> Result<Option<PathBuf>, String> {
    for e in &plan.edits {
        let now = fs::read(&e.path).map_err(|err| format!("{}: {err}", e.path.display()))?;
        if now != e.original {
            return Err(format!(
                "{} changed since it was read; nothing was written",
                e.path.display()
            ));
        }
    }
    for (from, to) in &plan.moves {
        if !from.is_file() {
            return Err(format!(
                "{} no longer exists; nothing was written",
                from.display()
            ));
        }
        if to.exists() && !to.as_os_str().eq_ignore_ascii_case(from.as_os_str()) {
            return Err(format!(
                "{} already exists; nothing was written",
                to.display()
            ));
        }
    }
    for (path, _) in &plan.creates {
        if path.exists() {
            return Err(format!(
                "{} already exists; nothing was written",
                path.display()
            ));
        }
    }
    let backup = if plan.edits.is_empty() {
        None
    } else {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or_default();
        let dir = backup_root.join(stamp.to_string());
        fs::create_dir_all(&dir)
            .map_err(|e| format!("cannot create backup folder {}: {e}", dir.display()))?;
        for (i, e) in plan.edits.iter().enumerate() {
            let name = format!(
                "{i}_{}",
                e.path.file_name().unwrap_or_default().to_string_lossy()
            );
            fs::write(dir.join(name), &e.original)
                .map_err(|err| format!("backup failed: {err}"))?;
        }
        Some(dir)
    };
    for (from, to) in &plan.moves {
        fs::rename(from, to).map_err(|e| format!("cannot rename {}: {e}", from.display()))?;
    }
    for e in &plan.edits {
        let target = plan
            .moves
            .iter()
            .find(|(from, _)| from == &e.path)
            .map_or(&e.path, |(_, to)| to);
        let bytes = encoding::encode(&e.new_text, &e.format)
            .map_err(|u| format!("{}: '{}' cannot be represented", target.display(), u.ch))?;
        atomic_write(target, &bytes).map_err(|err| format!("{}: {err}", target.display()))?;
    }
    let new_format = TextFormat {
        encoding: TextEncoding::Utf8,
        bom: true,
        line_ending: LineEnding::CrLf,
    };
    for (path, text) in &plan.creates {
        let bytes = encoding::encode(text, &new_format).expect("UTF-8 codifica qualquer texto");
        atomic_write(path, &bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(backup)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths_use_delphi_separators() {
        let dir = Path::new("C:\\Proj\\App");
        assert_eq!(
            relative_path(dir, Path::new("C:\\Proj\\App\\Unit1.pas")),
            "Unit1.pas"
        );
        assert_eq!(
            relative_path(dir, Path::new("C:\\Proj\\App\\Sub Pasta\\U.pas")),
            "Sub Pasta\\U.pas"
        );
        assert_eq!(
            relative_path(dir, Path::new("C:\\Proj\\Comum\\U.pas")),
            "..\\Comum\\U.pas"
        );
        assert!(
            is_valid_identifier("Unit_Ação2")
                && !is_valid_identifier("2Unit")
                && !is_valid_identifier("Unit-1")
        );
    }
}
