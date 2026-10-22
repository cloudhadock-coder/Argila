//! Fase 5: autocompletar, Class Completion, indice de simbolos, Find in Files, folding,
//! historico de navegacao e atalhos em sequencia (Shift+Ctrl+K, T).

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use argila::autocomplete::{self, ItemKind, World, class_completion, context};
use argila::editor::buffer::{EditKind, Selection, TextBuffer};
use argila::navigation::find_in_files::{self, FindOptions, Progress, Request};
use argila::navigation::{History, Location, SymbolIndex};
use argila::projects::outline;

const BASE: &str = "unit UnitBase;

interface

uses
  System.Classes;

type
  TModo = (mNovo, mEditar, mExcluir);

  TBase = class(TObject)
  private
    FNome: string;
  public
    Itens: TStringList;
    function Calcula(A: Integer): Integer; virtual;
    property Nome: string read FNome write FNome;
  end;

const
  VERSAO = 3;

var
  Global: TBase;

function Soma(A, B: Integer): Integer;

implementation

function TBase.Calcula(A: Integer): Integer;
begin
  Result := A;
end;

function Soma(A, B: Integer): Integer;
begin
  Result := A + B;
end;

end.
";

const TELA: &str = "unit UnitTela;

interface

uses
  UnitBase;

type
  TCliente = class(TBase)
  private
    FIdade: Integer;
  public
    Endereco: string;
    procedure Salvar(Forcar: Boolean = False);
    function Valida: Boolean;
    procedure Imprime(const Titulo: string); overload;
    procedure Imprime(Copias: Integer); overload;
    procedure Abstrato; virtual; abstract;
    property Codigo: Integer;
    property Idade: Integer read FIdade;
  end;

implementation

procedure TCliente.Salvar(Forcar: Boolean);
var
  Total, I: Integer;
  Outro: TCliente;
begin
  var Lista := TCliente.Create;
  Total := 0; // marca
end;

procedure TCliente.Imprime(const Titulo: string);
begin
end;

procedure TCliente.Extra(N: Integer);
begin
end;

end.
";

fn line_of(text: &str, needle: &str) -> usize {
    text.lines()
        .position(|l| l.contains(needle))
        .unwrap_or_else(|| panic!("{needle} not found"))
}

fn project(dir: &Path) -> (PathBuf, PathBuf, SymbolIndex) {
    let base = dir.join("UnitBase.pas");
    let tela = dir.join("UnitTela.pas");
    std::fs::write(&base, BASE).unwrap();
    std::fs::write(&tela, TELA).unwrap();
    let index = SymbolIndex::build(&[base.clone(), tela.clone()], None, &AtomicBool::new(false));
    (base, tela, index)
}

/// Completa `text` com a linha `marker` trocada por `typed` (o cursor fica no fim de `typed`).
fn complete_at(index: &SymbolIndex, typed: &str) -> autocomplete::Completion {
    let text = TELA.replace("  Total := 0; // marca", typed);
    let line = line_of(TELA, "// marca");
    let col = typed.chars().count();
    let o = outline::pascal(&text);
    let cx = context::analyze(&text, &o, line, col);
    let world = World::new(&o, Some(index), "UnitTela");
    autocomplete::Engine::default().complete(cx, &world, &[])
}

fn labels(c: &autocomplete::Completion) -> Vec<String> {
    c.items.iter().map(|i| i.label.clone()).collect()
}

#[test]
fn completion_knows_locals_members_inheritance_and_used_units() {
    let dir = tempfile::tempdir().unwrap();
    let (_, _, index) = project(dir.path());

    let c = complete_at(&index, "  To");
    assert_eq!(c.context.prefix, "To");
    let all = labels(&c);
    for expected in [
        "Total", "Forcar", "Outro", "Lista", "Self", "Endereco", "Calcula", "Nome", "Global",
        "Soma", "VERSAO", "TModo", "mNovo", "TCliente", "TBase", "UnitBase", "begin", "Integer",
    ] {
        assert!(all.iter().any(|l| l == expected), "{expected} missing");
    }
    // locais primeiro, depois membros, depois o resto
    let pos = |l: &str| all.iter().position(|x| x == l).unwrap();
    assert!(pos("Total") < pos("Endereco"));
    assert!(pos("Endereco") < pos("Global"));
    assert!(pos("Global") < pos("begin"));
    let local = c.items.iter().find(|i| i.label == "Outro").unwrap();
    assert_eq!(
        (local.kind, local.detail.as_str()),
        (ItemKind::Variable, ": TCliente")
    );
    let filtered = autocomplete::filter(&c.items, "to");
    assert_eq!(c.items[filtered[0]].label, "Total");

    let members = labels(&complete_at(&index, "  Self."));
    for expected in [
        "Salvar", "Valida", "Imprime", "Codigo", "FIdade", "Calcula", "Nome", "Itens",
    ] {
        assert!(
            members.contains(&expected.to_owned()),
            "Self.{expected} missing"
        );
    }
    assert!(!members.contains(&"begin".to_owned()));

    let inherited = complete_at(&index, "  Outro.Ca");
    assert_eq!(inherited.context.qualifier, ["Outro"]);
    assert!(labels(&inherited).contains(&"Calcula".to_owned()));
    let calcula = inherited
        .items
        .iter()
        .find(|i| i.label == "Calcula")
        .unwrap();
    assert_eq!(calcula.kind, ItemKind::Function);
    assert_eq!(calcula.detail, "(A: Integer): Integer");
    assert_eq!(calcula.source, "TBase");

    // var inline com tipo inferido de TCliente.Create e cadeia por campo de outra unit
    assert!(labels(&complete_at(&index, "  Lista.")).contains(&"Endereco".to_owned()));
    assert!(labels(&complete_at(&index, "  Global.")).contains(&"Calcula".to_owned()));

    let unit = labels(&complete_at(&index, "  UnitBase."));
    for expected in ["TBase", "TModo", "Global", "VERSAO", "Soma"] {
        assert!(
            unit.contains(&expected.to_owned()),
            "UnitBase.{expected} missing"
        );
    }
    assert_eq!(
        labels(&complete_at(&index, "  TModo.")),
        ["mEditar", "mExcluir", "mNovo"]
    );
    assert!(complete_at(&index, "  Desconhecido.").items.is_empty());
    assert!(complete_at(&index, "  S := 'Tot").items.is_empty());
    assert!(complete_at(&index, "  // Tot").items.is_empty());
}

#[test]
fn completion_in_uses_lists_project_units() {
    let dir = tempfile::tempdir().unwrap();
    let (_, _, index) = project(dir.path());
    let text = TELA.replace("  UnitBase;", "  UnitBase, Uni");
    let line = line_of(&text, "UnitBase, Uni");
    let o = outline::pascal(&text);
    let cx = context::analyze(&text, &o, line, "  UnitBase, Uni".len());
    assert!(cx.in_uses);
    let world = World::new(&o, Some(&index), "UnitTela");
    let c = autocomplete::Engine::default().complete(cx, &world, &[]);
    assert_eq!(labels(&c), ["UnitBase"]);
}

fn apply(text: &str, line: usize) -> Result<(String, class_completion::Plan), String> {
    let o = outline::pascal(text);
    let mut buf = TextBuffer::new(text);
    let plan = class_completion::complete(text, &o, line, &|l, c| buf.pos(l, c))?;
    let caret = Selection::caret(plan.caret.unwrap_or(0));
    buf.transact(plan.edits.clone(), caret, EditKind::Other, 0.0);
    Ok((buf.text(), plan))
}

#[test]
fn class_completion_adds_bodies_properties_and_declarations() {
    let (done, plan) = apply(TELA, line_of(TELA, "function Valida")).unwrap();
    assert!(done.contains("    property Codigo: Integer read FCodigo write SetCodigo;"));
    assert!(done.contains("  private\n    FCodigo: Integer;\n    FIdade: Integer;\n"));
    assert!(done.contains(
        "    FIdade: Integer;\n    procedure SetCodigo(const Value: Integer);\n  public\n"
    ));
    assert!(done.contains("\nfunction TCliente.Valida: Boolean;\nbegin\n  \nend;\n"));
    assert!(done.contains("\nprocedure TCliente.Imprime(Copias: Integer);\nbegin\n  \nend;\n"));
    assert!(done.contains(
        "\nprocedure TCliente.SetCodigo(const Value: Integer);\nbegin\n  FCodigo := Value;\nend;\n"
    ));
    // abstract, sobrecarga ja implementada e propriedade com read ficam como estavam
    assert!(!done.contains("TCliente.Abstrato"));
    assert_eq!(
        done.matches("procedure TCliente.Imprime(const Titulo")
            .count(),
        1
    );
    assert!(done.contains("property Idade: Integer read FIdade;"));
    // o cursor fica no primeiro corpo criado
    let caret = plan.caret.unwrap();
    let before: String = done.chars().take(caret).collect();
    assert!(before.ends_with("procedure TCliente.SetCodigo(const Value: Integer);\nbegin\n  "));
    // o resto do arquivo nao muda
    assert!(done.starts_with("unit UnitTela;\n\ninterface\n\nuses\n  UnitBase;\n"));
    // os corpos novos entram depois do ultimo metodo da classe
    assert!(done.contains(
        "procedure TCliente.Extra(N: Integer);\nbegin\nend;\n\nprocedure TCliente.SetCodigo"
    ));
    assert!(
        done.ends_with("procedure TCliente.Imprime(Copias: Integer);\nbegin\n  \nend;\n\nend.\n")
    );
    // de novo: nada a fazer
    let again = apply(&done, line_of(&done, "function Valida"));
    assert!(again.unwrap_err().contains("is complete"));

    // metodo implementado sem declaracao: a declaracao vai para a secao private
    let (declared, _) = apply(&done, line_of(&done, "procedure TCliente.Extra") + 1).unwrap();
    assert!(declared.contains(
        "    procedure SetCodigo(const Value: Integer);\n    procedure Extra(N: Integer);\n  public\n"
    ));
    assert!(apply("unit X;\ninterface\nimplementation\nend.\n", 1).is_err());
}

#[test]
fn class_completion_creates_private_section_and_first_bodies() {
    let src = "unit U;\ninterface\ntype\n  TX = class\n  public\n    procedure P(A: Integer = 1);\n    property V: string;\n  end;\nimplementation\nend.\n";
    let (done, _) = apply(src, 4).unwrap();
    assert_eq!(
        done,
        "unit U;\ninterface\ntype\n  TX = class\n  public\n    procedure P(A: Integer = 1);\n    property V: string read FV write SetV;\n  private\n    FV: string;\n    procedure SetV(const Value: string);\n  end;\nimplementation\nprocedure TX.SetV(const Value: string);\nbegin\n  FV := Value;\nend;\n\nprocedure TX.P(A: Integer);\nbegin\n  \nend;\n\nend.\n"
    );
}

#[test]
fn symbol_index_search_reuse_and_declarations() {
    let dir = tempfile::tempdir().unwrap();
    let (base, tela, index) = project(dir.path());
    assert_eq!((index.files.len(), index.parsed, index.reused), (2, 2, 0));
    let hits = index.search("TCli", 10);
    assert_eq!(hits[0].name, "TCliente");
    assert_eq!(hits[0].location.line, line_of(TELA, "TCliente = class"));
    let hits = index.search("tcliente.sal", 10);
    assert_eq!(
        (hits[0].name.as_str(), hits[0].container.as_str()),
        ("Salvar", "TCliente")
    );
    assert_eq!(
        index
            .search("bc", 50)
            .iter()
            .filter(|h| h.name == "TBase")
            .count(),
        0
    );
    assert!(index.search("calc", 10).iter().any(|h| h.name == "Calcula"));
    let soma = index
        .find_declaration("Soma", &["UnitBase".to_owned()])
        .unwrap();
    assert_eq!(
        (soma.path.as_path(), soma.line),
        (
            base.as_path(),
            line_of(BASE, "function Soma(A, B: Integer): Integer;")
        )
    );
    assert_eq!(index.find_declaration("TCliente", &[]).unwrap().path, tela);

    // reindexar so rele o que mudou
    let again = SymbolIndex::build(
        &[base.clone(), tela.clone()],
        Some(&index),
        &AtomicBool::new(false),
    );
    assert_eq!((again.parsed, again.reused), (0, 2));
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(&tela, TELA.replace("Endereco", "Logradouro")).unwrap();
    let third = SymbolIndex::build(&[base, tela], Some(&again), &AtomicBool::new(false));
    assert_eq!((third.parsed, third.reused), (1, 1));
    assert!(
        third
            .search("Logradouro", 5)
            .iter()
            .any(|h| h.name == "Logradouro")
    );
    // cancelado antes de comecar: indice vazio, sem travar
    let cancelled = SymbolIndex::build(&[dir.path().join("x.pas")], None, &AtomicBool::new(true));
    assert!(cancelled.files.is_empty());
}

fn find(
    files: Vec<PathBuf>,
    buffers: Vec<(PathBuf, String)>,
    query: &str,
    options: FindOptions,
) -> (Vec<find_in_files::FileMatches>, find_in_files::Summary) {
    let request = Request {
        query: query.to_owned(),
        options,
        files,
        buffers,
        no_bom_default: encoding_rs::WINDOWS_1252,
    };
    let results = std::sync::Mutex::new(Vec::new());
    let summary = std::sync::Mutex::new(None);
    find_in_files::run(&request, &AtomicBool::new(false), &|p| match p {
        Progress::File(f) => results.lock().unwrap().push(f),
        Progress::Done(s) => *summary.lock().unwrap() = Some(s),
    });
    let mut r = results.into_inner().unwrap();
    r.sort_by_key(|f| f.order);
    (r, summary.into_inner().unwrap().unwrap())
}

#[test]
fn find_in_files_reads_encodings_skips_binaries_and_uses_open_buffers() {
    let dir = tempfile::tempdir().unwrap();
    let ansi = dir.path().join("Ansi.pas");
    let (bytes, _, _) = encoding_rs::WINDOWS_1252.encode("// Ação\r\nShowMessage('Ação');\r\n");
    std::fs::write(&ansi, &bytes).unwrap();
    let bom = dir.path().join("Bom.pas");
    std::fs::write(&bom, "\u{feff}x := 'ação';\n").unwrap();
    let binary = dir.path().join("Form.dfm");
    std::fs::write(&binary, b"TPF0\x06TForm1\x05Form1\x00").unwrap();
    let open = dir.path().join("Aberto.pas");
    std::fs::write(&open, "nada aqui\n").unwrap();
    let missing = dir.path().join("Sumiu.pas");

    let files = vec![
        ansi.clone(),
        bom.clone(),
        binary.clone(),
        open.clone(),
        missing,
    ];
    let buffers = vec![(open.clone(), "linha\n\tação não salva\n".to_owned())];
    let (r, s) = find(files, buffers, "ação", FindOptions::default());
    assert_eq!(
        r.iter().map(|f| f.path.clone()).collect::<Vec<_>>(),
        [ansi, bom.clone(), open]
    );
    assert_eq!(
        r[0].matches
            .iter()
            .map(|m| (m.line, m.start, m.end))
            .collect::<Vec<_>>(),
        [(0, 3, 7), (1, 13, 17)]
    );
    assert_eq!(r[0].matches[1].text, "ShowMessage('Ação');");
    assert_eq!((r[2].matches[0].line, r[2].matches[0].start), (1, 1));
    assert_eq!(s.files_searched, 3);
    assert_eq!(s.skipped.len(), 2);
    assert!(s.skipped.iter().any(|e| e.contains("binary DFM")));
    assert!(!s.cancelled);

    let (r, _) = find(
        vec![bom.clone()],
        vec![],
        "AÇÃO",
        FindOptions {
            case_sensitive: true,
            ..FindOptions::default()
        },
    );
    assert!(r.is_empty());
    let (_, s) = find(
        vec![bom.clone()],
        vec![],
        "(",
        FindOptions {
            regex: true,
            ..FindOptions::default()
        },
    );
    assert!(s.skipped[0].contains("Invalid regular expression"));

    let sub = dir.path().join("Sub");
    std::fs::create_dir_all(sub.join("__history")).unwrap();
    std::fs::write(sub.join("Unit2.pas"), "x").unwrap();
    std::fs::write(sub.join("__history").join("Unit2.pas.~1~"), "x").unwrap();
    std::fs::write(sub.join("__history").join("Old.pas"), "x").unwrap();
    let masks = find_in_files::parse_masks("*.pas");
    let all = find_in_files::list_directory(dir.path(), &masks, true, &AtomicBool::new(false));
    let names: Vec<String> = all
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["Aberto.pas", "Ansi.pas", "Bom.pas", "Unit2.pas"]);
    let top = find_in_files::list_directory(dir.path(), &masks, false, &AtomicBool::new(false));
    assert_eq!(top.len(), 3);
}

#[test]
fn navigation_history_goes_back_and_forward() {
    let at = |l: usize| Location {
        path: PathBuf::from("A.pas"),
        line: l,
        col: 0,
    };
    let mut h = History::default();
    assert!(h.back(at(0)).is_none());
    h.record(at(1));
    h.record(at(5));
    assert_eq!(h.back(at(9)), Some(at(5)));
    assert_eq!(h.back(at(5)), Some(at(1)));
    assert!(h.can_forward());
    assert_eq!(h.forward(at(1)), Some(at(5)));
    h.record(at(7)); // novo salto descarta o avancar
    assert!(!h.can_forward());
}

#[test]
fn keymap_sequences_like_delphi() {
    use argila::shortcuts::{Command, KeyChord, Keymap};
    use eframe::egui::Key;
    let km = Keymap::delphi12();
    let prefix = KeyChord::parse("Shift+Ctrl+K").unwrap();
    assert!(km.is_prefix(prefix));
    assert_eq!(km.lookup(prefix), None);
    assert_eq!(
        km.lookup_sequence(prefix, Key::T),
        Some(Command::FoldToggle)
    );
    assert_eq!(
        km.lookup_sequence(prefix, Key::M),
        Some(Command::FoldMethods)
    );
    assert_eq!(
        km.lookup_sequence(prefix, Key::A),
        Some(Command::FoldExpandAll)
    );
    assert_eq!(km.lookup_sequence(prefix, Key::Z), None);
    assert_eq!(km.shortcut_text(Command::FoldToggle), "Shift+Ctrl+K, T");
    assert_eq!(km.shortcut_text(Command::NavigateBack), "Alt+Left");
    assert_eq!(km.shortcut_text(Command::GoToSymbol), "Shift+Alt+S");
    assert!(km.conflicts().is_empty(), "{:?}", km.conflicts());

    // export/import preserva as sequencias
    let mut copy = Keymap::delphi12();
    copy.set(Command::FoldToggle, []);
    assert!(copy.apply_json(&km.to_json()).unwrap().is_empty());
    assert_eq!(
        copy.lookup_sequence(prefix, Key::T),
        Some(Command::FoldToggle)
    );

    // uma combinacao simples igual ao prefixo esconde a sequencia: conflito
    let mut clash = Keymap::delphi12();
    clash.set(Command::Messages, [prefix]);
    let c = clash.conflicts();
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].chord, prefix);
    assert!(c[0].commands.contains(&Command::Messages));
    assert!(c[0].commands.contains(&Command::FoldToggle));
}

#[test]
fn outline_exposes_declarations_sections_and_folds() {
    use argila::projects::outline::{DeclKind, SymbolKind};
    use argila::syntax::folding::FoldKind;
    let o = outline::pascal(BASE);
    let decl = |name: &str| o.declarations.iter().find(|d| d.name == name).unwrap();
    assert_eq!(
        (decl("TModo").kind, decl("TModo").ty.as_str()),
        (DeclKind::Type, "(mNovo, mEditar, mExcluir)")
    );
    assert_eq!(
        (decl("mEditar").kind, decl("mEditar").ty.as_str()),
        (DeclKind::EnumValue, "TModo")
    );
    assert_eq!(
        (decl("VERSAO").kind, decl("Global").ty.as_str()),
        (DeclKind::Constant, "TBase")
    );
    let soma = decl("Soma");
    assert_eq!(
        (
            soma.kind,
            soma.params.as_str(),
            soma.ty.as_str(),
            soma.interface
        ),
        (DeclKind::Routine, "(A, B: Integer)", "Integer", true)
    );
    assert_eq!(o.unit_name(), Some("UnitBase"));
    assert_eq!(o.implementation_line, Some(line_of(BASE, "implementation")));
    assert_eq!(o.implementation_end, Some(line_of(BASE, "end.")));

    let base = o.find_type("TBase").unwrap();
    let calcula = base.children.iter().find(|s| s.name == "Calcula").unwrap();
    assert_eq!(
        (calcula.kind, calcula.params.as_str(), calcula.ty.as_str()),
        (SymbolKind::Method, "(A: Integer)", "Integer")
    );
    let class = &o.classes[0];
    assert_eq!(class.path, "TBase");
    assert_eq!(
        class
            .sections
            .iter()
            .map(|s| s.visibility.as_str())
            .collect::<Vec<_>>(),
        ["private", "public"]
    );
    assert_eq!(base.end_line, class.end_line);
    let prop = &o.properties[0];
    assert_eq!(
        (
            prop.name.as_str(),
            prop.has_read,
            prop.has_write,
            prop.indexed
        ),
        ("Nome", true, true, false)
    );

    let t = outline::pascal(TELA);
    let abstrato = t.decls.iter().find(|d| d.name == "Abstrato").unwrap();
    assert!(!abstrato.needs_body);
    assert!(
        t.decls
            .iter()
            .filter(|d| d.name != "Abstrato")
            .all(|d| d.needs_body)
    );
    // folds vem junto com o outline (o editor usa a mesma analise em segundo plano)
    assert!(
        o.folds
            .iter()
            .any(|f| f.kind == FoldKind::Type && f.start == line_of(BASE, "TBase = class"))
    );
    assert!(
        o.folds
            .iter()
            .any(|f| f.kind == FoldKind::Routine
                && f.start == line_of(BASE, "function TBase.Calcula"))
    );
    assert!(outline::symbols_only(BASE).folds.is_empty());
    let dfm = outline::dfm("object F: TF\n  object B: TButton\n  end\nend\n");
    assert_eq!(dfm.folds.len(), 2);
    assert_eq!(dfm.symbols[0].end_line, 3);
}

/// Projeto real (opcional): `ARGILA_REAL_PROJECT=caminho\Projeto.dproj cargo test --release
/// --test advanced real_project -- --nocapture`. So le arquivos.
#[test]
fn real_project_index_search_and_completion() {
    let Some(path) = std::env::var_os("ARGILA_REAL_PROJECT") else {
        return;
    };
    let project = argila::projects::Project::load(Path::new(&path)).unwrap();
    let files: Vec<PathBuf> = project
        .units
        .iter()
        .filter(|u| u.exists)
        .filter_map(|u| u.pas.clone())
        .collect();
    let started = std::time::Instant::now();
    let index = SymbolIndex::build(&files, None, &AtomicBool::new(false));
    println!(
        "index: {} units, {} symbols, {:.0} ms",
        index.files.len(),
        index.symbol_count(),
        started.elapsed().as_secs_f64() * 1000.0
    );
    assert!(index.failed.is_empty(), "{:?}", index.failed);
    let started = std::time::Instant::now();
    let again = SymbolIndex::build(&files, Some(&index), &AtomicBool::new(false));
    println!(
        "reindex unchanged: {} reused in {:.0} ms",
        again.reused,
        started.elapsed().as_secs_f64() * 1000.0
    );
    assert_eq!(again.parsed, 0);
    let started = std::time::Instant::now();
    let hits = index.search("form", 300);
    println!(
        "go to symbol 'form': {} hits in {:.1} ms",
        hits.len(),
        started.elapsed().as_secs_f64() * 1000.0
    );
    let mut all: Vec<PathBuf> = files.clone();
    all.extend(project.units.iter().filter_map(|u| u.dfm.clone()));
    let started = std::time::Instant::now();
    let (r, s) = find(
        all,
        vec![],
        "Close",
        FindOptions {
            whole_word: true,
            ..FindOptions::default()
        },
    );
    println!(
        "find 'Close': {} matches in {} files, {} searched, {:.0} ms",
        r.iter().map(|f| f.matches.len()).sum::<usize>(),
        r.len(),
        s.files_searched,
        started.elapsed().as_secs_f64() * 1000.0
    );
    // autocompletar no maior arquivo, dentro do ultimo metodo
    let biggest = index
        .files
        .iter()
        .max_by_key(|f| f.outline.routines.len())
        .unwrap();
    let text = argila::projects::read_text(&biggest.path).unwrap();
    let o = outline::pascal(&text);
    if let Some(r) = o.routines.iter().rfind(|r| r.key.is_some()) {
        let started = std::time::Instant::now();
        let cx = context::analyze(&text, &o, r.end_line, 0);
        let world = World::new(&o, Some(&index), &biggest.unit);
        let c = autocomplete::Engine::default().complete(cx, &world, &[]);
        println!(
            "completion in {} ({} lines): {} items in {:.1} ms",
            biggest.unit,
            text.lines().count(),
            c.items.len(),
            started.elapsed().as_secs_f64() * 1000.0
        );
        assert!(!c.items.is_empty());
    }
}
