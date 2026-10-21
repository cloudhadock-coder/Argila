//! Operacoes de edicao independentes da interface (testaveis sem janela).

use std::ops::Range;

use super::buffer::{Edit, EditKind, Selection, TextBuffer};
use crate::syntax::{Language, is_ident_char, line_chars};

/// Coluna visual (tabs expandidos) do caractere `col` da linha.
pub fn vcol_of(line: &[char], col: usize, tab: usize) -> usize {
    line.iter()
        .take(col)
        .fold(0, |v, c| if *c == '\t' { v + tab - v % tab } else { v + 1 })
}

/// Caractere mais proximo da coluna visual `vcol`.
pub fn col_of_vcol(line: &[char], vcol: usize, tab: usize) -> usize {
    let mut v = 0;
    for (i, c) in line.iter().enumerate() {
        let w = if *c == '\t' { tab - v % tab } else { 1 };
        if vcol < v + w.div_ceil(2) {
            return i;
        }
        v += w;
        if vcol < v {
            return i + 1;
        }
    }
    line.len()
}

#[derive(PartialEq)]
enum Class {
    Word,
    Space,
    Newline,
    Other,
}

fn class(c: char) -> Class {
    if c == '\n' {
        Class::Newline
    } else if c.is_whitespace() {
        Class::Space
    } else if is_ident_char(c) {
        Class::Word
    } else {
        Class::Other
    }
}

/// Inicio da palavra anterior (Ctrl+Esquerda / Ctrl+Backspace).
pub fn word_left(buf: &TextBuffer, pos: usize) -> usize {
    let mut p = pos;
    while p > 0 && class(buf.rope().char(p - 1)) == Class::Space {
        p -= 1;
    }
    if p > 0 {
        let k = class(buf.rope().char(p - 1));
        if k == Class::Newline {
            return p - 1;
        }
        while p > 0 && class(buf.rope().char(p - 1)) == k {
            p -= 1;
        }
    }
    p
}

/// Inicio da proxima palavra (Ctrl+Direita / Ctrl+Delete).
pub fn word_right(buf: &TextBuffer, pos: usize) -> usize {
    let len = buf.len_chars();
    let mut p = pos;
    if p < len {
        let k = class(buf.rope().char(p));
        if k == Class::Newline {
            return p + 1;
        }
        while p < len && class(buf.rope().char(p)) == k {
            p += 1;
        }
    }
    while p < len && class(buf.rope().char(p)) == Class::Space {
        p += 1;
    }
    p
}

/// Palavra sob o cursor (duplo clique).
pub fn word_at(buf: &TextBuffer, pos: usize) -> Range<usize> {
    let rope = buf.rope();
    let len = rope.len_chars();
    let probe = if pos < len && is_ident_char(rope.char(pos)) {
        pos
    } else if pos > 0 && is_ident_char(rope.char(pos - 1)) {
        pos - 1
    } else {
        return pos..(pos + 1).min(len);
    };
    let mut s = probe;
    while s > 0 && is_ident_char(rope.char(s - 1)) {
        s -= 1;
    }
    let mut e = probe;
    while e < len && is_ident_char(rope.char(e)) {
        e += 1;
    }
    s..e
}

/// Linha inteira, com a quebra (triplo clique).
pub fn line_range(buf: &TextBuffer, line: usize) -> Range<usize> {
    let start = buf.line_start(line);
    let end = if line + 1 < buf.line_count() {
        buf.line_start(line + 1)
    } else {
        buf.len_chars()
    };
    start..end
}

/// Linhas tocadas pela selecao; uma selecao que termina na coluna 0 nao inclui essa linha.
pub fn selected_lines(buf: &TextBuffer) -> Range<usize> {
    let r = buf.selection.range();
    let first = buf.line_of(r.start);
    let (mut last, col) = buf.line_col(r.end);
    if last > first && col == 0 {
        last -= 1;
    }
    first..last + 1
}

/// Enter com indentacao automatica: repete a indentacao da linha e acrescenta um nivel depois
/// de palavras que abrem bloco (`begin`, `try`, `then`, `var`...).
pub fn newline(buf: &mut TextBuffer, language: Language, tab: usize, now: f64) {
    let pos = buf.selection.range().start;
    let (line, col) = buf.line_col(pos);
    let chars = line_chars(buf.rope(), line);
    let indent: String = chars
        .iter()
        .take(col)
        .take_while(|c| **c == ' ' || **c == '\t')
        .collect();
    let mut text = format!("\n{indent}");
    if language == Language::Pascal && opens_block(&chars[..col.min(chars.len())]) {
        text.push_str(&" ".repeat(tab));
    }
    buf.insert(&text, EditKind::Other, now);
}

fn opens_block(before: &[char]) -> bool {
    let code: String = before.iter().collect();
    let code = code
        .split("//")
        .next()
        .unwrap_or_default()
        .trim_end()
        .to_ascii_lowercase();
    let word: String = code
        .chars()
        .rev()
        .take_while(|c| is_ident_char(*c))
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    const OPENERS: &[&str] = &[
        "begin",
        "try",
        "repeat",
        "record",
        "except",
        "finally",
        "of",
        "then",
        "do",
        "else",
        "var",
        "const",
        "type",
        "uses",
        "private",
        "protected",
        "public",
        "published",
        "class",
    ];
    OPENERS.contains(&word.as_str())
}

/// Backspace. Dentro da indentacao volta ate a parada de tabulacao anterior (como o Delphi).
pub fn backspace(buf: &mut TextBuffer, word: bool, tab: usize, now: f64) {
    let sel = buf.selection;
    if !sel.is_empty() {
        buf.insert("", EditKind::Other, now);
        return;
    }
    let pos = sel.head;
    if pos == 0 {
        return;
    }
    let start = if word {
        word_left(buf, pos)
    } else {
        let (line, col) = buf.line_col(pos);
        let chars = line_chars(buf.rope(), line);
        if col > 0 && chars[..col].iter().all(|c| *c == ' ') {
            pos - ((col - 1) % tab + 1)
        } else {
            pos - 1
        }
    };
    buf.replace(start..pos, "", EditKind::Deleting, now);
}

pub fn delete_forward(buf: &mut TextBuffer, word: bool, now: f64) {
    let sel = buf.selection;
    if !sel.is_empty() {
        buf.insert("", EditKind::Other, now);
        return;
    }
    let pos = sel.head;
    if pos >= buf.len_chars() {
        return;
    }
    let end = if word {
        word_right(buf, pos).max(pos + 1)
    } else {
        pos + 1
    };
    buf.replace(pos..end, "", EditKind::Deleting, now);
    buf.set_selection(Selection::caret(pos));
}

/// Tab sem selecao de varias linhas: espacos ate a proxima parada.
pub fn insert_tab(buf: &mut TextBuffer, tab: usize, now: f64) {
    let (line, col) = buf.line_col(buf.selection.range().start);
    let v = vcol_of(&line_chars(buf.rope(), line), col, tab);
    buf.insert(&" ".repeat(tab - v % tab), EditKind::Typing, now);
}

/// Aplica `change` a cada linha selecionada (de baixo para cima) e reajusta a selecao.
/// `change` devolve (coluna, texto removido, texto inserido) ou None para nao mexer na linha.
fn per_line(
    buf: &mut TextBuffer,
    now: f64,
    change: impl Fn(&[char]) -> Option<(usize, String, String)>,
) -> Vec<(usize, isize)> {
    let lines = selected_lines(buf);
    let mut edits = Vec::new();
    let mut deltas = Vec::new();
    for line in lines.clone().rev() {
        if let Some((col, removed, inserted)) = change(&line_chars(buf.rope(), line)) {
            let delta = inserted.chars().count() as isize - removed.chars().count() as isize;
            deltas.push((line, delta));
            edits.push(Edit {
                at: buf.line_start(line) + col,
                removed,
                inserted,
            });
        }
    }
    let total: isize = deltas.iter().map(|d| d.1).sum();
    let sel = buf.selection;
    let after = if sel.is_empty() {
        let (line, col) = buf.line_col(sel.head);
        let d = deltas.iter().find(|d| d.0 == line).map_or(0, |d| d.1);
        Selection::caret(buf.line_start(line) + (col as isize + d).max(0) as usize)
    } else {
        let start = buf.line_start(lines.start);
        let end = buf.line_start(lines.end - 1) + buf.line_len(lines.end - 1);
        Selection::new(start, (end as isize + total) as usize)
    };
    buf.transact(edits, after, EditKind::Other, now);
    deltas
}

/// Indenta as linhas selecionadas (Ctrl+Shift+I / Tab com selecao).
pub fn indent(buf: &mut TextBuffer, tab: usize, now: f64) {
    per_line(buf, now, |l| {
        (!l.is_empty()).then(|| (0, String::new(), " ".repeat(tab)))
    });
}

/// Recua as linhas selecionadas (Ctrl+Shift+U / Shift+Tab).
pub fn unindent(buf: &mut TextBuffer, tab: usize, now: f64) {
    per_line(buf, now, |l| {
        let n = if l.first() == Some(&'\t') {
            1
        } else {
            l.iter().take(tab).take_while(|c| **c == ' ').count()
        };
        (n > 0).then(|| (0, l[..n].iter().collect(), String::new()))
    });
}

/// Ctrl+/: comenta ou descomenta as linhas com `//` na coluna 1 (como o Delphi).
/// Sem selecao, o cursor desce uma linha para permitir repetir.
pub fn toggle_comment(buf: &mut TextBuffer, now: f64) {
    let lines = selected_lines(buf);
    let all_commented = lines.clone().all(|l| {
        let c = line_chars(buf.rope(), l);
        let t: Vec<&char> = c.iter().skip_while(|c| c.is_whitespace()).collect();
        t.is_empty() || (t.len() >= 2 && *t[0] == '/' && *t[1] == '/')
    });
    let single = buf.selection.is_empty();
    per_line(buf, now, |l| {
        let lead = l.iter().take_while(|c| c.is_whitespace()).count();
        if all_commented {
            l[lead..]
                .starts_with(&['/', '/'])
                .then(|| (lead, "//".to_owned(), String::new()))
        } else {
            (lead < l.len()).then(|| (0, String::new(), "//".to_owned()))
        }
    });
    if single && lines.end < buf.line_count() {
        let (line, col) = buf.line_col(buf.selection.head);
        buf.set_selection(Selection::caret(buf.pos(line + 1, col)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buf(text: &str, caret: usize) -> TextBuffer {
        let mut b = TextBuffer::new(text);
        b.set_selection(Selection::caret(caret));
        b
    }

    #[test]
    fn enter_keeps_indent_and_indents_after_block_openers() {
        let mut b = buf("  if X then", 11);
        newline(&mut b, Language::Pascal, 2, 0.0);
        assert_eq!(b.text(), "  if X then\n    ");
        let mut b = buf("  Y := 1; // begin", 18);
        newline(&mut b, Language::Pascal, 2, 0.0);
        assert_eq!(b.text(), "  Y := 1; // begin\n  ");
        let mut b = buf("  object A: TForm", 17);
        newline(&mut b, Language::Dfm, 2, 0.0);
        assert_eq!(b.text(), "  object A: TForm\n  ");
    }

    #[test]
    fn backspace_unindents_to_tab_stop() {
        let mut b = buf("x\n     y", 7);
        backspace(&mut b, false, 2, 0.0);
        assert_eq!(b.text(), "x\n    y");
        let mut b = buf("ab", 2);
        backspace(&mut b, false, 2, 0.0);
        assert_eq!(b.text(), "a");
    }

    #[test]
    fn block_indent_unindent_and_comment() {
        let mut b = TextBuffer::new("a\n\nb\nc");
        b.set_selection(Selection::new(0, 5)); // a..b (termina antes de c)
        indent(&mut b, 2, 0.0);
        assert_eq!(b.text(), "  a\n\n  b\nc");
        assert_eq!(b.slice(b.selection.range()), "  a\n\n  b");
        unindent(&mut b, 2, 1.0);
        assert_eq!(b.text(), "a\n\nb\nc");
        toggle_comment(&mut b, 2.0);
        assert_eq!(b.text(), "//a\n\n//b\nc");
        toggle_comment(&mut b, 3.0);
        assert_eq!(b.text(), "a\n\nb\nc");
        b.undo();
        assert_eq!(b.text(), "//a\n\n//b\nc");
    }

    #[test]
    fn comment_without_selection_moves_down() {
        let mut b = buf("  x;\n  y;", 3);
        toggle_comment(&mut b, 0.0);
        assert_eq!(b.text(), "//  x;\n  y;");
        assert_eq!(b.line_col(b.selection.head).0, 1);
    }

    #[test]
    fn words_and_visual_columns() {
        let b = TextBuffer::new("Tb.Close;  x");
        assert_eq!(word_right(&b, 0), 2);
        assert_eq!(word_right(&b, 3), 8);
        assert_eq!(word_left(&b, 12), 11);
        assert_eq!(word_at(&b, 4), 3..8);
        let line: Vec<char> = "\tab".chars().collect();
        assert_eq!(vcol_of(&line, 1, 4), 4);
        assert_eq!(col_of_vcol(&line, 5, 4), 2);
        assert_eq!(col_of_vcol(&line, 1, 4), 0);
        assert_eq!(col_of_vcol(&line, 3, 4), 1);
    }
}
