use super::{Editor, Grab, edge, upright};
use crate::theme::Hint;
use anyhow::Result;
use egui::{Vec2, vec2};
use klad_core::doc::Item;

pub(super) const FIELDS: [(&str, &str); 5] = [
    ("x", "the left edge of the selection's box, in canvas px"),
    ("y", "the top edge of the selection's box, in canvas px"),
    ("w", "the width of the selection's box, in canvas px"),
    ("h", "the height of the selection's box, in canvas px"),
    (
        "angle",
        "the shape's angle in degrees; for strokes, a turn by that many degrees",
    ),
];

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Typed {
    key: String,
    rev: u64,
    text: [String; 5],
    shown: [f32; 5],
}

fn tidy(v: f32) -> String {
    let r = (v * 100.0).round() / 100.0;
    match r.fract() == 0.0 {
        true => format!("{r:.0}"),
        false => format!("{r}"),
    }
}

fn span(corners: &[Vec2; 4]) -> (Vec2, Vec2) {
    corners
        .iter()
        .fold((corners[0], corners[0]), |(lo, hi), c| (lo.min(*c), hi.max(*c)))
}

impl Editor {
    fn typed_now(&self) -> Option<(String, [f32; 5])> {
        let (key, corners) = self.handles()?;
        let (lo, hi) = span(&corners);
        let angle = match (self.on_marks(), self.selection.as_ref()) {
            (false, Some(id)) => match self.scene.bundle.doc.item(id) {
                Some(Item::Node(_, _, node)) => node.transform.rotate,
                _ => 0.0,
            },
            _ => 0.0,
        };
        Some((key, [lo.x, lo.y, hi.x - lo.x, hi.y - lo.y, angle]))
    }

    pub(super) fn typed_menu(&mut self, ui: &mut egui::Ui) -> Result<()> {
        let Some((key, now)) = self.typed_now() else {
            ui.label("select strokes or a shape first");
            return Ok(());
        };
        let rev = self.scene.bundle.doc.rev;
        if self.typed.as_ref().is_none_or(|t| t.key != key || t.rev != rev) {
            self.typed = Some(Typed {
                key,
                rev,
                text: now.map(tidy),
                shown: now,
            });
        }
        let mut done: Option<(usize, f32)> = None;
        egui::Grid::new("typed numbers").num_columns(2).show(ui, |ui| {
            FIELDS.iter().enumerate().for_each(|(i, (label, hint))| {
                let Some(typed) = self.typed.as_mut() else {
                    return;
                };
                let name = ui.label(*label);
                let field = ui
                    .add(egui::TextEdit::singleline(&mut typed.text[i]).desired_width(64.0))
                    .labelled_by(name.id)
                    .hint(*hint);
                let entered = field.lost_focus();
                if entered
                    && let Ok(v) = typed.text[i].trim().parse::<f32>()
                    && (v - typed.shown[i]).abs() > 1e-3
                {
                    done = Some((i, v));
                }
                ui.end_row();
            });
        });
        ui.label(egui::RichText::new("type a number and press Return").color(crate::theme::WEAK));
        if let Some((i, v)) = done {
            self.apply_typed(i, v)?;
            self.typed = None;
        }
        Ok(())
    }

    fn drag_by(&mut self, grab: Grab, start: Vec2, now: Vec2) -> Result<()> {
        let Some((id, corners)) = self.handles() else {
            return Ok(());
        };
        self.start_drag(id, grab, corners, start, false);
        let Some(drag) = self.drag.as_mut() else {
            return Ok(());
        };
        drag.shift = false;
        drag.alt = false;
        drag.copy = false;
        drag.to(now);
        drag.crossed = true;
        self.finish_drag()
    }

    pub(super) fn apply_typed(&mut self, field: usize, value: f32) -> Result<()> {
        let (Some((_, now)), Some((_, corners))) = (self.typed_now(), self.handles()) else {
            return Ok(());
        };
        let center = corners.iter().fold(Vec2::ZERO, |a, c| a + *c) / 4.0;
        match field {
            0 | 1 => {
                let d = value - now[field];
                let by = if field == 0 { vec2(d, 0.0) } else { vec2(0.0, d) };
                self.drag_by(Grab::Move, center, center + by)
            }
            2 | 3 => {
                let old = now[field];
                if old < 1e-3 || value <= 0.0 {
                    return Ok(());
                }
                let k = value / old;
                match upright(&corners) && self.edges() {
                    true => {
                        let side = if field == 2 { 1 } else { 2 };
                        let (start, pivot) = (edge(&corners, side), edge(&corners, (side + 2) % 4));
                        self.drag_by(Grab::Stretch(side), start, pivot + (start - pivot) * k)
                    }
                    false => {
                        let (start, pivot) = (corners[2], corners[0]);
                        self.drag_by(Grab::Scale, start, pivot + (start - pivot) * k)
                    }
                }
            }
            _ => {
                let turn = (value - now[4]).to_radians();
                let start = center + vec2(100.0, 0.0);
                self.drag_by(Grab::Rotate, start, center + vec2(turn.cos(), turn.sin()) * 100.0)
            }
        }
    }
}
