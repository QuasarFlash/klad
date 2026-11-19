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
pub(super) struct Crop {
    pub from: Vec2,
    pub to: Vec2,
    pub dragging: bool,
}

impl Crop {
    fn area(&self, pixel: bool) -> (f32, f32, u32, u32) {
        let (lo, hi) = (self.from.min(self.to), self.from.max(self.to));
        let (lo, hi) = match pixel {
            true => (lo.round(), hi.round()),
            false => (lo.floor(), hi.ceil()),
        };
        (lo.x, lo.y, (hi.x - lo.x).max(0.0) as u32, (hi.y - lo.y).max(0.0) as u32)
    }
}

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
                let crop = Crop {
                    from: lo,
                    to: hi,
                    dragging: false,
                };
                let (x, y, width, height) = crop.area(self.scene.on_grid());
                self.reshape(Reshape::Crop { x, y, width, height }, "cropped to the selection")
            }
            Cmd::Trim => match klad_render::reshape::drawn(&self.scene.bundle)? {
                Some([x0, y0, x1, y1]) => {
                    let crop = Crop {
                        from: vec2(x0, y0),
                        to: vec2(x1, y1),
                        dragging: false,
                    };
                    let (x, y, width, height) = crop.area(self.scene.on_grid());
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
                            from: Vec2::ZERO,
                            to: vec2(w as f32, h as f32),
                            dragging: false,
                        })
                    }
                };
                if self.cropping.is_some() {
                    self.say(
                        Tone::Info,
                        "drag the area to keep, then press Return; Esc leaves the crop",
                    );
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
        let (w, h) = self.scene.size();
        let at = vec2(at.x.clamp(0.0, w as f32), at.y.clamp(0.0, h as f32));
        let Some(crop) = self.cropping.as_mut() else {
            return;
        };
        match phase {
            Phase::Down if on_canvas => {
                crop.from = at;
                crop.to = at;
                crop.dragging = true;
            }
            Phase::Move if crop.dragging => crop.to = at,
            Phase::Up if crop.dragging => {
                crop.to = at;
                crop.dragging = false;
            }
            _ => {}
        }
    }

    pub(super) fn crop_keys(&mut self, ui: &mut egui::Ui) -> Result<()> {
        let Some(crop) = self.cropping else {
            return Ok(());
        };
        let (enter, escape) = ui.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        if escape {
            self.cropping = None;
            self.say(Tone::Info, "left the crop; nothing changed");
        }
        if enter {
            self.cropping = None;
            let (x, y, width, height) = crop.area(self.scene.on_grid());
            if width == 0 || height == 0 {
                self.say(
                    Tone::Refused,
                    "drag an area on the canvas first: the crop keeps what is inside it",
                );
                return Ok(());
            }
            self.reshape(Reshape::Crop { x, y, width, height }, "cropped")?;
        }
        Ok(())
    }

    pub(super) fn crop_overlay(&self, painter: &egui::Painter, to_screen: impl Fn(Vec2) -> Pos2) {
        let Some(crop) = self.cropping else {
            return;
        };
        let (w, h) = self.scene.size();
        let (lo, hi) = (crop.from.min(crop.to), crop.from.max(crop.to));
        let canvas = Rect::from_two_pos(to_screen(Vec2::ZERO), to_screen(vec2(w as f32, h as f32)));
        let keep = Rect::from_two_pos(to_screen(lo), to_screen(hi));
        let shade = Color32::from_black_alpha(140);
        [
            Rect::from_min_max(canvas.min, egui::pos2(canvas.max.x, keep.min.y)),
            Rect::from_min_max(egui::pos2(canvas.min.x, keep.max.y), canvas.max),
            Rect::from_min_max(egui::pos2(canvas.min.x, keep.min.y), egui::pos2(keep.min.x, keep.max.y)),
            Rect::from_min_max(egui::pos2(keep.max.x, keep.min.y), egui::pos2(canvas.max.x, keep.max.y)),
        ]
        .into_iter()
        .filter(|r| r.is_positive())
        .for_each(|r| {
            painter.rect_filled(r, 0.0, shade);
        });
        painter.rect_stroke(
            keep,
            0.0,
            egui::Stroke::new(1.5, theme::ACCENT),
            egui::StrokeKind::Middle,
        );
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
