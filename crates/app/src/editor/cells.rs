use super::Editor;
use crate::commands::Cmd;
use crate::messages::Tone;
use crate::theme::{self, Hint, icon};
use anyhow::Result;
use egui::RichText;
use klad_core::doc::{Cel, Document, Layer};
use klad_core::ops::{ClipEdit, Op};
use klad_core::timing::{CelAction, CelSpec, OPEN, ends};

#[derive(Clone, Debug, PartialEq)]
pub(super) struct CellCopy {
    pub layer: String,
    pub offset: u32,
    pub source: String,
    pub length: u32,
    pub link: bool,
}

fn exposures(layer: &Layer, frames: u32) -> Vec<(Cel, u32, u32)> {
    let cels = layer.cels();
    cels.iter()
        .zip(ends(cels))
        .map(|(c, e)| (c.clone(), c.frame, if e == OPEN { frames.max(c.frame + 1) } else { e }))
        .collect()
}

pub(super) fn copy_cells(doc: &Document, rows: &[String], from: u32, to: u32) -> Vec<CellCopy> {
    let frames = doc.timeline.frames;
    rows.iter()
        .filter_map(|id| doc.layer(id).map(|l| (id, l)))
        .flat_map(|(id, layer)| {
            exposures(layer, frames)
                .into_iter()
                .filter(|(_, start, end)| *start <= to && *end > from)
                .map(|(cel, start, end)| {
                    let first = start.max(from);
                    CellCopy {
                        layer: id.clone(),
                        offset: first - from,
                        source: cel.link.clone().unwrap_or_else(|| cel.id.clone()),
                        length: end.min(to + 1) - first,
                        link: cel.link.is_some(),
                    }
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

pub(super) fn paste_ops(doc: &Document, cells: &[CellCopy], at: u32, links: bool) -> Vec<Op> {
    cells
        .iter()
        .map(|c| {
            let frame = at + c.offset;
            let taken = doc
                .layer(&c.layer)
                .is_some_and(|l| l.cels().iter().any(|x| x.frame == frame));
            Op::Cel(CelSpec {
                action: match links || c.link {
                    true => CelAction::Link { cel: c.source.clone() },
                    false => CelAction::Dup { cel: c.source.clone() },
                },
                id: None,
                frame: Some(frame),
                hold: Some(c.length),
                insert: taken,
            })
        })
        .collect()
}

pub(super) fn free_at_the_end(doc: &Document) -> bool {
    let frames = doc.timeline.frames;
    let last = frames.saturating_sub(1);
    frames > 1
        && !doc.timeline.clips.iter().any(|c| c.to >= last)
        && doc.all_layers().iter().all(|l| {
            let cels = l.cels();
            cels.iter()
                .zip(ends(cels))
                .all(|(c, end)| c.frame < last && (end == OPEN || end <= last))
        })
}

pub(super) fn repeat_ops(doc: &Document, rows: &[String], from: u32, to: u32) -> Vec<Op> {
    let frames = doc.timeline.frames;
    let (at, count) = (to + 1, to + 1 - from);
    let layers: Vec<&Layer> = rows
        .iter()
        .filter_map(|id| doc.layer(id))
        .filter(|l| !l.cels().is_empty())
        .collect();
    let covering = |layer: &Layer| {
        exposures(layer, frames)
            .into_iter()
            .find(|(_, start, end)| *start <= to && *end > to)
    };
    let cel_op = |action: CelAction, frame: u32, hold: Option<u32>| {
        Op::Cel(CelSpec {
            action,
            id: None,
            frame: Some(frame),
            hold,
            insert: false,
        })
    };
    let splits = layers
        .iter()
        .filter_map(|l| covering(l).filter(|(_, _, end)| *end > at))
        .map(|(cel, _, _)| {
            cel_op(
                CelAction::Link {
                    cel: cel.link.clone().unwrap_or(cel.id),
                },
                at,
                None,
            )
        });
    let room = Op::Frames(klad_core::timing::FrameSpec {
        insert: true,
        at: to,
        count,
        layers: Some(rows.to_vec()),
    });
    let copies = layers.iter().flat_map(|l| {
        let cells = copy_cells(doc, std::slice::from_ref(&l.id), from, to);
        let lands_at_once = cells.first().is_some_and(|c| c.offset == 0);
        let restore = covering(l)
            .filter(|(cel, _, _)| cel.hold.is_some() || !lands_at_once)
            .map(|(cel, start, _)| Op::Set {
                id: cel.id,
                props: serde_json::Map::from_iter([("hold".to_string(), serde_json::json!(at - start))]),
            });
        let pastes = cells.into_iter().map(|c| {
            cel_op(
                match c.link {
                    true => CelAction::Link { cel: c.source },
                    false => CelAction::Dup { cel: c.source },
                },
                at + c.offset,
                Some(c.length),
            )
        });
        restore.into_iter().chain(pastes).collect::<Vec<_>>()
    });
    splits.chain(std::iter::once(room)).chain(copies).collect()
}

impl Editor {
    pub(super) fn copy_selected_cells(&mut self) {
        let (rows, from, to) = match &self.timeline.span {
            Some(span) => (span.rows.clone(), span.from, span.to),
            None => match self.here() {
                Some((layer, Some((cel, end)))) => {
                    let frames = self.scene.bundle.doc.timeline.frames;
                    let end = if end == OPEN { frames.max(cel.frame + 1) } else { end };
                    (vec![layer], cel.frame, end - 1)
                }
                _ => return,
            },
        };
        let copied = copy_cells(&self.scene.bundle.doc, &rows, from, to);
        let drawings = copied.len();
        self.timeline.cells = Some(copied);
        self.cells_last = true;
        self.say(
            Tone::Done,
            format!(
                "copied {drawings} drawing{} of frames {from} to {to}: paste them at the playhead as copies or links",
                if drawings == 1 { "" } else { "s" }
            ),
        );
    }

    pub(super) fn cut_selected_cells(&mut self) -> Result<()> {
        self.copy_selected_cells();
        let ids = self.cels_here();
        if let Some(count) = self.remove_cels(ids)? {
            self.say(
                Tone::Done,
                format!(
                    "cut {count} drawing{}: paste them at the playhead as copies or links (cmd Z undoes the cut)",
                    if count == 1 { "" } else { "s" }
                ),
            );
        }
        Ok(())
    }

    pub(super) fn paste_cells(&mut self, links: bool) -> Result<()> {
        let Some(cells) = self.timeline.cells.clone() else {
            return Ok(());
        };
        let ops = paste_ops(&self.scene.bundle.doc, &cells, self.scene.frame, links);
        if self.timeline_apply(ops)?.is_some() {
            self.say(
                Tone::Done,
                format!(
                    "pasted {} drawing{} at frame {} as {}",
                    cells.len(),
                    if cells.len() == 1 { "" } else { "s" },
                    self.scene.frame,
                    if links { "links" } else { "copies" }
                ),
            );
        }
        Ok(())
    }

    pub(super) fn move_drawing(&mut self, by: i32) -> Result<()> {
        let Some((_, Some((cel, _)))) = self.here() else {
            return Ok(());
        };
        let to = (cel.frame as i64 + by as i64).max(0) as u32;
        let op = Op::Cel(CelSpec {
            action: CelAction::Move { cel: cel.id.clone() },
            id: None,
            frame: Some(to),
            hold: None,
            insert: false,
        });
        if self.timeline_apply(vec![op])?.is_some() {
            self.go_to(to)?;
        }
        Ok(())
    }

    pub(super) fn selection_toolbar(&mut self, ui: &mut egui::Ui) -> Result<()> {
        let mut hold_to = None;
        ui.horizontal_wrapped(|ui| {
            [
                Cmd::NewDrawing,
                Cmd::Duplicate,
                Cmd::Link,
                Cmd::Unlink,
                Cmd::DeleteDrawing,
            ]
            .into_iter()
            .for_each(|cmd| {
                self.command_button(ui, cmd, None);
            });
            ui.separator();
            self.command_button(ui, Cmd::HoldShorter, None);
            if let Some((_, Some((cel, end)))) = self.here() {
                let frames = self.scene.bundle.doc.timeline.frames;
                let end = if end == OPEN { frames.max(cel.frame + 1) } else { end };
                let mut hold = end - cel.frame;
                let label = ui.label("hold");
                let r = ui
                    .add(egui::DragValue::new(&mut hold).range(1..=10_000))
                    .labelled_by(label.id)
                    .hint("how many frames this drawing shows; later drawings move along");
                if (r.drag_stopped() || r.lost_focus() || (r.changed() && !r.dragged() && !r.has_focus()))
                    && hold != end - cel.frame
                {
                    hold_to = Some((cel.clone(), end, hold));
                }
            }
            self.command_button(ui, Cmd::HoldLonger, None);
            ui.separator();
            [
                Cmd::InsertFrame,
                Cmd::InsertFrameAll,
                Cmd::RemoveFrame,
                Cmd::RemoveFrameAll,
            ]
            .into_iter()
            .for_each(|cmd| {
                self.command_button(ui, cmd, None);
            });
            ui.separator();
            [Cmd::CutCells, Cmd::CopyCells, Cmd::PasteCells, Cmd::PasteCellsLinked]
                .into_iter()
                .for_each(|cmd| {
                    self.command_button(ui, cmd, None);
                });
            ui.separator();
            let span = self.timeline.span.is_some();
            ui.add_enabled_ui(span, |ui| {
                ui.menu_button(format!("{} retime", icon::RETIME), |ui| {
                    [Cmd::OnOnes, Cmd::OnTwos, Cmd::OnThrees].into_iter().for_each(|cmd| {
                        if self.command_button(ui, cmd, Some(cmd.spec().name)).clicked() {
                            ui.close();
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut self.timeline.ask.fit).range(1..=10_000))
                            .hint("the number of frames to fit the selection into");
                        if self.command_button(ui, Cmd::FitFrames, Some("fit")).clicked() {
                            ui.close();
                        }
                    });
                    [Cmd::Reverse, Cmd::Pingpong].into_iter().for_each(|cmd| {
                        if self.command_button(ui, cmd, Some(cmd.spec().name)).clicked() {
                            ui.close();
                        }
                    });
                    self.command_check(ui, Cmd::EveryLayer, "on every layer");
                })
                .response
                .hint("retime the selected frames: on ones, twos, threes, fit, reverse, ping-pong");
            })
            .response
            .on_disabled_hover_text("select frames first: drag over them in the timeline");
            self.command_button(ui, Cmd::Inbetween, Some("in-between"));
            self.command_button(ui, Cmd::RedoInbetweens, None);
            self.command_button(ui, Cmd::FillLike, None);
            self.command_button(ui, Cmd::NewClip, Some("new clip"));
            if let Some(span) = &self.timeline.span {
                ui.label(
                    RichText::new(format!(
                        "frames {} to {} on {} rows",
                        span.from,
                        span.to,
                        span.rows.len()
                    ))
                    .color(theme::WEAK),
                );
            }
        });
        if let Some((cel, end, hold)) = hold_to {
            let layer = self.scene.active_id().unwrap_or_default();
            let open = self
                .scene
                .active_layer()
                .and_then(|l| l.cels().iter().find(|c| c.id == cel.id).cloned())
                .is_some_and(|c| c.hold.is_none());
            self.resize(&layer, &cel.id, (cel.frame, end, open), cel.frame + hold)?;
        }
        Ok(())
    }

    pub(super) fn clip_window(&mut self, ui: &mut egui::Ui) -> Result<()> {
        let Some(clip) = self.current_clip() else {
            ui.label("no clip here: click a clip's bar above the frame numbers, or make one from a selection");
            self.command_button(ui, Cmd::NewClip, Some("new clip from the selection"));
            return Ok(());
        };
        let frames = self.scene.bundle.doc.timeline.frames;
        let (mut from, mut to) = (clip.from, clip.to);
        let mut ops = Vec::new();
        ui.horizontal(|ui| {
            let label = ui.label("frames");
            let a = ui
                .add(egui::DragValue::new(&mut from).range(0..=frames - 1))
                .labelled_by(label.id)
                .hint("the clip's first frame");
            ui.label("to");
            let b = ui
                .add(egui::DragValue::new(&mut to).range(0..=frames - 1))
                .hint("the clip's last frame");
            let done = |r: &egui::Response| r.drag_stopped() || r.lost_focus();
            if (done(&a) || done(&b)) && (from, to) != (clip.from, clip.to) && from <= to {
                ops.push(Op::Clip(ClipEdit::Set {
                    name: clip.name.clone(),
                    rename: None,
                    from: Some(from),
                    to: Some(to),
                    looping: None,
                    fps: None,
                }));
            }
        });
        self.timeline_apply(ops)?;
        self.clip_row(ui)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use klad_core::doc::Layer;

    fn doc() -> Document {
        let mut doc = Document::new(64, 64);
        doc.timeline.frames = 12;
        let mut layer = Layer::paint("l1", "lines", "c1");
        if let Some(cels) = layer.cels_mut() {
            cels[0].hold = Some(2);
            cels.push(Cel::at("c2", 2));
            cels[1].hold = Some(3);
            let mut link = Cel::at("c3", 5);
            link.link = Some("c1".into());
            link.hold = Some(1);
            cels.push(link);
        }
        doc.layers.push(layer);
        doc
    }

    fn shown(doc: &Document, layer: &str) -> Vec<(u32, u32, bool)> {
        let frames = doc.timeline.frames;
        exposures(doc.layer(layer).unwrap(), frames)
            .into_iter()
            .map(|(c, start, end)| (start, end, c.link.is_some()))
            .collect()
    }

    fn held(cels: &[(&str, u32, Option<u32>)], frames: u32) -> Document {
        let mut doc = Document::new(64, 64);
        doc.timeline.frames = frames;
        let mut layer = Layer::paint("l1", "lines", cels[0].0);
        if let Some(list) = layer.cels_mut() {
            list.clear();
            cels.iter().for_each(|(id, frame, hold)| {
                let mut cel = Cel::at(id, *frame);
                cel.hold = *hold;
                list.push(cel);
            });
        }
        doc.layers.push(layer);
        doc
    }

    #[test]
    fn selected_frames_repeat_after_themselves_as_copies_and_what_follows_moves_along() {
        let twos = held(&[("c1", 0, Some(2)), ("c2", 2, Some(2)), ("c3", 4, Some(2))], 6);
        let (after, _) = klad_core::ops::apply_all(&twos, &repeat_ops(&twos, &["l1".into()], 0, 5)).unwrap();
        assert_eq!(
            shown(&after, "l1"),
            [
                (0, 2, false),
                (2, 4, false),
                (4, 6, false),
                (6, 8, false),
                (8, 10, false),
                (10, 12, false)
            ],
            "three drawings on twos become six, none of them a link"
        );
        assert_eq!(after.timeline.frames, 12);

        let open = held(&[("c1", 0, Some(2)), ("c2", 2, None)], 6);
        let (after, _) = klad_core::ops::apply_all(&open, &repeat_ops(&open, &["l1".into()], 0, 5)).unwrap();
        assert_eq!(
            shown(&after, "l1"),
            [(0, 2, false), (2, 6, false), (6, 8, false), (8, 12, false)],
            "a last drawing that runs to the end keeps its length"
        );

        let part = held(&[("c1", 0, Some(4)), ("c2", 4, Some(2))], 6);
        let (after, _) = klad_core::ops::apply_all(&part, &repeat_ops(&part, &["l1".into()], 0, 1)).unwrap();
        assert_eq!(
            shown(&after, "l1"),
            [(0, 2, false), (2, 4, false), (4, 6, true), (6, 8, false)],
            "half a hold repeats as a copy, and the other half still shows the same drawing after it"
        );
        assert_eq!(after.timeline.frames, 8);

        let late = held(&[("c1", 2, Some(2))], 6);
        let (after, _) = klad_core::ops::apply_all(&late, &repeat_ops(&late, &["l1".into()], 0, 5)).unwrap();
        assert_eq!(
            shown(&after, "l1"),
            [(2, 4, false), (8, 10, false)],
            "blank frames in the selection stay blank in the copy"
        );
    }

    #[test]
    fn the_last_frame_is_free_when_only_open_drawings_run_into_it() {
        let open = held(&[("c1", 0, Some(2)), ("c2", 2, None)], 6);
        assert!(free_at_the_end(&open));
        let filled = held(&[("c1", 0, Some(2)), ("c2", 2, Some(4))], 6);
        assert!(
            !free_at_the_end(&filled),
            "a drawing held up to the last frame is content there"
        );
        let starts = held(&[("c1", 0, Some(5)), ("c2", 5, None)], 6);
        assert!(
            !free_at_the_end(&starts),
            "a drawing that starts on the last frame is content there"
        );
        let single = held(&[("c1", 0, None)], 1);
        assert!(!free_at_the_end(&single), "one frame cannot go");
    }

    #[test]
    fn cells_copy_their_drawings_holds_and_links_and_paste_as_copies_or_links() {
        let doc = doc();
        let cells = copy_cells(&doc, &["l1".into()], 1, 5);
        assert_eq!(
            cells,
            [
                CellCopy {
                    layer: "l1".into(),
                    offset: 0,
                    source: "c1".into(),
                    length: 1,
                    link: false
                },
                CellCopy {
                    layer: "l1".into(),
                    offset: 1,
                    source: "c2".into(),
                    length: 3,
                    link: false
                },
                CellCopy {
                    layer: "l1".into(),
                    offset: 4,
                    source: "c1".into(),
                    length: 1,
                    link: true
                },
            ]
        );
        let ops = paste_ops(&doc, &cells, 8, false);
        assert_eq!(ops.len(), 3);
        assert!(
            matches!(&ops[0], Op::Cel(CelSpec { action: CelAction::Dup { cel }, frame: Some(8), hold: Some(1), .. }) if cel == "c1")
        );
        assert!(
            matches!(&ops[2], Op::Cel(CelSpec { action: CelAction::Link { cel }, frame: Some(12), .. }) if cel == "c1")
        );
        let linked = paste_ops(&doc, &cells, 8, true);
        assert!(linked.iter().all(|op| matches!(
            op,
            Op::Cel(CelSpec {
                action: CelAction::Link { .. },
                ..
            })
        )));
        let onto = paste_ops(&doc, &cells, 2, false);
        assert!(matches!(&onto[0], Op::Cel(CelSpec { insert: true, .. })));
        let (pasted, _) = klad_core::ops::apply_all(&doc, &ops).unwrap();
        let cels: Vec<(u32, Option<u32>)> = pasted
            .layer("l1")
            .unwrap()
            .cels()
            .iter()
            .map(|c| (c.frame, c.hold))
            .collect();
        assert!(
            cels.contains(&(8, Some(1))) && cels.contains(&(9, Some(3))) && cels.contains(&(12, Some(1))),
            "{cels:?}"
        );
    }
}
