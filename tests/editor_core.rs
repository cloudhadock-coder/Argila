//! Testes de integracao da Fase 2: abrir, editar, salvar e reabrir sem perder nada.

use std::fs;
use std::time::Instant;

use argila::editor::buffer::{EditKind, Selection};
use argila::editor::document::{Document, OpenError, SaveError, SaveOptions};
use argila::filesystem::encoding::{LineEnding, TextEncoding, TextFormat, encoding_for_label};
use encoding_rs::WINDOWS_1252;

fn cp1252(s: &str) -> Vec<u8> {
    WINDOWS_1252.encode(s).0.into_owned()
}

/// Insere `text` no fim da linha `line` (base 0) e salva.
fn append_to_line(doc: &mut Document, line: usize, text: &str) {
    let pos = doc.buffer.pos(line, usize::MAX);
    doc.buffer.set_selection(Selection::caret(pos));
    doc.buffer.insert(text, EditKind::Other, 0.0);
    doc.save(SaveOptions::default()).unwrap();
}

#[test]
fn ansi_crlf_file_keeps_accents_bytes_and_line_endings() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Unit_Acentos.pas");
    let original = "unit Unit_Acentos;\r\n\r\n// Função de cálculo\r\nconst Msg = 'Atenção: não há dados';\r\n";
    fs::write(&path, cp1252(original)).unwrap();

    let mut doc = Document::open(&path, WINDOWS_1252).unwrap();
    assert_eq!(doc.format.encoding, TextEncoding::Legacy(WINDOWS_1252));
    assert_eq!(doc.format.line_ending, LineEnding::CrLf);
    assert!(doc.lossless);
    assert!(doc.buffer.text().contains("Função de cálculo"));

    append_to_line(&mut doc, 2, " e conversão");
    let expected = original.replace("cálculo", "cálculo e conversão");
    assert_eq!(
        fs::read(&path).unwrap(),
        cp1252(&expected),
        "bytes no disco devem ser Windows-1252 com CRLF"
    );

    let reopened = Document::open(&path, WINDOWS_1252).unwrap();
    assert_eq!(reopened.buffer.text(), expected.replace("\r\n", "\n"));
    assert_eq!(reopened.format, doc.format);
    assert!(!reopened.is_modified());
}

#[test]
fn utf8_bom_and_lf_are_preserved() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Unit_Utf8.pas");
    let original = "\u{FEFF}unit U;\n// coração ♥ 日本\n";
    fs::write(&path, original.as_bytes()).unwrap();

    let mut doc = Document::open(&path, WINDOWS_1252).unwrap();
    assert_eq!(
        (doc.format.encoding, doc.format.bom, doc.format.line_ending),
        (TextEncoding::Utf8, true, LineEnding::Lf)
    );
    append_to_line(&mut doc, 1, " ação");
    let bytes = fs::read(&path).unwrap();
    assert_eq!(
        bytes,
        "\u{FEFF}unit U;\n// coração ♥ 日本 ação\n".as_bytes()
    );
}

#[test]
fn other_ansi_codepages_are_not_assumed_to_be_1252() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Polski.pas");
    let cp1250 = encoding_for_label("windows-1250").unwrap();
    let text = "// Zażółć gęślą jaźń\r\n";
    fs::write(&path, cp1250.encode(text).0).unwrap();

    let mut doc = Document::open(&path, WINDOWS_1252).unwrap();
    assert_ne!(
        doc.buffer.text(),
        text.replace("\r\n", "\n"),
        "lido como 1252 o texto fica diferente"
    );
    doc.reopen_as(TextEncoding::Legacy(cp1250)).unwrap();
    assert_eq!(doc.buffer.text(), text.replace("\r\n", "\n"));
    append_to_line(&mut doc, 0, " ok");
    assert_eq!(
        fs::read(&path).unwrap(),
        cp1250.encode("// Zażółć gęślą jaźń ok\r\n").0.as_ref()
    );
}

#[test]
fn unencodable_character_blocks_save_until_format_changes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("A.pas");
    fs::write(&path, b"x := 1;\r\n").unwrap();
    let mut doc = Document::open(&path, WINDOWS_1252).unwrap();
    doc.buffer
        .set_selection(Selection::caret(doc.buffer.len_chars()));
    doc.buffer.insert("s := '€ ő';", EditKind::Other, 0.0);

    match doc.save(SaveOptions::default()) {
        Err(SaveError::Unencodable {
            ch: 'ő',
            line: 2,
            col: 9,
        }) => {}
        other => panic!("esperava erro de codificacao, veio {other:?}"),
    }
    assert_eq!(
        fs::read(&path).unwrap(),
        b"x := 1;\r\n",
        "arquivo original intacto"
    );

    doc.set_format(TextFormat {
        encoding: TextEncoding::Utf8,
        bom: true,
        line_ending: LineEnding::CrLf,
    });
    doc.save(SaveOptions::default()).unwrap();
    let reopened = Document::open(&path, WINDOWS_1252).unwrap();
    assert_eq!(
        (reopened.format.encoding, reopened.format.bom),
        (TextEncoding::Utf8, true)
    );
    assert_eq!(reopened.buffer.text(), "x := 1;\ns := '€ ő';");
}

#[test]
fn external_changes_are_not_overwritten_silently() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("B.pas");
    fs::write(&path, b"a\r\n").unwrap();
    let mut doc = Document::open(&path, WINDOWS_1252).unwrap();
    fs::write(&path, b"alterado por outro programa, maior\r\n").unwrap();
    doc.buffer.insert("x", EditKind::Other, 0.0);
    assert!(matches!(
        doc.save(SaveOptions::default()),
        Err(SaveError::ChangedOnDisk)
    ));
    doc.save(SaveOptions {
        overwrite_external_changes: true,
        ..Default::default()
    })
    .unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"xa\r\n");
}

#[test]
fn binary_dfm_and_binary_files_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let dfm = dir.path().join("Bin.dfm");
    fs::write(&dfm, b"TPF0\x06TForm1\x05Form1").unwrap();
    assert!(matches!(
        Document::open(&dfm, WINDOWS_1252),
        Err(OpenError::Binary(_))
    ));
    let exe = dir.path().join("x.pas");
    fs::write(&exe, b"MZ\x90\x00\x03").unwrap();
    assert!(matches!(
        Document::open(&exe, WINDOWS_1252),
        Err(OpenError::Binary(_))
    ));
}

#[test]
fn ten_thousand_line_unit_opens_edits_and_roundtrips() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Unit_Grande.pas");
    let mut src = String::from("unit Unit_Grande;\r\n\r\ninterface\r\n\r\nimplementation\r\n\r\n");
    for i in 0..2000 {
        src.push_str(&format!(
            "procedure TForm1.Botao{i}Click(Sender: TObject);\r\nbegin\r\n  ShowMessage('Operação {i} concluída');\r\nend;\r\n\r\n"
        ));
    }
    src.push_str("end.\r\n");
    fs::write(&path, cp1252(&src)).unwrap();

    let t = Instant::now();
    let mut doc = Document::open(&path, WINDOWS_1252).unwrap();
    let open_time = t.elapsed();
    assert!(doc.buffer.line_count() > 10_000);

    let t = Instant::now();
    let mut now = 0.0;
    for k in 0..500 {
        let line = 9000 - k * 7;
        doc.buffer
            .set_selection(Selection::caret(doc.buffer.pos(line, 2)));
        now += 0.01;
        doc.buffer.insert("ç", EditKind::Typing, now);
    }
    for _ in 0..500 {
        doc.buffer.undo();
    }
    let edit_time = t.elapsed();
    assert_eq!(doc.buffer.text(), src.replace("\r\n", "\n"));
    assert!(!doc.is_modified());

    doc.buffer
        .set_selection(Selection::caret(doc.buffer.pos(5000, 0)));
    doc.buffer
        .insert("// meio do arquivo\n", EditKind::Other, 99.0);
    let t = Instant::now();
    doc.save(SaveOptions::default()).unwrap();
    let save_time = t.elapsed();
    let reopened = Document::open(&path, WINDOWS_1252).unwrap();
    assert_eq!(reopened.buffer.text(), doc.buffer.text());
    assert_eq!(reopened.format, doc.format);
    eprintln!(
        "10k linhas: abrir {open_time:?}, 500 edicoes + 500 desfazer {edit_time:?}, salvar {save_time:?}"
    );
    assert!(
        open_time.as_secs_f64() < 2.0
            && edit_time.as_secs_f64() < 2.0
            && save_time.as_secs_f64() < 2.0
    );
}
