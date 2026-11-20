use super::Editor;
use crate::commands::Cmd;
use crate::messages::Tone;
use crate::pen::Phase;
use crate::theme::{self, Hint};
use anyhow::Result;
use egui::{Color32, Key, Modifiers, Pos2, Rect, RichText, Vec2, vec2};
use klad_core::ops::Op;
use klad_render::reshape::{ANCHORS, Reshape};

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Form {
    pub width: u32,
    pub height: u32,
    pub anchor: [f32; 2],
    pub scale: f32,
    pub seen: Option<(u32, u32)>,
}

impl Default for Form {
    fn default() -> Self {
        Self {
            width: 0,
            height: 0,
            anchor: [0.5, 0.5],
            scale: 1.0,
            seen: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Grip {
    New,
    Move,
    Sides(i8, i8),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Crop {
    pub from: Vec2,
    pub to: Vec2,
    pub whole: bool,
    pub held: Option<(Grip, Vec2, Vec2, Vec2)>,
}

const GRIP: f32 = 8.0;

impl Crop {
    pub(super) fn new(lo: Vec2, hi: Vec2) -> Self {
        Self {
            from: lo,
            to: hi,
            whole: false,
            held: None,
        }
    }

    fn span(&self) -> (Vec2, Vec2) {
        (self.from.min(self.to), self.from.max(self.to))
    }

    fn handles(&self) -> [(Grip, Vec2); 8] {
        let (lo, hi) = self.span();
        let mid = (lo + hi) / 2.0;
        [
            (Grip::Sides(-1, -1), lo),
            (Grip::Sides(0, -1), vec2(mid.x, lo.y)),
            (Grip::Sides(1, -1), vec2(hi.x, lo.y)),
            (Grip::Sides(1, 0), vec2(hi.x, mid.y)),
            (Grip::Sides(1, 1), hi),
            (Grip::Sides(0, 1), vec2(mid.x, hi.y)),
            (Grip::Sides(-1, 1), vec2(lo.x, hi.y)),
            (Grip::Sides(-1, 0), vec2(lo.x, mid.y)),
        ]
    }

    fn grip_at(&self, at: Vec2, slack: f32) -> Grip {
        let nearest = self
            .handles()
            .into_iter()
            .filter(|(_, p)| (*p - at).length() <= slack)
            .min_by(|a, b| (a.1 - at).length().total_cmp(&(b.1 - at).length()));
        let (lo, hi) = self.span();
        let inside = at.x > lo.x && at.x < hi.x && at.y > lo.y && at.y < hi.y;
        match (nearest, inside && !self.whole) {
            (Some((grip, _)), _) => grip,
            (None, true) => Grip::Move,
            (None, false) => Grip::New,
        }
    }

    fn dragged(&mut self, at: Vec2, keep: bool, centred: bool) {
        let Some((grip, start, lo, hi)) = self.held else {
            return;
        };
        let (a, b) = match grip {
            Grip::New => {
                let mut half = at - start;
                if keep {
                    let side = half.x.abs().max(half.y.abs());
                    half = vec2(side * half.x.signum(), side * half.y.signum());
                }
                match centred {
                    true => (start - half, start + half),
                    false => (start, start + half),
                }
            }
            Grip::Move => (lo + (at - start), hi + (at - start)),
            Grip::Sides(sx, sy) => {
                let pick = |s: i8, low: f32, high: f32| if s < 0 { low } else { high };
                let anchor = match centred {
                    true => (lo + hi) / 2.0,
                    false => vec2(pick(-sx, lo.x, hi.x), pick(-sy, lo.y, hi.y)),
                };
                let mut half = vec2(pick(sx, lo.x, hi.x), pick(sy, lo.y, hi.y)) + (at - start) - anchor;
                let (w, h) = (hi.x - lo.x, hi.y - lo.y);
                if keep && sx != 0 && sy != 0 && w > 0.0 && h > 0.0 {
                    let ratio = w / h;
                    match half.x.abs() > half.y.abs() * ratio {
                        true => half.y = half.y.signum() * half.x.abs() / ratio,
                        false => half.x = half.x.signum() * half.y.abs() * ratio,
                    }
                }
                let side = |s: i8, low: f32, high: f32, a: f32, h: f32| match (s, centred) {
                    (0, _) => (low, high),
                    (_, true) => (a - h, a + h),
                    (_, false) => (a, a + h),
                };
                let (x0, x1) = side(sx, lo.x, hi.x, anchor.x, half.x);
                let (y0, y1) = side(sy, lo.y, hi.y, anchor.y, half.y);
                (vec2(x0, y0), vec2(x1, y1))
            }
        };
        self.from = a.min(b);
        self.to = a.max(b);
    }

    fn area(&self, pixel: bool) -> (f32, f32, u32, u32) {
        let (lo, hi) = (self.from.min(self.to), self.from.max(self.to));
        let (lo, hi) = match pixel {
            true => (lo.round(), hi.round()),
            false => (lo.floor(), hi.ceil()),
        };
        (lo.x, lo.y, (hi.x - lo.x).max(0.0) as u32, (hi.y - lo.y).max(0.0) as u32)
    }
}

const CROP_HINT: &str = "drag the handles, the box or a new box over the area to keep, then Return; past the edges it grows the canvas; shift keeps the shape, alt sizes from the middle, Esc leaves";

pub(super) const COMMANDS: [Cmd; 5] = [
    Cmd::CanvasRight,
    Cmd::CanvasLeft,
    Cmd::Canvas180,
    Cmd::CanvasFlipLevel,
    Cmd::CanvasFlipUpright,
];

impl Editor {
    pub(super) fn reshape(&mut self, r: Reshape, done: &str) -> Result<()> {
        let ops = match klad_render::reshape::ops(&self.scene.bundle, &r) {
            Ok(ops) => ops,
            Err(e) => {
                self.say(Tone::Refused, format!("{e:#}"));
                return Ok(());
            }
        };
        if self.timeline_apply(ops)?.is_none() {
            return Ok(());
        }
        self.picked = None;
        self.selection = None;
        self.group.clear();
        self.fit_buffers();
        let (w, h) = self.scene.size();
        self.say(
            Tone::Done,
            format!("{done}: the canvas is {w} by {h} px (cmd Z takes it back)"),
        );
        Ok(())
    }

    pub(super) fn fit_buffers(&mut self) {
        let (w, h) = self.scene.size();
        if (self.buffer.grid().width, self.buffer.grid().height) != (w, h) {
            self.buffer = klad_brush::StrokeBuffer::new(w, h);
            self.mirror_buffer = klad_brush::StrokeBuffer::new(w, h);
            self.view = None;
        }
    }

    pub(super) fn run_canvas(&mut self, cmd: Cmd) -> Result<()> {
        match cmd {
            Cmd::CanvasRight => self.reshape(Reshape::Turn(90), "turned the drawing right"),
            Cmd::CanvasLeft => self.reshape(Reshape::Turn(-90), "turned the drawing left"),
            Cmd::Canvas180 => self.reshape(Reshape::Turn(180), "turned the drawing round"),
            Cmd::CanvasFlipLevel => self.reshape(Reshape::Flip { level: true }, "flipped the drawing left to right"),
            Cmd::CanvasFlipUpright => self.reshape(Reshape::Flip { level: false }, "flipped the drawing upside down"),
            Cmd::CropToSelection => {
                let Some((_, corners)) = self.handles() else {
                    return Ok(());
                };
                let (lo, hi) = corners
                    .iter()
                    .fold((corners[0], corners[0]), |(lo, hi), c| (lo.min(*c), hi.max(*c)));
                let (x, y, width, height) = Crop::new(lo, hi).area(self.scene.on_grid());
                self.reshape(Reshape::Crop { x, y, width, height }, "cropped to the selection")
            }
            Cmd::Trim => match klad_render::reshape::drawn(&self.scene.bundle)? {
                Some([x0, y0, x1, y1]) => {
                    let (x, y, width, height) = Crop::new(vec2(x0, y0), vec2(x1, y1)).area(self.scene.on_grid());
                    self.reshape(Reshape::Crop { x, y, width, height }, "trimmed to what is drawn")
                }
                None => {
                    self.say(Tone::Refused, "nothing is drawn yet: there is nothing to trim to");
                    Ok(())
                }
            },
            Cmd::CropTool => {
                self.cropping = match self.cropping {
                    Some(_) => None,
                    None => {
                        let (w, h) = self.scene.size();
                        Some(Crop {
                            whole: true,
                            ..Crop::new(Vec2::ZERO, vec2(w as f32, h as f32))
                        })
                    }
                };
                if self.cropping.is_some() {
                    self.say(Tone::Info, CROP_HINT);
                }
                Ok(())
            }
            Cmd::CanvasWindow => {
                self.windows.canvas = !self.windows.canvas;
                Ok(())
            }
            _ => Ok(()),
        }
    }

    pub(super) fn crop_pen(&mut self, phase: Phase, at: Vec2, on_canvas: bool) {
        let slack = GRIP / self.view.map_or(1.0, |v| v.zoom);
        let (keep, centred) = (self.modifiers.shift, self.modifiers.alt);
        let Some(crop) = self.cropping.as_mut() else {
            return;
        };
        match phase {
            Phase::Down if on_canvas => {
                let grip = crop.grip_at(at, slack);
                let (lo, hi) = crop.span();
                crop.held = Some((grip, at, lo, hi));
            }
            Phase::Move if crop.held.is_some() => {
                crop.dragged(at, keep, centred);
                crop.whole = false;
            }
            Phase::Up => {
                let Some((grip, _, lo, hi)) = crop.held else {
                    return;
                };
                crop.dragged(at, keep, centred);
                crop.held = None;
                let (a, b) = crop.span();
                if grip == Grip::New && ((b.x - a.x) < 1.0 || (b.y - a.y) < 1.0) {
                    crop.from = lo;
                    crop.to = hi;
                } else {
                    crop.whole = false;
                }
            }
            _ => {}
        }
    }

    pub(super) fn crop_keys(&mut self, ui: &mut egui::Ui) -> Result<()> {
        if self.cropping.is_none() {
            return Ok(());
        }
        let (enter, escape) = ui.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        if escape {
            self.leave_crop();
        }
        if enter {
            self.apply_crop()?;
        }
        Ok(())
    }

    fn leave_crop(&mut self) {
        self.cropping = None;
        self.say(Tone::Info, "left the crop; nothing changed");
    }

    fn apply_crop(&mut self) -> Result<()> {
        let Some(crop) = self.cropping.take() else {
            return Ok(());
        };
        let (x, y, width, height) = crop.area(self.scene.on_grid());
        if width == 0 || height == 0 {
            self.say(
                Tone::Refused,
                "drag an area on the canvas first: the crop keeps what is inside it",
            );
            return Ok(());
        }
        self.reshape(Reshape::Crop { x, y, width, height }, "cropped")
    }

    pub(super) fn crop_bar(&mut self, ui: &mut egui::Ui) -> Result<()> {
        let Some(crop) = self.cropping else {
            return Ok(());
        };
        let pixel = self.scene.on_grid();
        let (w, h) = self.scene.size();
        let (_, _, mut width, mut height) = crop.area(pixel);
        ui.label(RichText::new(format!("{} crop", Cmd::CropTool.spec().icon)).strong());
        ui.separator();
        let mut sized = false;
        [("w", &mut width), ("h", &mut height)]
            .into_iter()
            .for_each(|(name, value)| {
                let label = ui.label(name);
                sized |= ui
                    .add(
                        egui::DragValue::new(value)
                            .range(1..=klad_render::reshape::MOST)
                            .suffix(" px"),
                    )
                    .labelled_by(label.id)
                    .hint("the size of the area to keep, in canvas px; it grows to the right and down, past the canvas too")
                    .changed();
            });
        if sized && let Some(crop) = self.cropping.as_mut() {
            let (lo, _) = crop.span();
            crop.to = lo + vec2(width as f32, height as f32);
            crop.from = lo;
            crop.whole = false;
        }
        if ui
            .button("crop to the box")
            .hint("keep the area inside the box (Return); cmd Z takes it back")
            .clicked()
        {
            return self.apply_crop();
        }
        if ui
            .button("whole canvas")
            .hint("the box around the whole canvas again")
            .clicked()
        {
            self.cropping = Some(Crop {
                whole: true,
                ..Crop::new(Vec2::ZERO, vec2(w as f32, h as f32))
            });
        }
        if ui
            .button("cancel")
            .hint("leave the crop; nothing changes (Esc)")
            .clicked()
        {
            self.leave_crop();
        }
        ui.label(RichText::new(CROP_HINT).color(theme::WEAK));
        Ok(())
    }

    pub(super) fn crop_overlay(&self, painter: &egui::Painter, to_screen: impl Fn(Vec2) -> Pos2) {
        let Some(crop) = self.cropping else {
            return;
        };
        let (w, h) = self.scene.size();
        let (lo, hi) = crop.span();
        let canvas = Rect::from_two_pos(to_screen(Vec2::ZERO), to_screen(vec2(w as f32, h as f32)));
        let keep = Rect::from_two_pos(to_screen(lo), to_screen(hi));
        let shade = Color32::from_black_alpha(140);
        let rows = (keep.min.y.max(canvas.min.y), keep.max.y.min(canvas.max.y));
        [
            Rect::from_min_max(canvas.min, egui::pos2(canvas.max.x, keep.min.y)),
            Rect::from_min_max(egui::pos2(canvas.min.x, keep.max.y), canvas.max),
            Rect::from_min_max(egui::pos2(canvas.min.x, rows.0), egui::pos2(keep.min.x, rows.1)),
            Rect::from_min_max(egui::pos2(keep.max.x, rows.0), egui::pos2(canvas.max.x, rows.1)),
        ]
        .into_iter()
        .filter(|r| r.is_positive())
        .for_each(|r| {
            painter.rect_filled(r, 0.0, shade);
        });
        let grown = Color32::from_white_alpha(36);
        [
            Rect::from_min_max(keep.min, egui::pos2(keep.max.x, keep.max.y.min(canvas.min.y))),
            Rect::from_min_max(egui::pos2(keep.min.x, keep.min.y.max(canvas.max.y)), keep.max),
            Rect::from_min_max(
                egui::pos2(keep.min.x, rows.0),
                egui::pos2(keep.max.x.min(canvas.min.x), rows.1),
            ),
            Rect::from_min_max(
                egui::pos2(keep.min.x.max(canvas.max.x), rows.0),
                egui::pos2(keep.max.x, rows.1),
            ),
        ]
        .into_iter()
        .filter(|r| r.is_positive())
        .for_each(|r| {
            painter.rect_filled(r, 0.0, grown);
        });
        painter.rect_stroke(
            keep,
            0.0,
            egui::Stroke::new(1.5, theme::ACCENT),
            egui::StrokeKind::Middle,
        );
        if crop.held.is_some() {
            [1.0 / 3.0, 2.0 / 3.0].into_iter().for_each(|t| {
                let x = keep.min.x + keep.width() * t;
                let y = keep.min.y + keep.height() * t;
                let thin = egui::Stroke::new(1.0, Color32::from_white_alpha(110));
                painter.line_segment([egui::pos2(x, keep.min.y), egui::pos2(x, keep.max.y)], thin);
                painter.line_segment([egui::pos2(keep.min.x, y), egui::pos2(keep.max.x, y)], thin);
            });
        }
        let handles = crop.handles().map(|(grip, at)| (grip, to_screen(at)));
        handles.iter().for_each(|(_, at)| {
            let r = Rect::from_center_size(*at, Vec2::splat(GRIP));
            painter.rect_filled(r, 1.0, Color32::WHITE);
            painter.rect_stroke(r, 1.0, egui::Stroke::new(1.5, theme::ACCENT), egui::StrokeKind::Middle);
        });
        let ctx = painter.ctx();
        let Some(hover) = ctx
            .pointer_hover_pos()
            .filter(|p| painter.clip_rect().contains(*p))
            .filter(|p| ctx.layer_id_at(*p).is_none_or(|l| l.order == egui::Order::Background))
        else {
            return;
        };
        let grip = match crop.held {
            Some((grip, ..)) => grip,
            None => handles
                .iter()
                .filter(|(_, at)| at.distance(hover) <= GRIP)
                .min_by(|a, b| a.1.distance(hover).total_cmp(&b.1.distance(hover)))
                .map_or_else(
                    || match keep.contains(hover) && !crop.whole {
                        true => Grip::Move,
                        false => Grip::New,
                    },
                    |(grip, _)| *grip,
                ),
        };
        ctx.set_cursor_icon(match grip {
            Grip::New => egui::CursorIcon::Crosshair,
            Grip::Move if crop.held.is_some() => egui::CursorIcon::Grabbing,
            Grip::Move => egui::CursorIcon::Grab,
            Grip::Sides(0, _) => egui::CursorIcon::ResizeVertical,
            Grip::Sides(_, 0) => egui::CursorIcon::ResizeHorizontal,
            Grip::Sides(x, y) if x == y => egui::CursorIcon::ResizeNwSe,
            Grip::Sides(..) => egui::CursorIcon::ResizeNeSw,
        });
    }

    pub(super) fn canvas_window(&mut self, ui: &mut egui::Ui) -> Result<()> {
        let (w, h) = self.scene.size();
        if self.canvas_form.seen != Some((w, h)) {
            self.canvas_form = Form {
                width: w,
                height: h,
                seen: Some((w, h)),
                ..self.canvas_form.clone()
            };
        }
        let mut act: Option<(Reshape, &str)> = None;
        let mut paper: Option<Option<String>> = None;
        let pixel = self.scene.on_grid();
        let current = crate::color::hex(self.color);
        {
            {
                ui.label(RichText::new(format!("{w} by {h} px")).color(theme::WEAK));
                egui::Grid::new("canvas size")
                    .num_columns(2)
                    .spacing(vec2(12.0, 8.0))
                    .show(ui, |ui| {
                        ui.label("size");
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::DragValue::new(&mut self.canvas_form.width)
                                    .range(1..=16384)
                                    .suffix(" px"),
                            )
                            .hint("the new width; the drawing keeps its place by the anchor");
                            ui.label("by");
                            ui.add(
                                egui::DragValue::new(&mut self.canvas_form.height)
                                    .range(1..=16384)
                                    .suffix(" px"),
                            )
                            .hint("the new height");
                        });
                        ui.end_row();
                        ui.label("anchor");
                        egui::Grid::new("anchor").spacing(vec2(2.0, 2.0)).show(ui, |ui| {
                            ANCHORS.iter().enumerate().for_each(|(i, (name, a))| {
                                let on = self.canvas_form.anchor == *a;
                                if ui
                                    .selectable_label(on, if on { "\u{E3EE}" } else { "\u{E18A}" })
                                    .hint(format!(
                                        "keep the drawing at the {name} while the canvas grows or shrinks"
                                    ))
                                    .clicked()
                                {
                                    self.canvas_form.anchor = *a;
                                }
                                if i % 3 == 2 {
                                    ui.end_row();
                                }
                            });
                        });
                        ui.end_row();
                        ui.label("");
                        if ui
                            .button("change the canvas size")
                            .hint("grow or cut the canvas around the drawing; nothing is scaled")
                            .clicked()
                        {
                            act = Some((
                                Reshape::Size {
                                    width: self.canvas_form.width,
                                    height: self.canvas_form.height,
                                    anchor: self.canvas_form.anchor,
                                },
                                "changed the canvas size",
                            ));
                        }
                        ui.end_row();
                        ui.label("scale");
                        ui.horizontal(|ui| {
                            match pixel {
                                true => ui
                                    .add(
                                        egui::DragValue::new(&mut self.canvas_form.scale)
                                            .range(1.0..=16.0)
                                            .speed(0.05)
                                            .fixed_decimals(0)
                                            .prefix("× "),
                                    )
                                    .hint("a pixel drawing scales by whole numbers, so its pixels stay sharp squares"),
                                false => ui
                                    .add(
                                        egui::DragValue::new(&mut self.canvas_form.scale)
                                            .range(0.05..=8.0)
                                            .speed(0.01)
                                            .prefix("× "),
                                    )
                                    .hint("scale the whole drawing: strokes and shapes stay sharp"),
                            };
                            let k = self.canvas_form.scale;
                            ui.label(
                                RichText::new(format!("{} by {} px", (w as f32 * k).round(), (h as f32 * k).round()))
                                    .color(theme::WEAK),
                            );
                        });
                        ui.end_row();
                        ui.label("");
                        if ui
                            .button("scale the drawing")
                            .hint("the canvas and everything on it")
                            .clicked()
                        {
                            let k = match pixel {
                                true => self.canvas_form.scale.round(),
                                false => self.canvas_form.scale,
                            };
                            act = Some((Reshape::Scale { sx: k, sy: k }, "scaled the drawing"));
                        }
                        ui.end_row();
                        ui.label("turn");
                        ui.horizontal(|ui| {
                            super::canvas::COMMANDS.iter().for_each(|cmd| {
                                self.command_button(ui, *cmd, None);
                            });
                        });
                        ui.end_row();
                        ui.label("paper");
                        ui.horizontal(|ui| {
                            let shown = self.scene.bundle.doc.canvas.background.clone();
                            ui.label(shown.clone().unwrap_or_else(|| "none: see-through".to_string()));
                            if ui
                                .button(format!("use {current}"))
                                .hint("the paper takes the colour you paint with")
                                .clicked()
                            {
                                paper = Some(Some(current.clone()));
                            }
                            if shown.is_some() && ui.button("none").hint("a see-through paper").clicked() {
                                paper = Some(None);
                            }
                        });
                        ui.end_row();
                    });
            }
        }
        if let Some((r, done)) = act {
            self.reshape(r, done)?;
        }
        if let Some(paper) = paper {
            let said = match &paper {
                Some(hex) => format!("the paper is {hex} now"),
                None => "the paper is see-through now".to_string(),
            };
            let op = Op::Canvas {
                wrap: None,
                tile: None,
                size: None,
                background: Some(paper),
            };
            if self.timeline_apply(vec![op])?.is_some() {
                self.say(Tone::Done, said);
            }
        }
        Ok(())
    }
}
