//! Read-only canvas. Native controls and handlers from a DFM are never executed.
use super::{
    Preview,
    components::{self, Bounds, Control, Placed},
    parser::{Component, Form, Value},
};
use eframe::egui::{
    self, Align2, Color32, FontId, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2,
};

fn rect(b: Bounds, origin: egui::Pos2, zoom: f32) -> Rect {
    Rect::from_min_size(origin + vec2(b.x, b.y) * zoom, vec2(b.w, b.h) * zoom)
}

pub fn show(ui: &mut Ui, form: &Form, state: &mut Preview) -> Option<usize> {
    if state.zoom == 0. {
        state.zoom = 1.;
    }
    if form.root.find(&state.selected).is_none() {
        state.selected = form.root.name.clone();
    }
    let mut source_line = None;
    ui.horizontal(|ui| {
        ui.label("Read-only · VCL preview");
        ui.add(egui::Slider::new(&mut state.zoom, 0.25..=2.).text("Zoom"));
        ui.checkbox(&mut state.show_hidden, "Hidden controls");
        if ui.button("Go to source").clicked() {
            source_line = form.root.find(&state.selected).map(|c| c.line);
        }
    });
    if form.root.kind == super::parser::ObjectKind::Inherited {
        ui.label("Inherited form: base DFM is not resolved; only properties and controls stored here are shown.");
    }
    ui.horizontal(|ui| {
        let mut resize = state.size.is_some();
        if ui
            .checkbox(&mut resize, "Inspect resizing / anchors")
            .changed()
        {
            state.size = resize.then(|| components::client_size(&form.root));
        }
        if let Some(size) = &mut state.size {
            ui.label("Width");
            ui.add(egui::DragValue::new(&mut size[0]).range(64.0..=4096.0));
            ui.label("Height");
            ui.add(egui::DragValue::new(&mut size[1]).range(64.0..=4096.0));
        }
    });
    let height = ui.available_height();
    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(
            vec2(190., height),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.label("Components");
                egui::ScrollArea::both().id_salt("dfm_tree").show(ui, |ui| {
                    tree(
                        ui,
                        &form.root,
                        &form.root.name,
                        &mut state.selected,
                        &mut source_line,
                    );
                });
            },
        );
        ui.separator();
        egui::ScrollArea::both()
            .id_salt("dfm_canvas")
            .show(ui, |ui| {
                let size = state
                    .size
                    .unwrap_or_else(|| components::client_size(&form.root));
                let zoom = state.zoom;
                let (canvas, response) = ui.allocate_exact_size(
                    vec2(size[0] * zoom + 32., size[1] * zoom + 60.),
                    Sense::click(),
                );
                let origin = canvas.min + vec2(16., 44.);
                let title =
                    Rect::from_min_size(canvas.min + vec2(16., 16.), vec2(size[0] * zoom, 28.));
                ui.painter()
                    .rect_filled(title, 3., Color32::from_rgb(61, 52, 43));
                ui.painter().text(
                    title.left_center() + vec2(8., 0.),
                    Align2::LEFT_CENTER,
                    form.root.text("Caption").unwrap_or(&form.root.name),
                    FontId::proportional(14.),
                    Color32::WHITE,
                );
                let items = components::layout_selected(
                    &form.root,
                    size,
                    state.show_hidden,
                    &state.selected,
                );
                for item in &items {
                    paint(ui, item, origin, zoom, &state.selected);
                }
                if let Some(item) = items.iter().find(|i| i.path == state.selected) {
                    let r = rect(item.bounds, origin, zoom);
                    ui.painter().rect_stroke(
                        r,
                        0.,
                        Stroke::new(2_f32, Color32::from_rgb(196, 138, 90)),
                        StrokeKind::Outside,
                    );
                    for p in [
                        r.left_top(),
                        r.right_top(),
                        r.left_bottom(),
                        r.right_bottom(),
                    ] {
                        ui.painter().rect_filled(
                            Rect::from_center_size(p, vec2(5., 5.)),
                            0.,
                            Color32::from_rgb(196, 138, 90),
                        );
                    }
                }
                if response.clicked() || response.double_clicked() {
                    if let Some(point) = response.interact_pointer_pos() {
                        let tab_hit = items
                            .iter()
                            .rev()
                            .filter(|i| i.kind == Control::PageControl)
                            .find_map(|i| {
                                let r = rect(i.bounds, origin, zoom);
                                let mut x = r.left() + 4. * zoom;
                                for page in &i.component.children {
                                    if !page.boolean("TabVisible", true) {
                                        continue;
                                    }
                                    let title = page.text("Caption").unwrap_or(&page.name);
                                    let w = (title.chars().count() as f32 * 7. + 16.) * zoom;
                                    let tab =
                                        Rect::from_min_size(pos2(x, r.top()), vec2(w, 22. * zoom));
                                    x += w;
                                    if tab
                                        .intersect(r)
                                        .intersect(rect(i.clip, origin, zoom))
                                        .contains(point)
                                    {
                                        return Some((
                                            format!("{}/{}", i.path, page.name),
                                            page.line,
                                        ));
                                    }
                                }
                                None
                            });
                        if let Some((path, line)) = tab_hit {
                            state.selected = path;
                            if response.double_clicked() {
                                source_line = Some(line);
                            }
                            return;
                        }
                        if let Some(item) = items.iter().rev().find(|i| {
                            rect(i.bounds, origin, zoom)
                                .intersect(rect(i.clip, origin, zoom))
                                .contains(point)
                        }) {
                            state.selected = item.path.clone();
                            if response.double_clicked() {
                                source_line = Some(item.component.line);
                            }
                        }
                    }
                }
            });
    });
    source_line
}

fn tree(ui: &mut Ui, c: &Component, path: &str, selected: &mut String, line: &mut Option<usize>) {
    let label = format!(
        "{}: {}{}",
        c.name,
        c.class,
        if c.kind == super::parser::ObjectKind::Inherited {
            " (inherited)"
        } else {
            ""
        }
    );
    let draw = |ui: &mut Ui, selected: &mut String, line: &mut Option<usize>| {
        let r = ui.selectable_label(*selected == path, &label);
        if r.clicked() {
            *selected = path.into();
        }
        if r.double_clicked() {
            *line = Some(c.line);
        }
    };
    if c.children.is_empty() {
        draw(ui, selected, line);
    } else {
        egui::collapsing_header::CollapsingState::load_with_default_open(
            ui.ctx(),
            ui.make_persistent_id(path),
            true,
        )
        .show_header(ui, |ui| draw(ui, selected, line))
        .body(|ui| {
            for child in &c.children {
                tree(ui, child, &format!("{path}/{}", child.name), selected, line);
            }
        });
    }
}

fn paint(ui: &Ui, item: &Placed<'_>, origin: egui::Pos2, z: f32, selected: &str) {
    let r = rect(item.bounds, origin, z);
    let clip = rect(item.clip, origin, z)
        .intersect(ui.clip_rect())
        .intersect(r);
    if !r.intersects(clip) {
        return;
    }
    let p = ui.painter().with_clip_rect(clip);
    let c = item.component;
    let fg = if item.enabled && !item.hidden {
        Color32::BLACK
    } else {
        Color32::GRAY
    };
    let bg = Color32::from_rgb(240, 240, 240);
    let white = Color32::WHITE;
    let border = Stroke::new(1_f32, Color32::from_gray(150));
    let font = FontId::proportional((c.number("Font.Height", -13.).abs() * z).clamp(6., 72.));
    let text = |at, align, s: &str| {
        p.text(at, align, s, font.clone(), fg);
    };
    let box_fill = |fill| {
        p.rect(r, 0., fill, border, StrokeKind::Inside);
    };
    let caption = c.text("Caption").unwrap_or(&c.name).replace('&', "");
    let inset = vec2(4., 3.) * z;
    match item.kind {
        Control::Form | Control::Panel | Control::TabSheet => {
            box_fill(bg);
            if item.kind == Control::Panel {
                text(r.center(), Align2::CENTER_CENTER, &caption);
            }
        }
        Control::Button => {
            p.rect(r, 3., bg, border, StrokeKind::Inside);
            text(r.center(), Align2::CENTER_CENTER, &caption);
        }
        Control::Label => text(r.left_top(), Align2::LEFT_TOP, &caption),
        Control::Edit | Control::DBEdit | Control::ComboBox => {
            box_fill(white);
            text(
                r.left_top() + inset,
                Align2::LEFT_TOP,
                c.text("Text").or_else(|| c.text("DataField")).unwrap_or(""),
            );
            if item.kind == Control::ComboBox {
                text(
                    r.right_center() - vec2(10. * z, 0.),
                    Align2::CENTER_CENTER,
                    "▾",
                );
            }
        }
        Control::CheckBox | Control::RadioButton => {
            let mark = Rect::from_min_size(r.min + vec2(0., 2. * z), vec2(13., 13.) * z);
            if item.kind == Control::CheckBox {
                p.rect(mark, 0., white, border, StrokeKind::Inside);
                if c.boolean("Checked", false) {
                    text(mark.center(), Align2::CENTER_CENTER, "✓");
                }
            } else {
                p.circle(mark.center(), 6. * z, white, border);
                if c.boolean("Checked", false) {
                    p.circle_filled(mark.center(), 3. * z, fg);
                }
            }
            text(r.left_top() + vec2(19. * z, 0.), Align2::LEFT_TOP, &caption);
        }
        Control::Memo | Control::ListBox => {
            box_fill(white);
            let key = if item.kind == Control::Memo {
                "Lines.Strings"
            } else {
                "Items.Strings"
            };
            if let Some(Value::List(lines)) = c.property(key).map(|p| &p.value) {
                for (i, line) in lines
                    .iter()
                    .take((r.height() / (16. * z)).max(0.) as usize)
                    .enumerate()
                {
                    if let Value::String(s) = line {
                        text(
                            r.min + inset + vec2(0., i as f32 * 16. * z),
                            Align2::LEFT_TOP,
                            s,
                        );
                    }
                }
            }
        }
        Control::GroupBox => {
            box_fill(bg);
            text(r.min + vec2(8., 0.) * z, Align2::LEFT_TOP, &caption);
        }
        Control::PageControl => {
            box_fill(bg);
            let mut x = r.left() + 4. * z;
            for child in &c.children {
                if !child.boolean("TabVisible", true) {
                    continue;
                }
                let title = child.text("Caption").unwrap_or(&child.name);
                let width = (title.chars().count() as f32 * 7. + 16.) * z;
                let tab = Rect::from_min_size(pos2(x, r.top()), vec2(width, 22. * z));
                let path = format!("{}/{}", item.path, child.name);
                let selected_page = c.children.iter().any(|page| {
                    let path = format!("{}/{}", item.path, page.name);
                    selected == path || selected.starts_with(&format!("{path}/"))
                });
                let active = if selected_page {
                    selected == path || selected.starts_with(&format!("{path}/"))
                } else {
                    c.text("ActivePage")
                        .unwrap_or_else(|| c.children.first().map_or("", |p| &p.name))
                        == child.name
                };
                p.rect(
                    tab,
                    2.,
                    if active { white } else { bg },
                    border,
                    StrokeKind::Inside,
                );
                text(tab.center(), Align2::CENTER_CENTER, title);
                x += width;
            }
        }
        Control::DBGrid => {
            box_fill(white);
            for i in 1..=(r.height() / (20. * z)).min(1000.) as usize {
                let y = r.top() + i as f32 * 20. * z;
                p.line_segment([pos2(r.left(), y), pos2(r.right(), y)], border);
            }
            let titles: Vec<String> = match c.property("Columns").map(|p| &p.value) {
                Some(Value::Collection(items)) => items
                    .iter()
                    .map(|props| {
                        props
                            .iter()
                            .find(|p| p.name.eq_ignore_ascii_case("Title.Caption"))
                            .or_else(|| {
                                props
                                    .iter()
                                    .find(|p| p.name.eq_ignore_ascii_case("FieldName"))
                            })
                            .map_or(String::new(), |p| match &p.value {
                                Value::String(s) => s.clone(),
                                _ => p.raw.clone(),
                            })
                    })
                    .collect(),
                _ => vec!["(data unavailable)".into()],
            };
            for (i, title) in titles.iter().enumerate() {
                let x = r.left() + i as f32 * 100. * z;
                p.line_segment([pos2(x, r.top()), pos2(x, r.bottom())], border);
                text(pos2(x + 4. * z, r.top() + 3. * z), Align2::LEFT_TOP, title);
            }
        }
        Control::Unknown => {
            box_fill(Color32::from_rgb(230, 220, 207));
            text(
                r.center(),
                Align2::CENTER_CENTER,
                &format!("{}: {}", c.name, c.class),
            );
        }
    }
}
