//! Widget do editor. Virtualizado: a cada quadro so as linhas visiveis sao lidas do rope,
//! tokenizadas e desenhadas, entao o custo nao depende do tamanho do arquivo.
//!
//! Regioes recolhidas (code folding) somem da conta de linhas visiveis; o popup de
//! autocompletar e desenhado por cima, junto ao cursor. Varias vistas podem mostrar o mesmo
//! documento (editor dividido): cada uma acompanha o texto pelo registro de alteracoes.

use eframe::egui::{
    self, Align, CornerRadius, Event, EventFilter, FontId, Id, Key, Margin, Modifiers, Pos2, Rect,
    Response, RichText, Sense, Stroke, Ui, pos2, text::LayoutJob, vec2,
};

use super::buffer::{Edit, EditKind, Selection};
use super::document::Document;
use super::folding::Folding;
use super::ops;
use super::search::{self, SearchOptions};
use crate::autocomplete::{self, CompletionItem, ItemKind};
use crate::filesystem::encoding::LineEnding;
use crate::syntax::{Language, LineStates, is_ident_char, lex_line, line_chars};
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
    /// Coluna das marcas de folding, na direita da margem (0 sem regioes).
    fold_w: f32,
    char_w: f32,
    row_h: f32,
    tab: usize,
}

/// Lista de sugestoes aberta (Ctrl+Space ou depois de `.`).
pub struct CompletionPopup {
    items: Vec<CompletionItem>,
    /// Posicao (caractere) onde comeca o identificador que sera substituido.
    start: usize,
    shown: Vec<usize>,
    selected: usize,
    offset: f32,
    follow: bool,
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
    pub folding: Folding,
    /// Versao do buffer ja vista (realce e regioes acompanham as alteracoes a partir dela).
    seen_version: u64,
    pub completion: Option<CompletionPopup>,
    /// Digitou `.` depois de um identificador: o app abre o autocompletar.
    pub completion_request: bool,
    /// Abrir o autocompletar sozinho depois de `.` (Tools ▸ Options no Delphi).
    pub auto_completion: bool,
    caret_screen: Option<Rect>,
    /// Caixas "..." das regioes recolhidas no ultimo quadro (clique expande).
    fold_boxes: Vec<(Rect, usize)>,
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
            folding: Folding::default(),
            seen_version: 0,
            completion: None,
            completion_request: false,
            auto_completion: true,
            caret_screen: None,
            fold_boxes: Vec::new(),
        }
    }

    /// Depois de recarregar o documento (caches dependem da versao do buffer).
    pub fn reset(&mut self) {
        self.states.clear();
        self.width_cache = None;
        self.find.key = None;
        self.preferred_vcol = None;
        self.folding.clear();
        self.seen_version = 0;
        self.completion = None;
    }

    /// Regioes recalculadas em segundo plano para a versao `version` do documento.
    pub fn update_folds(
        &mut self,
        doc: &Document,
        ranges: &[crate::syntax::folding::FoldRange],
        version: u64,
    ) {
        self.folding.sync(&doc.buffer);
        if !self.folding.disabled {
            self.folding.set_ranges(ranges, version);
        }
    }

    /// Mantem realce e regioes em dia com o texto (inclusive edicoes feitas por outra vista).
    fn sync(&mut self, doc: &mut Document) {
        let version = doc.buffer.version();
        if version != self.seen_version {
            match doc.buffer.changes_since(self.seen_version) {
                Some(changes) => {
                    if let Some(line) = changes.iter().map(|c| c.line).min() {
                        self.states.invalidate_from(line);
                    }
                }
                None => self.states.clear(),
            }
            self.seen_version = version;
        }
        let _ = doc.buffer.take_dirty_line();
        self.folding.sync(&doc.buffer);
    }

    /// Liga/desliga o code folding desta vista (Shift+Ctrl+K, O).
    pub fn set_folding_enabled(&mut self, enabled: bool) {
        self.folding.disabled = !enabled;
        if !enabled {
            self.folding.clear();
        }
    }

    /// Abre a lista de sugestoes; `start` e onde comeca o que sera substituido.
    pub fn open_completion(
        &mut self,
        doc: &Document,
        items: Vec<CompletionItem>,
        start: usize,
    ) -> bool {
        self.completion = Some(CompletionPopup {
            items,
            start,
            shown: Vec::new(),
            selected: 0,
            offset: 0.0,
            follow: true,
        });
        self.refilter(doc);
        self.completion.is_some()
    }

    pub fn close_completion(&mut self) {
        self.completion = None;
    }

    /// Itens que combinam com o que foi digitado desde `start` (fecha se nada combina).
    fn refilter(&mut self, doc: &Document) {
        let Some(popup) = &mut self.completion else {
            return;
        };
        let head = doc.buffer.selection.head;
        if head < popup.start || doc.buffer.selection.anchor != head {
            self.completion = None;
            return;
        }
        let typed = doc.buffer.slice(popup.start..head);
        if typed.chars().any(|c| !is_ident_char(c)) {
            self.completion = None;
            return;
        }
        popup.shown = autocomplete::filter(&popup.items, &typed);
        if popup.shown.is_empty() {
            self.completion = None;
            return;
        }
        popup.selected = popup.selected.min(popup.shown.len() - 1);
        popup.follow = true;
    }

    fn accept_completion(&mut self, doc: &mut Document, now: f64) {
        let Some(popup) = self.completion.take() else {
            return;
        };
        let Some(item) = popup.shown.get(popup.selected).map(|&i| &popup.items[i]) else {
            return;
        };
        let head = doc.buffer.selection.head.max(popup.start);
        doc.buffer
            .replace(popup.start..head, &item.label, EditKind::Other, now);
        self.after_edit(ScrollTarget::Nearest);
    }

    /// Teclas que o popup usa; devolve true se a tecla foi consumida.
    fn completion_key(&mut self, doc: &mut Document, key: Key, m: Modifiers, now: f64) -> bool {
        let Some(popup) = &mut self.completion else {
            return false;
        };
        if m.ctrl || m.alt || m.command {
            return false;
        }
        let n = popup.shown.len();
        let page = 10;
        match key {
            Key::ArrowDown => popup.selected = (popup.selected + 1) % n,
            Key::ArrowUp => popup.selected = (popup.selected + n - 1) % n,
            Key::PageDown => popup.selected = (popup.selected + page).min(n - 1),
            Key::PageUp => popup.selected = popup.selected.saturating_sub(page),
            Key::Enter | Key::Tab => {
                self.accept_completion(doc, now);
                return true;
            }
            Key::Escape => {
                self.completion = None;
                return true;
            }
            _ => return false,
        }
        popup.follow = true;
        true
    }

    pub fn request_focus(&mut self) {
        self.wants_focus = true;
    }

    /// Vista criada em segundo plano (outro painel do editor dividido): nao rouba o foco.
    pub fn release_focus(&mut self) {
        self.wants_focus = false;
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
        self.folding.reveal(doc.buffer.line_of(pos));
        self.after_edit(ScrollTarget::Center);
        self.wants_focus = true;
    }

    /// Seleciona um trecho de uma linha (resultado do Find in Files), centralizado.
    pub fn select_range(&mut self, doc: &mut Document, line: usize, start: usize, end: usize) {
        let a = doc.buffer.pos(line, start);
        let b = doc.buffer.pos(line, end);
        doc.buffer.set_selection(Selection::new(a, b));
        self.folding.reveal(line);
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

    /// Centraliza o cursor no proximo quadro (depois de mudar o que esta recolhido).
    pub fn scroll_to_caret(&mut self) {
        self.scroll = Some(ScrollTarget::Center);
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
        self.sync(doc);
        let caret_line = doc.buffer.line_of(doc.buffer.selection.head);
        if self.folding.is_hidden(caret_line) {
            self.folding.reveal(caret_line);
        }
        let (glyph_w, font_h) =
            ui.fonts(|f| (f.glyph_width(&style.font, 'M'), f.row_height(&style.font)));
        let row_h = (font_h * 1.25).round();
        let lines = doc.buffer.line_count();
        let rows = self.folding.visible_rows(lines).max(1);
        let char_w = glyph_w;
        let fold_w = if self.folding.ranges().is_empty() {
            0.0
        } else {
            (char_w * 1.6).round()
        };
        let gutter_w = (lines.to_string().len().max(4) as f32 + 2.0) * char_w + fold_w;
        let content = vec2(
            gutter_w + (self.max_cols(doc) as f32 + 10.0) * char_w,
            (rows as f32 + 3.0) * row_h,
        );
        self.caret_screen = None;

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
                    fold_w,
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
                if !focused && self.completion.is_some() {
                    self.completion = None;
                }
                self.sync(doc);
                let caret_line = doc.buffer.line_of(doc.buffer.selection.head);
                if self.folding.is_hidden(caret_line) {
                    self.folding.reveal(caret_line);
                }
                if self.find.open {
                    self.refresh_matches(doc);
                }
                self.paint(ui, doc, style, &geo, viewport, focused, font_h);
                if let Some(target) = self.scroll.take() {
                    let (line, col) = doc.buffer.line_col(doc.buffer.selection.head);
                    let vcol = ops::vcol_of(&line_chars(doc.buffer.rope(), line), col, geo.tab);
                    let x = geo.text_x + vcol as f32 * char_w;
                    let y = geo.origin.y + self.folding.row_of(line) as f32 * row_h;
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

        if self.completion.is_some() && interactive {
            self.completion_ui(ui, doc, style, now);
        }
        let (line, col) = doc.buffer.line_col(doc.buffer.selection.head);
        let vcol = ops::vcol_of(&line_chars(doc.buffer.rope(), line), col, style.tab_size);
        EditorOutput {
            line: line + 1,
            col: vcol + 1,
        }
    }

    fn completion_ui(&mut self, ui: &Ui, doc: &mut Document, style: &EditorStyle, now: f64) {
        let (Some(anchor), Some(popup)) = (self.caret_screen, &mut self.completion) else {
            return;
        };
        let p = &style.palette;
        let font = FontId::monospace(style.font.size.min(14.0) - 1.0);
        let row_h = (ui.fonts(|f| f.row_height(&font)) * 1.35).round();
        let visible = popup.shown.len().min(12);
        let view_h = visible as f32 * row_h;
        let width = 520.0_f32;
        if popup.follow {
            let top = popup.selected as f32 * row_h;
            if top < popup.offset {
                popup.offset = top;
            } else if top + row_h > popup.offset + view_h {
                popup.offset = top + row_h - view_h;
            }
        }
        let screen = ui.ctx().screen_rect();
        let below = anchor.max.y + 2.0;
        let pos = if below + view_h + 12.0 > screen.max.y {
            pos2(anchor.min.x, anchor.min.y - view_h - 14.0)
        } else {
            pos2(anchor.min.x, below)
        };
        let mut accept = false;
        let follow = std::mem::take(&mut popup.follow);
        egui::Area::new(self.id.with("completion"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .constrain(true)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style())
                    .fill(p.bg_panel)
                    .stroke(Stroke::new(1.0_f32, p.accent.gamma_multiply(0.6)))
                    .inner_margin(Margin::same(3))
                    .show(ui, |ui| {
                        let mut area = egui::ScrollArea::vertical()
                            .id_salt(self.id.with("completion_scroll"))
                            .max_height(view_h)
                            // altura fixa: no primeiro quadro o egui ainda nao sabe o tamanho da lista
                            .min_scrolled_height(view_h)
                            .auto_shrink([false, true]);
                        if follow {
                            area = area.vertical_scroll_offset(popup.offset);
                        }
                        let out = area.show_rows(ui, row_h, popup.shown.len(), |ui, range| {
                            for i in range {
                                let item = &popup.items[popup.shown[i]];
                                let (rect, resp) =
                                    ui.allocate_exact_size(vec2(width, row_h), Sense::click());
                                resp.widget_info(|| {
                                    egui::WidgetInfo::selected(
                                        egui::WidgetType::SelectableLabel,
                                        true,
                                        i == popup.selected,
                                        format!("{} {}", item.kind.label(), item.label),
                                    )
                                });
                                let painter = ui.painter();
                                if i == popup.selected {
                                    painter.rect_filled(rect, CornerRadius::same(2), p.selection);
                                } else if resp.hovered() {
                                    painter.rect_filled(rect, CornerRadius::same(2), p.hover());
                                }
                                let y = rect.center().y;
                                painter.text(
                                    pos2(rect.min.x + 6.0, y),
                                    egui::Align2::LEFT_CENTER,
                                    item.kind.label(),
                                    font.clone(),
                                    kind_color(p, item.kind),
                                );
                                let name = painter.text(
                                    pos2(rect.min.x + 96.0, y),
                                    egui::Align2::LEFT_CENTER,
                                    &item.label,
                                    font.clone(),
                                    p.text,
                                );
                                // a origem fica a direita; o detalhe e cortado antes dela
                                let source = if item.source.is_empty() {
                                    Rect::from_min_max(rect.right_top(), rect.right_bottom())
                                } else {
                                    painter.text(
                                        pos2(rect.max.x - 6.0, y),
                                        egui::Align2::RIGHT_CENTER,
                                        &item.source,
                                        font.clone(),
                                        p.text_weak.gamma_multiply(0.75),
                                    )
                                };
                                let room = Rect::from_min_max(
                                    rect.min,
                                    pos2(source.min.x - 10.0, rect.max.y),
                                );
                                painter
                                    .with_clip_rect(room.intersect(painter.clip_rect()))
                                    .text(
                                        pos2(name.max.x + 4.0, y),
                                        egui::Align2::LEFT_CENTER,
                                        &item.detail,
                                        font.clone(),
                                        p.text_weak,
                                    );
                                if resp.clicked() {
                                    popup.selected = i;
                                }
                                if resp.double_clicked() {
                                    popup.selected = i;
                                    accept = true;
                                }
                            }
                        });
                        popup.offset = out.state.offset.y;
                    });
            });
        if accept {
            self.accept_completion(doc, now);
            self.wants_focus = true;
        }
    }

    fn line_at(&self, doc: &Document, g: &Geo, y: f32) -> usize {
        let row = ((y - g.origin.y) / g.row_h).floor().max(0.0) as usize;
        self.folding.line_of(row).min(doc.buffer.line_count() - 1)
    }

    fn hit(&self, doc: &Document, g: &Geo, p: Pos2) -> usize {
        let buf = &doc.buffer;
        let line = self.line_at(doc, g, p.y);
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
            self.completion = None;
            // marca de folding na margem ou a caixa "..." de uma regiao recolhida
            let in_fold_column = g.fold_w > 0.0
                && p.x >= g.gutter_left + g.gutter_w - g.fold_w
                && p.x < g.gutter_left + g.gutter_w;
            if in_fold_column {
                let line = self.line_at(doc, g, p.y);
                if self.folding.toggle(line) {
                    return;
                }
            }
            if let Some(&(_, start)) = self.fold_boxes.iter().find(|(r, _)| r.contains(p)) {
                self.folding.unfold(start);
                return;
            }
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
            if let Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } = &event
            {
                if self.completion_key(doc, *key, *modifiers, now) {
                    continue;
                }
            }
            match event {
                Event::Text(text) if !text.is_empty() && !text.chars().any(char::is_control) => {
                    let head = doc.buffer.selection.head;
                    let before = head.checked_sub(1).and_then(|i| doc.buffer.char_at(i));
                    doc.buffer.insert(&text, EditKind::Typing, now);
                    self.after_edit(ScrollTarget::Nearest);
                    if self.completion.is_some() {
                        self.refilter(doc);
                    }
                    if text == "."
                        && self.auto_completion
                        && doc.language == Language::Pascal
                        && before.is_some_and(|c| is_ident_char(c) || c == ')' || c == ']')
                    {
                        self.completion = None;
                        self.completion_request = true;
                    }
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
                } => {
                    self.key(doc, key, modifiers, g.tab, now);
                    if self.completion.is_some() {
                        self.refilter(doc);
                    }
                }
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
        let mut target = match key {
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
                // anda por linhas visiveis: regioes recolhidas contam como uma linha
                let rows = self.folding.visible_rows(buf.line_count()) as isize;
                let row = (self.folding.row_of(line) as isize + delta).clamp(0, rows - 1);
                let to = self.folding.line_of(row as usize).min(buf.line_count() - 1);
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
        // setas laterais pulam regioes recolhidas em vez de entrar nelas
        let to_line = buf.line_of(target);
        if self.folding.is_hidden(to_line) {
            let shown = self.folding.line_of(self.folding.row_of(to_line));
            target = if target > head {
                buf.line_start(self.folding.next_visible(shown))
            } else {
                buf.pos(shown, usize::MAX)
            };
        }
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
        let rows = self.folding.visible_rows(buf.line_count()).max(1);
        let first_row = ((viewport.min.y / g.row_h).floor().max(0.0) as usize).min(rows - 1);
        let last_row = ((viewport.max.y / g.row_h).ceil() as usize).min(rows - 1);
        // linhas do documento mostradas, com a linha visivel de cada uma
        let mut shown = Vec::with_capacity(last_row - first_row + 1);
        let mut l = self.folding.line_of(first_row);
        for row in first_row..=last_row {
            if l >= buf.line_count() {
                break;
            }
            shown.push((row, l));
            l = self.folding.next_visible(l);
        }
        let y_of = |row: usize| g.origin.y + row as f32 * g.row_h;
        let text_clip = Rect::from_min_max(pos2(g.gutter_left + g.gutter_w, clip.min.y), clip.max)
            .intersect(clip);
        let tp = painter.with_clip_rect(text_clip);
        let sel = buf.selection.range();
        let (caret_line, caret_col) = buf.line_col(buf.selection.head);
        let x_of = |chars: &[char], col: usize| {
            g.text_x + ops::vcol_of(chars, col, g.tab) as f32 * g.char_w
        };
        let y_pad = ((g.row_h - font_h) / 2.0).round();

        let caret_row = shown.iter().find(|s| s.1 == caret_line).map(|s| s.0);
        if let Some(row) = caret_row {
            let y = y_of(row);
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
        let first_line = shown.first().map_or(0, |s| s.1);
        let mut mi = matches.partition_point(|m| m.end <= buf.line_start(first_line));
        let mut tokens = Vec::new();
        self.fold_boxes.clear();
        for &(row, line) in &shown {
            let y = y_of(row);
            let chars = line_chars(buf.rope(), line);
            let start = buf.line_start(line);
            let end = start + chars.len();
            while mi < matches.len() && matches[mi].end <= start {
                mi += 1;
            }

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
            let text_w = galley.size().x;
            tp.galley(pos2(g.text_x, y + y_pad), galley, p.text);
            if self.folding.is_folded(line) {
                let r = Rect::from_min_size(
                    pos2(g.text_x + text_w + g.char_w, y + 2.0),
                    vec2(g.char_w * 4.0, g.row_h - 4.0),
                );
                tp.rect_filled(r, CornerRadius::same(3), p.selection.gamma_multiply(0.7));
                tp.rect_stroke(
                    r,
                    CornerRadius::same(3),
                    Stroke::new(1.0_f32, p.accent.gamma_multiply(0.7)),
                    egui::StrokeKind::Inside,
                );
                tp.text(
                    r.center(),
                    egui::Align2::CENTER_CENTER,
                    "...",
                    style.font.clone(),
                    p.text_weak,
                );
                self.fold_boxes.push((r, line));
            }
        }

        if let Some(row) = caret_row {
            let chars = line_chars(buf.rope(), caret_line);
            let x = x_of(&chars, caret_col);
            let y = y_of(row);
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
            self.caret_screen = Some(Rect::from_min_size(pos2(x, y), vec2(2.0, g.row_h)));
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
        for &(row, line) in &shown {
            let y = y_of(row) + y_pad;
            let color = if line == caret_line {
                p.text
            } else {
                p.text_weak.gamma_multiply(0.7)
            };
            painter.text(
                pos2(gutter.max.x - g.fold_w - g.char_w, y),
                egui::Align2::RIGHT_TOP,
                (line + 1).to_string(),
                style.font.clone(),
                color,
            );
            if g.fold_w > 0.0 && self.folding.range_at(line).is_some() {
                // caixa [-] / [+] como no Delphi
                let c = pos2(gutter.max.x - g.fold_w / 2.0, y_of(row) + g.row_h / 2.0);
                let half = (g.char_w * 0.45).round();
                let r = Rect::from_center_size(c, vec2(half * 2.0, half * 2.0));
                let folded = self.folding.is_folded(line);
                let stroke = Stroke::new(1.0_f32, if folded { p.accent } else { p.text_weak });
                painter.rect_stroke(r, CornerRadius::same(2), stroke, egui::StrokeKind::Inside);
                painter.hline(r.x_range().shrink(half * 0.45), c.y, stroke);
                if folded {
                    painter.vline(c.x, r.y_range().shrink(half * 0.45), stroke);
                }
            }
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

fn kind_color(p: &Palette, kind: ItemKind) -> egui::Color32 {
    match kind {
        ItemKind::Keyword => themes::syntax_color(p, crate::syntax::TokenKind::Keyword),
        ItemKind::Class | ItemKind::Record | ItemKind::Interface | ItemKind::Type => p.warning,
        ItemKind::Procedure | ItemKind::Function | ItemKind::Constructor => p.success,
        ItemKind::Property | ItemKind::Field => p.accent,
        ItemKind::Variable | ItemKind::Parameter | ItemKind::Constant | ItemKind::EnumValue => {
            themes::syntax_color(p, crate::syntax::TokenKind::Directive)
        }
        ItemKind::Unit | ItemKind::Word => p.text_weak,
    }
}

/// Setas sem Shift com selecao ativa apenas recolhem a selecao (como no Delphi).
fn shift_or_collapse(m: Modifiers, sel: &Selection) -> bool {
    m.shift || sel.is_empty()
}
