use argila::dfm::{
    Preview,
    components::{self, Control},
    designer,
    parser::{self, ObjectKind, Value},
};
use argila::editor::{
    buffer::{EditKind, Selection},
    document::{Document, OpenError, SaveOptions},
};
use encoding_rs::WINDOWS_1252;

const DEMO: &str = include_str!("fixtures/preview.dfm");

#[test]
fn real_form_properties_collections_and_unknown_data_are_preserved() {
    let form = parser::parse(DEMO).unwrap();
    assert_eq!(&*form.source, DEMO);
    let grid = form.root.find("Demo/Pages/DataPage/Grid").unwrap();
    let p = grid.property("Columns").unwrap();
    let Value::Collection(items) = &p.value else {
        panic!()
    };
    assert_eq!(items.len(), 2);
    assert_eq!(items[0][1].value, Value::String("Código".into()));
    let unknown = form.root.find("Demo/Vendor").unwrap();
    assert_eq!(unknown.text("Vendor.Unknown"), Some("Preservado"));
    let data = unknown.property("Picture.Data").unwrap();
    assert_eq!(data.value, Value::Binary);
    assert_eq!(data.raw, "{00FF1234ABCD}");
    assert!(form.source[data.span.clone()].contains(&data.raw));
    assert_eq!(
        form.root.path_at_line(grid.line),
        "Demo/Pages/DataPage/Grid"
    );
}

#[test]
fn strings_lists_unicode_and_continuations() {
    let form=parser::parse("object F: TForm\n Caption = 'D''água '#231#227'o' +\n '!'#$D83D#$DE00\n Lines.Strings = (\n 'one'\n 'two' +\n ' continued'\n #65#66)\nend").unwrap();
    assert_eq!(form.root.text("Caption"), Some("D'água ção!😀"));
    let Value::List(lines) = &form.root.property("Lines.Strings").unwrap().value else {
        panic!()
    };
    assert_eq!(
        lines,
        &vec![
            Value::String("one".into()),
            Value::String("two continued".into()),
            Value::String("AB".into())
        ]
    );
}

#[test]
fn inherited_inline_indices_and_typed_values() {
    let form=parser::parse("inherited F: TForm1 [0]\n X = -1.25E+2\n Color = $FFAA00\n Set = []\n Data = {}\n inherited Button1\n Caption = 'Override'\n end\n inline Frame: TFrame [1]\n end\nend").unwrap();
    assert_eq!(form.root.kind, ObjectKind::Inherited);
    assert_eq!(form.root.children[0].kind, ObjectKind::Inherited);
    assert_eq!(form.root.children[0].class, "(inherited)");
    assert_eq!(form.root.children[1].kind, ObjectKind::Inline);
    assert_eq!(form.root.number("X", 0.), -125.);
    assert_eq!(
        form.root.property("Color").unwrap().value,
        Value::Number(16755200.)
    );
}

#[test]
fn invalid_input_reports_locations_and_never_returns_partial_form() {
    for text in [
        "",
        "object F: TForm\n Caption = 'oops\nend",
        "object F: TForm\n Data = {123}\nend",
        "object F: TForm\n A = [akLeft\nend",
        "object F: TForm\n A = <item X = 1\nend",
        "object F: TForm\n Width = 2",
        "object F: TForm\nend\n garbage",
        "object F: TForm\n A = 'a' + end",
    ] {
        assert!(parser::parse(text).is_err(), "{text}");
    }
    let err = parser::parse("object F: TForm\n Caption = 'bad\nend").unwrap_err();
    assert!(err.line >= 1);
    let deep = format!("{}{}", "object F: TForm\n".repeat(140), "end\n".repeat(140));
    assert!(parser::parse(&deep).unwrap_err().message.contains("limit"));
    let deep = format!(
        "object F: TForm\n A = {}0{}\nend",
        "(".repeat(140),
        ")".repeat(140)
    );
    assert!(parser::parse(&deep).unwrap_err().message.contains("limit"));
}

#[test]
fn alignment_anchors_visibility_enabled_and_unknown_controls() {
    let form = parser::parse(DEMO).unwrap();
    let items = components::layout(&form.root, [1000., 660.], false);
    let item = |name: &str| items.iter().find(|i| i.component.name == name).unwrap();
    assert_eq!(item("Header").bounds.w, 1000.);
    assert_eq!(item("Footer").bounds.y, 622.);
    assert_eq!(item("SaveButton").bounds.x, 890.);
    assert_eq!(item("NameEdit").bounds.w, 500.);
    assert_eq!(item("Pages").bounds.h, 300.);
    assert_eq!(item("Vendor").kind, Control::Unknown);
    assert!(!item("DatabaseEdit").enabled);
    assert!(!items.iter().any(|i| i.component.name == "HiddenButton"));
    assert!(!items.iter().any(|i| i.component.name == "OtherLabel"));
    assert!(
        components::layout(&form.root, [800., 560.], true)
            .iter()
            .any(|i| i.component.name == "HiddenButton" && i.hidden)
    );
    let selected = components::layout_selected(
        &form.root,
        [800., 560.],
        false,
        "Demo/Pages/OtherPage/OtherLabel",
    );
    assert!(selected.iter().any(|i| i.component.name == "OtherLabel"));
    assert!(!selected.iter().any(|i| i.component.name == "Grid"));
}

#[test]
fn aligned_children_are_clipped_to_their_parent_and_inherit_disabled_state() {
    let form=parser::parse("object F: TForm\n ClientWidth = 300\n ClientHeight = 200\n Enabled = False\n object P: TPanel\n Left = 10\n Top = 20\n Width = 100\n Height = 90\n object B: TButton\n Align = alClient\n end\n end\nend").unwrap();
    let items = components::layout(&form.root, [300., 200.], false);
    let b = items.iter().find(|i| i.component.name == "B").unwrap();
    assert_eq!(b.bounds.x, 12.);
    assert_eq!(b.bounds.y, 22.);
    assert_eq!(b.bounds.w, 96.);
    assert!(!b.enabled);
    assert!(b.clip.w <= 100.);
}

#[test]
fn opening_preview_and_saving_ansi_dfm_preserves_unknown_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Old.dfm");
    let source = "object F: TForm\r\n Caption = 'Ação'\r\n Custom.Data = {00ABFF}\r\n Custom.Options = <item Name = 'Não' end>\r\nend\r\n";
    let bytes = WINDOWS_1252.encode(source).0.into_owned();
    std::fs::write(&path, &bytes).unwrap();
    let mut doc = Document::open(&path, WINDOWS_1252).unwrap();
    let parsed = parser::parse(&doc.buffer.text()).unwrap();
    assert_eq!(parsed.root.text("Caption"), Some("Ação"));
    assert!(!doc.is_modified());
    doc.save(SaveOptions::default()).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    let line = doc
        .buffer
        .text()
        .lines()
        .position(|s| s.contains("Caption"))
        .unwrap();
    doc.buffer.set_selection(Selection::new(
        doc.buffer.pos(line, 0),
        doc.buffer.pos(line + 1, 0),
    ));
    doc.buffer
        .insert(" Caption = 'Coração'\n", EditKind::Other, 0.);
    parser::parse(&doc.buffer.text()).unwrap();
    doc.save(SaveOptions::default()).unwrap();
    assert_eq!(
        std::fs::read(&path).unwrap(),
        WINDOWS_1252
            .encode(&source.replace("Ação", "Coração"))
            .0
            .as_ref()
    );
}

#[test]
fn binary_dfm_is_detected_and_original_is_never_opened_as_editable_text() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Binary.dfm");
    let bytes = b"TPF0\x06TForm1\x05Form1\x00\x00";
    std::fs::write(&path, bytes).unwrap();
    assert!(parser::is_binary(bytes));
    assert!(matches!(
        Document::open(&path, WINDOWS_1252),
        Err(OpenError::Binary(_))
    ));
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

#[test]
fn designer_renders_supported_controls_and_unknown_placeholder_without_native_vcl() {
    let form = parser::parse(DEMO).unwrap();
    let ctx = eframe::egui::Context::default();
    let mut state = Preview::default();
    state.show_hidden = true;
    for selected in ["Demo", "Demo/Pages/OtherPage/OtherLabel"] {
        state.selected = selected.into();
        let output = ctx.run(
            eframe::egui::RawInput {
                screen_rect: Some(eframe::egui::Rect::from_min_size(
                    eframe::egui::Pos2::ZERO,
                    eframe::egui::vec2(1400., 1000.),
                )),
                ..Default::default()
            },
            |ctx| {
                eframe::egui::CentralPanel::default().show(ctx, |ui| {
                    designer::show(ui, &form, &mut state);
                });
            },
        );
        assert!(!output.shapes.is_empty());
        assert_eq!(state.selected, selected);
    }
}

#[test]
fn background_preview_discards_stale_results_and_caches_current_version() {
    let ctx = eframe::egui::Context::default();
    let mut state = Preview::default();
    state.update(0, || "object Old: TForm end".into(), &ctx);
    let started = std::time::Instant::now();
    while state.version != Some(1) {
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
        state.update(1, || "object Current: TForm end".into(), &ctx);
        if let Some(Ok(form)) = &state.result {
            assert_eq!(form.root.name, "Current");
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    state.update(1, || panic!("unchanged buffer must use cached model"), &ctx);
    state.update(2, || "object Invalid: TForm Caption = 'oops".into(), &ctx);
    assert!(
        state.result.is_none(),
        "old preview must immediately disappear after edits"
    );
    while state.version != Some(2) {
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
        state.update(2, || panic!("worker already pending"), &ctx);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(state.result.as_ref().unwrap().is_err());
}

#[test]
fn malformed_unicode_inputs_never_panic() {
    let chars = [
        '\0', 'é', '中', '\'', '#', '{', '}', '[', ']', '<', '>', '(', ')', '\n', ' ', '=', '1',
        '+', ':',
    ];
    for seed in 0..400_u64 {
        let mut n = seed + 1;
        let mut text = String::from("object F: TForm\n Value = ");
        for _ in 0..80 {
            n = n.wrapping_mul(6364136223846793005).wrapping_add(1);
            text.push(chars[(n >> 32) as usize % chars.len()]);
        }
        let _ = parser::parse(&text);
    }
}

#[test]
fn designer_pointer_selects_controls_switches_pages_and_navigates_to_source() {
    use eframe::egui::{self, Event, Modifiers, PointerButton, Pos2, Rect, Shape, vec2};
    fn frame(
        ctx: &egui::Context,
        form: &parser::Form,
        state: &mut Preview,
        events: Vec<Event>,
        time: f64,
    ) -> (egui::FullOutput, Option<usize>) {
        let mut line = None;
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1600., 1000.))),
                events,
                time: Some(time),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    line = designer::show(ui, form, state);
                });
            },
        );
        (output, line)
    }
    fn click(
        ctx: &egui::Context,
        form: &parser::Form,
        state: &mut Preview,
        point: Pos2,
        time: f64,
    ) -> Option<usize> {
        let button = |pressed| Event::PointerButton {
            pos: point,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        frame(
            ctx,
            form,
            state,
            vec![Event::PointerMoved(point), button(true)],
            time,
        );
        frame(ctx, form, state, vec![button(false)], time + 0.02).1
    }
    let ctx = egui::Context::default();
    let form = parser::parse(DEMO).unwrap();
    let mut state = Preview::default();
    // Stabilize the immediate-mode scroll-area dimensions before sending pointer events.
    frame(&ctx, &form, &mut state, vec![], 0.);
    let (output, _) = frame(&ctx, &form, &mut state, vec![], 0.1);
    let title = output
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            Shape::Text(t) if t.galley.job.text == "Inspeção VCL" => Some(t),
            _ => None,
        })
        .expect("form title must be painted");
    let origin = title.pos + vec2(-8., 28. - (28. - title.galley.size().y) / 2.);
    let button = origin + vec2(730., 70.);
    assert!(click(&ctx, &form, &mut state, button, 1.).is_none());
    assert_eq!(state.selected, "Demo/SaveButton");
    let line = click(&ctx, &form, &mut state, button, 1.15);
    assert_eq!(line, Some(form.root.find("Demo/SaveButton").unwrap().line));
    let tab = origin + vec2(16. + 4. + (5. * 7. + 16.) + 30., 294. + 10.);
    click(&ctx, &form, &mut state, tab, 2.);
    assert_eq!(state.selected, "Demo/Pages/OtherPage");
    assert_eq!(
        &*form.source, DEMO,
        "designer interaction must never modify the DFM"
    );
}
