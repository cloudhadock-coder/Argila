//! Componentes de interface. So desenham e devolvem o que o usuario pediu; quem executa e o `app`.

pub mod console;
pub mod dialogs;
pub mod explorer;
pub mod inspector;
pub mod statusbar;
pub mod toolbar;
pub mod welcome;

use eframe::egui::{self, Align, Layout, Margin, Response, RichText, Stroke, Ui};

use crate::shortcuts::Command;
use crate::themes::Palette;

/// O que um painel pediu neste quadro.
#[derive(Default)]
pub struct PanelOutput {
    pub close: bool,
    pub command: Option<Command>,
}

/// Aba no estilo da IDE: texto + sublinhado na cor de destaque quando ativa.
pub fn tab_button(ui: &mut Ui, p: &Palette, selected: bool, label: &str) -> Response {
    let text = RichText::new(label).color(if selected { p.text } else { p.text_weak });
    let response = ui.selectable_label(selected, text);
    if selected {
        let r = response.rect;
        ui.painter().hline(
            r.x_range(),
            r.bottom() + 1.0,
            Stroke::new(2.0_f32, p.accent),
        );
    }
    response
}

/// Faixa de abas no topo de um painel; `extra` desenha botoes a direita (antes do fechar).
/// Devolve true quando o usuario pede para esconder o painel.
pub fn panel_tabs<T: PartialEq + Copy>(
    ui: &mut Ui,
    p: &Palette,
    current: &mut T,
    tabs: &[(T, String)],
    extra: impl FnOnce(&mut Ui),
) -> bool {
    let mut close = false;
    header_frame(p).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.horizontal(|ui| {
            for (tab, label) in tabs {
                if tab_button(ui, p, *current == *tab, label).clicked() {
                    *current = *tab;
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                close = close_button(ui, "Hide panel");
                extra(ui);
            });
        });
    });
    close
}

/// Cabecalho com titulo simples (Object Inspector). Devolve true para esconder.
pub fn panel_title(ui: &mut Ui, p: &Palette, title: &str) -> bool {
    let mut close = false;
    header_frame(p).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new(title).strong().color(p.text));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                close = close_button(ui, "Hide panel");
            });
        });
    });
    close
}

fn header_frame(p: &Palette) -> egui::Frame {
    egui::Frame::new()
        .fill(p.header())
        .inner_margin(Margin::symmetric(8, 4))
        .stroke(Stroke::new(1.0_f32, p.border))
}

pub fn close_button(ui: &mut Ui, hint: &str) -> bool {
    ui.add(egui::Button::new(RichText::new("×").size(15.0)).frame(false))
        .on_hover_text(hint)
        .clicked()
}

/// Mensagem de "nada aqui ainda", centralizada e discreta.
pub fn empty_state(ui: &mut Ui, p: &Palette, title: &str, hint: &str) {
    ui.add_space(18.0);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new(title).color(p.text_weak).strong());
        ui.add_space(2.0);
        ui.label(RichText::new(hint).color(p.text_weak).size(12.0));
    });
}

/// Botao que dispara um comando, com o atalho como dica.
pub fn command_button(ui: &mut Ui, label: &str, shortcut: &str) -> bool {
    let response = ui.button(label);
    if shortcut.is_empty() {
        response.clicked()
    } else {
        response.on_hover_text(shortcut).clicked()
    }
}
