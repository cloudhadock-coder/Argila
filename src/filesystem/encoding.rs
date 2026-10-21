//! Deteccao e preservacao de codificacao e quebras de linha dos fontes Delphi.
//!
//! Ordem de deteccao: BOM (UTF-8, UTF-16 LE/BE) -> UTF-8 valido com caracteres nao ASCII ->
//! codificacao padrao para arquivos sem BOM. Como no Delphi, o padrao e o ANSI do sistema
//! (GetACP), nao um Windows-1252 fixo; pode ser trocado nas configuracoes ou na barra de status.
//!
//! Todo arquivo passa por um teste de ida e volta: se recodificar o texto lido nao devolver
//! exatamente os mesmos bytes, o documento e marcado como "com perda" e salvar pede confirmacao.

use encoding_rs::{Encoding, UTF_8, WINDOWS_1252};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextEncoding {
    Utf8,
    Utf16Le,
    Utf16Be,
    /// Codificacao de um byte (ou multibyte asiatica) do Windows: "ANSI" no vocabulario do Delphi.
    Legacy(&'static Encoding),
}

impl TextEncoding {
    pub fn label(self) -> String {
        match self {
            TextEncoding::Utf8 => "UTF-8".to_owned(),
            TextEncoding::Utf16Le => "UTF-16 LE".to_owned(),
            TextEncoding::Utf16Be => "UTF-16 BE".to_owned(),
            TextEncoding::Legacy(e) => pretty_name(e),
        }
    }

    fn bom(self) -> &'static [u8] {
        match self {
            TextEncoding::Utf8 => &[0xEF, 0xBB, 0xBF],
            TextEncoding::Utf16Le => &[0xFF, 0xFE],
            TextEncoding::Utf16Be => &[0xFE, 0xFF],
            TextEncoding::Legacy(_) => &[],
        }
    }

    pub fn supports_bom(self) -> bool {
        !self.bom().is_empty()
    }
}

fn pretty_name(e: &'static Encoding) -> String {
    let name = e.name();
    match name.strip_prefix("windows-") {
        Some(cp) => format!("Windows-{cp}"),
        None => name.to_owned(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineEnding {
    CrLf,
    Lf,
}

impl LineEnding {
    pub fn label(self) -> &'static str {
        match self {
            LineEnding::CrLf => "CRLF",
            LineEnding::Lf => "LF",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextFormat {
    pub encoding: TextEncoding,
    pub bom: bool,
    pub line_ending: LineEnding,
}

impl TextFormat {
    pub fn label(&self) -> String {
        if self.bom {
            format!("{} BOM", self.encoding.label())
        } else {
            self.encoding.label()
        }
    }
}

#[derive(Debug)]
pub struct Decoded {
    /// Texto com quebras normalizadas para `\n` (o formato original fica em `format`).
    pub text: String,
    pub format: TextFormat,
    /// O arquivo misturava CRLF e LF; sera salvo todo com `format.line_ending`.
    pub mixed_line_endings: bool,
    /// Recodificar devolve exatamente os bytes lidos.
    pub lossless: bool,
}

/// Caractere que nao existe na codificacao de destino.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unencodable {
    pub ch: char,
    /// Indice (em caracteres) no texto normalizado.
    pub char_index: usize,
}

/// Codificacoes oferecidas em "reabrir com"/"salvar com" (rotulos do encoding_rs).
pub const LEGACY_CHOICES: &[&str] = &[
    "windows-1252",
    "windows-1250",
    "windows-1251",
    "windows-1253",
    "windows-1254",
    "windows-1257",
    "iso-8859-15",
    "ibm866",
    "shift_jis",
    "gbk",
    "big5",
    "euc-kr",
];

pub fn encoding_for_label(label: &str) -> Option<&'static Encoding> {
    Encoding::for_label(label.trim().as_bytes())
}

/// Codificacao usada para arquivos sem BOM que nao sao UTF-8.
pub fn decode(bytes: &[u8], no_bom_default: &'static Encoding) -> Decoded {
    for enc in [
        TextEncoding::Utf8,
        TextEncoding::Utf16Le,
        TextEncoding::Utf16Be,
    ] {
        if bytes.starts_with(enc.bom()) {
            return decode_as(bytes, enc);
        }
    }
    let valid_utf8 = std::str::from_utf8(bytes).is_ok();
    let encoding = if valid_utf8 && !bytes.is_ascii() {
        TextEncoding::Utf8
    } else if no_bom_default == UTF_8 {
        // padrao UTF-8, mas o arquivo nao e UTF-8 valido: Windows-1252 nunca perde bytes
        if valid_utf8 {
            TextEncoding::Utf8
        } else {
            TextEncoding::Legacy(WINDOWS_1252)
        }
    } else {
        TextEncoding::Legacy(no_bom_default)
    };
    decode_as(bytes, encoding)
}

/// Le os bytes na codificacao indicada (usado tambem por "reabrir com codificacao").
pub fn decode_as(bytes: &[u8], encoding: TextEncoding) -> Decoded {
    let bom = encoding.supports_bom() && bytes.starts_with(encoding.bom());
    let body = if bom {
        &bytes[encoding.bom().len()..]
    } else {
        bytes
    };
    let raw = match encoding {
        TextEncoding::Utf8 => String::from_utf8_lossy(body).into_owned(),
        TextEncoding::Utf16Le => decode_utf16(body, u16::from_le_bytes),
        TextEncoding::Utf16Be => decode_utf16(body, u16::from_be_bytes),
        TextEncoding::Legacy(e) => e.decode_without_bom_handling(body).0.into_owned(),
    };
    let lossless = encode_raw(&raw, encoding, bom).is_ok_and(|b| b == bytes);
    let (text, line_ending, mixed_line_endings) = normalize_line_endings(&raw);
    Decoded {
        text,
        format: TextFormat {
            encoding,
            bom,
            line_ending,
        },
        mixed_line_endings,
        lossless,
    }
}

fn decode_utf16(body: &[u8], read: fn([u8; 2]) -> u16) -> String {
    let units = body.chunks(2).map(|c| {
        if c.len() == 2 {
            read([c[0], c[1]])
        } else {
            0xFFFD
        }
    });
    char::decode_utf16(units)
        .map(|r| r.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}

/// Troca CRLF por LF e diz qual quebra predomina. CR isolado continua como caractere comum
/// (assim "\r\r\n" volta identico ao salvar).
fn normalize_line_endings(raw: &str) -> (String, LineEnding, bool) {
    let crlf = raw.matches("\r\n").count();
    let lf = raw.matches('\n').count() - crlf;
    // sem nenhuma quebra: CRLF, o padrao dos fontes Delphi no Windows
    let ending = if crlf >= lf {
        LineEnding::CrLf
    } else {
        LineEnding::Lf
    };
    let text = if crlf > 0 {
        raw.replace("\r\n", "\n")
    } else {
        raw.to_owned()
    };
    (text, ending, crlf > 0 && lf > 0)
}

/// Converte o texto normalizado de volta para bytes no formato do arquivo.
pub fn encode(text: &str, format: &TextFormat) -> Result<Vec<u8>, Unencodable> {
    let result = match format.line_ending {
        LineEnding::CrLf => encode_raw(&text.replace('\n', "\r\n"), format.encoding, format.bom),
        LineEnding::Lf => encode_raw(text, format.encoding, format.bom),
    };
    // quebras existem em toda codificacao: o primeiro caractere invalido e o mesmo no texto
    // normalizado, onde o indice faz sentido para o editor
    result.map_err(|e| match format.encoding {
        TextEncoding::Legacy(enc) => first_unencodable(text, enc),
        _ => e,
    })
}

fn encode_raw(text: &str, encoding: TextEncoding, bom: bool) -> Result<Vec<u8>, Unencodable> {
    let mut out = Vec::with_capacity(text.len() + 3);
    if bom {
        out.extend_from_slice(encoding.bom());
    }
    match encoding {
        TextEncoding::Utf8 => out.extend_from_slice(text.as_bytes()),
        TextEncoding::Utf16Le => text
            .encode_utf16()
            .for_each(|u| out.extend_from_slice(&u.to_le_bytes())),
        TextEncoding::Utf16Be => text
            .encode_utf16()
            .for_each(|u| out.extend_from_slice(&u.to_be_bytes())),
        TextEncoding::Legacy(e) => {
            let (bytes, _, had_errors) = e.encode(text);
            if had_errors {
                return Err(first_unencodable(text, e));
            }
            out.extend_from_slice(&bytes);
        }
    }
    Ok(out)
}

fn first_unencodable(text: &str, e: &'static Encoding) -> Unencodable {
    let mut buf = [0u8; 4];
    for (char_index, ch) in text.chars().enumerate() {
        if e.encode(ch.encode_utf8(&mut buf)).2 {
            return Unencodable { ch, char_index };
        }
    }
    unreachable!("encoding_rs relatou erro mas todos os caracteres codificam")
}

/// Assinatura de .dfm binario (TPF0) ou bytes nulos sem BOM UTF-16: nao abrir como texto.
pub fn binary_kind(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"TPF0")
        || (bytes.starts_with(&[0xFF, 0x0A]) && bytes.windows(4).take(64).any(|w| w == b"TPF0"))
    {
        return Some("binary DFM (TPF0)");
    }
    let utf16 = bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]);
    if !utf16 && bytes.iter().take(8192).any(|b| *b == 0) {
        return Some("binary file");
    }
    None
}

/// Codificacao ANSI do sistema (a mesma que o Delphi usa para fontes sem BOM).
pub fn system_ansi_encoding() -> &'static Encoding {
    codepage_to_encoding(system_codepage()).unwrap_or(WINDOWS_1252)
}

#[cfg(windows)]
fn system_codepage() -> u32 {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        safe fn GetACP() -> u32;
    }
    GetACP()
}

#[cfg(not(windows))]
fn system_codepage() -> u32 {
    1252
}

pub fn codepage_to_encoding(cp: u32) -> Option<&'static Encoding> {
    let label = match cp {
        874 => "windows-874".to_owned(),
        932 => "shift_jis".to_owned(),
        936 => "gbk".to_owned(),
        949 => "euc-kr".to_owned(),
        950 => "big5".to_owned(),
        1250..=1258 => format!("windows-{cp}"),
        866 => "ibm866".to_owned(),
        65001 => "utf-8".to_owned(),
        _ => return None,
    };
    encoding_for_label(&label)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CP1252: TextEncoding = TextEncoding::Legacy(WINDOWS_1252);

    #[test]
    fn every_byte_roundtrips_in_windows_1252() {
        let bytes: Vec<u8> = (0u8..=255).filter(|b| *b != b'\r' && *b != b'\n').collect();
        let d = decode_as(&bytes, CP1252);
        assert!(d.lossless);
        assert_eq!(encode(&d.text, &d.format).unwrap(), bytes);
    }

    #[test]
    fn ansi_without_bom_uses_the_configured_codepage() {
        // 0xB9 e 'ą' em Windows-1250 e '¹' em Windows-1252: nao presumir 1252
        let bytes = b"x := '\xB9';\r\n";
        let cp1250 = encoding_for_label("windows-1250").unwrap();
        assert_eq!(decode(bytes, cp1250).text, "x := 'ą';\n");
        assert_eq!(decode(bytes, WINDOWS_1252).text, "x := '¹';\n");
    }

    #[test]
    fn utf8_with_accents_is_detected_without_bom() {
        let d = decode("Função := 'ação';\r\n".as_bytes(), WINDOWS_1252);
        assert_eq!(d.format.encoding, TextEncoding::Utf8);
        assert!(!d.format.bom && d.lossless);
    }

    #[test]
    fn ascii_only_file_stays_ansi_so_new_accents_do_not_become_utf8() {
        let d = decode(b"unit A;\r\n", WINDOWS_1252);
        assert_eq!(d.format.encoding, CP1252);
        let out = encode("unit Ação;\n", &d.format).unwrap();
        assert_eq!(out, b"unit A\xE7\xE3o;\r\n");
    }

    #[test]
    fn boms_are_detected_and_kept() {
        for (bytes, enc) in [
            (
                vec![0xEF, 0xBB, 0xBF, b'a', b'\r', b'\n'],
                TextEncoding::Utf8,
            ),
            (vec![0xFF, 0xFE, b'a', 0, 0xE7, 0], TextEncoding::Utf16Le),
            (vec![0xFE, 0xFF, 0, b'a', 0, 0xE7], TextEncoding::Utf16Be),
        ] {
            let d = decode(&bytes, WINDOWS_1252);
            assert_eq!(d.format.encoding, enc);
            assert!(d.format.bom && d.lossless, "{enc:?}");
            assert_eq!(encode(&d.text, &d.format).unwrap(), bytes);
        }
    }

    #[test]
    fn line_endings_are_preserved_or_reported() {
        let crlf = decode(b"a\r\nb\r\n", WINDOWS_1252);
        assert_eq!(
            (
                crlf.text.as_str(),
                crlf.format.line_ending,
                crlf.mixed_line_endings
            ),
            ("a\nb\n", LineEnding::CrLf, false)
        );
        let lf = decode(b"a\nb", WINDOWS_1252);
        assert_eq!(lf.format.line_ending, LineEnding::Lf);
        assert_eq!(encode(&lf.text, &lf.format).unwrap(), b"a\nb");
        let mixed = decode(b"a\r\nb\r\nc\n", WINDOWS_1252);
        assert!(mixed.mixed_line_endings);
        assert_eq!(
            encode(&mixed.text, &mixed.format).unwrap(),
            b"a\r\nb\r\nc\r\n"
        );
        let lone_cr = decode(b"a\r\r\nb", WINDOWS_1252);
        assert_eq!(encode(&lone_cr.text, &lone_cr.format).unwrap(), b"a\r\r\nb");
    }

    #[test]
    fn unencodable_characters_are_reported_with_position() {
        let format = TextFormat {
            encoding: CP1252,
            bom: false,
            line_ending: LineEnding::CrLf,
        };
        let err = encode("ok\nőx", &format).unwrap_err();
        assert_eq!(
            err,
            Unencodable {
                ch: 'ő',
                char_index: 3
            }
        );
    }

    #[test]
    fn invalid_utf8_is_marked_lossy_when_forced() {
        let d = decode_as(b"caf\xE9", TextEncoding::Utf8);
        assert!(!d.lossless);
        assert!(decode(b"caf\xE9", WINDOWS_1252).lossless);
    }

    #[test]
    fn binary_files_are_recognized() {
        assert!(binary_kind(b"TPF0\x0cTForm").is_some());
        assert!(binary_kind(b"abc\0def").is_some());
        assert!(binary_kind(&[0xFF, 0xFE, b'a', 0]).is_none());
        assert!(binary_kind(b"object Form1: TForm1\r\nend\r\n").is_none());
    }
}
