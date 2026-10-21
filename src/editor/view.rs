//! Widget do editor. Virtualizado: a cada quadro so as linhas visiveis sao lidas do rope,
//! tokenizadas e desenhadas, entao o custo nao depende do tamanho do arquivo.

use eframe::egui::{
    self, Align, CornerRadius, Event, EventFilter, FontId, Id, Key, Margin, Modifiers, Pos2, Rect,
    Response, RichText, Sense, Stroke, Ui, pos2, text::LayoutJob, vec2,
};

use super::buffer::{Edit, EditKind, Selection};
use super::document::Document;
use super::ops;
use super::search::{self, SearchOptions};
use crate::filesystem::encoding::LineEnding;
use crate::syntax::{LineStates, lex_line, line_chars};
use crate::themes::{self, Palette};

pub struct EditorStyle {
    pub font: FontId,
    pub tab_size: usize,
    pub palette: Palette,
}

#[derive(Default)]
pub struct FindBar {
    pub open: bool,
    pub replace_open: bool,
    pub query: String,
    pub replacement: String,
    pub options: SearchOptions,
    focus_query: bool,
    origin: usize,
    matches: Vec<std::ops::Range<usize>>,
    key: Option<(u64, String, SearchOptions)>,
    message: Option<String>,
}

enum FindAction {
    Incremental,
    Next,
    Prev,
    ReplaceOne,
    ReplaceAll,
    Close,
}

#[derive(Clone, Copy)]
enum ScrollTarget {
    Nearest,
    Center,
}

/// Geometria do quadro atual (coordenadas de tela).
struct Geo {
    origin: Pos2,
    text_x: f32,
    gutter_left: f32,
    gutter_w: f32,
    char_w: f32,
    row_h: f32,
    tab: usize,
}

#[derive(Default)]
pub struct EditorOutput {
    /// Linha e coluna (base 1, coluna visual) do cursor.
    pub line: usize,
    pub col: usize,
}

pub struct EditorView {
    id: Id,
    states: LineStates,
    width_cache: Option<(u64, usize)>,
    preferred_vcol: Option<usize>,
    scroll: Option<ScrollTarget>,
    wants_focus: bool,
    dragging: bool,
    last_click: Option<(f64, usize, u8)>,
    page_lines: usize,
    pub find: FindBar,
    /// Ctrl+clique: o app trata como "ir para a declaracao" (o cursor ja foi posto no lugar).
    pub ctrl_clicked: bool,
}

impl EditorView {
    pub fn new(id: Id) -> Self {
        Self {
            id,
            states: LineStates::default(),
            width_cache: None,
            preferred_vcol: None,
            scroll: None,
            wants_focus: true,
            dragging: false,
            last_click: None,
            page_lines: 30,
            find: FindBar::default(),
            ctrl_clicked: false,
        }
    }

    /// Depois de recarregar o documento (caches dependem da versao do buffer).
    pub fn reset(&mut self) {
        self.states.clear();
        self.width_cache = None;
        self.find.key = None;
        self.preferred_vcol = None;
    }

    pub fn request_focus(&mut self) {
        self.wants_focus = true;
    }

    pub fn undo(&mut self, doc: &mut Document) {
        if doc.buffer.undo() {
            self.after_edit(ScrollTarget::Nearest);
        }
    }

    pub fn redo(&mut self, doc: &mut Document) {
        if doc.buffer.redo() {
            self.after_edit(ScrollTarget::Nearest);
        }
    }

    pub fn toggle_comment(&mut self, doc: &mut Document, now: f64) {
        ops::toggle_comment(&mut doc.buffer, now);
        self.after_edit(ScrollTarget::Nearest);
    }

    /// Cursor na linha/coluna (base 0), centralizado.
    pub fn goto(&mut self, doc: &mut Document, line: usize, col: usize) {
        let pos = doc.buffer.pos(line, col);
        doc.buffer.set_selection(Selection::caret(pos));
        self.after_edit(ScrollTarget::Center);
        self.wants_focus = true;
    }

    /// Identificador sob o cursor, incluindo pontos (`Vcl.Forms`), e sua linha/coluna inicial.
    pub fn word_at_caret(doc: &Document) -> Option<(String, usize, usize)> {
        let (line, col) = doc.buffer.line_col(doc.buffer.selection.head);
        let chars = crate::syntax::line_chars(doc.buffer.rope(), line);
        let ok = |c: char| c == '_' || c == '.' || c.is_alphanumeric();
        let mut s = col.min(chars.len());
        while s > 0 && ok(chars[s - 1]) {
            s -= 1;
        }
        let mut e = col.min(chars.len());
        while e < chars.len() && ok(chars[e]) {
            e += 1;
        }
        let word: String = chars[s..e]
            .iter()
            .collect::<String>()
            .trim_matches('.')
            .to_owned();
        (!word.is_empty()).then_some((word, line, s))
    }

    pub fn goto_line(&mut self, doc: &mut Document, line: usize) {
        let pos = doc.buffer.pos(line.saturating_sub(1), 0);
        doc.buffer.set_selection(Selection::caret(pos));
        self.after_edit(ScrollTarget::Center);
        self.wants_focus = true;
    }

    /// Ctrl+F / Ctrl+R: abre a barra; uma selecao de uma linha vira o texto buscado.
    pub fn open_find(&mut self, doc: &Document, replace: bool) {
        let sel = doc.buffer.selection.range();
        if !sel.is_empty() && doc.buffer.line_of(sel.start) == doc.buffer.line_of(sel.end) {
            self.find.query = doc.buffer.slice(sel.clone());
        }
        self.find.open = true;
        self.find.replace_open |= replace;
        self.find.focus_query = true;
        self.find.origin = sel.start;
        self.find.key = None;
    }

    /// F3 / Enter na busca.
    pub fn find_next(&mut self, doc: &mut Document, backwards: bool) {
        if self.find.query.is_empty() {
            self.open_find(doc, false);
            return;
        }
        self.find.open = true;
        self.refresh_matches(doc);
        let sel = doc.buffer.selection.range();
        let idx = if backwards {
            search::prev_before(&self.find.matches, sel.start)
        } else {
            search::next_from(
                &self.find.matches,
                if sel.is_empty() {
                    sel.start
                } else {
                    sel.start + 1
                },
            )
        };
        self.select_match(doc, idx);
    }

    fn select_match(&mut self, doc: &mut Document, idx: Option<usize>) {
        match idx.and_then(|i| self.find.matches.get(i).cloned()) {
            Some(m) => {
                doc.buffer.set_selection(Selection::new(m.start, m.end));
                self.find.message = None;
                self.scroll = Some(ScrollTarget::Center);
            }
            None => self.find.message = Some("No results".to_owned()),
        }
    }

    fn refresh_matches(&mut self, doc: &Document) {
        let key = (
            doc.buffer.version(),
            self.find.query.clone(),
            self.find.options,
        );
        if self.find.key.as_ref() != Some(&key) {
            self.find.matches =
                search::find_all(doc.buffer.rope(), &self.find.query, self.find.options);
            self.find.key = Some(key);
        }
    }

    fn after_edit(&mut self, target: ScrollTarget) {
        self.preferred_vcol = None;
        self.scroll = Some(target);
    }

    fn max_cols(&mut self, doc: &Document) -> usize {
        let version = doc.buffer.version();
        match self.width_cache {
            Some((v, n)) if v == version => n,
            _ => {
                let n = (0..doc.buffer.line_count())
                    .map(|l| doc.buffer.line_len(l))
                    .max()
                    .unwrap_or(0);
                self.width_cache = Some((version, n));
                n
            }
        }
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        style: &EditorStyle,
        now: f64,
        interactive: bool,
    ) -> EditorOutput {
        if self.find.open {
            self.find_bar(ui, doc, style, now, interactive);
        }
        let (glyph_w, font_h) =
            ui.fonts(|f| (f.glyph_width(&style.font, 'M'), f.row_height(&style.font)));
        let row_h = (font_h * 1.25).round();
        let lines = doc.buffer.line_count();
        let char_w = glyph_w;
        let gutter_w = (lines.to_string().len().max(4) as f32 + 2.0) * char_w;
        let content = vec2(
            gutter_w + (self.max_cols(doc) as f32 + 10.0) * char_w,
            (lines as f32 + 3.0) * row_h,
        );

        egui::ScrollArea::both()
            .id_salt(self.id.with("scroll"))
            .auto_shrink([false; 2])
            .drag_to_scroll(false)
            .show_viewport(ui, |ui, viewport| {
                let (rect, _) = ui.allocate_exact_size(content, Sense::hover());
                let response = ui.interact(rect, self.id, Sense::click_and_drag());
                let geo = Geo {
                    origin: rect.min,
                    text_x: rect.min.x + gutter_w + char_w * 0.75,
                    gutter_left: rect.min.x + viewport.min.x,
                    gutter_w,
                    char_w,
                    row_h,
                    tab: style.tab_size,
                };
                self.page_lines = ((viewport.height() / row_h) as usize)
                    .saturating_sub(1)
                    .max(1);
                // no quadro em que o foco chega, as teclas ainda sao de quem pediu (ex.: o Enter
                // que abriu o arquivo pela arvore) e nao podem ser digitadas aqui
                let just_focused = std::mem::take(&mut self.wants_focus) && interactive;
                if just_focused {
                    ui.memory_mut(|m| m.request_focus(self.id));
                }
                let focused = ui.memory(|m| m.has_focus(self.id));
                if focused {
                    let filter = EventFilter {
                        tab: true,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: true,
                    };
                    ui.memory_mut(|m| m.set_focus_lock_filter(self.id, filter));
                }
                if interactive {
                    self.mouse(ui, &response, doc, &geo, now);
                    if focused && !just_focused {
                        self.keyboard(ui, doc, &geo, now);
                    }
                }
                if let Some(line) = doc.buffer.take_dirty_line() {
                    self.states.invalidate_from(line);
                }
                if self.find.open {
                    self.refresh_matches(doc);
                }
                self.paint(ui, doc, style, &geo, viewport, focused, font_h);
                if let Some(target) = self.scroll.take() {
                    let (line, col) = doc.buffer.line_col(doc.buffer.selection.head);
                    let vcol = ops::vcol_of(&line_chars(doc.buffer.rope(), line), col, geo.tab);
                    let x = geo.text_x + vcol as f32 * char_w;
                    let y = geo.origin.y + line as f32 * row_h;
                    let mut caret = Rect::from_min_max(
                        pos2(x - gutter_w - char_w * 2.0, y),
                        pos2(x + char_w * 4.0, y + row_h),
                    );
                    // centralizar e so na vertical: se o cursor ja esta visivel na horizontal,
                    // usa a faixa visivel atual para a rolagem horizontal nao se mexer
                    let (left, right) = (rect.min.x + viewport.min.x, rect.min.x + viewport.max.x);
                    if (left + gutter_w..right - char_w).contains(&x) {
                        caret.min.x = left;
                        caret.max.x = right;
                    }
                    match target {
                        ScrollTarget::Nearest => {
                            ui.scroll_to_rect(caret.expand2(vec2(0.0, row_h)), None)
                        }
                        ScrollTarget::Center => ui.scroll_to_rect(caret, Some(Align::Center)),
                    }
                }
            });

        let (line, col) = doc.buffer.line_col(doc.buffer.selection.head);
        let vcol = ops::vcol_of(&line_chars(doc.buffer.rope(), line), col, style.tab_size);
        EditorOutput {
            line: line + 1,
            col: vcol + 1,
        }
    }

    fn hit(&self, doc: &Document, g: &Geo, p: Pos2) -> usize {
        let buf = &doc.buffer;
        let line =
            (((p.y - g.origin.y) / g.row_h).floor().max(0.0) as usize).min(buf.line_count() - 1);
        let vcol = ((p.x - g.text_x) / g.char_w).round().max(0.0) as usize;
        buf.line_start(line) + ops::col_of_vcol(&line_chars(buf.rope(), line), vcol, g.tab)
    }

    fn mouse(&mut self, ui: &Ui, response: &Response, doc: &mut Document, g: &Geo, now: f64) {
        let (pos, pressed, down, mods) = ui.input(|i| {
            (
                i.pointer.interact_pos(),
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                i.modifiers,
            )
        });
        let Some(p) = pos else { return };
        if pressed && response.hovered() {
            ui.memory_mut(|m| m.request_focus(self.id));
            let at = self.hit(doc, g, p);
            let count = match self.last_click {
                Some((t, last, n)) if now - t < 0.45 && last == at => n % 3 + 1,
                _ => 1,
            };
            self.last_click = Some((now, at, count));
            self.ctrl_clicked = mods.ctrl || mods.command;
            let buf = &mut doc.buffer;
            let sel = match count {
                2 => {
                    let w = ops::word_at(buf, at);
                    Selection::new(w.start, w.end)
                }
                3 => {
                    let r = ops::line_range(buf, buf.line_of(at));
                    Selection::new(r.start, r.end)
                }
                _ if mods.shift => Selection::new(buf.selection.anchor, at),
                _ => Selection::caret(at),
            };
            buf.set_selection(sel);
            self.preferred_vcol = None;
            self.dragging = count == 1;
        } else if down && self.dragging {
            let at = self.hit(doc, g, p);
            let anchor = doc.buffer.selection.anchor;
            doc.buffer.set_selection(Selection::new(anchor, at));
            if !ui.clip_rect().contains(p) {
                ui.scroll_to_rect(Rect::from_center_size(p, vec2(g.char_w, g.row_h)), None);
            }
        } else if !down {
            self.dragging = false;
        }
    }

    fn keyboard(&mut self, ui: &Ui, doc: &mut Document, g: &Geo, now: f64) {
        let events = ui.input(|i| i.events.clone());
        for event in events {
            match event {
                Event::Text(text) if !text.is_empty() && !text.chars().any(char::is_control) => {
                    doc.buffer.insert(&text, EditKind::Typing, now);
                    self.after_edit(ScrollTarget::Nearest);
                }
                Event::Paste(text) => {
                    let text = text.replace("\r\n", "\n").replace('\r', "\n");
                    doc.buffer.insert(&text, EditKind::Other, now);
                    self.after_edit(ScrollTarget::Nearest);
                }
                Event::Copy | Event::Cut => {
                    let range = doc.buffer.selection.range();
                    if range.is_empty() {
                        continue;
                    }
                    let mut text = doc.buffer.slice(range);
                    if doc.format.line_ending == LineEnding::CrLf {
                        text = text.replace('\n', "\r\n");
                    }
                    ui.ctx().copy_text(text);
                    if matches!(event, Event::Cut) {
                        doc.buffer.insert("", EditKind::Other, now);
                        self.after_edit(ScrollTarget::Nearest);
                    }
                }
                Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => self.key(doc, key, modifiers, g.tab, now),
                _ => {}
            }
        }
    }

    fn key(&mut self, doc: &mut Document, key: Key, m: Modifiers, tab: usize, now: f64) {
        let language = doc.language;
        let buf = &mut doc.buffer;
        let ctrl = m.ctrl || m.command;
        let sel = buf.selection;
        let head = sel.head;
        let (line, col) = buf.line_col(head);
        let mut keep_vcol = false;
        let target = match key {
            Key::ArrowLeft if !shift_or_collapse(m, &sel) => sel.range().start,
            Key::ArrowRight if !shift_or_collapse(m, &sel) => sel.range().end,
            Key::ArrowLeft if ctrl => ops::word_left(buf, head),
            Key::ArrowRight if ctrl => ops::word_right(buf, head),
            Key::ArrowLeft => head.saturating_sub(1),
            Key::ArrowRight => (head + 1).min(buf.len_chars()),
            Key::ArrowUp | Key::ArrowDown | Key::PageUp | Key::PageDown if !ctrl => {
                let page = self.page_lines as isize;
                let delta = match key {
                    Key::ArrowUp => -1,
                    Key::ArrowDown => 1,
                    Key::PageUp => -page,
                    _ => page,
                };
                let vcol = self
                    .preferred_vcol
                    .unwrap_or_else(|| ops::vcol_of(&line_chars(buf.rope(), line), col, tab));
                let to = (line as isize + delta).clamp(0, buf.line_count() as isize - 1) as usize;
                self.preferred_vcol = Some(vcol);
                keep_vcol = true;
                buf.line_start(to) + ops::col_of_vcol(&line_chars(buf.rope(), to), vcol, tab)
            }
            Key::Home if ctrl => 0,
            Key::End if ctrl => buf.len_chars(),
            Key::Home => {
                // primeiro Home vai para o inicio do codigo, o segundo para a coluna 1
                let indent = line_chars(buf.rope(), line)
                    .iter()
                    .take_while(|c| c.is_whitespace())
                    .count();
                buf.line_start(line) + if col == indent { 0 } else { indent }
            }
            Key::End => buf.pos(line, usize::MAX),
            Key::A if ctrl => {
                buf.set_selection(Selection::new(0, buf.len_chars()));
                return;
            }
            Key::Backspace if !m.alt => {
                return self.edit(|b| ops::backspace(b, ctrl, tab, now), doc);
            }
            Key::Delete if !m.shift => {
                return self.edit(|b| ops::delete_forward(b, ctrl, now), doc);
            }
            Key::Enter => return self.edit(|b| ops::newline(b, language, tab, now), doc),
            Key::Tab if m.shift => return self.edit(|b| ops::unindent(b, tab, now), doc),
            Key::Tab if ops::selected_lines(buf).len() > 1 => {
                return self.edit(|b| ops::indent(b, tab, now), doc);
            }
            Key::Tab => return self.edit(|b| ops::insert_tab(b, tab, now), doc),
            Key::I if ctrl && m.shift => return self.edit(|b| ops::indent(b, tab, now), doc),
            Key::U if ctrl && m.shift => return self.edit(|b| ops::unindent(b, tab, now), doc),
            Key::Escape if self.find.open => {
                self.find.open = false;
                return;
            }
            _ => return,
        };
        let anchor = if m.shift { sel.anchor } else { target };
        buf.set_selection(Selection::new(anchor, target));
        if !keep_vcol {
            self.preferred_vcol = None;
        }
        self.scroll = Some(ScrollTarget::Nearest);
    }

    fn edit(&mut self, f: impl FnOnce(&mut super::buffer::TextBuffer), doc: &mut Document) {
        f(&mut doc.buffer);
        self.after_edit(ScrollTarget::Nearest);
    }

    #[allow(clippy::too_many_arguments)]
    fn paint(
        &mut self,
        ui: &Ui,
        doc: &Document,
        style: &EditorStyle,
        g: &Geo,
        viewport: Rect,
        focused: bool,
        font_h: f32,
    ) {
        let p = &style.palette;
        let buf = &doc.buffer;
        let clip = ui.clip_rect();
        let painter = ui.painter();
        let first = (viewport.min.y / g.row_h).floor().max(0.0) as usize;
        let last = ((viewport.max.y / g.row_h).ceil() as usize).min(buf.line_count() - 1);
        let text_clip = Rect::from_min_max(pos2(g.gutter_left + g.gutter_w, clip.min.y), clip.max)
            .intersect(clip);
        let tp = painter.with_clip_rect(text_clip);
        let sel = buf.selection.range();
        let (caret_line, caret_col) = buf.line_col(buf.selection.head);
        let x_of = |chars: &[char], col: usize| {
            g.text_x + ops::vcol_of(chars, col, g.tab) as f32 * g.char_w
        };
        let y_pad = ((g.row_h - font_h) / 2.0).round();

        if (first..=last).contains(&caret_line) {
            let y = g.origin.y + caret_line as f32 * g.row_h;
            let r = Rect::from_min_size(pos2(text_clip.min.x, y), vec2(text_clip.width(), g.row_h));
            tp.rect_filled(
                r,
                CornerRadius::ZERO,
                themes::mix(p.bg_editor, p.selection, 0.25),
            );
        }

        let matches = if self.find.open {
            self.find.matches.as_slice()
        } else {
            &[]
        };
        let mut mi = matches.partition_point(|m| m.end <= buf.line_start(first));
        let mut tokens = Vec::new();
        for line in first..=last {
            let y = g.origin.y + line as f32 * g.row_h;
            let chars = line_chars(buf.rope(), line);
            let start = buf.line_start(line);
            let end = start + chars.len();

            if !sel.is_empty() && sel.start <= end && sel.end > start {
                let x0 = x_of(&chars, sel.start.max(start) - start);
                let mut x1 = x_of(&chars, sel.end.min(end) - start);
                if sel.end > end {
                    x1 += g.char_w * 0.6;
                }
                let r = Rect::from_min_max(pos2(x0, y), pos2(x1, y + g.row_h));
                tp.rect_filled(r, CornerRadius::ZERO, p.selection);
            }
            while mi < matches.len() && matches[mi].start < end {
                let m = &matches[mi];
                let r = Rect::from_min_max(
                    pos2(x_of(&chars, m.start - start), y),
                    pos2(x_of(&chars, m.end - start), y + g.row_h),
                );
                if *m == sel {
                    tp.rect_stroke(
                        r,
                        CornerRadius::ZERO,
                        Stroke::new(1.5_f32, p.accent),
                        egui::StrokeKind::Inside,
                    );
                } else {
                    tp.rect_filled(r, CornerRadius::ZERO, p.accent.gamma_multiply(0.28));
                }
                mi += 1;
            }

            let state = self.states.state_at(doc.language, buf.rope(), line);
            lex_line(doc.language, &chars, state, &mut tokens);
            let mut job = LayoutJob::default();
            let mut vcol = 0;
            for t in &tokens {
                let mut text = String::with_capacity(t.end - t.start);
                for &c in &chars[t.start..t.end] {
                    if c == '\t' {
                        let n = g.tab - vcol % g.tab;
                        text.extend(std::iter::repeat_n(' ', n));
                        vcol += n;
                    } else {
                        text.push(if c.is_control() { '·' } else { c });
                        vcol += 1;
                    }
                }
                job.append(
                    &text,
                    0.0,
                    egui::TextFormat::simple(style.font.clone(), themes::syntax_color(p, t.kind)),
                );
            }
            let galley = ui.fonts(|f| f.layout_job(job));
            tp.galley(pos2(g.text_x, y + y_pad), galley, p.text);
        }

        if (first..=last).contains(&caret_line) {
            let chars = line_chars(buf.rope(), caret_line);
            let x = x_of(&chars, caret_col);
            let y = g.origin.y + caret_line as f32 * g.row_h;
            let color = if focused {
                p.text
            } else {
                p.text_weak.gamma_multiply(0.6)
            };
            tp.rect_filled(
                Rect::from_min_size(pos2(x - 1.0, y + 1.0), vec2(2.0, g.row_h - 2.0)),
                CornerRadius::ZERO,
                color,
            );
        }

        let gutter = Rect::from_min_size(
            pos2(g.gutter_left, clip.min.y),
            vec2(g.gutter_w, clip.height()),
        );
        painter.rect_filled(gutter, CornerRadius::ZERO, p.bg_panel);
        painter.vline(
            gutter.max.x,
            gutter.y_range(),
            Stroke::new(1.0_f32, p.border),
        );
        for line in first..=last {
            let y = g.origin.y + line as f32 * g.row_h + y_pad;
            let color = if line == caret_line {
                p.text
            } else {
                p.text_weak.gamma_multiply(0.7)
            };
            painter.text(
                pos2(gutter.max.x - g.char_w, y),
                egui::Align2::RIGHT_TOP,
                (line + 1).to_string(),
                style.font.clone(),
                color,
            );
        }
    }

    fn find_bar(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        style: &EditorStyle,
        now: f64,
        interactive: bool,
    ) {
        let p = style.palette;
        let mut action = None;
        self.refresh_matches(doc);
        let current = self
            .find
            .matches
            .iter()
            .position(|m| *m == doc.buffer.selection.range());
        egui::Frame::new()
            .fill(p.bg_panel)
            .inner_margin(Margin::symmetric(8, 4))
            .stroke(Stroke::new(1.0_f32, p.border))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    let field = egui::TextEdit::singleline(&mut self.find.query)
                        .id(self.id.with("find_query"))
                        .hint_text("Find")
                        .font(egui::TextStyle::Monospace)
                        .desired_width(260.0);
                    let q = ui.add(field);
                    if std::mem::take(&mut self.find.focus_query) {
                        q.request_focus();
                    }
                    if q.changed() {
                        action = Some(FindAction::Incremental);
                    }
                    let (enter, escape, shift) = ui.input(|i| {
                        (
                            i.key_pressed(Key::Enter),
                            i.key_pressed(Key::Escape),
                            i.modifiers.shift,
                        )
                    });
                    if q.lost_focus() && enter {
                        action = Some(if shift {
                            FindAction::Prev
                        } else {
                            FindAction::Next
                        });
                        q.request_focus();
                    }
                    if q.lost_focus() && escape {
                        action = Some(FindAction::Close);
                    }
                    if ui
                        .small_button("▲")
                        .on_hover_text("Previous (Shift+Enter)")
                        .clicked()
                    {
                        action = Some(FindAction::Prev);
                    }
                    if ui
                        .small_button("▼")
                        .on_hover_text("Next (Enter / F3)")
                        .clicked()
                    {
                        action = Some(FindAction::Next);
                    }
                    let case = ui
                        .toggle_value(&mut self.find.options.case_sensitive, "Aa")
                        .on_hover_text("Match case");
                    let word = ui
                        .toggle_value(&mut self.find.options.whole_word, "W")
                        .on_hover_text("Whole word");
                    if case.changed() || word.changed() {
                        action = Some(FindAction::Incremental);
                    }
                    let label = if self.find.replace_open {
                        "Replace ▴"
                    } else {
                        "Replace ▾"
                    };
                    if ui.small_button(label).clicked() {
                        self.find.replace_open = !self.find.replace_open;
                    }
                    let status = match (&self.find.message, current, self.find.matches.len()) {
                        (Some(msg), _, _) => msg.clone(),
                        (None, _, 0) if self.find.query.is_empty() => String::new(),
                        (None, _, 0) => "No results".to_owned(),
                        (None, Some(i), n) => format!("{} of {n}", i + 1),
                        (None, None, n) => format!("{n} found"),
                    };
                    ui.label(RichText::new(status).size(12.0).color(p.text_weak));
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        if crate::ui::close_button(ui, "Close (Esc)") {
                            action = Some(FindAction::Close);
                        }
                    });
                });
                if self.find.replace_open {
                    ui.horizontal(|ui| {
                        let field = egui::TextEdit::singleline(&mut self.find.replacement)
                            .hint_text("Replace with")
                            .font(egui::TextStyle::Monospace)
                            .desired_width(260.0);
                        ui.add(field);
                        if ui
                            .small_button("Replace")
                            .on_hover_text("Replace the selected match and find the next")
                            .clicked()
                        {
                            action = Some(FindAction::ReplaceOne);
                        }
                        if ui.small_button("Replace All").clicked() {
                            action = Some(FindAction::ReplaceAll);
                        }
                    });
                }
            });
        if !interactive {
            return;
        }
        match action {
            Some(FindAction::Incremental) => {
                self.find.message = None;
                self.refresh_matches(doc);
                let idx = search::next_from(&self.find.matches, self.find.origin);
                self.select_match(doc, idx);
            }
            Some(FindAction::Next) => self.find_next(doc, false),
            Some(FindAction::Prev) => self.find_next(doc, true),
            Some(FindAction::ReplaceOne) => {
                self.refresh_matches(doc);
                let sel = doc.buffer.selection.range();
                if self.find.matches.contains(&sel) {
                    doc.buffer
                        .replace(sel, &self.find.replacement.clone(), EditKind::Other, now);
                    self.refresh_matches(doc);
                    let idx = search::next_from(&self.find.matches, doc.buffer.selection.head);
                    self.select_match(doc, idx);
                } else {
                    self.find_next(doc, false);
                }
            }
            Some(FindAction::ReplaceAll) => {
                self.refresh_matches(doc);
                let n = self.find.matches.len();
                let buf = &mut doc.buffer;
                let edits: Vec<Edit> = self
                    .find
                    .matches
                    .iter()
                    .rev()
                    .map(|m| Edit {
                        at: m.start,
                        removed: buf.slice(m.clone()),
                        inserted: self.find.replacement.clone(),
                    })
                    .collect();
                let caret = Selection::caret(buf.selection.head.min(buf.len_chars()));
                buf.transact(edits, caret, EditKind::Other, now);
                self.find.message = Some(format!("Replaced {n} occurrence(s)"));
                self.after_edit(ScrollTarget::Nearest);
            }
            Some(FindAction::Close) => {
                self.find.open = false;
                self.wants_focus = true;
            }
            None => {}
        }
    }
}

/// Setas sem Shift com selecao ativa apenas recolhem a selecao (como no Delphi).
fn shift_or_collapse(m: Modifiers, sel: &Selection) -> bool {
    m.shift || sel.is_empty()
}
