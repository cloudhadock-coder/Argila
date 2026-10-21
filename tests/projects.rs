//! Testes de integracao da Fase 3: projetos Delphi (DPR, DPROJ, DPK, pasta), associacao PAS/DFM,
//! navegacao, referencias ausentes, alteracoes seguras e persistencia da sessao.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use argila::projects::files::{self, Plan};
use argila::projects::{Project, ProjectKind, outline, related_file};
use argila::settings::{SessionFile, Settings, SettingsStore};
use encoding_rs::WINDOWS_1252;

const LONG_UNIT: &str = "Unit_Relatório_de_Movimentação_Financeira_Mensal_Consolidada";

/// Projeto com acentos, nome longo, subpasta com espaco, unit fora da pasta, unit ausente,
/// unit de biblioteca, .dpr em Windows-1252 e .dproj em UTF-8 com BOM (como o Delphi grava).
fn fixture() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp
        .path()
        .join("Projeto Ação com um nome bem comprido para testar caminhos longos");
    fs::create_dir_all(dir.join("Sub Pasta")).unwrap();
    fs::create_dir_all(tmp.path().join("Comum")).unwrap();
    let dpr = format!(
        "program Projeto_Teste;\r\n\r\n// comentário em ANSI: ação\r\nuses\r\n  Vcl.Forms,\r\n  Unit_Principal in 'Unit_Principal.pas' {{Form_Principal}},\r\n  {LONG_UNIT} in 'Sub Pasta\\{LONG_UNIT}.pas' {{Form_Relatorio}},\r\n  Unit_Comum in '..\\Comum\\Unit_Comum.pas',\r\n  Unit_Dados in 'Unit_Dados.pas' {{Dm: TDataModule}},\r\n  Unit_Fantasma in 'Unit_Fantasma.pas';\r\n\r\n{{$R *.res}}\r\n\r\nbegin\r\n  Application.Initialize;\r\n  Application.Run;\r\nend.\r\n"
    );
    fs::write(dir.join("Projeto_Teste.dpr"), WINDOWS_1252.encode(&dpr).0).unwrap();
    let dproj = format!(
        "\u{FEFF}<Project xmlns=\"http://schemas.microsoft.com/developer/msbuild/2003\">\r\n    <!-- mantido pelo Delphi -->\r\n    <PropertyGroup>\r\n        <MainSource>Projeto_Teste.dpr</MainSource>\r\n        <Config Condition=\"'$(Config)'==''\">Debug</Config>\r\n    </PropertyGroup>\r\n    <PropertyGroup Condition=\"'$(Config)'=='Base' or '$(Base)'!=''\">\r\n        <DCC_UnitSearchPath>..\\Comum;$(DCC_UnitSearchPath)</DCC_UnitSearchPath>\r\n    </PropertyGroup>\r\n    <ItemGroup>\r\n        <DelphiCompile Include=\"$(MainSource)\"><MainSource>MainSource</MainSource></DelphiCompile>\r\n        <DCCReference Include=\"Unit_Principal.pas\"><Form>Form_Principal</Form></DCCReference>\r\n        <DCCReference Include=\"Sub Pasta\\{LONG_UNIT}.pas\"><Form>Form_Relatorio</Form></DCCReference>\r\n        <DCCReference Include=\"..\\Comum\\Unit_Comum.pas\"/>\r\n        <DCCReference Include=\"Unit_Dados.pas\"><Form>Dm</Form><DesignClass>TDataModule</DesignClass></DCCReference>\r\n        <DCCReference Include=\"Unit_Fantasma.pas\"/>\r\n        <BuildConfiguration Include=\"Debug\"><Key>Cfg_1</Key><CfgParent>Base</CfgParent></BuildConfiguration>\r\n    </ItemGroup>\r\n    <ProjectExtensions><BorlandProject><Platforms><Platform value=\"Win32\">True</Platform></Platforms></BorlandProject></ProjectExtensions>\r\n</Project>\r\n"
    );
    fs::write(dir.join("Projeto_Teste.dproj"), dproj.as_bytes()).unwrap();
    let unit = |name: &str, form: Option<&str>| {
        let text = format!("unit {name};\r\n\r\ninterface\r\n\r\nimplementation\r\n\r\nend.\r\n");
        (
            text,
            form.map(|f| format!("object {f}: T{f}\r\n  Caption = 'Ação'\r\nend\r\n")),
        )
    };
    for (path, name, form) in [
        (
            dir.join("Unit_Principal.pas"),
            "Unit_Principal",
            Some("Form_Principal"),
        ),
        (
            dir.join("Sub Pasta").join(format!("{LONG_UNIT}.pas")),
            LONG_UNIT,
            Some("Form_Relatorio"),
        ),
        (
            tmp.path().join("Comum").join("Unit_Comum.pas"),
            "Unit_Comum",
            None,
        ),
        (dir.join("Unit_Dados.pas"), "Unit_Dados", Some("Dm")),
    ] {
        let (pas, dfm) = unit(name, form);
        fs::write(&path, WINDOWS_1252.encode(&pas).0).unwrap();
        if let Some(dfm) = dfm {
            fs::write(path.with_extension("dfm"), WINDOWS_1252.encode(&dfm).0).unwrap();
        }
    }
    fs::write(dir.join("Projeto_Teste.res"), b"\0\0\0\0").unwrap();
    (tmp, dir)
}

#[test]
fn dproj_and_dpr_open_the_same_project() {
    let (_tmp, dir) = fixture();
    let by_dproj = Project::load(&dir.join("Projeto_Teste.dproj")).unwrap();
    let by_dpr = Project::load(&dir.join("Projeto_Teste.dpr")).unwrap();
    for p in [&by_dproj, &by_dpr] {
        assert_eq!(
            (p.kind, p.name.as_str()),
            (ProjectKind::Program, "Projeto_Teste")
        );
        let names: Vec<_> = p.units.iter().map(|u| u.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Vcl.Forms",
                "Unit_Principal",
                LONG_UNIT,
                "Unit_Comum",
                "Unit_Dados",
                "Unit_Fantasma"
            ]
        );
        assert!(p.dproj.is_some(), "o .dproj ao lado do .dpr tambem e lido");
    }
    let p = by_dproj;
    let long = &p.units[2];
    assert!(long.exists && long.in_source && long.in_dproj);
    assert_eq!(long.form.as_deref(), Some("Form_Relatorio"));
    assert!(
        long.pas
            .as_ref()
            .unwrap()
            .ends_with(Path::new("Sub Pasta").join(format!("{LONG_UNIT}.pas")))
    );
    assert!(
        p.units[3].exists,
        "..\\Comum resolvido para fora da pasta do projeto"
    );
    assert_eq!(p.units[4].form_type.as_deref(), Some("TDataModule"));
    assert!(p.units[0].is_library() && !p.units[0].is_missing());
    assert!(p.units[5].is_missing());
    assert_eq!(p.missing_count(), 1);
    assert!(
        p.problems.iter().any(|m| m.contains("Unit_Fantasma")),
        "{:?}",
        p.problems
    );
    assert_eq!(p.others.len(), 1);
    assert!(p.others[0].exists && p.others[0].path.ends_with("Projeto_Teste.res"));
}

#[test]
fn pas_and_dfm_are_associated() {
    let (_tmp, dir) = fixture();
    let p = Project::load(&dir.join("Projeto_Teste.dpr")).unwrap();
    let principal = &p.units[1];
    let dfm = principal.dfm.clone().expect("dfm ao lado do pas");
    assert_eq!(
        related_file(principal.pas.as_ref().unwrap()),
        Some(dfm.clone())
    );
    assert_eq!(related_file(&dfm).as_deref(), principal.pas.as_deref());
    assert!(p.units[3].dfm.is_none(), "unit sem form nao tem dfm");
    assert_eq!(
        p.unit_for_path(&dfm).map(|u| u.name.as_str()),
        Some("Unit_Principal")
    );
    assert_eq!(
        files::form_of_dfm(&dfm),
        Some(("Form_Principal".to_owned(), "TForm_Principal".to_owned()))
    );
}

#[test]
fn units_resolve_through_project_and_search_path() {
    let (tmp, dir) = fixture();
    fs::write(
        tmp.path().join("Comum").join("Unit_SoNaBusca.pas"),
        b"unit Unit_SoNaBusca;\r\ninterface\r\nimplementation\r\nend.\r\n",
    )
    .unwrap();
    let p = Project::load(&dir.join("Projeto_Teste.dproj")).unwrap();
    assert_eq!(
        p.search_dirs().len(),
        2,
        "pasta do projeto + DCC_UnitSearchPath"
    );
    assert!(
        p.resolve_unit("unit_dados")
            .unwrap()
            .ends_with("Unit_Dados.pas")
    );
    assert!(
        p.resolve_unit("Unit_SoNaBusca")
            .unwrap()
            .ends_with("Unit_SoNaBusca.pas")
    );
    assert_eq!(p.resolve_unit("Vcl.Forms"), None);
    assert_eq!(p.resolve_unit("Unit_Fantasma"), None);
}

#[test]
fn packages_list_requires_and_contains() {
    let tmp = tempfile::tempdir().unwrap();
    let dpk = tmp.path().join("MeusComponentes.dpk");
    fs::write(&dpk, "package MeusComponentes;\r\n{$R *.res}\r\nrequires\r\n  rtl,\r\n  vcl;\r\ncontains\r\n  uBotao in 'uBotao.pas';\r\nend.\r\n").unwrap();
    fs::write(
        tmp.path().join("uBotao.pas"),
        "unit uBotao;\r\ninterface\r\nimplementation\r\nend.\r\n",
    )
    .unwrap();
    let p = Project::load(&dpk).unwrap();
    assert_eq!(p.kind, ProjectKind::Package);
    assert_eq!(p.requires, ["rtl", "vcl"]);
    assert_eq!(p.units.len(), 1);
    assert!(p.units[0].exists);
    assert!(p.problems.is_empty(), "{:?}", p.problems);
}

#[test]
fn broken_inputs_do_not_panic() {
    let tmp = tempfile::tempdir().unwrap();
    let bad = tmp.path().join("Quebrado.dproj");
    fs::write(&bad, "<Project><PropertyGroup><MainSource>Nada.dpr").unwrap();
    assert!(Project::load(&bad).is_err());
    let no_main = tmp.path().join("SemFonte.dproj");
    fs::write(
        &no_main,
        "<Project><PropertyGroup><MainSource>Nada.dpr</MainSource></PropertyGroup></Project>",
    )
    .unwrap();
    let p = Project::load(&no_main).unwrap();
    assert!(
        p.problems
            .iter()
            .any(|m| m.contains("Main source not found"))
    );
    assert!(Project::load(&tmp.path().join("naoexiste.dpr")).is_err());
    let weird = tmp.path().join("Estranho.dpr");
    fs::write(&weird, "{ sem cabecalho } uses ; begin end.").unwrap();
    assert!(Project::load(&weird).is_ok());
}

#[test]
fn folder_projects_list_units_and_related_files() {
    let (_tmp, dir) = fixture();
    fs::create_dir_all(dir.join("__history")).unwrap();
    fs::write(
        dir.join("__history").join("Unit_Velha.pas"),
        "unit Unit_Velha;",
    )
    .unwrap();
    let p = Project::load(&dir).unwrap();
    assert_eq!(p.kind, ProjectKind::Folder);
    let names: Vec<_> = p.units.iter().map(|u| u.name.as_str()).collect();
    assert_eq!(
        names,
        ["Unit_Dados", "Unit_Principal", LONG_UNIT],
        "__history fica de fora"
    );
    assert!(p.units.iter().all(|u| u.dfm.is_some()));
    assert!(
        p.others
            .iter()
            .any(|o| o.path.ends_with("Projeto_Teste.dproj"))
    );
}

fn apply(plan: &Plan, backups: &Path) {
    files::apply(plan, backups).unwrap();
}

#[test]
fn adding_and_removing_units_touch_only_the_reference_and_keep_encoding() {
    let (tmp, dir) = fixture();
    let backups = tmp.path().join("backups");
    let dpr_path = dir.join("Projeto_Teste.dpr");
    let dproj_path = dir.join("Projeto_Teste.dproj");
    let (dpr_before, dproj_before) = (fs::read(&dpr_path).unwrap(), fs::read(&dproj_path).unwrap());
    let nova = dir.join("Unit_Nova_Ação.pas");
    fs::write(
        &nova,
        "unit Unit_Nova_Ação;\r\ninterface\r\nimplementation\r\nend.\r\n",
    )
    .unwrap();

    let project = Project::load(&dproj_path).unwrap();
    let plan = files::plan_add_unit(&project, &nova).unwrap();
    assert_eq!(plan.describe().len(), 2);
    apply(&plan, &backups);

    let dpr_after = fs::read(&dpr_path).unwrap();
    let inserted = WINDOWS_1252
        .encode(",\r\n  Unit_Nova_Ação in 'Unit_Nova_Ação.pas'")
        .0
        .into_owned();
    let pos = dpr_after
        .windows(inserted.len())
        .position(|w| w == inserted.as_slice())
        .expect("entrada em cp1252");
    let mut restored = dpr_after.clone();
    restored.drain(pos..pos + inserted.len());
    assert_eq!(
        restored, dpr_before,
        "o resto do .dpr e identico byte a byte"
    );
    let dproj_after = String::from_utf8(fs::read(&dproj_path).unwrap()).unwrap();
    assert!(
        dproj_after.starts_with('\u{FEFF}') && dproj_after.contains("<!-- mantido pelo Delphi -->")
    );
    assert!(dproj_after.contains("<DCCReference Include=\"Unit_Nova_Ação.pas\"/>\r\n"));
    assert_eq!(
        fs::read_dir(&backups).unwrap().count(),
        1,
        "backup criado antes de gravar"
    );

    let reloaded = Project::load(&dproj_path).unwrap();
    let unit = reloaded
        .units
        .iter()
        .find(|u| u.name == "Unit_Nova_Ação")
        .unwrap();
    assert!(unit.exists && unit.in_source && unit.in_dproj);

    let plan = files::plan_remove_unit(&reloaded, unit).unwrap();
    apply(&plan, &backups);
    assert_eq!(fs::read(&dpr_path).unwrap(), dpr_before);
    assert_eq!(fs::read(&dproj_path).unwrap(), dproj_before);
    assert!(nova.is_file(), "remover do projeto nao apaga o arquivo");
}

#[test]
fn rename_moves_files_and_updates_header_and_references() {
    let (tmp, dir) = fixture();
    let project = Project::load(&dir.join("Projeto_Teste.dproj")).unwrap();
    let pas = dir.join("Unit_Dados.pas");
    let plan = files::plan_rename_unit(Some(&project), &pas, "Unit_BancoDeDados").unwrap();
    apply(&plan, &tmp.path().join("backups"));
    let new_pas = dir.join("Unit_BancoDeDados.pas");
    assert!(!pas.exists() && new_pas.is_file() && dir.join("Unit_BancoDeDados.dfm").is_file());
    assert!(
        String::from_utf8_lossy(&fs::read(&new_pas).unwrap())
            .starts_with("unit Unit_BancoDeDados;")
    );
    let reloaded = Project::load(&dir.join("Projeto_Teste.dproj")).unwrap();
    let u = reloaded
        .units
        .iter()
        .find(|u| u.name == "Unit_BancoDeDados")
        .unwrap();
    assert!(u.exists && u.in_source && u.in_dproj && u.dfm.is_some());
    assert_eq!(
        u.form.as_deref(),
        Some("Dm"),
        "o comentario {{Dm: TDataModule}} e mantido"
    );
    assert!(!reloaded.units.iter().any(|u| u.name == "Unit_Dados"));
}

#[test]
fn plans_refuse_to_overwrite_files_changed_after_reading() {
    let (tmp, dir) = fixture();
    let project = Project::load(&dir.join("Projeto_Teste.dproj")).unwrap();
    let nova = dir.join("Unit_X.pas");
    fs::write(&nova, "unit Unit_X;").unwrap();
    let plan = files::plan_add_unit(&project, &nova).unwrap();
    fs::write(dir.join("Projeto_Teste.dpr"), "program Mudou;").unwrap();
    let err = files::apply(&plan, &tmp.path().join("backups")).unwrap_err();
    assert!(err.contains("changed since it was read"), "{err}");
    assert_eq!(
        fs::read(dir.join("Projeto_Teste.dpr")).unwrap(),
        b"program Mudou;"
    );
}

#[test]
fn new_unit_is_created_and_optionally_added() {
    let (tmp, dir) = fixture();
    let project = Project::load(&dir.join("Projeto_Teste.dproj")).unwrap();
    let plan = files::plan_new_unit(&dir, "Unit_Criada", Some(&project)).unwrap();
    apply(&plan, &tmp.path().join("backups"));
    let created = fs::read(dir.join("Unit_Criada.pas")).unwrap();
    assert!(created.starts_with(b"\xEF\xBB\xBFunit Unit_Criada;\r\n"));
    let reloaded = Project::load(&dir.join("Projeto_Teste.dproj")).unwrap();
    assert!(
        reloaded
            .units
            .iter()
            .any(|u| u.name == "Unit_Criada" && u.exists && u.in_dproj)
    );
    assert!(
        files::plan_new_unit(&dir, "Unit_Criada", None).is_err(),
        "nao sobrescreve"
    );
    assert!(files::plan_new_unit(&dir, "1Invalida", None).is_err());
}

#[test]
fn outline_navigation_on_a_real_style_unit() {
    let src = "unit Unit_Principal;\r\ninterface\r\nuses Vcl.Forms, Unit_Dados;\r\ntype\r\n  TForm_Principal = class(TForm)\r\n    procedure FormCreate(Sender: TObject);\r\n  end;\r\nimplementation\r\nuses Unit_Comum;\r\nprocedure TForm_Principal.FormCreate(Sender: TObject);\r\nbegin\r\n  Caption := 'Ação';\r\nend;\r\nend.\r\n";
    let o = outline::pascal(src);
    assert_eq!(
        o.used_units
            .iter()
            .map(|u| u.name.as_str())
            .collect::<Vec<_>>(),
        ["Vcl.Forms", "Unit_Dados", "Unit_Comum"]
    );
    assert_eq!(
        o.decl_impl_target(11),
        Some((5, 14)),
        "de dentro do corpo para a declaracao"
    );
    assert_eq!(o.decl_impl_target(5).map(|p| p.0), Some(9));
    assert_eq!(o.used_unit_at(2, 18), Some("Unit_Dados"));
}

#[test]
fn session_persists_project_files_and_tree_state() {
    let tmp = tempfile::tempdir().unwrap();
    let store = SettingsStore::new(tmp.path().join("settings.toml"));
    let mut s = Settings::default();
    s.session.project = Some("D:\\Projetos\\Ação\\Projeto_Teste.dproj".into());
    s.session.files = vec![SessionFile {
        path: "D:\\Projetos\\Ação\\Unit_Principal.pas".into(),
        line: 42,
    }];
    s.session.active = Some("D:\\Projetos\\Ação\\Unit_Principal.pas".into());
    s.session.selected = Some("unit:unit_principal".into());
    s.session.expanded = vec![
        "project".into(),
        "units".into(),
        "unit:unit_principal".into(),
    ];
    store.save(&s).unwrap();
    let (loaded, warning) = store.load();
    assert_eq!(warning, None);
    assert_eq!(loaded.session, s.session);
}

#[test]
fn hundreds_of_units_load_quickly() {
    let tmp = tempfile::tempdir().unwrap();
    let mut uses = String::from("program Grande;\r\nuses\r\n  Vcl.Forms");
    for i in 0..600 {
        let name = format!("Unit_Modulo_{i:04}");
        uses.push_str(&format!(
            ",\r\n  {name} in 'Modulos\\{name}.pas' {{Form_{i}}}"
        ));
    }
    uses.push_str(";\r\nbegin\r\nend.\r\n");
    fs::create_dir_all(tmp.path().join("Modulos")).unwrap();
    for i in (0..600).step_by(2) {
        let pas = tmp
            .path()
            .join("Modulos")
            .join(format!("Unit_Modulo_{i:04}.pas"));
        fs::write(&pas, "unit x;").unwrap();
        fs::write(pas.with_extension("dfm"), "object Form: TForm\r\nend\r\n").unwrap();
    }
    fs::write(tmp.path().join("Grande.dpr"), uses).unwrap();
    let t = Instant::now();
    let p = Project::load(&tmp.path().join("Grande.dpr")).unwrap();
    let elapsed = t.elapsed();
    assert_eq!(p.units.len(), 601);
    assert_eq!(p.missing_count(), 300);
    assert_eq!(p.units.iter().filter(|u| u.dfm.is_some()).count(), 300);
    eprintln!("600 units: {elapsed:?}");
    assert!(elapsed.as_secs_f64() < 3.0);
}

/// Projeto real (somente leitura): `ARGILA_REAL_PROJECT=C:\caminho\Projeto.dproj cargo test --release real_project -- --nocapture`.
#[test]
fn real_project_smoke() {
    let Some(path) = std::env::var_os("ARGILA_REAL_PROJECT") else {
        return;
    };
    let p = Project::load(Path::new(&path)).unwrap();
    let with_dfm = p.units.iter().filter(|u| u.dfm.is_some()).count();
    eprintln!(
        "{}: {} units ({} com dfm, {} ausentes, {} de biblioteca), {} problemas, carregado em {:.1} ms",
        p.name,
        p.units.len(),
        with_dfm,
        p.missing_count(),
        p.units.iter().filter(|u| u.is_library()).count(),
        p.problems.len(),
        p.load_ms
    );
    for msg in p.problems.iter().take(5) {
        eprintln!("  problema: {msg}");
    }
    if let Some(d) = &p.dproj {
        eprintln!(
            "  configs {:?}, plataformas {:?}",
            d.config_names(),
            d.platform_names()
        );
    }
    let t = Instant::now();
    let (mut symbols, mut biggest) = (0usize, (0usize, String::new()));
    for u in p.units.iter().filter(|u| u.exists) {
        let text = argila::projects::read_text(u.pas.as_ref().unwrap()).unwrap();
        let o = outline::pascal(&text);
        let n: usize = o.symbols.iter().map(|s| 1 + s.children.len()).sum();
        symbols += n;
        if text.lines().count() > biggest.0 {
            biggest = (text.lines().count(), u.name.clone());
        }
    }
    eprintln!(
        "  outline de todas as units: {symbols} simbolos em {:?} (maior: {} com {} linhas)",
        t.elapsed(),
        biggest.1,
        biggest.0
    );
}
