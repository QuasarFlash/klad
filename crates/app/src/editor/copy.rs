use super::selection::{Selected, strip_ids};
use super::{Editor, Part, Picked, Tool, ms};
use crate::commands::Cmd;
use crate::messages::Tone;
use anyhow::Result;
use klad_core::doc::{Item, Kind, Loc};
use klad_core::markedit::{self, LassoAction, MarkChange, Side};
use klad_core::ops::{Applied, CopySpec, CopyTarget, Op, Place};
use klad_core::stroke::Mark;
use serde_json::{Value, json};
use std::time::Instant;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Piece {
    layer: String,
    cel: String,
    cut: Option<Vec<[f32; 2]>>,
    around: Vec<Mark>,
    whole: Vec<Mark>,
    picked: usize,
}

impl Piece {
    fn count(&self) -> usize {
        self.picked
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Copied {
    pieces: Vec<Piece>,
    from: std::path::PathBuf,
    brushes: Vec<(String, klad_core::brushes::BrushDef)>,
}

impl Copied {
    pub(super) fn marks(&self) -> Vec<Mark> {
        self.pieces
            .iter()
            .flat_map(|p| {
                let cut = p.cut.clone();
                p.around
                    .iter()
                    .cloned()
                    .map(move |mut m| {
                        if let Some(polygon) = &cut {
                            m.cuts_mut().push(klad_core::cut::Cut {
                                keep: klad_core::cut::Keep::Inside,
                                polygon: polygon.clone(),
                            });
                        }
                        m
                    })
                    .chain(p.whole.iter().cloned())
            })
            .collect()
    }

    fn count(&self) -> usize {
        self.pieces.iter().map(Piece::count).sum()
    }
}

fn held(marks: &[Mark], ids: &[u32]) -> Vec<Mark> {
    marks
        .iter()
        .filter(|m| ids.contains(&m.id()) || m.as_stroke().is_some_and(|s| s.on.iter().any(|id| ids.contains(id))))
        .cloned()
        .collect()
}

pub(super) fn piece(layer: &str, cel: &str, ids: &[u32], cut: Option<&Vec<[f32; 2]>>, marks: &[Mark]) -> Piece {
    let (around, whole, cut) = match cut {
        Some(polygon) => {
            let around: Vec<u32> = markedit::sides(marks, polygon)
                .into_iter()
                .filter(|(_, side)| *side != Side::Outside)
                .map(|(id, _)| id)
                .collect();
            let whole: Vec<u32> = ids.iter().copied().filter(|id| !around.contains(id)).collect();
            let cut = (!around.is_empty()).then(|| polygon.clone());
            (around, whole, cut)
        }
        None => (Vec::new(), ids.to_vec(), None),
    };
    Piece {
        layer: layer.to_string(),
        cel: cel.to_string(),
        cut,
        picked: around.len() + whole.len(),
        around: held(marks, &around),
        whole: held(marks, &whole),
    }
}

pub(super) fn paste_ops(piece: &Piece, to: &CopyTarget) -> Vec<Op> {
    let op = |marks: &[Mark], cut: Option<Vec<[f32; 2]>>| {
        Op::Copy(CopySpec {
            ids: Vec::new(),
            held: marks.iter().map(|m| (piece.cel.clone(), m.clone())).collect(),
            to: to.clone(),
            transform: None,
            cut,
        })
    };
    let around = (!piece.around.is_empty()).then(|| op(&piece.around, piece.cut.clone()));
    let whole = (!piece.whole.is_empty()).then(|| op(&piece.whole, None));
    around.into_iter().chain(whole).collect()
}

fn counted(n: usize, one: &str) -> String {
    match n {
        1 => format!("one {one}"),
        n => format!("{n} {one}s"),
    }
}

fn made_by(applied: &Applied) -> Vec<(String, String, u32)> {
    applied
        .marks
        .iter()
        .flat_map(|p| {
            p.changes.iter().flat_map(move |c| match c {
                MarkChange::Paste { made, .. } => made.iter().map(|m| (p.layer.clone(), p.cel.clone(), m.to)).collect(),
                _ => Vec::new(),
            })
        })
        .collect()
}

impl Editor {
    pub(super) fn copied(&self) -> Option<Copied> {
        let picked = self.picked.as_ref()?;
        let layer = self.scene.active_id()?;
        let mut pieces: Vec<Piece> = std::iter::once(piece(
            &layer,
            &picked.cel,
            &picked.ids,
            picked.cut.as_ref(),
            self.scene.active_marks(),
        ))
        .chain(picked.more.iter().map(|p| {
            piece(
                &p.layer,
                &p.cel,
                &p.ids,
                picked.cut.as_ref(),
                self.scene.marks_of(&p.cel),
            )
        }))
        .filter(|p| p.count() > 0)
        .collect();
        let order: Vec<&str> = self
            .scene
            .bundle
            .doc
            .all_layers()
            .into_iter()
            .map(|l| l.id.as_str())
            .collect();
        pieces.sort_by_key(|p| order.iter().position(|id| *id == p.layer));
        let brushes = pieces
            .iter()
            .flat_map(|p| p.around.iter().chain(&p.whole))
            .filter_map(Mark::as_stroke)
            .map(|s| s.brush.clone())
            .collect::<std::collections::BTreeSet<String>>()
            .into_iter()
            .filter_map(|id| self.embedded.get(&id).map(|def| (id, def.clone())))
            .collect();
        (!pieces.is_empty()).then_some(Copied {
            pieces,
            from: self.scene.bundle.root.clone(),
            brushes,
        })
    }

    fn copied_shapes(&self) -> Vec<Value> {
        match self.selected() {
            Some(Selected::Shape(_)) => self
                .group_nodes()
                .into_iter()
                .filter_map(|(_, node)| serde_json::to_value(node).ok())
                .map(|mut value| {
                    strip_ids(&mut value);
                    value
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    fn keep_picked(&mut self, ctx: &egui::Context) -> Option<String> {
        if let Some(picture) = self.selection_picture() {
            self.picture_out(ctx, picture);
        }
        let shapes = self.copied_shapes();
        if !shapes.is_empty() {
            let what = counted(shapes.len(), "shape");
            self.shape_clip = Some(shapes);
            self.shape_from = Some(self.scene.bundle.root.clone());
            self.clipboard = None;
            self.cells_last = false;
            return Some(what);
        }
        let copied = self.copied()?;
        self.shape_clip = None;
        let what = counted(copied.count(), "mark");
        self.clipboard = Some(copied);
        self.cells_last = false;
        Some(what)
    }

    pub(super) fn copy_picked(&mut self, ctx: &egui::Context) {
        match self.keep_picked(ctx) {
            Some(what) => self.say(
                Tone::Done,
                format!("copied {what}: cmd V puts a copy on this or another frame, in place"),
            ),
            None => self.say(
                Tone::Refused,
                "select strokes (S) or a shape (V) first; cmd C copies them",
            ),
        }
    }

    pub(super) fn cut_picked(&mut self) -> Result<()> {
        let ctx = self.ctx.clone();
        let Some(what) = self.keep_picked(&ctx) else {
            self.say(
                Tone::Refused,
                "select strokes (S) or a shape (V) first; cmd X cuts them",
            );
            return Ok(());
        };
        match self.on_marks() {
            true => self.edit_picked(LassoAction::Remove)?,
            false => self.remove_shapes()?,
        }
        self.say(
            Tone::Done,
            format!("cut {what}: cmd V puts them back on this or another frame, in place (cmd Z undoes the cut)"),
        );
        Ok(())
    }

    pub(super) fn remove_shapes(&mut self) -> Result<()> {
        let ids = self.shapes();
        self.selection = None;
        self.group.clear();
        if !ids.is_empty() {
            self.apply(vec![Op::Remove { ids }])?;
        }
        Ok(())
    }

    pub(super) fn paste(&mut self) -> Result<()> {
        if let Some(shapes) = self.shape_clip.clone() {
            return self.paste_shapes(shapes);
        }
        match self.clipboard.clone() {
            Some(copied) => self.paste_copied(copied, "pasted"),
            None => Ok(()),
        }
    }

    fn bring_pictures(&mut self, shapes: &[Value]) -> Result<usize> {
        let Some(from) = self.shape_from.clone().filter(|f| *f != self.scene.bundle.root) else {
            return Ok(0);
        };
        fn sources(v: &Value, out: &mut Vec<String>) {
            match v {
                Value::Object(map) => {
                    if let Some(Value::String(src)) = map.get("src") {
                        out.push(src.clone());
                    }
                    map.values().for_each(|c| sources(c, out));
                }
                Value::Array(items) => items.iter().for_each(|c| sources(c, out)),
                _ => {}
            }
        }
        let mut found = Vec::new();
        shapes.iter().for_each(|s| sources(s, &mut found));
        let root = self.scene.bundle.root.clone();
        found
            .iter()
            .filter(|src| !src.contains("..") && !std::path::Path::new(src).is_absolute())
            .filter(|src| !root.join(src).exists() && from.join(src).is_file())
            .try_fold(0, |n, src| {
                klad_core::bundle::write_atomic(&root.join(src), &std::fs::read(from.join(src))?)?;
                Ok(n + 1)
            })
    }

    fn bring_brushes(&mut self, copied: &Copied) -> Result<()> {
        if copied.from == self.scene.bundle.root {
            return Ok(());
        }
        copied.brushes.iter().try_for_each(|(id, def)| {
            if self.scene.bundle.embed_brush(id, def)? {
                self.embedded.insert(id.clone(), def.clone());
                self.brushes.insert(id, def.clone());
            }
            Ok(())
        })
    }

    fn rebound(&self, copied: Copied) -> Copied {
        if copied.from == self.scene.bundle.root {
            return copied;
        }
        let palette = &self.scene.bundle.doc.palette;
        let swatch_of = |colour: [u16; 4]| {
            palette
                .iter()
                .filter(|s| !s.id.is_empty())
                .find(|s| {
                    klad_core::color::parse_hex(&s.color).is_ok_and(|c| klad_core::color::to_u16(c)[..3] == colour[..3])
                })
                .map(|s| s.id.clone())
        };
        let rebind = |m: &Mark| -> Mark {
            match m {
                Mark::Fill(fill) => {
                    let mut fill = fill.clone();
                    if let klad_core::fill::FillSource::Cells { entry, .. } = &mut fill.source
                        && !entry.is_empty()
                    {
                        *entry = swatch_of(fill.color).unwrap_or_default();
                    }
                    Mark::Fill(fill)
                }
                other => other.clone(),
            }
        };
        Copied {
            pieces: copied
                .pieces
                .into_iter()
                .map(|p| Piece {
                    around: p.around.iter().map(rebind).collect(),
                    whole: p.whole.iter().map(rebind).collect(),
                    ..p
                })
                .collect(),
            ..copied
        }
    }

    fn paste_shapes(&mut self, shapes: Vec<Value>) -> Result<()> {
        self.bring_pictures(&shapes)?;
        let frame = self.scene.frame;
        let home = self.vector_home("shapes");
        let ops: Vec<Op> = home
            .ops
            .iter()
            .cloned()
            .chain(shapes.into_iter().map(|node| Op::AddNode {
                node,
                place: home.place(frame),
            }))
            .collect();
        let (dirty, applied) = match self.write(&ops) {
            Ok(done) => done,
            Err(e) => {
                self.say(Tone::Refused, format!("could not paste here: {e:#}"));
                return Ok(());
            }
        };
        self.dirty.extend(dirty);
        self.move_in(&home)?;
        let doc = &self.scene.bundle.doc;
        let nodes: Vec<String> = applied
            .added
            .iter()
            .filter(|id| matches!(doc.item(id), Some(Item::Node(..))))
            .cloned()
            .collect();
        let what = counted(nodes.len(), "shape");
        self.tool = Tool::Select;
        self.picked = None;
        self.select_shapes(nodes);
        self.say(
            Tone::Done,
            format!(
                "pasted {what}{} on frame {frame}, in place and selected: drag to move it (one cmd Z takes it off)",
                home.said()
            ),
        );
        Ok(())
    }

    fn pick_made(&mut self, made: &[(String, String, u32)]) {
        let Some(cel) = self.scene.active_cel().filter(|_| !made.is_empty()) else {
            return;
        };
        if self.tool != Tool::Select {
            self.tool = Tool::Strokes;
        }
        self.selection = None;
        let ids = made
            .iter()
            .filter(|(_, c, _)| *c == cel)
            .map(|(_, _, id)| *id)
            .collect();
        let more = made
            .iter()
            .filter(|(_, c, _)| *c != cel)
            .fold(Vec::<Part>::new(), |mut parts, (layer, c, id)| {
                match parts.iter_mut().find(|p| p.cel == *c) {
                    Some(p) => p.ids.push(*id),
                    None => parts.push(Part {
                        layer: layer.clone(),
                        cel: c.clone(),
                        ids: vec![*id],
                    }),
                }
                parts
            });
        self.picked = Some(Picked {
            more,
            ..Picked::whole(cel, ids)
        });
    }

    pub(super) fn move_to_new_layer(&mut self) -> Result<()> {
        let frame = self.scene.frame;
        let taken = self.scene.bundle.doc.ids();
        let layer = (1..)
            .map(|n| format!("l{n}"))
            .find(|id| !taken.contains(id))
            .unwrap_or_default();
        let (above, name) = self.scene.active_layer().map_or((None, "part".to_string()), |l| {
            (Some(l.id.clone()), format!("{} part", l.name))
        });
        let add = |kind: &str| Op::AddLayer {
            layer: json!({"id": layer, "kind": kind, "name": name, "cels": [{"frame": frame}]}),
            place: Place {
                above: above.clone(),
                ..Place::default()
            },
        };
        let (ops, what) = match self.selected() {
            Some(Selected::Shape(_)) => {
                let shapes = self.shapes();
                let moves = shapes.iter().map(|id| Op::Move {
                    id: id.clone(),
                    place: Place {
                        into: Some(layer.clone()),
                        frame: Some(frame),
                        ..Place::default()
                    },
                });
                (
                    std::iter::once(add("vector")).chain(moves).collect::<Vec<Op>>(),
                    counted(shapes.len(), "shape"),
                )
            }
            Some(Selected::Strokes) => {
                let (Some(copied), Some((removal, _))) = (self.copied(), self.picked_ops(LassoAction::Remove)) else {
                    self.say(Tone::Refused, "the selection is not ready yet: try again in a moment");
                    return Ok(());
                };
                let to = CopyTarget::Frame {
                    frame,
                    layer: Some(layer.clone()),
                };
                let kind = match self.scene.on_grid() {
                    true => "pixel",
                    false => "paint",
                };
                (
                    std::iter::once(add(kind))
                        .chain(copied.pieces.iter().flat_map(|p| paste_ops(p, &to)))
                        .chain(removal)
                        .collect(),
                    counted(copied.count(), "mark"),
                )
            }
            _ => return Ok(()),
        };
        let shapes = self.shapes();
        let (dirty, applied) = match self.write(&ops) {
            Ok(done) => done,
            Err(e) => {
                self.say(Tone::Refused, format!("could not move it to a new layer: {e:#}"));
                return Ok(());
            }
        };
        self.dirty.extend(dirty);
        self.picked = None;
        self.selection = None;
        if let Some(Loc::Layer(path)) = self.scene.bundle.doc.locate(&layer) {
            self.activate(path)?;
        }
        match made_by(&applied) {
            made if made.is_empty() => {
                self.tool = Tool::Select;
                self.select_shapes(shapes);
            }
            made => self.pick_made(&made),
        }
        self.forget_missing_selection();
        self.say(
            Tone::Done,
            format!("moved {what} to the new layer {name}, right above; they stay selected (one cmd Z puts them back)"),
        );
        Ok(())
    }

    pub(super) fn canvas_menu(&mut self, ui: &mut egui::Ui) {
        [
            Cmd::Cut,
            Cmd::Copy,
            Cmd::Paste,
            Cmd::DuplicateInPlace,
            Cmd::Delete,
            Cmd::MoveToNewLayer,
        ]
        .into_iter()
        .for_each(|cmd| self.menu_entry(ui, cmd));
        ui.separator();
        [Cmd::SelectAll, Cmd::Deselect]
            .into_iter()
            .for_each(|cmd| self.menu_entry(ui, cmd));
        ui.separator();
        super::arrange::ARRANGE
            .into_iter()
            .for_each(|cmd| self.menu_entry(ui, cmd));
    }

    fn target(&self, layer: &klad_core::doc::Layer) -> CopyTarget {
        let frame = self.scene.frame;
        match layer.content_at(frame) {
            Some(cel) if cel.frame == frame => CopyTarget::Cel(cel.id.clone()),
            _ => CopyTarget::Frame {
                frame,
                layer: Some(layer.id.clone()),
            },
        }
    }

    pub(super) fn paste_copied(&mut self, copied: Copied, word: &str) -> Result<()> {
        self.bring_brushes(&copied)?;
        let copied = self.rebound(copied);
        let doc = &self.scene.bundle.doc;
        let paintable = |l: &&klad_core::doc::Layer| l.kind() == Kind::Paint && !l.locked;
        let in_hand = self.scene.active_id();
        let here = copied.from == self.scene.bundle.root;
        let own = here && copied.pieces.len() > 1 && copied.pieces.iter().any(|p| Some(&p.layer) == in_hand.as_ref());
        let (targets, onto): (Vec<(&Piece, CopyTarget)>, String) = match own {
            false => {
                let Some(layer) = self.scene.active_layer().filter(paintable) else {
                    self.say(Tone::Refused, "make a paint layer active to paste strokes into it");
                    return Ok(());
                };
                let to = self.target(layer);
                let onto = match copied.pieces.len() {
                    1 => String::new(),
                    n => format!(" from {n} layers onto {}", layer.name),
                };
                (copied.pieces.iter().map(|p| (p, to.clone())).collect(), onto)
            }
            true => {
                let missing: Vec<&str> = copied
                    .pieces
                    .iter()
                    .filter(|p| doc.layer(&p.layer).filter(paintable).is_none())
                    .map(|p| doc.layer(&p.layer).map_or("one that was removed", |l| l.name.as_str()))
                    .collect();
                if !missing.is_empty() {
                    self.say(
                        Tone::Refused,
                        format!(
                            "could not {word}: these strokes go back to the layers they came from, and of those {} cannot take them (locked or gone). Unlock it, or pick a layer they did not come from to get them all there",
                            missing.join(", ")
                        ),
                    );
                    return Ok(());
                }
                let layers: Vec<&klad_core::doc::Layer> =
                    copied.pieces.iter().filter_map(|p| doc.layer(&p.layer)).collect();
                let names: Vec<&str> = layers.iter().map(|l| l.name.as_str()).collect();
                (
                    copied
                        .pieces
                        .iter()
                        .zip(&layers)
                        .map(|(p, l)| (p, self.target(l)))
                        .collect(),
                    format!(", each on the layer it came from ({})", names.join(", ")),
                )
            }
        };
        let frame = self.scene.frame;
        let start = Instant::now();
        let held = targets.iter().any(|(_, to)| {
            matches!(to, CopyTarget::Frame { layer: Some(l), .. } if doc.layer(l).and_then(|l| l.content_at(frame)).is_some())
        });
        let ops: Vec<Op> = targets.iter().flat_map(|(p, to)| paste_ops(p, to)).collect();
        let (dirty, applied) = match self.write(&ops) {
            Ok(done) => done,
            Err(e) => {
                self.say(Tone::Refused, format!("could not {word} here: {e:#}"));
                return Ok(());
            }
        };
        self.dirty.extend(dirty);
        let made = made_by(&applied);
        if self.scene.active_cel().is_some() && !made.is_empty() {
            self.pick_made(&made);
            self.measure("paste", format!("{word} {} marks in {:.0} ms", made.len(), ms(start)));
            self.say(
                Tone::Done,
                format!(
                    "{word} {}{onto} on frame {frame}, in place and selected: drag to move them (one cmd Z takes them off){}",
                    counted(made.len(), "mark"),
                    match held {
                        true => format!("; frame {frame} showed an earlier drawing, so it got its own copy of it first and the frames before it stay as they were"),
                        false => String::new(),
                    }
                ),
            );
        }
        self.forget_missing_selection();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use klad_core::stroke::Stroke;

    fn line(id: u32, x: f32) -> Mark {
        Stroke::new(id, "ink", [0.0, 0.0, 0.0, 1.0], 4.0, 1)
            .with_points(&[(x, 10.0, 1.0), (x, 90.0, 1.0)], 8.0)
            .into()
    }

    #[test]
    fn a_cut_selection_copies_the_parts_inside_and_whole_picks_whole() {
        let marks = vec![line(1, 20.0), line(2, 50.0), line(3, 200.0)];
        let polygon = vec![[0.0, 0.0], [100.0, 0.0], [100.0, 50.0], [0.0, 50.0]];
        let cut = piece("l4", "c4", &[3], Some(&polygon), &marks);
        let ids = |marks: &[Mark]| marks.iter().map(Mark::id).collect::<Vec<_>>();
        assert_eq!(
            (ids(&cut.around), ids(&cut.whole), cut.count()),
            (vec![1, 2], vec![3], 3)
        );
        let ops = paste_ops(&cut, &CopyTarget::Cel("c9".into()));
        assert_eq!(ops.len(), 2);
        let Op::Copy(first) = &ops[0] else { panic!() };
        assert!(first.ids.is_empty(), "the copy holds the marks, not their places");
        assert_eq!(
            first.held,
            vec![
                ("c4".to_string(), marks[0].clone()),
                ("c4".to_string(), marks[1].clone())
            ]
        );
        assert_eq!(first.cut.as_deref(), Some(&polygon[..]));
        let Op::Copy(second) = &ops[1] else { panic!() };
        assert_eq!(
            (second.held.clone(), second.cut.clone()),
            (vec![("c4".to_string(), marks[2].clone())], None)
        );
        let plain = piece("l4", "c4", &[2], None, &marks);
        assert_eq!(paste_ops(&plain, &CopyTarget::Cel("c9".into())).len(), 1);
        assert_eq!(piece("l4", "c4", &[], None, &marks).count(), 0);
    }
}
