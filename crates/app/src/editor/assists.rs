use super::Editor;
use crate::messages::Tone;
use crate::theme::{self, Hint};
use anyhow::Result;
use egui::{Color32, Pos2, Rect, Stroke, Vec2, vec2};
use klad_core::assist::{Assist, Path};
use klad_core::guides::AssistEdit;
use klad_core::ops::Op;
use klad_core::stroke::{POS_SCALE, Sample};

const SHOWN: Color32 = Color32::from_rgb(120, 200, 140);
const HIT: f32 = 9.0;

#[derive(Clone, Debug)]
pub(super) struct Follow {
    start: [f32; 2],
    assists: Vec<Assist>,
    zoom: f32,
    path: Option<Path>,
    held: Vec<Sample>,
}

fn at(s: &Sample) -> [f32; 2] {
    let (x, y) = s.pos();
    [x, y]
}

fn onto(path: &Path, s: Sample) -> Sample {
    let [x, y] = klad_core::assist::project(path, at(&s));
    Sample {
        x: (x * POS_SCALE).round() as i32,
        y: (y * POS_SCALE).round() as i32,
        ..s
    }
}

impl Follow {
    pub fn new(assists: Vec<Assist>, first: Sample, zoom: f32) -> Self {
        Self {
            start: at(&first),
            assists,
            zoom,
            path: None,
            held: vec![first],
        }
    }

    pub fn take(&mut self, sample: Sample) -> Vec<Sample> {
        match self.path {
            Some(path) => vec![onto(&path, sample)],
            None => {
                self.held.push(sample);
                self.path = klad_core::assist::decide(&self.assists, self.start, at(&sample), self.zoom);
                match self.path {
                    Some(path) => self.held.drain(..).map(|s| onto(&path, s)).collect(),
                    None => Vec::new(),
                }
            }
        }
    }

    pub fn flush(self, tail: Vec<Sample>) -> Vec<Sample> {
        match self.path {
            Some(path) => tail.into_iter().map(|s| onto(&path, s)).collect(),
            None => self.held.into_iter().chain(tail).collect(),
        }
    }
}

impl Editor {
    pub(super) fn following(&self) -> bool {
        let g = &self.scene.bundle.doc.guides;
        g.follow && !g.assists.is_empty() && !self.modifiers.command
    }

    pub(super) fn toggle_follow(&mut self) -> Result<()> {
        let g = &self.scene.bundle.doc.guides;
        if g.assists.is_empty() {
            self.windows.assists = true;
            self.say(Tone::Info, "add a ruler, perspective or an ellipse first");
            return Ok(());
        }
        let on = !g.follow;
        self.assist_ops(
            AssistEdit {
                on: Some(on),
                ..Default::default()
            },
            if on {
                "the brush follows the drawing guides"
            } else {
                "the brush paints freely"
            },
        )
    }

    fn assist_ops(&mut self, edit: AssistEdit, said: &str) -> Result<()> {
        if self.timeline_apply(vec![Op::Assists(edit)])?.is_some() {
            self.say(Tone::Done, said.to_string());
        }
        Ok(())
    }

    pub(super) fn assists_window(&mut self, ui: &mut egui::Ui) -> Result<()> {
        let (w, h) = self.scene.size();
        let (w, h) = (w as f32, h as f32);
        let g = self.scene.bundle.doc.guides.clone();
        let mut add: Option<(Assist, &str)> = None;
        let mut gone: Option<usize> = None;
        let mut follow: Option<bool> = None;
        ui.label(
            egui::RichText::new(
                "Guides the brush follows: drag their points on the canvas while this window is open, or with V.",
            )
            .color(theme::WEAK),
        );
        ui.horizontal_wrapped(|ui| {
            if ui
                .button("ruler")
                .hint("a straight line: strokes near it run along it, others parallel to it")
                .clicked()
            {
                add = Some((
                    Assist::Ruler {
                        from: [w * 0.2, h * 0.5],
                        to: [w * 0.8, h * 0.5],
                    },
                    "added a ruler",
                ));
            }
            if ui
                .button("1-point perspective")
                .hint("one vanishing point; strokes run to it, level or upright")
                .clicked()
            {
                add = Some((
                    Assist::Perspective {
                        points: vec![[w * 0.5, h * 0.4]],
                    },
                    "added one-point perspective",
                ));
            }
            if ui
                .button("2-point perspective")
                .hint("two vanishing points on the horizon; uprights stay upright")
                .clicked()
            {
                add = Some((
                    Assist::Perspective {
                        points: vec![[-w * 0.3, h * 0.4], [w * 1.3, h * 0.4]],
                    },
                    "added two-point perspective",
                ));
            }
            if ui
                .button("3-point perspective")
                .hint("two points on the horizon and one far above or below")
                .clicked()
            {
                add = Some((
                    Assist::Perspective {
                        points: vec![[-w * 0.3, h * 0.4], [w * 1.3, h * 0.4], [w * 0.5, h * 2.5]],
                    },
                    "added three-point perspective",
                ));
            }
            if ui
                .button("ellipse")
                .hint("strokes go round it, or round an ellipse of its shape through where they start")
                .clicked()
            {
                add = Some((
                    Assist::Ellipse {
                        center: [w * 0.5, h * 0.5],
                        radii: [w * 0.25, h * 0.15],
                        turn: 0.0,
                    },
                    "added an ellipse",
                ));
            }
        });
        g.assists.iter().enumerate().for_each(|(i, a)| {
            ui.horizontal(|ui| {
                ui.label(a.word());
                if ui
                    .add(egui::Button::new(crate::theme::icon::TRASH).min_size(vec2(theme::CONTROL, theme::CONTROL)))
                    .hint("take this guide away")
                    .clicked()
                {
                    gone = Some(i);
                }
            });
        });
        let mut on = g.follow;
        if ui
            .checkbox(&mut on, "the brush follows them")
            .hint("hold cmd while you paint to draw freely")
            .changed()
        {
            follow = Some(on);
        }
        let mirrored = self.mirror.is_some();
        if ui
            .selectable_label(mirrored, format!("{} mirror painting", crate::theme::icon::FLIP_H))
            .hint("the symmetry guide: what you paint is painted again mirrored across an upright line")
            .clicked()
        {
            self.queued.push(crate::commands::Cmd::Symmetry);
        }
        if let Some((assist, said)) = add {
            self.assist_ops(
                AssistEdit {
                    add: vec![assist],
                    on: Some(true),
                    ..Default::default()
                },
                said,
            )?;
        }
        if let Some(i) = gone {
            self.assist_ops(
                AssistEdit {
                    remove: vec![i],
                    ..Default::default()
                },
                "took the guide away",
            )?;
        }
        if let Some(on) = follow {
            self.assist_ops(
                AssistEdit {
                    on: Some(on),
                    ..Default::default()
                },
                if on {
                    "the brush follows the guides"
                } else {
                    "the brush paints freely"
                },
            )?;
        }
        Ok(())
    }

    pub(super) fn assist_pen(
        &mut self,
        e: &crate::pen::PenEvent,
        view: crate::view::View,
        rect: Rect,
        c: Vec2,
    ) -> Result<bool> {
        use crate::pen::Phase;
        if !self.guide_points_held() {
            return Ok(false);
        }
        let pos = e.pos;
        let to_screen = |s: &Self, p: Vec2| s.screen(view, rect, p);
        match e.phase {
            Phase::Down => {
                let found = self
                    .scene
                    .bundle
                    .doc
                    .guides
                    .assists
                    .iter()
                    .enumerate()
                    .flat_map(|(i, a)| a.handles().into_iter().enumerate().map(move |(k, p)| (i, k, p)))
                    .map(|(i, k, p)| (i, k, to_screen(self, vec2(p[0], p[1])).distance(pos)))
                    .filter(|(_, _, d)| *d <= HIT)
                    .min_by(|a, b| a.2.total_cmp(&b.2));
                match found {
                    Some((i, k, _)) => {
                        self.assist_drag = Some((i, k, [c.x, c.y]));
                        Ok(true)
                    }
                    None => Ok(false),
                }
            }
            Phase::Move => match self.assist_drag.as_mut() {
                Some(d) => {
                    d.2 = [c.x, c.y];
                    Ok(true)
                }
                None => Ok(false),
            },
            Phase::Up => match self.assist_drag.take() {
                Some((i, k, to)) => {
                    let Some(assist) = self.scene.bundle.doc.guides.assists.get(i).map(|a| a.moved(k, to)) else {
                        return Ok(true);
                    };
                    self.timeline_apply(vec![Op::Assists(AssistEdit {
                        set: Some((i, assist)),
                        ..Default::default()
                    })])?;
                    Ok(true)
                }
                None => Ok(false),
            },
        }
    }

    fn guide_points_held(&self) -> bool {
        self.windows.assists || (self.tool == super::Tool::Select && self.scene.bundle.doc.guides.follow)
    }

    pub(super) fn draw_assists(&self, painter: &egui::Painter, rect: Rect, to_screen: impl Fn(Vec2) -> Pos2) {
        let g = &self.scene.bundle.doc.guides;
        if g.assists.is_empty() || (!g.follow && !self.windows.assists) {
            return;
        }
        let colour = if g.follow { SHOWN } else { SHOWN.gamma_multiply(0.5) };
        let line = Stroke::new(1.0, colour.gamma_multiply(0.7));
        let p = |q: [f32; 2]| to_screen(vec2(q[0], q[1]));
        g.assists.iter().enumerate().for_each(|(i, a)| {
            let shown = match (self.assist_drag, a) {
                (Some((di, k, to)), _) if di == i => a.moved(k, to),
                _ => a.clone(),
            };
            match &shown {
                Assist::Ruler { from, to } => {
                    painter.line_segment([p(*from), p(*to)], Stroke::new(1.5, colour));
                }
                Assist::Perspective { points } => {
                    points.iter().for_each(|v| {
                        let at = p(*v);
                        (0..24).for_each(|k| {
                            let t = std::f32::consts::TAU * k as f32 / 24.0;
                            let far = at + vec2(t.cos(), t.sin()) * (rect.width() + rect.height()) * 2.0;
                            painter.line_segment([at, far], line);
                        });
                    });
                    if points.len() >= 2 {
                        painter.line_segment([p(points[0]), p(points[1])], Stroke::new(1.5, colour));
                    }
                }
                Assist::Ellipse { center, radii, turn } => {
                    let (s, c) = turn.to_radians().sin_cos();
                    let ring: Vec<Pos2> = (0..=64)
                        .map(|k| {
                            let t = std::f32::consts::TAU * k as f32 / 64.0;
                            let (x, y) = (radii[0] * t.cos(), radii[1] * t.sin());
                            p([center[0] + x * c - y * s, center[1] + x * s + y * c])
                        })
                        .collect();
                    painter.add(egui::Shape::line(ring, Stroke::new(1.5, colour)));
                }
            }
            if self.guide_points_held() {
                shown.handles().iter().for_each(|h| {
                    painter.circle_filled(p(*h), 5.0, Color32::WHITE);
                    painter.circle_stroke(p(*h), 5.0, Stroke::new(1.5, colour));
                });
            }
        });
    }
}
