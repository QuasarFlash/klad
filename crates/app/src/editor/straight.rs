use super::{DRAG_SLOP, Editor, make};
use egui::{Vec2, vec2};
use klad_core::stroke::{POS_SCALE, Sample};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Figure {
    #[default]
    Free,
    Line,
    Rect,
    Ellipse,
}

impl Figure {
    pub(crate) const ALL: [(Figure, &'static str); 4] = [
        (Figure::Free, "free"),
        (Figure::Line, "line"),
        (Figure::Rect, "rectangle"),
        (Figure::Ellipse, "ellipse"),
    ];

    pub(crate) fn label(self) -> &'static str {
        Figure::ALL.iter().find(|(f, _)| *f == self).map_or("free", |(_, l)| l)
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Straight {
    figure: Figure,
    down: Vec2,
    now: Vec2,
    from: Option<Vec2>,
    dragged: bool,
    shift: bool,
    alt: bool,
    slop: f32,
    first: Sample,
}

const ROUND: usize = 72;

pub(super) const HINT: &str = "what a drag paints with this brush: free, or a line, rectangle or ellipse (shift: 45 degrees, a square or a circle; alt: from the middle). Shift click paints a straight line from the end of the last stroke";

pub(super) fn level(from: Vec2, to: Vec2) -> Vec2 {
    let d = to - from;
    if d.length() < 1e-3 {
        return to;
    }
    let step = std::f32::consts::FRAC_PI_4;
    let a = (d.y.atan2(d.x) / step).round() * step;
    let u = vec2(a.cos(), a.sin());
    from + u * d.dot(u)
}

fn corners(down: Vec2, now: Vec2, shift: bool, alt: bool) -> (Vec2, Vec2) {
    let d = now - down;
    let d = match shift {
        true => {
            let side = d.x.abs().max(d.y.abs());
            vec2(side.copysign(d.x), side.copysign(d.y))
        }
        false => d,
    };
    match alt {
        true => (down - d, down + d),
        false => (down, down + d),
    }
}

fn along(points: &[Vec2], first: &Sample) -> Vec<Sample> {
    let at = |p: Vec2, dt: u16| Sample {
        x: (p.x * POS_SCALE).round() as i32,
        y: (p.y * POS_SCALE).round() as i32,
        dt_ms: dt,
        ..*first
    };
    let Some(start) = points.first() else {
        return Vec::new();
    };
    std::iter::once(at(*start, 0))
        .chain(points.windows(2).flat_map(|w| {
            let n = ((w[1] - w[0]).length() / 2.0).ceil().max(1.0) as usize;
            (1..=n).map(move |i| at(w[0] + (w[1] - w[0]) * (i as f32 / n as f32), 4))
        }))
        .collect()
}

impl Straight {
    pub(super) fn new(figure: Figure, down: Vec2, from: Option<Vec2>, first: Sample, zoom: f32) -> Self {
        Self {
            figure,
            down,
            now: down,
            from,
            dragged: false,
            shift: false,
            alt: false,
            slop: DRAG_SLOP / zoom.max(1e-3),
            first,
        }
    }

    pub(super) fn to(&mut self, at: Vec2, m: egui::Modifiers) {
        self.now = at;
        self.dragged |= (at - self.down).length() > self.slop;
        self.shift = m.shift;
        self.alt = m.alt;
    }

    fn points(&self) -> Vec<Vec2> {
        match self.figure {
            Figure::Free | Figure::Line => match (self.dragged, self.from) {
                (false, Some(from)) => vec![from, self.down],
                (false, None) => vec![self.down],
                (true, _) if self.shift => vec![self.down, level(self.down, self.now)],
                (true, _) => vec![self.down, self.now],
            },
            Figure::Rect | Figure::Ellipse if !self.dragged => vec![self.down],
            Figure::Rect => {
                let (a, b) = corners(self.down, self.now, self.shift, self.alt);
                vec![a, vec2(b.x, a.y), b, vec2(a.x, b.y), a]
            }
            Figure::Ellipse => {
                let (a, b) = corners(self.down, self.now, self.shift, self.alt);
                let (c, r) = ((a + b) / 2.0, (b - a).abs() / 2.0);
                (0..=ROUND)
                    .map(|i| {
                        let t = std::f32::consts::TAU * i as f32 / ROUND as f32 - std::f32::consts::FRAC_PI_2;
                        c + vec2(r.x * t.cos(), r.y * t.sin())
                    })
                    .collect()
            }
        }
    }

    pub(super) fn samples(&self) -> Vec<Sample> {
        along(&self.points(), &self.first)
    }

    #[cfg(test)]
    pub(super) fn end(&self) -> Option<Vec2> {
        self.points().last().copied()
    }
}

impl Editor {
    pub(super) fn straight_figure(&self) -> Option<(Figure, Option<Vec2>)> {
        let layer = self.scene.active_id();
        let from = self
            .brush_last
            .as_ref()
            .filter(|(l, _)| Some(l) == layer.as_ref())
            .map(|(_, p)| *p);
        match (self.brush_figure, self.modifiers.shift) {
            (Figure::Free, false) => None,
            (Figure::Free, true) => Some((Figure::Line, from)),
            (figure, _) => Some((figure, None)),
        }
    }

    pub(super) fn show_straight(&mut self) {
        let Some(live) = self.live.as_mut() else {
            return;
        };
        let Some(straight) = live.straight.as_ref() else {
            return;
        };
        live.stroke.samples = straight.samples();
        self.dirty.extend(self.buffer.touched().iter().copied());
        self.buffer.reset();
        klad_brush::draw_stroke_on(&mut self.buffer, &live.brush, &live.stroke, self.scene.active_raster());
        self.dirty.extend(self.buffer.touched().iter().copied());
        if let Some((axis, copy, _)) = live.twin.as_mut() {
            copy.samples = live.stroke.samples.iter().map(|s| make::mirrored(s, *axis)).collect();
            self.dirty.extend(self.mirror_buffer.touched().iter().copied());
            self.mirror_buffer.reset();
            klad_brush::draw_stroke_on(&mut self.mirror_buffer, &live.brush, copy, self.scene.active_raster());
            self.dirty.extend(self.mirror_buffer.touched().iter().copied());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn first() -> Sample {
        Sample::quantize(0.0, 0.0, 0.7, (0.0, 0.0), 0.0)
    }

    #[test]
    fn a_shift_click_draws_from_the_last_stroke_and_a_drag_keeps_to_45_degrees() {
        let mut s = Straight::new(Figure::Line, vec2(100.0, 100.0), Some(vec2(10.0, 10.0)), first(), 1.0);
        assert_eq!(s.points(), [vec2(10.0, 10.0), vec2(100.0, 100.0)]);
        let shift = egui::Modifiers {
            shift: true,
            ..Default::default()
        };
        s.to(vec2(200.0, 112.0), shift);
        let end = s.end().unwrap();
        assert!((end.y - 100.0).abs() < 1e-3 && (end.x - 200.0).abs() < 1e-3, "{end:?}");
        s.to(vec2(190.0, 205.0), shift);
        let end = s.end().unwrap();
        assert!((end.x - 100.0 - (end.y - 100.0)).abs() < 1e-3, "{end:?}");
        let samples = s.samples();
        assert!(samples.len() > 50);
        assert!(samples.iter().all(|p| p.pressure == first().pressure));
    }

    #[test]
    fn rectangles_and_ellipses_close_and_shift_makes_them_square() {
        let shift = egui::Modifiers {
            shift: true,
            ..Default::default()
        };
        let mut r = Straight::new(Figure::Rect, vec2(0.0, 0.0), None, first(), 1.0);
        r.to(vec2(40.0, 20.0), shift);
        let p = r.points();
        assert_eq!(p.first(), p.last());
        assert_eq!(p[2], vec2(40.0, 40.0));
        let alt = egui::Modifiers {
            alt: true,
            ..Default::default()
        };
        let mut e = Straight::new(Figure::Ellipse, vec2(50.0, 50.0), None, first(), 1.0);
        e.to(vec2(70.0, 60.0), alt);
        let p = e.points();
        assert_eq!(p.len(), ROUND + 1);
        assert!(p.iter().all(|q| (30.0 - 1e-3..=70.0 + 1e-3).contains(&q.x)));
    }
}
