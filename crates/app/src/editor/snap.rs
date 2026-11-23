use super::Editor;
use crate::messages::Tone;
use crate::theme::{self, Hint};
use anyhow::Result;
use egui::{Color32, Pos2, Rect, Stroke, Vec2, pos2, vec2};
use klad_core::guides::{Grid, GuideEdit, nearest_line};
use klad_core::ops::Op;

pub(super) const RULER: f32 = 18.0;
const REACH: f32 = 6.0;
const SNAPPED: Color32 = Color32::from_rgb(232, 70, 160);

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct GuideDrag {
    pub across: bool,
    pub at: f32,
    pub from: Option<f32>,
    pub over_ruler: bool,
}

#[derive(Clone, Debug, Default)]
pub(super) struct Targets {
    xs: Vec<f32>,
    ys: Vec<f32>,
    points: Vec<Vec2>,
    grid: Option<Grid>,
}

fn closest(value: f32, list: &[f32], reach: f32) -> Option<f32> {
    list.iter()
        .copied()
        .filter(|v| (v - value).abs() <= reach)
        .min_by(|a, b| (a - value).abs().total_cmp(&(b - value).abs()))
}

fn iso_point(c: Vec2, grid: &Grid) -> Vec2 {
    let (hx, hy) = (grid.every[0] / 2.0, grid.every[1] / 2.0);
    let (u, v) = ((c.x - grid.offset[0]) / hx, (c.y - grid.offset[1]) / hy);
    let candidates = [
        (u.floor(), v.floor()),
        (u.ceil(), v.floor()),
        (u.floor(), v.ceil()),
        (u.ceil(), v.ceil()),
    ];
    candidates
        .iter()
        .filter(|(i, j)| (*i as i64 + *j as i64).rem_euclid(2) == 0)
        .map(|(i, j)| vec2(grid.offset[0] + i * hx, grid.offset[1] + j * hy))
        .min_by(|a, b| (*a - c).length().total_cmp(&(*b - c).length()))
        .unwrap_or(c)
}

impl Targets {
    fn grid_x(&self, v: f32) -> Option<f32> {
        self.grid
            .as_ref()
            .filter(|g| !g.iso)
            .map(|g| nearest_line(v, g.offset[0], g.every[0]))
    }

    fn grid_y(&self, v: f32) -> Option<f32> {
        self.grid
            .as_ref()
            .filter(|g| !g.iso)
            .map(|g| nearest_line(v, g.offset[1], g.every[1]))
    }

    fn x(&self, v: f32, reach: f32) -> Option<f32> {
        let mut list = self.xs.clone();
        list.extend(self.grid_x(v));
        closest(v, &list, reach)
    }

    fn y(&self, v: f32, reach: f32) -> Option<f32> {
        let mut list = self.ys.clone();
        list.extend(self.grid_y(v));
        closest(v, &list, reach)
    }
}

impl Editor {
    pub(super) fn snapping(&self) -> bool {
        self.scene.bundle.doc.guides.snap && !self.modifiers.command
    }

    fn reach(&self) -> f32 {
        REACH / self.view.map_or(1.0, |v| v.zoom).max(1e-3)
    }

    pub(super) fn snap_targets(&self, own: &[String]) -> Targets {
        let doc = &self.scene.bundle.doc;
        let (w, h) = self.scene.size();
        let (w, h) = (w as f32, h as f32);
        let g = &doc.guides;
        let mut t = Targets {
            xs: vec![0.0, w / 2.0, w],
            ys: vec![0.0, h / 2.0, h],
            points: vec![
                vec2(0.0, 0.0),
                vec2(w, 0.0),
                vec2(w, h),
                vec2(0.0, h),
                vec2(w / 2.0, h / 2.0),
            ],
            grid: g.grid.clone().filter(|_| g.show_grid),
        };
        if !g.hidden {
            t.xs.extend(g.x.iter().copied());
            t.ys.extend(g.y.iter().copied());
        }
        let frame = self.scene.frame;
        let shapes: Vec<String> = self
            .scene
            .active_layer()
            .and_then(|l| l.content_at(frame))
            .map(|c| c.nodes.iter().map(|n| n.id.clone()).collect())
            .unwrap_or_default();
        shapes
            .iter()
            .filter(|id| !own.contains(id))
            .filter_map(|id| klad_render::overlay::node_world_box(doc, id).ok().flatten())
            .for_each(|[x, y, bw, bh]| {
                let (cx, cy) = (x + bw / 2.0, y + bh / 2.0);
                t.xs.extend([x, cx, x + bw]);
                t.ys.extend([y, cy, y + bh]);
                t.points.extend([
                    vec2(x, y),
                    vec2(x + bw, y),
                    vec2(x + bw, y + bh),
                    vec2(x, y + bh),
                    vec2(cx, cy),
                ]);
            });
        t
    }

    pub(super) fn snap_point(&mut self, c: Vec2, own: &[String]) -> Vec2 {
        if !self.snapping() {
            self.snap_lines = None;
            return c;
        }
        let reach = self.reach();
        let t = self.snap_targets(own);
        if let Some(grid) = t.grid.as_ref().filter(|g| g.iso) {
            let p = iso_point(c, grid);
            if (p - c).length() <= reach {
                self.snap_lines = Some((Some(p.x), Some(p.y)));
                return p;
            }
        }
        if let Some(p) = t
            .points
            .iter()
            .copied()
            .filter(|p| (*p - c).length() <= reach)
            .min_by(|a, b| (*a - c).length().total_cmp(&(*b - c).length()))
        {
            self.snap_lines = Some((Some(p.x), Some(p.y)));
            return p;
        }
        let (x, y) = (t.x(c.x, reach), t.y(c.y, reach));
        self.snap_lines = (x.is_some() || y.is_some()).then_some((x, y));
        vec2(x.unwrap_or(c.x), y.unwrap_or(c.y))
    }

    pub(super) fn snap_box(&mut self, lo: Vec2, hi: Vec2, own: &[String]) -> Vec2 {
        if !self.snapping() {
            self.snap_lines = None;
            return Vec2::ZERO;
        }
        let reach = self.reach();
        let t = self.snap_targets(own);
        let mid = (lo + hi) / 2.0;
        let best = |edges: [f32; 3], find: &dyn Fn(f32) -> Option<f32>| {
            edges
                .iter()
                .filter_map(|e| find(*e).map(|to| (to - e, to)))
                .min_by(|a, b| a.0.abs().total_cmp(&b.0.abs()))
        };
        let dx = best([lo.x, mid.x, hi.x], &|v| t.x(v, reach));
        let dy = best([lo.y, mid.y, hi.y], &|v| t.y(v, reach));
        self.snap_lines = (dx.is_some() || dy.is_some()).then_some((dx.map(|d| d.1), dy.map(|d| d.1)));
        vec2(dx.map_or(0.0, |d| d.0), dy.map_or(0.0, |d| d.0))
    }

    pub(super) fn rulers(&self) -> bool {
        self.settings.rulers
    }

    pub(super) fn ruler_rects(&self) -> Option<(Rect, Rect)> {
        let rect = self.canvas_rect;
        self.rulers().then(|| {
            (
                Rect::from_min_size(rect.min, vec2(rect.width(), RULER)),
                Rect::from_min_size(rect.min, vec2(RULER, rect.height())),
            )
        })
    }

    pub(super) fn guide_at(&self, pos: Pos2, to_screen: impl Fn(Vec2) -> Pos2) -> Option<(bool, f32)> {
        let g = &self.scene.bundle.doc.guides;
        if g.hidden || g.locked {
            return None;
        }
        g.x.iter()
            .map(|x| (false, *x, (to_screen(vec2(*x, 0.0)).x - pos.x).abs()))
            .chain(
                g.y.iter()
                    .map(|y| (true, *y, (to_screen(vec2(0.0, *y)).y - pos.y).abs())),
            )
            .filter(|(_, _, d)| *d <= 4.0)
            .min_by(|a, b| a.2.total_cmp(&b.2))
            .map(|(across, at, _)| (across, at))
    }

    pub(super) fn finish_guide(&mut self, drag: GuideDrag) -> Result<()> {
        let mut edit = GuideEdit::default();
        if let Some(from) = drag.from {
            match drag.across {
                true => edit.rm_y.push(from),
                false => edit.rm_x.push(from),
            }
        }
        let at = drag.at.round();
        if !drag.over_ruler {
            match drag.across {
                true => edit.add_y.push(at),
                false => edit.add_x.push(at),
            }
        }
        if drag.from.is_none() && drag.over_ruler {
            return Ok(());
        }
        if self.timeline_apply(vec![Op::Guides(edit)])?.is_some() {
            let what = if drag.across { "level" } else { "upright" };
            self.say(
                Tone::Done,
                match (drag.over_ruler, drag.from.is_some()) {
                    (true, _) => format!("took the {what} guide away"),
                    (false, true) => format!("moved the {what} guide to {at}"),
                    (false, false) => format!("a {what} guide at {at}: drag it back onto the ruler to take it away"),
                },
            );
        }
        Ok(())
    }

    pub(super) fn draw_guides(&self, painter: &egui::Painter, rect: Rect, to_screen: impl Fn(Vec2) -> Pos2) {
        let doc = &self.scene.bundle.doc;
        let g = &doc.guides;
        let (w, h) = (doc.canvas.width as f32, doc.canvas.height as f32);
        let canvas = Rect::from_two_pos(to_screen(Vec2::ZERO), to_screen(vec2(w, h))).intersect(rect);
        if let Some(grid) = g.grid.as_ref().filter(|_| g.show_grid) {
            let rgba = klad_core::color::parse_hex(&grid.colour).unwrap_or([0.3, 0.55, 0.9, 1.0]);
            let colour = |strong: bool| {
                let a = grid.opacity * if strong { 1.0 } else { 0.55 };
                Color32::from_rgba_unmultiplied(
                    (rgba[0] * 255.0) as u8,
                    (rgba[1] * 255.0) as u8,
                    (rgba[2] * 255.0) as u8,
                    (a.clamp(0.0, 1.0) * 255.0) as u8,
                )
            };
            let step = (to_screen(vec2(grid.every[0], 0.0)) - to_screen(Vec2::ZERO)).length();
            let major = |i: usize| grid.sub > 0 && i.is_multiple_of(grid.sub as usize);
            if step >= 3.0 && !grid.iso {
                g.lines_x(grid, w).iter().enumerate().for_each(|(i, x)| {
                    let (a, b) = (to_screen(vec2(*x, 0.0)), to_screen(vec2(*x, h)));
                    painter.line_segment([a, b], Stroke::new(1.0, colour(major(i))));
                });
                g.lines_y(grid, h).iter().enumerate().for_each(|(i, y)| {
                    let (a, b) = (to_screen(vec2(0.0, *y)), to_screen(vec2(w, *y)));
                    painter.line_segment([a, b], Stroke::new(1.0, colour(major(i))));
                });
            }
            if step >= 3.0 && grid.iso {
                let (ex, ey) = (grid.every[0], grid.every[1]);
                let slope = ey / ex;
                let count = ((w / ex) + (h / ey) * 2.0).ceil() as i64 + 2;
                (-count..=count).for_each(|i| {
                    let start = grid.offset[0] + i as f32 * ex;
                    [1.0f32, -1.0].iter().for_each(|dir| {
                        let a = vec2(start, grid.offset[1]);
                        let far = a + vec2(dir * (w + h * 2.0), (w + h * 2.0) * slope);
                        let back = a - vec2(dir * (w + h * 2.0), (w + h * 2.0) * slope);
                        painter.line_segment([to_screen(back), to_screen(far)], Stroke::new(1.0, colour(false)));
                    });
                });
            }
        }
        if !g.hidden {
            let line = Stroke::new(1.0, theme::MARK);
            g.x.iter().for_each(|x| {
                let s = to_screen(vec2(*x, 0.0)).x;
                painter.line_segment([pos2(s, rect.top()), pos2(s, rect.bottom())], line);
            });
            g.y.iter().for_each(|y| {
                let s = to_screen(vec2(0.0, *y)).y;
                painter.line_segment([pos2(rect.left(), s), pos2(rect.right(), s)], line);
            });
        }
        if let Some(drag) = self.guide_drag {
            let line = Stroke::new(1.0, SNAPPED);
            match drag.across {
                true => {
                    let s = to_screen(vec2(0.0, drag.at)).y;
                    painter.line_segment([pos2(rect.left(), s), pos2(rect.right(), s)], line);
                }
                false => {
                    let s = to_screen(vec2(drag.at, 0.0)).x;
                    painter.line_segment([pos2(s, rect.top()), pos2(s, rect.bottom())], line);
                }
            }
        }
        if let Some((x, y)) = self.snap_lines.filter(|_| self.dragging_now()) {
            let line = Stroke::new(1.0, SNAPPED);
            if let Some(x) = x {
                let s = to_screen(vec2(x, 0.0)).x;
                painter.line_segment([pos2(s, canvas.top()), pos2(s, canvas.bottom())], line);
            }
            if let Some(y) = y {
                let s = to_screen(vec2(0.0, y)).y;
                painter.line_segment([pos2(canvas.left(), s), pos2(canvas.right(), s)], line);
            }
        }
        if let Some((top, left)) = self.ruler_rects() {
            draw_ruler(painter, top, true, &to_screen, w);
            draw_ruler(painter, left, false, &to_screen, h);
        }
    }

    fn dragging_now(&self) -> bool {
        self.drag.is_some()
            || self.point_drag.is_some()
            || self.shapes.drag.is_some()
            || self.live.as_ref().is_some_and(|l| l.straight.is_some())
    }
}

fn draw_ruler(painter: &egui::Painter, area: Rect, across: bool, to_screen: &impl Fn(Vec2) -> Pos2, length: f32) {
    painter.rect_filled(area, 0.0, Color32::from_gray(36));
    let unit = |v: f32| match across {
        true => to_screen(vec2(v, 0.0)).x,
        false => to_screen(vec2(0.0, v)).y,
    };
    let per_px = (unit(100.0) - unit(0.0)).abs() / 100.0;
    let step = [1.0f32, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0]
        .into_iter()
        .find(|s| s * per_px >= 8.0)
        .unwrap_or(5000.0);
    let font = theme::regular(theme::SMALL);
    let count = (length / step).ceil() as i64;
    (0..=count).for_each(|i| {
        let v = i as f32 * step;
        let s = unit(v);
        let big = i % 5 == 0;
        let tick = if big { RULER * 0.6 } else { RULER * 0.3 };
        let (a, b) = match across {
            true => (pos2(s, area.bottom() - tick), pos2(s, area.bottom())),
            false => (pos2(area.right() - tick, s), pos2(area.right(), s)),
        };
        if area.contains(a) || area.contains(b) {
            painter.line_segment([a, b], Stroke::new(1.0, theme::WEAK));
            if big {
                let at = match across {
                    true => pos2(s + 2.0, area.top() + 1.0),
                    false => pos2(area.left() + 1.0, s + 2.0),
                };
                painter.text(
                    at,
                    egui::Align2::LEFT_TOP,
                    format!("{}", v as i64),
                    font.clone(),
                    theme::WEAK,
                );
            }
        }
    });
}

impl Editor {
    pub(super) fn grid_window(&mut self, ui: &mut egui::Ui) -> Result<()> {
        let doc = &self.scene.bundle.doc;
        let g = doc.guides.clone();
        let grid = g.grid_or_default(doc.canvas.tile);
        let mut edit = klad_core::guides::GridEdit::default();
        let mut changed = false;
        egui::Grid::new("grid settings").num_columns(2).spacing(vec2(12.0, 8.0)).show(ui, |ui| {
            ui.label("spacing");
            ui.horizontal(|ui| {
                let across = self.number(ui, "grid.x", "across ", grid.every[0], 1.0..=4096.0, 0.5, "px between the grid's upright lines");
                let down = self.number(ui, "grid.y", "down ", grid.every[1], 1.0..=4096.0, 0.5, "px between the grid's level lines");
                if across.is_some() || down.is_some() {
                    edit.every = Some([across.unwrap_or(grid.every[0]), down.unwrap_or(grid.every[1])]);
                    changed = true;
                }
            });
            ui.end_row();
            ui.label("stronger line");
            if let Some(v) = self.number(ui, "grid.sub", "every ", grid.sub as f32, 0.0..=64.0, 0.1, "a stronger line every this many lines; 0 for none") {
                edit.sub = Some(v.round() as u32);
                changed = true;
            }
            ui.end_row();
            ui.label("starts at");
            ui.horizontal(|ui| {
                let x = self.number(ui, "grid.ox", "x ", grid.offset[0], -4096.0..=4096.0, 0.5, "where the grid's first upright line is");
                let y = self.number(ui, "grid.oy", "y ", grid.offset[1], -4096.0..=4096.0, 0.5, "where the grid's first level line is");
                if x.is_some() || y.is_some() {
                    edit.offset = Some([x.unwrap_or(grid.offset[0]), y.unwrap_or(grid.offset[1])]);
                    changed = true;
                }
            });
            ui.end_row();
            ui.label("kind");
            ui.horizontal(|ui| {
                if ui.selectable_label(!grid.iso, "square").hint("upright and level lines").clicked() && grid.iso {
                    edit.iso = Some(false);
                    changed = true;
                }
                if ui.selectable_label(grid.iso, "isometric").hint("lines at 2:1, for isometric pixel art").clicked() && !grid.iso {
                    edit.iso = Some(true);
                    changed = true;
                }
            });
            ui.end_row();
            ui.label("look");
            ui.horizontal(|ui| {
                if ui
                    .button(format!("use {}", crate::color::hex(self.color)))
                    .hint(format!("the grid's colour is {}: this gives it the colour you paint with", grid.colour))
                    .clicked()
                {
                    edit.colour = Some(crate::color::hex(self.color));
                    changed = true;
                }
                if let Some(v) = self.number(ui, "grid.a", "opacity ", grid.opacity, 0.0..=1.0, 0.01, "how strong the grid's lines are") {
                    edit.opacity = Some(v);
                    changed = true;
                }
            });
            ui.end_row();
            ui.label("");
            ui.vertical(|ui| {
                let mut show = g.show_grid && g.grid.is_some();
                if ui.checkbox(&mut show, "show the grid").hint("cmd '").changed() {
                    edit.show = Some(show);
                    changed = true;
                }
                let mut snap = g.snap;
                if ui.checkbox(&mut snap, "snap").hint("shift cmd ': shapes, points, moves and bones snap to the grid, the guides and other shapes; hold cmd while dragging to leave it").changed() {
                    edit.snap = Some(snap);
                    changed = true;
                }
            });
            ui.end_row();
        });
        if changed {
            self.timeline_apply(vec![Op::Grid(edit)])?;
        }
        Ok(())
    }

    pub(super) fn toggle_grid(&mut self, snap: bool) -> Result<()> {
        let g = &self.scene.bundle.doc.guides;
        let edit = match snap {
            true => klad_core::guides::GridEdit {
                snap: Some(!g.snap),
                ..Default::default()
            },
            false => klad_core::guides::GridEdit {
                show: Some(!(g.show_grid && g.grid.is_some())),
                ..Default::default()
            },
        };
        let on = match snap {
            true => !g.snap,
            false => !(g.show_grid && g.grid.is_some()),
        };
        if self.timeline_apply(vec![Op::Grid(edit)])?.is_some() {
            let said = match (snap, on) {
                (true, true) => "snapping on: hold cmd while dragging to leave it",
                (true, false) => "snapping off",
                (false, true) => "the grid shows: View > Grid… sets its spacing",
                (false, false) => "the grid is hidden",
            };
            self.say(Tone::Info, said);
        }
        Ok(())
    }

    pub(super) fn guides_switch(&mut self, hide: bool) -> Result<()> {
        let g = &self.scene.bundle.doc.guides;
        let edit = match hide {
            true => GuideEdit {
                hidden: Some(!g.hidden),
                ..Default::default()
            },
            false => GuideEdit {
                locked: Some(!g.locked),
                ..Default::default()
            },
        };
        self.timeline_apply(vec![Op::Guides(edit)])?;
        Ok(())
    }

    pub(super) fn clear_guides(&mut self) -> Result<()> {
        if self
            .timeline_apply(vec![Op::Guides(GuideEdit {
                clear: true,
                ..Default::default()
            })])?
            .is_some()
        {
            self.say(Tone::Done, "took every guide away (cmd Z brings them back)");
        }
        Ok(())
    }
}

impl Editor {
    pub(super) fn snapped(&mut self, e: &crate::pen::PenEvent, c: Vec2) -> Vec2 {
        use super::Tool;
        use crate::pen::Phase;
        let straight = self.live.as_ref().is_some_and(|l| l.straight.is_some())
            || (e.phase == Phase::Down && self.straight_figure().is_some());
        match (self.tool, e.phase) {
            (Tool::Shape | Tool::Bone, _) => self.snap_point(c, &[]),
            (Tool::Pen, Phase::Down) => self.snap_point(c, &[]),
            (Tool::Direct, Phase::Move) if self.point_drag.is_some() => {
                let own: Vec<String> = self.direct.as_ref().map(|d| d.node.clone()).into_iter().collect();
                self.snap_point(c, &own)
            }
            (Tool::Brush, _) if straight => self.snap_point(c, &[]),
            (Tool::Select | Tool::Strokes, Phase::Move) if self.drag.is_some() => {
                let own = self.shapes();
                self.snap_drag(c, &own)
            }
            _ => c,
        }
    }

    fn snap_drag(&mut self, c: Vec2, own: &[String]) -> Vec2 {
        let Some((grab, start, corners)) = self.drag.as_ref().map(|d| (d.grab, d.start, d.corners)) else {
            return c;
        };
        let delta = c - start;
        match grab {
            super::Grab::Move => {
                let moved = corners.map(|p| p + delta);
                let (lo, hi) = moved
                    .iter()
                    .fold((moved[0], moved[0]), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
                c + self.snap_box(lo, hi, own)
            }
            super::Grab::Scale => {
                let near = corners
                    .iter()
                    .copied()
                    .min_by(|a, b| (*a - start).length().total_cmp(&(*b - start).length()))
                    .unwrap_or(start);
                let moved = near + delta;
                let to = self.snap_point(moved, own);
                c + (to - moved)
            }
            _ => c,
        }
    }

    pub(super) fn guide_pen(
        &mut self,
        e: &crate::pen::PenEvent,
        view: crate::view::View,
        rect: Rect,
        c: Vec2,
        on_canvas: bool,
    ) -> Result<bool> {
        use crate::pen::Phase;
        let in_ruler = |s: &Self, p: Pos2| {
            s.ruler_rects()
                .is_some_and(|(top, left)| top.contains(p) || left.contains(p))
        };
        match e.phase {
            Phase::Down => {
                if let Some((top, left)) = self.ruler_rects()
                    && (top.contains(e.pos) || left.contains(e.pos))
                {
                    let across = top.contains(e.pos) && !left.contains(e.pos);
                    self.guide_drag = Some(GuideDrag {
                        across,
                        at: if across { c.y } else { c.x },
                        from: None,
                        over_ruler: true,
                    });
                    return Ok(true);
                }
                if self.tool == super::Tool::Select
                    && on_canvas
                    && self.drag.is_none()
                    && let Some((across, at)) = self.guide_at(e.pos, |p| self.screen(view, rect, p))
                {
                    self.guide_drag = Some(GuideDrag {
                        across,
                        at,
                        from: Some(at),
                        over_ruler: false,
                    });
                    return Ok(true);
                }
                Ok(false)
            }
            Phase::Move => {
                let over = in_ruler(self, e.pos);
                match self.guide_drag.as_mut() {
                    Some(d) => {
                        d.at = if d.across { c.y } else { c.x };
                        d.over_ruler = over;
                        Ok(true)
                    }
                    None => Ok(false),
                }
            }
            Phase::Up => {
                let over = in_ruler(self, e.pos);
                match self.guide_drag.take() {
                    Some(mut d) => {
                        d.over_ruler = over;
                        self.finish_guide(d)?;
                        Ok(true)
                    }
                    None => {
                        self.snap_lines = None;
                        Ok(false)
                    }
                }
            }
        }
    }
}
