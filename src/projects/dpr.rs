//! Leitura de .dpr / .dpk: cabecalho, `uses`/`contains`, `requires` e diretivas `{$R}`/`{$I}`.
//! As edicoes (adicionar, remover, renomear unit) trocam so o trecho da entrada e preservam o
//! resto do arquivo byte a byte, inclusive comentarios e blocos `{$IFDEF}`.

use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    Program,
    Library,
    Package,
    Unit,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsesEntry {
    pub name: String,
    /// Caminho do `in '...'`, como escrito.
    pub path: Option<String>,
    /// `{Form1}` ou `{Dm: TDataModule}`.
    pub form: Option<String>,
    pub form_type: Option<String>,
    /// Dentro de `{$IFDEF}`/`{$IF}`.
    pub conditional: bool,
    /// Bytes da entrada inteira (do nome ao fim do comentario/string).
    pub span: Range<usize>,
    pub name_span: Range<usize>,
    pub path_span: Option<Range<usize>>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProjectSource {
    pub kind: Option<SourceKind>,
    pub name: String,
    /// `uses` de programa/biblioteca ou `contains` de pacote.
    pub uses: Vec<UsesEntry>,
    pub requires: Vec<String>,
    pub resources: Vec<String>,
    pub includes: Vec<String>,
    /// Byte do `;` que fecha o uses/contains.
    pub uses_end: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Tok {
    Ident(String, Range<usize>),
    Str(String, Range<usize>),
    Comment {
        text: String,
        range: Range<usize>,
        directive: bool,
    },
    Sym(char, Range<usize>),
}

fn tokenize(text: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(c) = text[i..].chars().next() {
        let rest = &text[i..];
        if rest.starts_with("//") {
            let end = rest.find('\n').map_or(text.len(), |p| i + p);
            out.push(Tok::Comment {
                text: text[i + 2..end].to_owned(),
                range: i..end,
                directive: false,
            });
            i = end;
        } else if c == '{' {
            let end = rest.find('}').map_or(text.len(), |p| i + p + 1);
            let inner = text[i + 1..end].trim_end_matches('}');
            out.push(Tok::Comment {
                text: inner.to_owned(),
                range: i..end,
                directive: inner.starts_with('$'),
            });
            i = end;
        } else if rest.starts_with("(*") {
            let end = rest[2..].find("*)").map_or(text.len(), |p| i + 2 + p + 2);
            let inner = text[i + 2..end].trim_end_matches(')').trim_end_matches('*');
            out.push(Tok::Comment {
                text: inner.to_owned(),
                range: i..end,
                directive: inner.starts_with('$'),
            });
            i = end;
        } else if c == '\'' {
            let mut value = String::new();
            let mut j = i + 1;
            loop {
                match text[j..].find('\'') {
                    Some(p) => {
                        value.push_str(&text[j..j + p]);
                        j += p + 1;
                        if text[j..].starts_with('\'') {
                            value.push('\'');
                            j += 1;
                        } else {
                            break;
                        }
                    }
                    None => {
                        value.push_str(&text[j..]);
                        j = text.len();
                        break;
                    }
                }
            }
            out.push(Tok::Str(value, i..j));
            i = j;
        } else if c == '_' || c.is_alphabetic() || c == '&' {
            let start = i;
            let mut j = i + c.len_utf8();
            for d in text[j..].chars() {
                if d == '_' || d == '.' || d.is_alphanumeric() {
                    j += d.len_utf8();
                } else {
                    break;
                }
            }
            let word = text[start..j].trim_start_matches('&').trim_end_matches('.');
            out.push(Tok::Ident(
                word.to_owned(),
                start..start + (j - start).min(text[start..j].trim_end_matches('.').len()),
            ));
            i = j;
        } else if c.is_whitespace() {
            i += c.len_utf8();
        } else {
            out.push(Tok::Sym(c, i..i + c.len_utf8()));
            i += c.len_utf8();
        }
    }
    out
}

/// Argumento de `{$R arquivo}` / `{$I arquivo}`; None para chaves de compilador (`{$R+}`, `{$I-}`).
fn directive_arg(text: &str) -> Option<(String, String)> {
    let body = text.strip_prefix('$')?;
    let (name, arg) = body.split_once(char::is_whitespace).unwrap_or((body, ""));
    let arg = arg.trim();
    if arg.is_empty() || arg == "+" || arg == "-" {
        return Some((name.to_ascii_uppercase(), String::new()));
    }
    let arg = match arg.strip_prefix('\'') {
        Some(q) => q.split('\'').next().unwrap_or_default().to_owned(),
        None => arg.split_whitespace().next().unwrap_or_default().to_owned(),
    };
    Some((name.to_ascii_uppercase(), arg))
}

pub fn parse(text: &str) -> ProjectSource {
    let toks = tokenize(text);
    let mut src = ProjectSource::default();
    let mut depth = 0usize;
    let mut i = 0;
    let mut seen_uses = false;
    while i < toks.len() {
        match &toks[i] {
            Tok::Comment {
                text,
                directive: true,
                ..
            } => {
                if let Some((name, arg)) = directive_arg(text) {
                    match name.as_str() {
                        "IFDEF" | "IFNDEF" | "IF" | "IFOPT" => depth += 1,
                        "ENDIF" | "IFEND" => depth = depth.saturating_sub(1),
                        "R" | "RESOURCE" if !arg.is_empty() => src.resources.push(arg),
                        "I" | "INCLUDE" if !arg.is_empty() => src.includes.push(arg),
                        _ => {}
                    }
                }
            }
            Tok::Ident(word, _) if src.kind.is_none() => {
                src.kind = Some(match word.to_ascii_lowercase().as_str() {
                    "program" => SourceKind::Program,
                    "library" => SourceKind::Library,
                    "package" => SourceKind::Package,
                    "unit" => SourceKind::Unit,
                    _ => SourceKind::Unknown,
                });
                if let Some(Tok::Ident(name, _)) = toks.get(i + 1) {
                    src.name = name.clone();
                    i += 1;
                }
            }
            Tok::Ident(word, _)
                if !seen_uses
                    && (word.eq_ignore_ascii_case("uses")
                        || word.eq_ignore_ascii_case("contains")) =>
            {
                seen_uses = true;
                i = parse_uses(&toks, i + 1, &mut depth, &mut src);
                continue;
            }
            Tok::Ident(word, _) if word.eq_ignore_ascii_case("requires") => {
                i += 1;
                while let Some(t) = toks.get(i) {
                    match t {
                        Tok::Ident(name, _) => src.requires.push(name.clone()),
                        Tok::Sym(';', _) => break,
                        _ => {}
                    }
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    src
}

fn parse_uses(toks: &[Tok], mut i: usize, depth: &mut usize, src: &mut ProjectSource) -> usize {
    while i < toks.len() {
        match &toks[i] {
            Tok::Sym(';', r) => {
                src.uses_end = Some(r.start);
                return i + 1;
            }
            Tok::Comment {
                text,
                directive: true,
                ..
            } => {
                if let Some((name, _)) = directive_arg(text) {
                    match name.as_str() {
                        "IFDEF" | "IFNDEF" | "IF" | "IFOPT" => *depth += 1,
                        "ENDIF" | "IFEND" => *depth = depth.saturating_sub(1),
                        _ => {}
                    }
                }
                i += 1;
            }
            Tok::Ident(name, r) => {
                let mut entry = UsesEntry {
                    name: name.clone(),
                    path: None,
                    form: None,
                    form_type: None,
                    conditional: *depth > 0,
                    span: r.clone(),
                    name_span: r.clone(),
                    path_span: None,
                };
                i += 1;
                if let (Some(Tok::Ident(kw, _)), Some(Tok::Str(path, pr))) =
                    (toks.get(i), toks.get(i + 1))
                {
                    if kw.eq_ignore_ascii_case("in") {
                        entry.path = Some(path.clone());
                        entry.path_span = Some(pr.clone());
                        entry.span.end = pr.end;
                        i += 2;
                    }
                }
                if let Some(Tok::Comment {
                    text,
                    range,
                    directive: false,
                }) = toks.get(i)
                {
                    let (form, ty) = match text.split_once(':') {
                        Some((f, t)) => (f.trim().to_owned(), Some(t.trim().to_owned())),
                        None => (text.trim().to_owned(), None),
                    };
                    if !form.is_empty() {
                        entry.form = Some(form);
                        entry.form_type = ty;
                    }
                    entry.span.end = range.end;
                    i += 1;
                }
                src.uses.push(entry);
            }
            _ => i += 1,
        }
    }
    i
}

/// Escreve um caminho Pascal entre aspas (aspas internas sao duplicadas).
fn quote(path: &str) -> String {
    format!("'{}'", path.replace('\'', "''"))
}

fn directive_between(text: &str, range: Range<usize>) -> bool {
    tokenize(&text[range]).iter().any(|t| {
        matches!(
            t,
            Tok::Comment {
                directive: true,
                ..
            }
        )
    })
}

/// Acrescenta `Nome in 'caminho' {Form}` no fim do uses/contains.
pub fn add_unit(text: &str, name: &str, path: &str, form: Option<&str>) -> Result<String, String> {
    let src = parse(text);
    if src.uses.iter().any(|u| u.name.eq_ignore_ascii_case(name)) {
        return Err(format!("{name} is already in the uses clause"));
    }
    let last = src
        .uses
        .last()
        .ok_or("the project has no uses/contains clause")?;
    let line_start = text[..last.span.start].rfind('\n').map_or(0, |p| p + 1);
    let indent: String = text[line_start..last.span.start]
        .chars()
        .take_while(|c| c.is_whitespace())
        .collect();
    let form = form.map(|f| format!(" {{{f}}}")).unwrap_or_default();
    let entry = format!(",\n{indent}{name} in {}{form}", quote(path));
    let mut out = text.to_owned();
    out.insert_str(last.span.end, &entry);
    Ok(out)
}

/// Remove a entrada da unit (o arquivo da unit nao e tocado).
pub fn remove_unit(text: &str, name: &str) -> Result<String, String> {
    let src = parse(text);
    let idx = src
        .uses
        .iter()
        .position(|u| u.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| format!("{name} is not in the uses clause"))?;
    if src.uses.len() == 1 {
        return Err("cannot remove the only unit of the uses clause".to_owned());
    }
    let range = if idx > 0 {
        src.uses[idx - 1].span.end..src.uses[idx].span.end
    } else {
        src.uses[0].span.start..src.uses[1].span.start
    };
    if directive_between(text, range.clone()) {
        return Err(format!(
            "{name} is next to a conditional directive; edit the file manually"
        ));
    }
    let mut out = text.to_owned();
    out.replace_range(range, "");
    Ok(out)
}

/// Troca nome e caminho de uma entrada (o comentario `{Form}` e mantido).
pub fn rename_unit(
    text: &str,
    old: &str,
    new_name: &str,
    new_path: Option<&str>,
) -> Result<String, String> {
    let src = parse(text);
    let entry = src
        .uses
        .iter()
        .find(|u| u.name.eq_ignore_ascii_case(old))
        .ok_or_else(|| format!("{old} is not in the uses clause"))?;
    let mut out = text.to_owned();
    // de tras para frente para os offsets continuarem validos
    if let (Some(ps), Some(np)) = (&entry.path_span, new_path) {
        out.replace_range(ps.clone(), &quote(np));
    }
    out.replace_range(entry.name_span.clone(), new_name);
    Ok(out)
}

/// Troca o nome no cabecalho `unit Nome;` de um .pas.
pub fn rename_unit_header(text: &str, new_name: &str) -> Result<String, String> {
    let toks = tokenize(text);
    let mut idents = toks.iter().filter_map(|t| match t {
        Tok::Ident(w, r) => Some((w, r)),
        _ => None,
    });
    match (idents.next(), idents.next()) {
        (Some((kw, _)), Some((_, r))) if kw.eq_ignore_ascii_case("unit") => {
            let mut out = text.to_owned();
            out.replace_range(r.clone(), new_name);
            Ok(out)
        }
        _ => Err("the file does not start with a unit header".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DPR: &str = "program Teste;\n\n{$R *.dres}\n\nuses\n  Forms,\n  Unit_Dm in 'Unit_Dm.pas' {Dm: TDataModule},\n  {$IFDEF DEBUG}\n  Unit_Debug in 'debug\\Unit_Debug.pas',\n  {$ENDIF}\n  Unit_Ação in '..\\Comum\\Unit_Ação.pas' {Form_Ação},\n  Vcl.Themes;\n\n{$R *.RES}\n{$I versao.inc}\n{$R+}\n\nbegin\n  Application.Run;\nend.\n";

    #[test]
    fn parses_program_uses_forms_and_directives() {
        let src = parse(DPR);
        assert_eq!(
            (src.kind, src.name.as_str()),
            (Some(SourceKind::Program), "Teste")
        );
        let names: Vec<_> = src.uses.iter().map(|u| u.name.as_str()).collect();
        assert_eq!(
            names,
            ["Forms", "Unit_Dm", "Unit_Debug", "Unit_Ação", "Vcl.Themes"]
        );
        assert_eq!(src.uses[1].form.as_deref(), Some("Dm"));
        assert_eq!(src.uses[1].form_type.as_deref(), Some("TDataModule"));
        assert!(src.uses[2].conditional && !src.uses[3].conditional);
        assert_eq!(
            src.uses[3].path.as_deref(),
            Some("..\\Comum\\Unit_Ação.pas")
        );
        assert_eq!(src.resources, ["*.dres", "*.RES"]);
        assert_eq!(src.includes, ["versao.inc"]);
        assert_eq!(&DPR[src.uses_end.unwrap()..src.uses_end.unwrap() + 1], ";");
    }

    #[test]
    fn parses_package_requires_and_contains() {
        let dpk = "package MeusComps;\n{$R *.res}\nrequires\n  rtl,\n  vcl;\ncontains\n  uBotao in 'uBotao.pas',\n  uGrade in 'src\\uGrade.pas';\nend.\n";
        let src = parse(dpk);
        assert_eq!(src.kind, Some(SourceKind::Package));
        assert_eq!(src.requires, ["rtl", "vcl"]);
        assert_eq!(src.uses.len(), 2);
        assert_eq!(src.uses[1].path.as_deref(), Some("src\\uGrade.pas"));
    }

    #[test]
    fn add_remove_rename_preserve_everything_else() {
        let added = add_unit(DPR, "Unit_Nova", "Unit_Nova.pas", Some("Form_Nova")).unwrap();
        assert!(added.contains("  Vcl.Themes,\n  Unit_Nova in 'Unit_Nova.pas' {Form_Nova};"));
        assert_eq!(
            added.replace(",\n  Unit_Nova in 'Unit_Nova.pas' {Form_Nova}", ""),
            DPR
        );
        assert!(add_unit(DPR, "unit_dm", "x.pas", None).is_err());

        let removed = remove_unit(DPR, "Unit_Dm").unwrap();
        assert_eq!(
            removed,
            DPR.replace(",\n  Unit_Dm in 'Unit_Dm.pas' {Dm: TDataModule}", "")
        );
        let first = remove_unit(DPR, "Forms").unwrap();
        assert!(first.starts_with("program Teste;\n\n{$R *.dres}\n\nuses\n  Unit_Dm in"));
        assert!(
            remove_unit(DPR, "Unit_Debug").is_err(),
            "entrada condicional nao e removida automaticamente"
        );

        let renamed = rename_unit(DPR, "Unit_Dm", "Unit_Dados", Some("Unit_Dados.pas")).unwrap();
        assert_eq!(
            renamed,
            DPR.replace("Unit_Dm in 'Unit_Dm.pas'", "Unit_Dados in 'Unit_Dados.pas'")
        );
    }

    #[test]
    fn unit_header_rename() {
        let pas = "{ cabecalho }\nunit Unit1;\n\ninterface\n";
        assert_eq!(
            rename_unit_header(pas, "Unit_Novo").unwrap(),
            "{ cabecalho }\nunit Unit_Novo;\n\ninterface\n"
        );
        assert!(rename_unit_header("program X;", "Y").is_err());
    }
}
