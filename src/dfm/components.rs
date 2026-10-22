//! Approximate VCL layout, independent of the GUI and native Windows controls.
use super::parser::{Component, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Form,
    Panel,
    Button,
    Edit,
    Label,
    Memo,
    ComboBox,
    CheckBox,
    RadioButton,
    ListBox,
    GroupBox,
    PageControl,
    TabSheet,
    DBGrid,
    DBEdit,
    Unknown,
}
impl Control {
    pub fn of(class: &str) -> Self {
        match class.to_ascii_lowercase().as_str() {
            "tform" => Self::Form,
            "tpanel" => Self::Panel,
            "tbutton" => Self::Button,
            "tedit" => Self::Edit,
            "tlabel" => Self::Label,
            "tmemo" => Self::Memo,
            "tcombobox" => Self::ComboBox,
            "tcheckbox" => Self::CheckBox,
            "tradiobutton" => Self::RadioButton,
            "tlistbox" => Self::ListBox,
            "tgroupbox" => Self::GroupBox,
            "tpagecontrol" => Self::PageControl,
            "ttabsheet" => Self::TabSheet,
            "tdbgrid" => Self::DBGrid,
            "tdbedit" => Self::DBEdit,
            _ => Self::Unknown,
        }
    }
    pub fn default_size(self) -> [f32; 2] {
        match self {
            Self::Button => [75., 25.],
            Self::Edit | Self::DBEdit | Self::ComboBox => [121., 21.],
            Self::Label => [65., 17.],
            Self::CheckBox | Self::RadioButton => [113., 17.],
            Self::Unknown => [100., 28.],
            _ => [185., 89.],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}
impl Bounds {
    pub fn inset(self, n: f32) -> Self {
        Self {
            x: self.x + n,
            y: self.y + n,
            w: (self.w - 2. * n).max(0.),
            h: (self.h - 2. * n).max(0.),
        }
    }
}

pub fn client_size(c: &Component) -> [f32; 2] {
    [
        c.number("ClientWidth", c.number("Width", 640.))
            .clamp(1., 100_000.),
        c.number("ClientHeight", c.number("Height", 480.))
            .clamp(1., 100_000.),
    ]
}

pub struct Placed<'a> {
    pub component: &'a Component,
    pub path: String,
    pub bounds: Bounds,
    pub clip: Bounds,
    pub enabled: bool,
    pub hidden: bool,
    pub kind: Control,
}

/// Designer dimensions can differ from the stored client dimensions to inspect anchors.
pub fn layout(root: &Component, size: [f32; 2], show_hidden: bool) -> Vec<Placed<'_>> {
    layout_selected(root, size, show_hidden, "")
}

pub fn layout_selected<'a>(
    root: &'a Component,
    size: [f32; 2],
    show_hidden: bool,
    selected: &str,
) -> Vec<Placed<'a>> {
    let mut out = Vec::new();
    let bounds = Bounds {
        x: 0.,
        y: 0.,
        w: size[0],
        h: size[1],
    };
    out.push(Placed {
        component: root,
        path: root.name.clone(),
        bounds,
        clip: bounds,
        enabled: root.boolean("Enabled", true),
        hidden: !root.boolean("Visible", true),
        kind: Control::Form,
    });
    children(
        root,
        &root.name,
        bounds,
        bounds,
        client_size(root),
        root.boolean("Enabled", true),
        !root.boolean("Visible", true),
        show_hidden,
        selected,
        &mut out,
    );
    out
}

#[allow(clippy::too_many_arguments)]
fn children<'a>(
    parent: &'a Component,
    path: &str,
    client: Bounds,
    clip: Bounds,
    original: [f32; 2],
    enabled: bool,
    hidden: bool,
    show_hidden: bool,
    selected: &str,
    out: &mut Vec<Placed<'a>>,
) {
    let mut remaining = client;
    // VCL aligns edge controls before alClient. Preserve source order within each group.
    let mut indices: Vec<_> = (0..parent.children.len()).collect();
    indices.sort_by_key(|&i| {
        match parent.children[i]
            .text("Align")
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str()
        {
            "altop" => 0,
            "albottom" => 1,
            "alleft" => 2,
            "alright" => 3,
            "alclient" => 4,
            _ => 5,
        }
    });
    for i in indices {
        let c = &parent.children[i];
        let hidden = hidden
            || !c.boolean("Visible", true)
            || (Control::of(&c.class) == Control::TabSheet && !c.boolean("TabVisible", true));
        if hidden && !show_hidden {
            continue;
        }
        if Control::of(&parent.class) == Control::PageControl
            && Control::of(&c.class) == Control::TabSheet
        {
            let active = parent
                .text("ActivePage")
                .or_else(|| parent.children.first().map(|c| c.name.as_str()));
            if active != Some(c.name.as_str()) {
                continue;
            }
        }
        let kind = Control::of(&c.class);
        let defaults = kind.default_size();
        let mut b = Bounds {
            x: client.x + c.number("Left", 0.),
            y: client.y + c.number("Top", 0.),
            w: c.number("Width", defaults[0]).max(0.),
            h: c.number("Height", defaults[1]).max(0.),
        };
        let align = c.text("Align").unwrap_or("alNone").to_ascii_lowercase();
        match align.as_str() {
            "altop" => {
                b.x = remaining.x;
                b.y = remaining.y;
                b.w = remaining.w;
                b.h = b.h.min(remaining.h);
                remaining.y += b.h;
                remaining.h -= b.h;
            }
            "albottom" => {
                b.x = remaining.x;
                b.h = b.h.min(remaining.h);
                b.y = remaining.y + remaining.h - b.h;
                b.w = remaining.w;
                remaining.h -= b.h;
            }
            "alleft" => {
                b.x = remaining.x;
                b.y = remaining.y;
                b.h = remaining.h;
                b.w = b.w.min(remaining.w);
                remaining.x += b.w;
                remaining.w -= b.w;
            }
            "alright" => {
                b.w = b.w.min(remaining.w);
                b.x = remaining.x + remaining.w - b.w;
                b.y = remaining.y;
                b.h = remaining.h;
                remaining.w -= b.w;
            }
            "alclient" => b = remaining,
            _ => {
                let anchors = match c.property("Anchors").map(|p| &p.value) {
                    Some(Value::Set(a)) => a.clone(),
                    _ => vec!["akLeft".into(), "akTop".into()],
                };
                let has = |s: &str| anchors.iter().any(|a| a.eq_ignore_ascii_case(s));
                let dx = client.w - original[0];
                let dy = client.h - original[1];
                if has("akRight") {
                    if has("akLeft") {
                        b.w = (b.w + dx).max(0.);
                    } else {
                        b.x += dx;
                    }
                } else if !has("akLeft") {
                    b.x += dx / 2.;
                }
                if has("akBottom") {
                    if has("akTop") {
                        b.h = (b.h + dy).max(0.);
                    } else {
                        b.y += dy;
                    }
                } else if !has("akTop") {
                    b.y += dy / 2.;
                }
            }
        }
        let path = format!("{path}/{}", c.name);
        let enabled = enabled && c.boolean("Enabled", true);
        out.push(Placed {
            component: c,
            path: path.clone(),
            bounds: b,
            clip,
            enabled,
            hidden,
            kind,
        });
        let inset = match kind {
            Control::GroupBox => 4.,
            Control::Panel | Control::PageControl => 2.,
            _ => 0.,
        };
        let mut inner = b.inset(inset);
        let mut old = [
            c.number("ClientWidth", c.number("Width", defaults[0])),
            c.number("ClientHeight", c.number("Height", defaults[1])),
        ];
        old[0] = (old[0] - 2. * inset).max(0.);
        old[1] = (old[1] - 2. * inset).max(0.);
        if matches!(kind, Control::GroupBox | Control::PageControl) {
            inner.y += 20.;
            inner.h = (inner.h - 20.).max(0.);
            old[1] = (old[1] - 20.).max(0.);
        }
        if kind == Control::PageControl {
            // Tab sheets occupy the page client, regardless of stale saved Left/Top.
            let selected_page = c.children.iter().find(|p| {
                let page_path = format!("{path}/{}", p.name);
                selected == page_path || selected.starts_with(&format!("{page_path}/"))
            });
            if let Some(page) = selected_page.or_else(|| {
                c.children.iter().find(|p| {
                    p.name
                        == c.text("ActivePage")
                            .unwrap_or_else(|| c.children.first().map_or("", |p| &p.name))
                })
            }) {
                if !show_hidden
                    && (!page.boolean("Visible", true) || !page.boolean("TabVisible", true))
                {
                    continue;
                }
                let page_path = format!("{path}/{}", page.name);
                out.push(Placed {
                    component: page,
                    path: page_path.clone(),
                    bounds: inner,
                    clip: intersect(clip, inner),
                    enabled,
                    hidden,
                    kind: Control::TabSheet,
                });
                children(
                    page,
                    &page_path,
                    inner,
                    intersect(clip, inner),
                    [page.number("Width", old[0]), page.number("Height", old[1])],
                    enabled,
                    hidden,
                    show_hidden,
                    selected,
                    out,
                );
            }
        } else {
            children(
                c,
                &path,
                inner,
                intersect(clip, inner),
                old,
                enabled,
                hidden,
                show_hidden,
                selected,
                out,
            );
        }
    }
}
fn intersect(a: Bounds, b: Bounds) -> Bounds {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    Bounds {
        x,
        y,
        w: ((a.x + a.w).min(b.x + b.w) - x).max(0.),
        h: ((a.y + a.h).min(b.y + b.h) - y).max(0.),
    }
}
