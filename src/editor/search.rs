//! Busca no buffer: todas as ocorrencias, com ou sem diferenciar maiusculas e palavra inteira.

use std::ops::Range;

use ropey::Rope;

use crate::syntax::is_ident_char;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct SearchOptions {
    pub case_sensitive: bool,
    pub whole_word: bool,
}

/// Minuscula de um caractere quando ela tambem e um unico caractere (mantem os indices alinhados).
fn fold(c: char) -> char {
    let mut lower = c.to_lowercase();
    match (lower.next(), lower.next()) {
        (Some(l), None) => l,
        _ => c,
    }
}

/// Ocorrencias sem sobreposicao, em ordem, como faixas de caracteres.
pub fn find_all(rope: &Rope, query: &str, options: SearchOptions) -> Vec<Range<usize>> {
    if query.is_empty() {
        return Vec::new();
    }
    let map = |c: char| if options.case_sensitive { c } else { fold(c) };
    let needle: Vec<char> = query.chars().map(map).collect();
    let hay: Vec<char> = rope.chars().map(map).collect();
    let m = needle.len();
    let mut found = Vec::new();
    let mut i = 0;
    while i + m <= hay.len() {
        let hit = hay[i] == needle[0]
            && hay[i..i + m] == needle[..]
            && (!options.whole_word || is_word_boundary(&hay, i, i + m));
        if hit {
            found.push(i..i + m);
            i += m;
        } else {
            i += 1;
        }
    }
    found
}

fn is_word_boundary(hay: &[char], start: usize, end: usize) -> bool {
    !(start > 0 && is_ident_char(hay[start - 1])) && !(end < hay.len() && is_ident_char(hay[end]))
}

/// Indice da primeira ocorrencia que comeca em `from` ou depois (volta ao inicio do arquivo).
pub fn next_from(matches: &[Range<usize>], from: usize) -> Option<usize> {
    if matches.is_empty() {
        return None;
    }
    Some(matches.partition_point(|m| m.start < from) % matches.len())
}

/// Indice da ultima ocorrencia que comeca antes de `before` (volta ao fim do arquivo).
pub fn prev_before(matches: &[Range<usize>], before: usize) -> Option<usize> {
    if matches.is_empty() {
        return None;
    }
    let i = matches.partition_point(|m| m.start < before);
    Some(if i == 0 { matches.len() - 1 } else { i - 1 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn case_insensitive_with_accents_and_whole_word() {
        let rope = Rope::from_str("Ação acao AÇÃO açãoX\nTbTabela.Close; Tb.Close");
        let r = find_all(&rope, "ação", SearchOptions::default());
        assert_eq!(r.len(), 3);
        let r = find_all(
            &rope,
            "ação",
            SearchOptions {
                case_sensitive: false,
                whole_word: true,
            },
        );
        assert_eq!(r.len(), 2);
        let r = find_all(
            &rope,
            "Close",
            SearchOptions {
                case_sensitive: true,
                whole_word: true,
            },
        );
        assert_eq!(r.len(), 2);
        assert_eq!(rope.slice(r[0].clone()).to_string(), "Close");
    }

    #[test]
    fn navigation_wraps_around() {
        let m = vec![2..3, 10..11, 20..21];
        assert_eq!(next_from(&m, 0), Some(0));
        assert_eq!(next_from(&m, 11), Some(2));
        assert_eq!(next_from(&m, 21), Some(0));
        assert_eq!(prev_before(&m, 10), Some(0));
        assert_eq!(prev_before(&m, 2), Some(2));
        assert_eq!(next_from(&[], 0), None);
    }
}
