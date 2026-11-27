use super::{Editor, Tool};
use crate::color;
use crate::jobs::{Spec, Tag};
use crate::messages::Tone;
use crate::theme::{self, Hint, icon};
use crate::widgets::PenScroll;
use anyhow::Result;
use egui::{Color32, Key, Rect, RichText, Stroke, TextureHandle, Vec2, pos2, vec2};
use klad_core::doc::{Item, Kind, Layer, Node, Paint, Shape};
use klad_core::generators::{Align, Text};
use klad_core::ops::{Op, Place};
use klad_render::fonts::{self, Family};
use klad_render::overlay;
use klad_render::text::{self as typeset, Layout};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;

const PREVIEW: f32 = 22.0;
const IDENTITY: [f32; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

pub(super) struct TextTool {
    pub font: String,
    pub weight: Option<f32>,
    pub size: f32,
    pub align: Align,
    pub tracking: f32,
    pub leading: f32,
}

impl Default for TextTool {
    fn default() -> Self {
        Self {
            font: "Inter".into(),
            weight: None,
            size: TEXT_SIZE,
            align: Align::Left,
            tracking: 0.0,
            leading: 1.2,
        }
    }
}

pub(super) struct Typing {
    pub id: Option<String>,
    pub layer: Option<String>,
    pub pixels: Option<String>,
    pub frame: u32,
    pub text: Text,
    pub before: String,
    pub caret: usize,
    pub anchor: Option<usize>,
    pub undo: Vec<(String, usize)>,
    pub run: bool,
    pub fill: [f32; 4],
    pub matrix: [f32; 6],
    pub focused: bool,
    pub bytes: Arc<Vec<u8>>,
    pub shown: Option<(String, Option<(TextureHandle, Rect)>)>,
}

struct Written {
    text: Text,
    fill: [f32; 4],
    matrix: [f32; 6],
    hidden: Layer,
}

#[derive(Default)]
pub(super) struct Picker {
    pub search: String,
    pub group: Option<&'static str>,
    pub faces: HashMap<String, Option<TextureHandle>>,
}

pub(super) const TEXT_SIZE: f32 = 48.0;
const SELECTED_TEXT: f32 = 0.35;
const TYPING_ACTS: [Key; 9] = [
    Key::Enter,
    Key::Backspace,
    Key::Delete,
    Key::ArrowLeft,
    Key::ArrowRight,
    Key::ArrowUp,
    Key::ArrowDown,
    Key::Home,
    Key::End,
];
const DOUBLE_CLICK: std::time::Duration = std::time::Duration::from_millis(400);

fn word_around(text: &str, at: usize) -> (usize, usize) {
    let word = |c: char| c.is_alphanumeric() || c == '\'' || c == '_';
    let from = text[..at]
        .char_indices()
        .rev()
        .take_while(|(_, c)| word(*c))
        .last()
        .map_or(at, |(i, _)| i);
    let to = text[at..]
        .char_indices()
        .find(|(_, c)| !word(*c))
        .map_or(text.len(), |(i, _)| at + i);
    (from, to)
}
const PICTURE_MOST: usize = 8192;
const PICTURE_ROOM: f32 = 0.9;

fn compose(s: [f32; 6], m: [f32; 6]) -> [f32; 6] {
    [
        s[0] * m[0] + s[2] * m[1],
        s[1] * m[0] + s[3] * m[1],
        s[0] * m[2] + s[2] * m[3],
        s[1] * m[2] + s[3] * m[3],
        s[0] * m[4] + s[2] * m[5] + s[4],
        s[1] * m[4] + s[3] * m[5] + s[5],
    ]
}

fn apply(m: [f32; 6], x: f32, y: f32) -> Vec2 {
    vec2(m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
}

fn invert(m: [f32; 6]) -> Option<[f32; 6]> {
    let det = m[0] * m[3] - m[2] * m[1];
    if det.abs() < 1e-9 {
        return None;
    }
    let (a, b, c, d) = (m[3] / det, -m[1] / det, -m[2] / det, m[0] / det);
    Some([a, b, c, d, -(a * m[4] + c * m[5]), -(b * m[4] + d * m[5])])
}

fn prev_char(text: &str, at: usize) -> usize {
    text[..at].char_indices().last().map_or(0, |(i, _)| i)
}

fn next_char(text: &str, at: usize) -> usize {
    at + text[at..].chars().next().map_or(0, char::len_utf8)
}

fn texture(ctx: &egui::Context, name: &str, picture: &typeset::Picture) -> TextureHandle {
    let image =
        egui::ColorImage::from_rgba_premultiplied([picture.width as usize, picture.height as usize], &picture.rgba);
    ctx.load_texture(name, image, egui::TextureOptions::LINEAR)
}

fn hide(nodes: &mut [Node], id: &str) -> bool {
    nodes.iter_mut().any(|n| {
        if n.id == id {
            n.visible = false;
            return true;
        }
        n.shape.children_mut().is_some_and(|c| hide(c, id))
    })
}

fn texts_in(nodes: &[Node], out: &mut Vec<String>) {
    nodes.iter().filter(|n| n.visible).for_each(|n| {
        if matches!(n.shape, Shape::Text(_)) {
            out.push(n.id.clone());
        }
        texts_in(n.children(), out);
    });
}

fn bounds(layout: &Layout) -> Option<[f32; 4]> {
    let first = layout.lines.first()?;
    let last = layout.lines.last()?;
    let left = layout.lines.iter().map(|l| l.left).fold(f32::MAX, f32::min);
    let right = layout.lines.iter().map(|l| l.left + l.width).fold(f32::MIN, f32::max);
    Some([
        left,
        first.baseline - layout.ascent,
        right,
        last.baseline + layout.descent,
    ])
}

impl Editor {
    pub(super) fn text_at(&self, c: Vec2) -> Option<String> {
        let doc = &self.scene.bundle.doc;
        let slack = 4.0 / self.view.map_or(1.0, |v| v.zoom);
        let mut found = Vec::new();
        doc.all_layers()
            .iter()
            .rev()
            .filter(|l| l.kind() == Kind::Vector && !l.locked && l.visible)
            .filter_map(|l| l.content_at(self.scene.frame))
            .for_each(|cel| texts_in(&cel.nodes, &mut found));
        found.into_iter().find(|id| {
            overlay::node_world_box(doc, id)
                .ok()
                .flatten()
                .is_some_and(|[x, y, w, h]| {
                    c.x >= x - slack && c.x <= x + w + slack && c.y >= y - slack && c.y <= y + h + slack
                })
        })
    }

    fn typing_layout(&self) -> Option<Layout> {
        let t = self.text_edit.as_ref()?;
        typeset::layout_in(&t.bytes, &t.text).ok()
    }

    pub(super) fn text_drag(&mut self, c: Vec2) {
        let Some(at) = self.inside_typing(c) else {
            return;
        };
        if let Some(t) = self.text_edit.as_mut().filter(|t| t.anchor.is_some()) {
            t.caret = at;
        }
    }

    fn inside_typing(&self, c: Vec2) -> Option<usize> {
        let t = self.text_edit.as_ref()?;
        let layout = self.typing_layout()?;
        let local = apply(invert(t.matrix)?, c.x, c.y);
        let slack = 6.0 / self.view.map_or(1.0, |v| v.zoom);
        let [x0, y0, x1, y1] = bounds(&layout)?;
        let x1 = x1.max(x0 + t.text.size * 0.3);
        (local.x >= x0 - slack && local.x <= x1 + slack && local.y >= y0 - slack && local.y <= y1 + slack)
            .then(|| layout.nearest(local.x, local.y))
    }

    pub(super) fn text_down(&mut self, c: Vec2) -> Result<()> {
        if let Some(at) = self.inside_typing(c) {
            let shift = self.modifiers.shift;
            let twice = self
                .text_click
                .is_some_and(|(when, was)| was == at && when.elapsed() < DOUBLE_CLICK);
            self.text_click = Some((std::time::Instant::now(), at));
            self.text_pressing = !twice;
            if let Some(t) = self.text_edit.as_mut() {
                t.run = false;
                match (shift, twice) {
                    (true, _) => {
                        t.anchor = t.anchor.or(Some(t.caret));
                        t.caret = at;
                    }
                    (false, true) => {
                        let (from, to) = word_around(&t.text.text, at);
                        t.anchor = Some(from);
                        t.caret = to;
                    }
                    (false, false) => {
                        t.anchor = Some(at);
                        t.caret = at;
                    }
                }
            }
            return Ok(());
        }
        self.finish_typing()?;
        match self.text_at(c) {
            Some(id) => self.edit_text(&id, Some(c)),
            None => self.new_text(c),
        }
    }

    fn font_bytes(&mut self, file: &str) -> Option<Arc<Vec<u8>>> {
        match std::fs::read(self.scene.bundle.root.join(klad_render::assets::FONTS).join(file)) {
            Ok(bytes) => Some(Arc::new(bytes)),
            Err(_) => {
                self.say(
                    Tone::Refused,
                    format!("the font {file} is not in the drawing; pick another in layer settings"),
                );
                None
            }
        }
    }

    fn written(&self, id: &str) -> Result<Option<Written>> {
        let doc = &self.scene.bundle.doc;
        let Some(Item::Node(layer, _, node)) = doc.item(id) else {
            return Ok(None);
        };
        let Shape::Text(text) = &node.shape else {
            return Ok(None);
        };
        let fill = match &node.fill {
            Some(Paint::Solid(hex)) => color::rgba_of(hex).unwrap_or([0.0, 0.0, 0.0, 1.0]),
            Some(_) => [0.25, 0.77, 1.0, 1.0],
            None => [0.0, 0.0, 0.0, 1.0],
        };
        let mut hidden = layer.clone();
        if let Some(cels) = hidden.cels_mut() {
            cels.iter_mut().any(|cel| hide(&mut cel.nodes, id));
        }
        Ok(Some(Written {
            text: text.clone(),
            fill: [fill[0], fill[1], fill[2], fill[3] * node.opacity],
            matrix: overlay::local_to_canvas(doc, id)?.map_or(IDENTITY, |m| [m[0], m[2], m[1], m[3], m[4], m[5]]),
            hidden,
        }))
    }

    pub(super) fn edit_text(&mut self, id: &str, at: Option<Vec2>) -> Result<()> {
        let Some(written) = self.written(id)? else {
            return Ok(());
        };
        let Some(bytes) = self.font_bytes(&written.text.font) else {
            return Ok(());
        };
        self.end_look_preview()?;
        let layer = written.hidden.id.clone();
        self.scene.preview_layer(written.hidden)?;
        self.dirty.extend(self.scene.all_tiles());
        let caret = written.text.text.len();
        self.text_edit = Some(Typing {
            id: Some(id.to_string()),
            pixels: None,
            layer: Some(layer),
            frame: self.scene.frame,
            before: written.text.text.clone(),
            text: written.text,
            caret,
            anchor: None,
            undo: Vec::new(),
            run: false,
            fill: written.fill,
            matrix: written.matrix,
            focused: false,
            bytes,
            shown: None,
        });
        if let Some(at) = at.and_then(|c| self.inside_typing(c))
            && let Some(t) = self.text_edit.as_mut()
        {
            t.caret = at;
        }
        self.selection = Some(id.to_string());
        Ok(())
    }

    fn new_text(&mut self, c: Vec2) -> Result<()> {
        let root = self.scene.bundle.root.clone();
        let chosen = match fonts::bring(&root, &self.text_tool.font) {
            Ok(chosen) => chosen,
            Err(e) => {
                self.say(Tone::Refused, format!("{e:#}"));
                return Ok(());
            }
        };
        let Some(bytes) = self.font_bytes(&chosen.file) else {
            return Ok(());
        };
        let tool = &self.text_tool;
        let mut text = Text {
            text: String::new(),
            font: chosen.file,
            x: c.x,
            y: c.y,
            size: tool.size,
            tracking: tool.tracking,
            leading: tool.leading,
            align: tool.align,
            weight: tool.weight.or(chosen.weight),
        };
        if let Ok(layout) = typeset::layout_in(&bytes, &text) {
            text.y = c.y - (layout.ascent + layout.descent) / 2.0;
        }
        let layer = self
            .scene
            .active_layer()
            .filter(|l| l.kind() == Kind::Vector && !l.locked && !l.reference && !self.scene.active_is_mask())
            .map(|l| l.id.clone());
        let pixels = self
            .scene
            .active_layer()
            .filter(|l| l.pixel() && !l.locked && !self.scene.active_is_mask() && self.hidden_in_hand().is_none())
            .map(|l| (l.id.clone(), l.name.clone()));
        if let Some((_, name)) = &pixels {
            self.say(
                Tone::Info,
                format!("{name} is a pixel layer: the text becomes pixels when you finish it, and cannot be edited as text after that"),
            );
        }
        self.text_edit = Some(Typing {
            id: None,
            layer,
            pixels: pixels.map(|(id, _)| id),
            frame: self.scene.frame,
            text,
            before: String::new(),
            caret: 0,
            anchor: None,
            undo: Vec::new(),
            run: false,
            fill: self.color,
            matrix: IDENTITY,
            focused: false,
            bytes,
            shown: None,
        });
        Ok(())
    }

    fn end_typing_preview(&mut self) -> Result<()> {
        let layer = self
            .text_edit
            .as_ref()
            .filter(|t| t.id.is_some())
            .and_then(|t| t.layer.clone());
        if let Some(layer) = layer
            && self.scene.end_preview(&layer)?
        {
            self.dirty.extend(self.scene.all_tiles());
        }
        Ok(())
    }

    pub(super) fn keep_typing_preview(&mut self) -> Result<()> {
        if self.scene.take_unshown() {
            self.dirty.extend(self.scene.all_tiles());
        }
        let Some(id) = self.text_edit.as_ref().and_then(|t| t.id.clone()) else {
            return Ok(());
        };
        let shows = match self.scene.bundle.doc.item(&id) {
            Some(Item::Node(_, _, node)) if matches!(node.shape, Shape::Text(_)) => Some(node.visible),
            _ => None,
        };
        let written = match shows {
            Some(true) => self.written(&id)?,
            Some(false) => return Ok(()),
            None => None,
        };
        let Some(written) = written else {
            self.text_edit = None;
            self.say(
                Tone::Info,
                "the text that was open is no longer in the drawing, so it is closed",
            );
            return Ok(());
        };
        let font = self
            .text_edit
            .as_ref()
            .is_some_and(|t| t.text.font != written.text.font)
            .then(|| self.font_bytes(&written.text.font))
            .flatten();
        let layer = written.hidden.id.clone();
        self.scene.preview_layer(written.hidden)?;
        self.dirty.extend(self.scene.all_tiles());
        if let Some(t) = self.text_edit.as_mut() {
            let typed = std::mem::take(&mut t.text.text);
            let kept = match &font {
                Some(_) => written.text.font.clone(),
                None => std::mem::take(&mut t.text.font),
            };
            t.before = written.text.text.clone();
            t.text = written.text;
            t.text.text = typed;
            t.text.font = kept;
            t.fill = written.fill;
            t.matrix = written.matrix;
            t.layer = Some(layer);
            if let Some(bytes) = font {
                t.bytes = bytes;
            }
        }
        Ok(())
    }

    pub(super) fn finish_typing(&mut self) -> Result<()> {
        self.end_typing_preview()?;
        let Some(typing) = self.text_edit.take() else {
            return Ok(());
        };
        let words = typing.text.text.clone();
        let empty = words.trim().is_empty();
        match typing.id {
            Some(id) if empty => {
                self.apply(vec![Op::Remove { ids: vec![id] }])?;
                self.selection = None;
                self.say(Tone::Done, "the text was emptied, so it is removed");
            }
            Some(id) => {
                if words != typing.before {
                    self.set(&id, vec![("text".into(), json!(words))])?;
                }
                self.selection = Some(id);
            }
            None if empty => {}
            None if typing.pixels.is_some() => {
                let layer = typing.pixels.clone().unwrap_or_default();
                let cells = typeset::cells(&typing.bytes, &typing.text)?;
                if cells.is_empty() {
                    self.say(
                        Tone::Refused,
                        "the text is too small to leave any pixels: make it larger in the bar",
                    );
                    return Ok(());
                }
                let before = self.scene.bundle.doc.rev;
                self.apply_pixels(klad_core::ops::PixelSpec {
                    layer: layer.clone(),
                    frame: typing.frame,
                    color: Some(color::hex(typing.fill)),
                    cells,
                    by_hand: true,
                    ..klad_core::ops::PixelSpec::default()
                })?;
                if self.scene.bundle.doc.rev != before {
                    let name = self.layer_name(&layer);
                    self.say(
                        Tone::Done,
                        format!("the text is pixels on {name} now and cannot be edited as text; cmd Z takes it back"),
                    );
                }
            }
            None => {
                let taken = self.scene.bundle.doc.ids();
                let fresh = |stem: &str| {
                    (1..)
                        .map(|n| format!("{stem}{n}"))
                        .find(|i| !taken.contains(i))
                        .unwrap_or_default()
                };
                let id = fresh("text");
                let mut node = serde_json::to_value(&typing.text)?;
                node["type"] = json!("text");
                node["id"] = json!(id);
                node["fill"] = json!(color::hex(typing.fill));
                let (mut ops, layer, made, said) = match typing.layer {
                    Some(layer) => (Vec::new(), layer, false, String::new()),
                    None => {
                        let home = self.new_vector_layer("text", typing.frame);
                        let said = home.said();
                        (home.ops, home.layer, true, said)
                    }
                };
                ops.push(Op::AddNode {
                    node,
                    place: Place {
                        into: Some(layer.clone()),
                        frame: Some(typing.frame),
                        ..Place::default()
                    },
                });
                self.apply(ops)?;
                if made && let Some(klad_core::doc::Loc::Layer(path)) = self.scene.bundle.doc.locate(&layer) {
                    self.activate(path)?;
                }
                self.selection = Some(id);
                self.used_colour();
                self.say(
                    Tone::Done,
                    format!(
                        "text added{said}: T and a click on it types more, V moves it, layer settings change its font"
                    ),
                );
            }
        }
        Ok(())
    }

    pub(super) fn typing_command(&mut self, cmd: crate::commands::Cmd) -> bool {
        use crate::commands::Cmd;
        let Some(t) = self.text_edit.as_mut() else {
            return false;
        };
        match cmd {
            Cmd::Undo => {
                let Some((was, caret)) = t.undo.pop() else {
                    return false;
                };
                t.text.text = was;
                t.caret = caret.min(t.text.text.len());
                t.anchor = None;
                t.run = false;
            }
            Cmd::SelectAll => {
                t.anchor = Some(0);
                t.caret = t.text.text.len();
                t.run = false;
            }
            Cmd::Copy => {
                if let Some(range) = t
                    .anchor
                    .filter(|a| *a != t.caret)
                    .map(|a| a.min(t.caret)..a.max(t.caret))
                {
                    self.ctx.copy_text(t.text.text[range].to_string());
                }
            }
            _ => return false,
        }
        true
    }

    pub(super) fn typing_keys(&mut self, ui: &egui::Ui) -> Result<()> {
        if self.text_edit.is_none() {
            return Ok(());
        }
        let events = ui.input(|i| i.events.clone());
        let mut finish = false;
        let mut copied: Option<String> = None;
        events.iter().for_each(|e| {
            let layout = self.typing_layout();
            let Some(t) = self.text_edit.as_mut() else {
                return;
            };
            let chosen = t
                .anchor
                .filter(|a| *a != t.caret)
                .map(|a| a.min(t.caret)..a.max(t.caret));
            let shift = matches!(e, egui::Event::Key { modifiers, .. } if modifiers.shift);
            let moving = matches!(
                e,
                egui::Event::Key {
                    key: Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown | Key::Home | Key::End,
                    pressed: true,
                    ..
                }
            );
            if moving && shift && t.anchor.is_none() {
                t.anchor = Some(t.caret);
            }
            let words = &mut t.text.text;
            let cut = |words: &mut String, caret: &mut usize, undo: &mut Vec<(String, usize)>| -> bool {
                match chosen.clone() {
                    Some(range) => {
                        undo.push((words.clone(), *caret));
                        *caret = range.start;
                        words.replace_range(range, "");
                        true
                    }
                    None => false,
                }
            };
            let acted = match e {
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => modifiers.command || modifiers.alt || TYPING_ACTS.contains(key),
                egui::Event::Copy | egui::Event::Cut | egui::Event::Paste(_) => true,
                _ => false,
            };
            let fresh = acted || !t.run;
            match e {
                egui::Event::Text(s) | egui::Event::Paste(s) => {
                    let clean: String = s.chars().filter(|c| !c.is_control() || *c == '\n').collect();
                    if !cut(words, &mut t.caret, &mut t.undo) && fresh {
                        t.undo.push((words.clone(), t.caret));
                    }
                    words.insert_str(t.caret, &clean);
                    t.caret += clean.len();
                    t.anchor = None;
                }
                egui::Event::Copy | egui::Event::Cut => {
                    if let Some(range) = chosen.clone() {
                        copied = Some(words[range].to_string());
                        if matches!(e, egui::Event::Cut) {
                            cut(words, &mut t.caret, &mut t.undo);
                            t.anchor = None;
                        }
                    }
                }
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => match key {
                    Key::Escape => finish = true,
                    Key::Enter if modifiers.command => finish = true,
                    Key::A if modifiers.command => {
                        t.anchor = Some(0);
                        t.caret = words.len();
                    }
                    Key::Z if modifiers.command && !t.undo.is_empty() => {
                        if let Some((was, caret)) = t.undo.pop() {
                            *words = was;
                            t.caret = caret.min(words.len());
                            t.anchor = None;
                        }
                    }
                    Key::Enter => {
                        if !cut(words, &mut t.caret, &mut t.undo) {
                            t.undo.push((words.clone(), t.caret));
                        }
                        words.insert(t.caret, '\n');
                        t.caret += 1;
                        t.anchor = None;
                    }
                    Key::Backspace | Key::Delete if chosen.is_some() => {
                        cut(words, &mut t.caret, &mut t.undo);
                        t.anchor = None;
                    }
                    Key::Backspace if t.caret > 0 => {
                        t.undo.push((words.clone(), t.caret));
                        let from = prev_char(words, t.caret);
                        words.replace_range(from..t.caret, "");
                        t.caret = from;
                    }
                    Key::Delete if t.caret < words.len() => {
                        t.undo.push((words.clone(), t.caret));
                        let to = next_char(words, t.caret);
                        words.replace_range(t.caret..to, "");
                    }
                    Key::ArrowLeft if !shift && chosen.is_some() => {
                        t.caret = chosen.clone().map_or(t.caret, |r| r.start);
                        t.anchor = None;
                    }
                    Key::ArrowRight if !shift && chosen.is_some() => {
                        t.caret = chosen.clone().map_or(t.caret, |r| r.end);
                        t.anchor = None;
                    }
                    Key::ArrowLeft => t.caret = prev_char(words, t.caret),
                    Key::ArrowRight => t.caret = next_char(words, t.caret),
                    Key::ArrowUp | Key::ArrowDown | Key::Home | Key::End => {
                        if let Some(layout) = layout {
                            let row = layout.lines.iter().rposition(|l| l.start <= t.caret).unwrap_or(0);
                            let x = layout.caret(t.caret).map_or(0.0, |c| c.0);
                            let target = match key {
                                Key::ArrowUp => row.checked_sub(1),
                                Key::ArrowDown => Some(row + 1).filter(|r| *r < layout.lines.len()),
                                _ => Some(row),
                            };
                            if let Some(line) = target.and_then(|r| layout.lines.get(r)) {
                                t.caret = match key {
                                    Key::Home => line.carets.first().map_or(t.caret, |c| c.0),
                                    Key::End => line.carets.last().map_or(t.caret, |c| c.0),
                                    _ => line
                                        .carets
                                        .iter()
                                        .min_by(|a, b| (a.1 - x).abs().total_cmp(&(b.1 - x).abs()))
                                        .map_or(t.caret, |c| c.0),
                                };
                            }
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
            if moving && !shift {
                t.anchor = None;
            }
            t.run = match e {
                egui::Event::Text(_) => true,
                _ => t.run && !acted,
            };
        });
        if let Some(text) = copied {
            ui.ctx().copy_text(text);
        }
        if finish {
            self.finish_typing()?;
        }
        Ok(())
    }

    pub(super) fn keep_typing_focus(&mut self, ui: &egui::Ui, response: &egui::Response) -> Result<()> {
        if self.text_edit.is_some() && self.tool != Tool::Text {
            self.finish_typing()?;
        }
        let Some(t) = self.text_edit.as_mut() else {
            if response.has_focus() {
                response.surrender_focus();
            }
            return Ok(());
        };
        let elsewhere = ui.memory(|m| m.focused()).is_some_and(|f| f != response.id);
        if elsewhere && t.focused {
            return self.finish_typing();
        }
        t.focused = true;
        response.request_focus();
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                response.id,
                egui::EventFilter {
                    tab: true,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: true,
                },
            )
        });
        Ok(())
    }

    pub(super) fn prepare_typing(&mut self, ctx: &egui::Context, view: super::View, rect: Rect) {
        let ppp = ctx.pixels_per_point();
        let o = self.screen(view, rect, vec2(0.0, 0.0));
        let ex = self.screen(view, rect, vec2(1.0, 0.0)) - o;
        let ey = self.screen(view, rect, vec2(0.0, 1.0)) - o;
        let laid = self.typing_layout().and_then(|l| bounds(&l));
        let Some(t) = self.text_edit.as_mut() else {
            return;
        };
        let screen = [ex.x * ppp, ex.y * ppp, ey.x * ppp, ey.y * ppp, o.x * ppp, o.y * ppp];
        let m = compose(screen, t.matrix);
        let key = format!("{:?}{m:?}{:?}", t.text, t.fill);
        if t.shown.as_ref().is_some_and(|(k, _)| *k == key) {
            return;
        }
        let limit = ctx.input(|i| i.max_texture_side).min(PICTURE_MOST) as f32 * PICTURE_ROOM;
        let side = laid.map_or(0.0, |[x0, y0, x1, y1]| {
            let corners = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)].map(|(x, y)| apply(m, x, y));
            let (lo, hi) = corners
                .iter()
                .fold((corners[0], corners[0]), |(lo, hi), c| (lo.min(*c), hi.max(*c)));
            (hi - lo).max_elem()
        });
        let k = match side > limit {
            true => limit / side,
            false => 1.0,
        };
        let scaled = m.map(|v| v * k);
        let made = typeset::picture(&t.bytes, &t.text, scaled, t.fill)
            .ok()
            .flatten()
            .map(|p| {
                let handle = texture(ctx, "typing", &p);
                let min = pos2(p.left as f32, p.top as f32) / (k * ppp);
                (
                    handle,
                    Rect::from_min_size(min, vec2(p.width as f32, p.height as f32) / (k * ppp)),
                )
            });
        t.shown = Some((key, made));
    }

    pub(super) fn decorate_typing(&self, painter: &egui::Painter, view: super::View, rect: Rect, accent: Color32) {
        let Some(t) = self.text_edit.as_ref() else {
            return;
        };
        if let Some((_, Some((handle, at)))) = &t.shown {
            painter.image(
                handle.id(),
                *at,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        let to_screen = |x: f32, y: f32| self.screen(view, rect, apply(t.matrix, x, y));
        let Some(layout) = self.typing_layout() else {
            return;
        };
        if let Some([x0, y0, x1, y1]) = bounds(&layout) {
            let x1 = x1.max(x0 + t.text.size * 0.3);
            let corners = [(x0, y0), (x1, y0), (x1, y1), (x0, y1), (x0, y0)].map(|(x, y)| to_screen(x, y));
            painter.add(egui::Shape::dashed_line(&corners, Stroke::new(1.0, accent), 5.0, 4.0));
        }
        if let Some(range) = t
            .anchor
            .filter(|a| *a != t.caret)
            .map(|a| a.min(t.caret)..a.max(t.caret))
        {
            layout.lines.iter().for_each(|line| {
                let at = |i: usize| line.carets.iter().find(|(c, _)| *c == i).map(|(_, x)| *x);
                let first = line
                    .carets
                    .iter()
                    .map(|(c, _)| *c)
                    .filter(|c| range.contains(c) || *c == range.end);
                let (lo, hi) = first.fold((usize::MAX, 0), |(lo, hi), c| (lo.min(c), hi.max(c)));
                if let (Some(x0), Some(x1)) = (at(lo), at(hi))
                    && lo < hi
                {
                    let (top, bottom) = (line.baseline - layout.ascent, line.baseline + layout.descent);
                    let corners = [(x0, top), (x1, top), (x1, bottom), (x0, bottom)].map(|(x, y)| to_screen(x, y));
                    painter.add(egui::Shape::convex_polygon(
                        corners.to_vec(),
                        accent.gamma_multiply(SELECTED_TEXT),
                        Stroke::NONE,
                    ));
                }
            });
        }
        if let Some((x, top, bottom)) = layout.caret(t.caret) {
            let line = [to_screen(x, top), to_screen(x, bottom)];
            painter.line_segment(line, Stroke::new(3.0, Color32::from_black_alpha(160)));
            painter.line_segment(line, Stroke::new(1.5, accent));
        }
    }

    fn face(&mut self, ctx: &egui::Context, key: &str, path: &std::path::Path, label: &str) -> Option<TextureHandle> {
        if let Some(found) = self.picker.faces.get(key) {
            return found.clone();
        }
        let ppp = ctx.pixels_per_point();
        let made = std::fs::read(path).ok().and_then(|bytes| {
            let text = Text {
                text: label.to_string(),
                font: key.to_string(),
                size: PREVIEW * ppp,
                ..Text::default()
            };
            let ink = theme::TEXT;
            let colour = [
                ink.r() as f32 / 255.0,
                ink.g() as f32 / 255.0,
                ink.b() as f32 / 255.0,
                1.0,
            ];
            typeset::picture(&bytes, &text, IDENTITY, colour)
                .ok()
                .flatten()
                .map(|p| texture(ctx, key, &p))
        });
        self.picker.faces.insert(key.to_string(), made.clone());
        made
    }

    fn family_row(&mut self, ui: &mut egui::Ui, family: &Family, current: &str) -> Option<String> {
        let mut picked = None;
        let regular = family.regular().clone();
        let ppp = ui.ctx().pixels_per_point();
        let preview = self.face(
            &ui.ctx().clone(),
            &format!("{}/{}", family.dir.display(), regular.file),
            &family.path(&regular),
            &family.family,
        );
        ui.horizontal(|ui| {
            let chosen = fonts::key(current).starts_with(&fonts::key(&family.family));
            let response = match &preview {
                Some(t) => {
                    let size = t.size_vec2() / ppp;
                    let response = ui.add(
                        egui::Button::image(egui::Image::new(t).fit_to_exact_size(size))
                            .selected(chosen)
                            .min_size(vec2(200.0, theme::CONTROL)),
                    );
                    let name = family.family.clone();
                    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
                    response
                }
                None => ui.add(
                    egui::Button::new(&family.family)
                        .selected(chosen)
                        .min_size(vec2(200.0, theme::CONTROL)),
                ),
            };
            let what = match family.shipped {
                true => format!("{}: {}, {}", family.family, family.group, family.licence),
                false => format!("{}: {}, {}, downloaded", family.family, family.group, family.licence),
            };
            if response.hint(what).clicked() {
                picked = Some(family.family.clone());
            }
            family.files.iter().filter(|f| f.file != regular.file).for_each(|f| {
                if ui
                    .small_button(&f.style)
                    .hint(format!("{} {}", family.family, f.style))
                    .clicked()
                {
                    picked = Some(format!("{} {}", family.family, f.style));
                }
            });
        });
        picked
    }

    pub(super) fn font_picker(&mut self, ui: &mut egui::Ui, salt: &str, shown: &str, hint: &str) -> Option<String> {
        let mut picked = None;
        let busy = self.jobs.busy(Tag::Fonts);
        if busy {
            fonts::forget();
        }
        egui::ComboBox::from_id_salt(salt)
            .width(180.0)
            .height(460.0)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .selected_text(format!("{} {shown}", icon::TEXT_T))
            .show_ui(ui, |ui| {
            ui.set_min_width(380.0);
            ui.horizontal(|ui| {
                let label = ui.label(icon::SEARCH);
                ui.add(
                    egui::TextEdit::singleline(&mut self.picker.search)
                        .desired_width(220.0)
                        .hint_text("search fonts"),
                )
                .labelled_by(label.id)
                .hint("type part of a font's name or a group; open fonts that klad does not have yet show below to download");
            });
            ui.horizontal_wrapped(|ui| {
                if ui.selectable_label(self.picker.group.is_none(), "all").hint("every group").clicked() {
                    self.picker.group = None;
                }
                fonts::GROUPS.iter().for_each(|g| {
                    if ui
                        .selectable_label(self.picker.group == Some(*g), *g)
                        .hint(format!("only {g} fonts"))
                        .clicked()
                    {
                        self.picker.group = Some(g);
                    }
                });
            });
            ui.separator();
            let words = self.picker.search.clone();
            let wanted = fonts::key(&words);
            let group = self.picker.group;
            let families: Vec<Family> = fonts::installed()
                .into_iter()
                .filter(|f| group.is_none_or(|g| f.group == g))
                .filter(|f| wanted.is_empty() || format!("{} {}", fonts::key(&f.family), f.group).contains(&wanted))
                .collect();
            let own: Vec<String> = self
                .fonts()
                .into_iter()
                .filter(|file| !fonts::installed().iter().any(|f| f.files.iter().any(|x| x.file == *file)))
                .filter(|file| wanted.is_empty() || fonts::key(file).contains(&wanted))
                .collect();
            let more: Vec<fonts::Listed> = match wanted.len() >= 2 {
                true => fonts::search(&words)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|l| group.is_none_or(|g| l.group == g))
                    .filter(|l| !families.iter().any(|f| fonts::key(&f.family) == fonts::key(&l.family)))
                    .filter(|l| !fonts::installed().iter().any(|f| fonts::key(&f.family) == fonts::key(&l.family)))
                    .take(12)
                    .collect(),
                false => Vec::new(),
            };
            egui::ScrollArea::vertical().max_height(380.0).pen_scroll(ui.ctx()).show(ui, |ui| {
                families.iter().for_each(|f| {
                    if let Some(name) = self.family_row(ui, f, shown) {
                        picked = Some(name);
                    }
                });
                if !own.is_empty() {
                    ui.label(RichText::new("in this drawing").color(theme::WEAK));
                    own.iter().for_each(|file| {
                        if ui
                            .add(egui::Button::new(file).min_size(vec2(200.0, theme::CONTROL)))
                            .hint("a font file copied into this drawing")
                            .clicked()
                        {
                            picked = Some(file.clone());
                        }
                    });
                }
                if !more.is_empty() {
                    ui.label(RichText::new("more open fonts, to download").color(theme::WEAK));
                    more.iter().for_each(|l| {
                        ui.horizontal(|ui| {
                            ui.label(format!("{} ({})", l.family, l.group));
                            if ui
                                .add_enabled(!busy, egui::Button::new(format!("{} download", icon::DOWNLOAD)))
                                .hint("download this family from Google Fonts; it then shows above")
                                .clicked()
                            {
                                self.download_font(&l.family);
                            }
                        });
                    });
                } else if families.is_empty() && own.is_empty() {
                    ui.label(RichText::new("no font matches; type at least two letters to search the open fonts").color(theme::WEAK));
                }
            });
            if picked.is_some() {
                ui.close();
            }
        })
        .response
        .hint(hint);
        picked
    }

    pub(super) fn download_font(&mut self, family: &str) {
        self.start_job(Spec {
            tag: Tag::Fonts,
            name: format!("download {family}"),
            exe: std::path::PathBuf::new(),
            args: vec!["fonts".into(), "--get".into(), family.to_string()],
            env: Vec::new(),
            reruns: 0,
            summary: |out, _| out.lines().next().unwrap_or_default().to_string(),
            failure: |_, err| err.lines().last().unwrap_or_default().to_string(),
        });
    }

    pub(super) fn weight_slider(&self, ui: &mut egui::Ui, file: &str, weight: Option<f32>) -> Option<f32> {
        let bytes = klad_render::assets::font(file).ok()?;
        let axis = typeset::weight_axis(&bytes)?;
        let mut value = weight.unwrap_or(axis.default);
        let moved = ui
            .add(
                egui::Slider::new(&mut value, axis.min..=axis.max)
                    .step_by(1.0)
                    .text("weight"),
            )
            .hint("this font comes in every weight: 400 is regular, 700 bold")
            .changed();
        moved.then_some(value)
    }

    pub(super) fn text_options(&mut self, ui: &mut egui::Ui) -> Result<()> {
        let shown = self.text_tool.font.clone();
        if let Some(name) = self.font_picker(
            ui,
            "text.tool.font",
            &shown,
            "the font new text is typed in: klad's own, downloaded ones and the drawing's",
        ) {
            self.text_tool.font = name.clone();
            self.text_tool.weight = None;
            let root = self.scene.bundle.root.clone();
            if let Some(t) = self.text_edit.as_mut().filter(|t| t.id.is_none())
                && let Ok(chosen) = fonts::bring(&root, &name)
                && let Ok(bytes) = std::fs::read(root.join(klad_render::assets::FONTS).join(&chosen.file))
            {
                t.text.font = chosen.file;
                t.text.weight = chosen.weight;
                t.bytes = Arc::new(bytes);
            }
        }
        let file = fonts::find(&self.text_tool.font).map(|p| (p.family.path(&p.face), p.weight));
        if let Some((path, implied)) = file
            && let Some(axis) = std::fs::read(path).ok().and_then(|b| typeset::weight_axis(&b))
        {
            let mut value = self.text_tool.weight.or(implied).unwrap_or(axis.default);
            if super::options::whole(ui, "weight", |ui| {
                ui.add(
                    egui::Slider::new(&mut value, axis.min..=axis.max)
                        .step_by(1.0)
                        .text("weight"),
                )
            })
            .hint("this font comes in every weight: 400 is regular, 700 bold")
            .changed()
            {
                self.text_tool.weight = Some(value);
            }
        }
        let tool = &mut self.text_tool;
        ui.add(
            egui::DragValue::new(&mut tool.size)
                .speed(0.5)
                .range(1.0..=4096.0)
                .suffix(" px"),
        )
        .hint("the size of new text");
        [
            (Align::Left, "left"),
            (Align::Center, "centre"),
            (Align::Right, "right"),
        ]
        .iter()
        .for_each(|(align, label)| {
            if ui
                .selectable_label(tool.align == *align, *label)
                .hint("which end of each line sits where you click")
                .clicked()
            {
                tool.align = *align;
            }
        });
        ui.add(
            egui::DragValue::new(&mut tool.tracking)
                .speed(0.2)
                .range(-200.0..=1000.0)
                .prefix("tracking "),
        )
        .hint("extra px between letters");
        ui.add(
            egui::DragValue::new(&mut tool.leading)
                .speed(0.01)
                .range(0.1..=10.0)
                .prefix("line height "),
        )
        .hint("the distance between lines as a multiple of the size");
        let (size, align, tracking, leading, weight) =
            (tool.size, tool.align, tool.tracking, tool.leading, tool.weight);
        let colour = self.color;
        if let Some(t) = self.text_edit.as_mut().filter(|t| t.id.is_none()) {
            t.text.size = size;
            t.text.align = align;
            t.text.tracking = tracking;
            t.text.leading = leading;
            if weight.is_some() {
                t.text.weight = weight;
            }
            t.fill = colour;
        }
        super::options::note(
            ui,
            RichText::new(
                "click to type, Return for a new line, Esc to finish; the first colour fills it; click text to edit it",
            )
            .color(theme::WEAK),
        );
        Ok(())
    }

    pub(super) fn font_props(&mut self, name: &str) -> Vec<(String, serde_json::Value)> {
        let root = self.scene.bundle.root.clone();
        match fonts::bring(&root, name) {
            Ok(chosen) => {
                let varies = std::fs::read(root.join(klad_render::assets::FONTS).join(&chosen.file))
                    .ok()
                    .and_then(|b| typeset::weight_axis(&b))
                    .is_some();
                let weight = match chosen.weight {
                    Some(w) => Some(json!(w)),
                    None if !varies => Some(serde_json::Value::Null),
                    None => None,
                };
                [("font".to_string(), json!(chosen.file))]
                    .into_iter()
                    .chain(weight.map(|w| ("weight".to_string(), w)))
                    .collect()
            }
            Err(e) => {
                self.say(Tone::Refused, format!("{e:#}"));
                Vec::new()
            }
        }
    }

    pub(super) fn double_click_text(&mut self, response: &egui::Response, view: super::View, rect: Rect) -> Result<()> {
        if !matches!(self.tool, Tool::Select | Tool::Text) || !response.double_clicked() {
            return Ok(());
        }
        let Some(p) = response.interact_pointer_pos() else {
            return Ok(());
        };
        let c = view.to_canvas(p - rect.min, rect.size());
        if let Some(id) = self.text_at(c) {
            self.drag = None;
            self.lasso = None;
            self.tool = Tool::Text;
            if self.text_edit.as_ref().and_then(|t| t.id.as_deref()) != Some(id.as_str()) {
                self.finish_typing()?;
                self.edit_text(&id, Some(c))?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
pub(super) fn screen_point(m: [f32; 6], x: f32, y: f32) -> egui::Pos2 {
    let v = apply(m, x, y);
    pos2(v.x, v.y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn affines_compose_and_invert() {
        let a = [2.0, 0.0, 0.0, 3.0, 10.0, 20.0];
        let b = [0.0, 1.0, -1.0, 0.0, 5.0, 0.0];
        let ab = compose(a, b);
        let p = apply(ab, 1.0, 2.0);
        let q = apply(a, apply(b, 1.0, 2.0).x, apply(b, 1.0, 2.0).y);
        assert!((p - q).length() < 1e-5);
        let back = apply(invert(ab).unwrap(), p.x, p.y);
        assert!((back - vec2(1.0, 2.0)).length() < 1e-4);
        assert_eq!(prev_char("aé", 3), 1);
        assert_eq!(next_char("aé", 1), 3);
        assert_eq!(screen_point(IDENTITY, 1.0, 2.0), pos2(1.0, 2.0));
    }
}
