//! Arquivo aberto no editor: buffer + formato original (codificacao, BOM, quebra de linha).
//!
//! Salvar nunca perde nada em silencio: caractere que nao existe na codificacao, arquivo
//! alterado por outro programa ou arquivo lido com perda viram erros que a IDE pergunta ao usuario.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use encoding_rs::Encoding;

use super::buffer::TextBuffer;
use crate::filesystem::atomic_write;
use crate::filesystem::encoding::{self, Decoded, TextEncoding, TextFormat};
use crate::syntax::Language;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileStamp {
    modified: Option<SystemTime>,
    len: u64,
}

impl FileStamp {
    fn of(path: &Path) -> io::Result<Self> {
        let meta = fs::metadata(path)?;
        Ok(Self {
            modified: meta.modified().ok(),
            len: meta.len(),
        })
    }
}

#[derive(Debug)]
pub enum OpenError {
    Io(io::Error),
    Binary(&'static str),
    TooLarge(u64),
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpenError::Io(e) => write!(f, "{e}"),
            OpenError::Binary(kind) => write!(f, "{kind}: cannot be opened as text"),
            OpenError::TooLarge(n) => write!(f, "file too large ({} MB)", n / (1024 * 1024)),
        }
    }
}

#[derive(Debug)]
pub enum SaveError {
    /// Caractere sem representacao na codificacao do arquivo (linha/coluna base 1).
    Unencodable {
        ch: char,
        line: usize,
        col: usize,
    },
    /// O arquivo mudou no disco desde que foi aberto/salvo.
    ChangedOnDisk,
    /// O arquivo foi lido com bytes substituidos; salvar grava a versao substituida.
    Lossy,
    Io(io::Error),
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SaveError::Unencodable { ch, line, col } => {
                write!(
                    f,
                    "'{ch}' (U+{:04X}) at line {line}, column {col} cannot be represented",
                    *ch as u32
                )
            }
            SaveError::ChangedOnDisk => {
                write!(f, "the file was changed on disk by another program")
            }
            SaveError::Lossy => write!(f, "the file was read with replaced bytes"),
            SaveError::Io(e) => write!(f, "{e}"),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SaveOptions {
    pub overwrite_external_changes: bool,
    pub accept_lossy: bool,
}

#[derive(Debug)]
pub struct Document {
    pub path: PathBuf,
    pub buffer: TextBuffer,
    pub format: TextFormat,
    pub language: Language,
    pub mixed_line_endings: bool,
    pub lossless: bool,
    format_changed: bool,
    stamp: Option<FileStamp>,
}

impl Document {
    pub const MAX_BYTES: u64 = 64 * 1024 * 1024;

    /// `no_bom_default`: codificacao para arquivos sem BOM que nao sao UTF-8 (ANSI do sistema).
    pub fn open(path: &Path, no_bom_default: &'static Encoding) -> Result<Self, OpenError> {
        let bytes = read_text_bytes(path)?;
        Ok(Self::from_decoded(
            path,
            encoding::decode(&bytes, no_bom_default),
        ))
    }

    /// Le de novo o arquivo interpretando-o com outra codificacao (descarta o buffer atual).
    pub fn reopen_as(&mut self, encoding: TextEncoding) -> Result<(), OpenError> {
        let bytes = read_text_bytes(&self.path)?;
        *self = Self::from_decoded(&self.path.clone(), encoding::decode_as(&bytes, encoding));
        Ok(())
    }

    fn from_decoded(path: &Path, d: Decoded) -> Self {
        Self {
            path: path.to_path_buf(),
            buffer: TextBuffer::new(&d.text),
            format: d.format,
            language: Language::from_path(path),
            mixed_line_endings: d.mixed_line_endings,
            lossless: d.lossless,
            format_changed: false,
            stamp: FileStamp::of(path).ok(),
        }
    }

    pub fn file_name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    pub fn is_modified(&self) -> bool {
        self.buffer.is_modified() || self.format_changed
    }

    /// Muda a codificacao/quebra usada ao salvar (conta como alteracao nao salva).
    pub fn set_format(&mut self, format: TextFormat) {
        if format != self.format {
            self.format = format;
            self.format_changed = true;
        }
    }

    pub fn save(&mut self, options: SaveOptions) -> Result<(), SaveError> {
        if !self.lossless && !options.accept_lossy {
            return Err(SaveError::Lossy);
        }
        let text = self.buffer.text();
        let bytes = encoding::encode(&text, &self.format).map_err(|u| {
            let (line, col) = self.buffer.line_col(u.char_index);
            SaveError::Unencodable {
                ch: u.ch,
                line: line + 1,
                col: col + 1,
            }
        })?;
        if !options.overwrite_external_changes {
            if let (Some(known), Ok(current)) = (self.stamp, FileStamp::of(&self.path)) {
                if known != current {
                    return Err(SaveError::ChangedOnDisk);
                }
            }
        }
        atomic_write(&self.path, &bytes).map_err(SaveError::Io)?;
        self.stamp = FileStamp::of(&self.path).ok();
        self.buffer.mark_saved();
        self.format_changed = false;
        self.lossless = true;
        self.mixed_line_endings = false;
        Ok(())
    }
}

fn read_text_bytes(path: &Path) -> Result<Vec<u8>, OpenError> {
    let len = fs::metadata(path).map_err(OpenError::Io)?.len();
    if len > Document::MAX_BYTES {
        return Err(OpenError::TooLarge(len));
    }
    let bytes = fs::read(path).map_err(OpenError::Io)?;
    match encoding::binary_kind(&bytes) {
        Some(kind) => Err(OpenError::Binary(kind)),
        None => Ok(bytes),
    }
}
