use super::{Editor, Tool};
use crate::commands::Cmd;
use crate::messages::Tone;
use crate::theme::Hint;
use anyhow::Result;
use klad_core::doc::{Cap, Item, Join, Node, Shape};
use klad_core::ops::Op;
use klad_render::combine::Combine;
use serde_json::{Map, Value, json};

pub(super) fn combine_of(cmd: Cmd) -> Option<Combine> {
    match cmd {
        Cmd::Unite => Some(Combine::Unite),
        Cmd::SubtractShapes => Some(Combine::Subtract),
        Cmd::Intersect => Some(Combine::Intersect),
        Cmd::Exclude => Some(Combine::Exclude),
        _ => None,
    }
}

pub(super) const COMBINE: [Cmd; 4] = [Cmd::Unite, Cmd::SubtractShapes, Cmd::Intersect, Cmd::Exclude];

const CAPS: [(Cap, &str); 3] = [(Cap::Butt, "flat"), (Cap::Round, "round"), (Cap::Square, "square")];
const JOINS: [(Join, &str); 3] = [(Join::Miter, "sharp"), (Join::Round, "round"), (Join::Bevel, "cut")];

fn word(v: &impl serde::Serialize) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

impl Editor {
    fn picked_nodes(&self) -> Vec<Node> {
        self.shapes()
            .iter()
            .filter_map(|id| match self.scene.bundle.doc.item(id) {
                Some(Item::Node(_, _, node)) => Some(node.clone()),
                _ => None,
            })
            .collect()
    }

    pub(super) fn outline_row(&mut self, ui: &mut egui::Ui) -> Result<()> {
        let nodes = self.picked_nodes();
        let Some(first) = nodes.first() else {
            return Ok(());
        };
        let width = first.stroke.as_ref().map_or(0.0, |s| s.width);
        let key = format!("{}.outline", first.id);
        let mut props: Option<Map<String, Value>> = None;
        if let Some(w) = self.number(
            ui,
            &key,
            "outline ",
            width,
            0.0..=400.0,
            0.25,
            "the outline's width in px for every selected shape; 0 takes the outline off",
        ) {
            let mut map = Map::new();
            match w > 0.0 {
                true => {
                    map.insert("stroke.width".into(), json!(w));
                    if first.stroke.is_none() {
                        map.insert("stroke.paint".into(), json!(crate::color::hex(self.second)));
                    }
                }
                false => {
                    map.insert("stroke".into(), Value::Null);
                }
            }
            props = Some(map);
        }
        let (cap, join) = first
            .stroke
            .as_ref()
            .map_or((Cap::Round, Join::Round), |s| (s.cap, s.join));
        ui.add_enabled_ui(first.stroke.is_some(), |ui| {
            ui.menu_button("ends", |ui| {
                ui.label("ends");
                CAPS.iter().for_each(|(c, label)| {
                    if ui.selectable_label(cap == *c, *label).clicked() {
                        props = Some(Map::from_iter([("stroke.cap".to_string(), word(c))]));
                        ui.close();
                    }
                });
                ui.separator();
                ui.label("corners");
                JOINS.iter().for_each(|(j, label)| {
                    if ui.selectable_label(join == *j, *label).clicked() {
                        props = Some(Map::from_iter([("stroke.join".to_string(), word(j))]));
                        ui.close();
                    }
                });
            })
            .response
            .hint("how the outline ends and turns corners");
        });
        if let Some(props) = props {
            let ops: Vec<Op> = nodes
                .iter()
                .map(|n| Op::Set {
                    id: n.id.clone(),
                    props: props.clone(),
                })
                .collect();
            self.apply(ops)?;
        }
        Ok(())
    }

    pub(super) fn copy_look(&mut self) {
        let Some(node) = self.picked_nodes().into_iter().next() else {
            self.say(Tone::Refused, "select a shape with V first: its look is copied");
            return;
        };
        let name = match node.name.is_empty() {
            true => format!("this {}", node.type_name()),
            false => node.name.clone(),
        };
        self.look_from = Some(node.id.clone());
        self.say(
            Tone::Done,
            format!("copied the look of {name}: select other shapes and press alt cmd V to give it to them"),
        );
    }

    pub(super) fn paste_look(&mut self) -> Result<()> {
        let Some(from) = self.look_from.clone() else {
            return Ok(());
        };
        if !matches!(self.scene.bundle.doc.item(&from), Some(Item::Node(..))) {
            self.look_from = None;
            self.say(
                Tone::Refused,
                "the shape whose look you copied is gone: copy a look again",
            );
            return Ok(());
        }
        let ids: Vec<String> = self.shapes().into_iter().filter(|id| *id != from).collect();
        if ids.is_empty() {
            return Ok(());
        }
        let count = ids.len();
        self.apply(vec![Op::Style { from, ids }])?;
        self.say(
            Tone::Done,
            format!(
                "gave {} the copied fill, outline and opacity",
                if count == 1 {
                    "the shape".to_string()
                } else {
                    format!("{count} shapes")
                }
            ),
        );
        Ok(())
    }

    pub(super) fn can_combine(&self) -> Option<String> {
        (self.shapes().len() < 2).then(|| "select two shapes or more with V (shift click adds)".to_string())
    }

    pub(super) fn combine_shapes(&mut self, how: Combine) -> Result<()> {
        let ids = self.shapes();
        let ops = match klad_render::combine::combine(&self.scene.bundle.doc, &ids, how) {
            Ok(ops) => ops,
            Err(e) => {
                self.say(Tone::Refused, format!("{e:#}"));
                return Ok(());
            }
        };
        let (dirty, applied) = self.write(&ops)?;
        self.dirty.extend(dirty);
        let made = applied
            .added
            .iter()
            .find(|id| matches!(self.scene.bundle.doc.item(id), Some(Item::Node(..))))
            .cloned();
        self.group.clear();
        self.selection = made;
        self.tool = Tool::Select;
        self.say(
            Tone::Done,
            format!(
                "{} {} shapes into one path (cmd Z takes it back)",
                how.word(),
                ids.len()
            ),
        );
        Ok(())
    }

    pub(super) fn text_node(&self) -> Option<String> {
        let id = self
            .direct
            .as_ref()
            .map(|d| d.node.clone())
            .or_else(|| self.selection.clone())?;
        match self.scene.bundle.doc.item(&id) {
            Some(Item::Node(_, _, n)) if matches!(n.shape, Shape::Text(_)) => Some(id),
            _ => None,
        }
    }

    pub(super) fn text_to_path(&mut self, id: &str) -> Result<()> {
        let Some(ops) = klad_render::combine::text_to_path(&self.scene.bundle.doc, id)? else {
            return Ok(());
        };
        let (dirty, applied) = self.write(&ops)?;
        self.dirty.extend(dirty);
        self.direct = None;
        self.selection = applied.added.first().cloned();
        self.tool = Tool::Select;
        self.say(
            Tone::Done,
            "the text is a path now, the outlines of its letters: A edits their points (cmd Z takes it back)",
        );
        Ok(())
    }
}
