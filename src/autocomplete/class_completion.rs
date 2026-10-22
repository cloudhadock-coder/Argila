//! Class Completion (Shift+Ctrl+C), como no Delphi:
//!
//! - metodos declarados na classe sem implementacao ganham um corpo vazio na implementation;
//! - `property Nome: Tipo;` sem `read`/`write` vira `read FNome write SetNome`, com o campo, o
//!   setter declarado na secao private e o corpo `FNome := Value;`;
//! - com o cursor num metodo implementado mas nao declarado, a declaracao entra na classe.
//!
//! Tudo vira uma unica transacao no buffer (um Ctrl+Z desfaz). Nada e gravado em disco.

use crate::editor::buffer::Edit;
use crate::projects::outline::{ClassInfo, MethodDecl, Outline, Routine};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    /// Prontas para `TextBuffer::transact` (da maior posicao para a menor).
    pub edits: Vec<Edit>,
    /// Cursor no primeiro corpo criado (posicao final, ja com as insercoes).
    pub caret: Option<usize>,
    pub summary: Vec<String>,
}

struct Insert {
    pos: usize,
    text: String,
    caret: Option<usize>,
}

/// Remove `= valor` dos parametros (a implementacao nao repete os valores padrao).
pub fn strip_defaults(header: &str) -> String {
    let mut out = String::new();
    let mut depth = 0i32;
    let mut skipping = false;
    let mut quoted = false;
    for c in header.chars() {
        if quoted {
            if !skipping {
                out.push(c);
            }
            quoted = c != '\'';
            continue;
        }
        match c {
            '\'' => quoted = true,
            '(' | '[' => depth += 1,
            ')' | ']' => {
                depth -= 1;
                if depth == 0 {
                    skipping = false;
                }
            }
            ';' if depth == 1 => skipping = false,
            '=' if depth == 1 => {
                skipping = true;
                while out.ends_with(' ') {
                    out.pop();
                }
                continue;
            }
            _ => {}
        }
        if !skipping {
            out.push(c);
        }
    }
    out
}

fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Texto entre duas posicoes (linha, coluna em caracteres).
fn span(lines: &[&str], from: (usize, usize), to: (usize, usize)) -> String {
    let mut out = String::new();
    let last = to.0.min(lines.len().saturating_sub(1));
    for (l, line) in lines.iter().enumerate().take(last + 1).skip(from.0) {
        let line = line.trim_end_matches('\r');
        let start = if l == from.0 { from.1 } else { 0 };
        let end = if l == to.0 { to.1 } else { usize::MAX };
        out.extend(line.chars().skip(start).take(end.saturating_sub(start)));
        out.push(' ');
    }
    out
}

fn norm_params(p: &str) -> String {
    strip_defaults(p)
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_lowercase()
}

fn implemented(decl: &MethodDecl, all: &[&MethodDecl], routines: &[Routine]) -> bool {
    let mut impls = routines
        .iter()
        .filter(|r| r.key.as_deref() == Some(decl.key.as_str()));
    let overloads = all.iter().filter(|d| d.key == decl.key).count();
    if overloads <= 1 {
        impls.next().is_some()
    } else {
        impls.any(|r| norm_params(&r.params) == norm_params(&decl.params))
    }
}

/// Classe do cursor: a declaracao mais interna que o contem ou a do metodo implementado.
fn target(o: &Outline, line: usize) -> Option<(&ClassInfo, Option<&Routine>)> {
    if let Some(c) = o
        .classes
        .iter()
        .filter(|c| c.line <= line && line <= c.end_line)
        .min_by_key(|c| c.end_line - c.line)
    {
        return Some((c, None));
    }
    let r = o
        .routines
        .iter()
        .filter(|r| r.line <= line && line <= r.end_line && r.key.is_some())
        .max_by_key(|r| r.line)?;
    let (class, _) = r.name.rsplit_once('.')?;
    let c = o
        .classes
        .iter()
        .find(|c| c.path.eq_ignore_ascii_case(class))?;
    Some((c, Some(r)))
}

pub fn complete(
    text: &str,
    o: &Outline,
    line: usize,
    pos: &dyn Fn(usize, usize) -> usize,
) -> Result<Plan, String> {
    let (class, routine) = target(o, line).ok_or_else(|| {
        "Class Completion: place the cursor inside a class declaration or one of its methods."
            .to_owned()
    })?;
    if class.end_line == class.line {
        return Err(format!(
            "Class Completion: {} is declared on a single line; split it first.",
            class.path
        ));
    }
    let lines: Vec<&str> = text.split('\n').collect();
    let cp = class.path.as_str();
    let cp_low = format!("{}.", cp.to_lowercase());
    let symbol = o.find_type(cp);
    let has_member = |name: &str| {
        symbol.is_some_and(|s| s.children.iter().any(|c| c.name.eq_ignore_ascii_case(name)))
    };
    let mut inserts: Vec<Insert> = Vec::new();
    let mut summary = Vec::new();
    let mut fields: Vec<String> = Vec::new();
    let mut method_decls: Vec<String> = Vec::new();
    // (cabecalho da implementacao, corpo)
    let mut bodies: Vec<(String, String)> = Vec::new();

    for p in o.properties.iter().filter(|p| {
        p.class_path.eq_ignore_ascii_case(cp)
            && !p.has_read
            && !p.has_write
            && !p.indexed
            && !p.ty.is_empty()
    }) {
        let field = format!("F{}", p.name);
        let setter = format!("Set{}", p.name);
        inserts.push(Insert {
            pos: pos(p.semi.0, p.semi.1),
            text: format!(" read {field} write {setter}"),
            caret: None,
        });
        if !has_member(&field) && !fields.iter().any(|f| f.starts_with(&format!("{field}:"))) {
            fields.push(format!("{field}: {};", p.ty));
        }
        if !has_member(&setter) {
            method_decls.push(format!("procedure {setter}(const Value: {});", p.ty));
            bodies.push((
                format!("procedure {cp}.{setter}(const Value: {});", p.ty),
                format!("{field} := Value;"),
            ));
        }
        summary.push(format!("property {}: read {field} write {setter}", p.name));
    }

    let decls: Vec<&MethodDecl> = o
        .decls
        .iter()
        .filter(|d| d.needs_body && d.class_path.eq_ignore_ascii_case(cp))
        .collect();
    for d in &decls {
        if implemented(d, &decls, &o.routines) {
            continue;
        }
        let before = collapse(&span(&lines, d.header.0, (d.line, d.col)));
        let mut rest = collapse(&span(&lines, (d.line, d.col), d.header.1));
        if !rest.ends_with(';') {
            rest.push(';');
        }
        let header = strip_defaults(&format!("{before} {cp}.{rest}"));
        summary.push(format!("implementation of {cp}.{}", d.name));
        bodies.push((header, String::new()));
    }

    if let Some(r) = routine {
        let declared = o
            .decls
            .iter()
            .any(|d| Some(d.key.as_str()) == r.key.as_deref());
        if !declared {
            let before = collapse(&span(&lines, r.header.0, (r.line, r.col)));
            let rest = collapse(&span(&lines, (r.line, r.col), r.header.1));
            let rest = if rest.to_lowercase().starts_with(&cp_low) {
                rest[cp_low.len()..].to_owned()
            } else {
                rest
            };
            let name = r.name.rsplit('.').next().unwrap_or(&r.name);
            summary.push(format!("declaration of {name} in the private section"));
            method_decls.push(format!("{before} {rest}"));
        }
    }

    if inserts.is_empty() && fields.is_empty() && method_decls.is_empty() && bodies.is_empty() {
        return Err(format!(
            "Class Completion: {cp} is complete (every method has an implementation)."
        ));
    }

    // campos e declaracoes na secao private (criada antes do `end` se nao existir)
    let section_indent = " ".repeat(class.end_col);
    let member_indent = " ".repeat(class.end_col + 2);
    let lines_of = |items: &[String]| -> String {
        items
            .iter()
            .map(|i| format!("{member_indent}{i}\n"))
            .collect()
    };
    let private = class
        .sections
        .iter()
        .position(|s| s.visibility == "private")
        .or_else(|| {
            class
                .sections
                .iter()
                .position(|s| s.visibility == "strict private")
        });
    match private {
        Some(k) => {
            let sec = &class.sections[k];
            if !fields.is_empty() {
                inserts.push(Insert {
                    pos: pos(sec.line + 1, 0),
                    text: lines_of(&fields),
                    caret: None,
                });
            }
            if !method_decls.is_empty() {
                let end = class.sections.get(k + 1).map_or(class.end_line, |s| s.line);
                inserts.push(Insert {
                    pos: pos(end, 0),
                    text: lines_of(&method_decls),
                    caret: None,
                });
            }
        }
        None if !fields.is_empty() || !method_decls.is_empty() => {
            let mut members = fields.clone();
            members.extend(method_decls.iter().cloned());
            inserts.push(Insert {
                pos: pos(class.end_line, 0),
                text: format!("{section_indent}private\n{}", lines_of(&members)),
                caret: None,
            });
        }
        None => {}
    }

    // corpos depois do ultimo metodo da classe (ou antes de initialization/end.)
    if !bodies.is_empty() {
        let last = o
            .routines
            .iter()
            .filter(|r| {
                r.key
                    .as_deref()
                    .is_some_and(|k| k.starts_with(&cp_low) && !k[cp_low.len()..].contains('.'))
            })
            .max_by_key(|r| r.end_line);
        let (at, lead, trail) = match last {
            Some(r) => (pos(r.end_line + 1, 0), "\n", ""),
            None => {
                let end = o.implementation_end.ok_or_else(|| {
                    "Class Completion: no implementation section to add the method bodies to."
                        .to_owned()
                })?;
                (pos(end, 0), "", "\n")
            }
        };
        let mut text = String::new();
        let mut caret = None;
        for (k, (header, body)) in bodies.iter().enumerate() {
            if k > 0 || !lead.is_empty() {
                text.push('\n');
            }
            text.push_str(header);
            text.push_str("\nbegin\n  ");
            if caret.is_none() {
                caret = Some(text.chars().count());
            }
            text.push_str(body);
            text.push_str("\nend;\n");
        }
        text.push_str(trail);
        inserts.push(Insert {
            pos: at,
            text,
            caret,
        });
    }

    // posicoes finais: cada insercao anda o tamanho das que ficam antes dela
    let mut order: Vec<usize> = (0..inserts.len()).collect();
    order.sort_by_key(|&k| (inserts[k].pos, k));
    let mut shift = 0;
    let mut caret = None;
    for &k in &order {
        if let (None, Some(off)) = (caret, inserts[k].caret) {
            caret = Some(inserts[k].pos + shift + off);
        }
        shift += inserts[k].text.chars().count();
    }
    let edits = order
        .iter()
        .rev()
        .map(|&k| Edit {
            at: inserts[k].pos,
            removed: String::new(),
            inserted: inserts[k].text.clone(),
        })
        .collect();
    Ok(Plan {
        edits,
        caret,
        summary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_removed_from_headers() {
        assert_eq!(
            strip_defaults("procedure TX.P(A: Integer = 0; const S: string = 'a;b'; B: Boolean);"),
            "procedure TX.P(A: Integer; const S: string; B: Boolean);"
        );
        assert_eq!(
            strip_defaults("function TX.F: Integer;"),
            "function TX.F: Integer;"
        );
    }
}
