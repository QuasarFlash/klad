mod arrange;
mod assists;
mod between;
mod bones;
mod brushwin;
mod buffer;
mod canvas;
mod cells;
mod checks;
mod clip;
mod clips;
mod commands;
mod compare;
mod copy;
mod depth;
mod drawingfile;
mod export;
mod first;
mod framesin;
mod history;
mod keys;
mod layerops;
mod layers;
mod lock;
mod look;
mod make;
mod maps;
mod marquee;
mod options;
mod outline;
mod paintover;
mod panels;
mod pins;
mod pixels;
mod prefs;
mod selection;
mod shift;
mod snap;
mod straight;
mod target;
mod text;
mod timeline;
mod tweens;
mod typed;
mod words;
mod yours;

#[cfg(test)]
pub(crate) use commands::install_job;
#[cfg(test)]
mod drag_time;
#[cfg(test)]
mod edit_tests;
#[cfg(test)]
mod inventory;
#[cfg(test)]
mod journeys;
#[cfg(test)]
mod monkey;
#[cfg(test)]
mod sizes;
#[cfg(test)]
mod sweep;
#[cfg(test)]
mod testkit;
#[cfg(test)]
mod ui_tests;

use crate::color::{self, Hsva};
use crate::commands::Cmd;
use crate::composite::{Preview, Scene, Tiles};
use crate::frames::FrameCache;
use crate::gpu::{CanvasPaint, Outbox};
use crate::jobs::Jobs;
use crate::lift;
use crate::messages::{Messages, Tone};
use crate::pen::{self, PenEvent, Phase, device_key};
use crate::preview;
use crate::settings::{self, Settings};
use crate::stats::{Stats, lock};
use crate::sync::{Watch, watch};
use crate::theme::Hint;
use crate::view::View;
use anyhow::{Result, anyhow};
use egui::{Color32, Key, Pos2, Rect, Sense, Vec2, vec2};
use klad_brush::{
    Brush, BrushSet, PressureCurve, Raster, Raw, Recorder, StrokeBuffer, StrokePainter, flood, needs_repaint, polygon,
    stroke_blend,
};
use klad_core::brushes::{BrushDef, Curve, TipDef, pick_id};
use klad_core::bundle::{Whereabouts, now_ms};
use klad_core::doc::{Item, Kind, Linear, Loc, MASK, Node, Paint, Radial, Shape, Stop};
use klad_core::fill::{self, Fill, FillRef, FillSource, Seed};
use klad_core::history::Stack;
use klad_core::markedit::{self, LassoAction, MarkTransform, address, apply_affine, invert};
use klad_core::notes::{self, Note};
use klad_core::ops::{LassoSpec, Op, apply_all};
use klad_core::pathedit::{self, PathEdit, Side, Subpath};
use klad_core::rng::{FNV_OFFSET, Rng, fnv};
use klad_core::stroke::{Mark, Stroke, encode_cel};
use klad_render::{overlay, vector};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering as AtomicOrdering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use target::Target;

static SERIALS: AtomicU64 = AtomicU64::new(1);

const OWN_JOB: Duration = Duration::from_secs(3);
const TEXT_SHARE: f32 = 0.25;
const HANDLE: f32 = 12.0;
const DROPPER: f32 = 22.0;
const TWIST_PAUSE: f64 = 0.3;
const CHANNEL_TRACK: f32 = 150.0;
const REFILL_MS: f64 = 8.0;
const FINISH_WAIT: Duration = Duration::from_secs(2);
const DRAG_SLOP: f32 = 3.0;
const KNOB: f32 = 30.0;
const IDENTITY: [f32; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
const GRID_FROM: f32 = 8.0;
const SETTLING_FRAMES: u32 = 10;
const REFIT_CHANGE: f32 = 0.15;
pub(crate) const REAL_MAX: u32 = 512;

type RealKey = (u64, u32, u64);

fn raw_of(e: &PenEvent, c: Vec2, dt_ms: f32) -> Raw {
    Raw {
        x: c.x,
        y: c.y,
        pressure: e.recorded_pressure(),
        tilt: e.tilt,
        dt_ms,
    }
}

fn to_canvas(m: [f32; 6], p: [f32; 2]) -> Vec2 {
    let (x, y) = apply_affine(m, (p[0], p[1]));
    vec2(x, y)
}

fn to_local(m: [f32; 6], c: Vec2) -> [f32; 2] {
    invert(m).map_or([c.x, c.y], |inverse| {
        let (x, y) = apply_affine(inverse, (c.x, c.y));
        [x, y]
    })
}

fn ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Focus {
    Canvas,
    Timeline,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Mixing {
    model: color::Model,
    values: [f32; 3],
    of: [f32; 4],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Dye {
    Strokes,
    Fill,
    Outline,
}

impl Dye {
    fn of(cmd: Cmd) -> Option<Dye> {
        match cmd {
            Cmd::Recolour => Some(Dye::Strokes),
            Cmd::FillShape => Some(Dye::Fill),
            Cmd::OutlineShape => Some(Dye::Outline),
            _ => None,
        }
    }

    fn what(self) -> &'static str {
        match self {
            Dye::Strokes => "the colour of the selected strokes",
            Dye::Fill => "the shape's fill",
            Dye::Outline => "the shape's outline",
        }
    }
}

fn paint_colour(paint: &klad_core::doc::Paint) -> Option<String> {
    use klad_core::doc::Paint;
    let stops = match paint {
        Paint::Solid(hex) => return Some(hex.clone()),
        Paint::Linear { linear } => &linear.stops,
        Paint::Radial { radial } => &radial.stops,
    };
    let colours: Vec<[f32; 4]> = stops.iter().filter_map(|s| color::rgba_of(&s.1)).collect();
    klad_core::color::mean(&colours).map(klad_core::color::to_hex)
}

const ALPHA: color::Channel = color::Channel {
    name: "A",
    lo: 0.0,
    hi: 100.0,
    about: "alpha: how see-through the colour is, 0 to 100",
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tool {
    Brush,
    Fill,
    Select,
    Strokes,
    Direct,
    Pen,
    Shape,
    Text,
    Note,
    Bone,
}

struct PenPath {
    anchors: Vec<pathedit::Anchor>,
    dragging: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct Part {
    layer: String,
    cel: String,
    ids: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq)]
struct Picked {
    cel: String,
    ids: Vec<u32>,
    cut: Option<Vec<[f32; 2]>>,
    more: Vec<Part>,
}

type PartMarks<'a> = (String, String, &'a [Mark], &'a [u32]);

impl Picked {
    fn whole(cel: String, ids: Vec<u32>) -> Self {
        Self {
            cel,
            ids,
            cut: None,
            more: Vec::new(),
        }
    }

    fn key(&self) -> String {
        let list = |cel: &str, ids: &[u32]| {
            let ids: Vec<String> = ids.iter().map(u32::to_string).collect();
            format!("{cel}:{}", ids.join(","))
        };
        let base = std::iter::once(list(&self.cel, &self.ids))
            .chain(self.more.iter().map(|p| list(&p.cel, &p.ids)))
            .collect::<Vec<_>>()
            .join(";");
        match &self.cut {
            Some(polygon) => {
                let bytes: Vec<u8> = polygon.iter().flatten().flat_map(|v| v.to_le_bytes()).collect();
                format!("{base}:cut{:x}", fnv(FNV_OFFSET, &bytes))
            }
            None => base,
        }
    }

    fn addresses(&self) -> Vec<String> {
        self.ids
            .iter()
            .map(|id| address(&self.cel, *id))
            .chain(
                self.more
                    .iter()
                    .flat_map(|p| p.ids.iter().map(|id| address(&p.cel, *id))),
            )
            .collect()
    }

    fn count(&self) -> usize {
        self.ids.len() + self.more.iter().map(|p| p.ids.len()).sum::<usize>()
    }
}

fn union_box(a: Option<[f32; 4]>, b: Option<[f32; 4]>) -> Option<[f32; 4]> {
    match (a, b) {
        (Some(a), Some(b)) => Some([a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]),
        (a, b) => a.or(b),
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Direct {
    node: String,
    points: BTreeSet<usize>,
    also: BTreeMap<String, BTreeSet<usize>>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum PointGrab {
    Anchors,
    Handle(usize, Side),
    Marquee { on_shape: bool },
}

#[derive(Clone, Copy, Debug)]
struct PointDrag {
    grab: PointGrab,
    start: Vec2,
    now: Vec2,
    free: bool,
}

struct PathCommit {
    job: Receiver<Result<(u64, Raster)>>,
    ops: Vec<Op>,
    cel: String,
    preview: Vec<Subpath>,
    also: Vec<Shaped>,
    node: String,
}

#[derive(Clone)]
struct Shaped {
    node: String,
    rev: u64,
    subs: Vec<Subpath>,
    matrix: [f32; 6],
}

#[derive(Clone, Debug, PartialEq)]
struct FillTool {
    reference: FillRef,
    tolerance: f32,
    gap: f32,
    expand: f32,
    opacity: f32,
}

impl Default for FillTool {
    fn default() -> Self {
        Self {
            reference: FillRef::All,
            tolerance: fill::TOLERANCE,
            gap: 4.0,
            expand: fill::EXPAND,
            opacity: 1.0,
        }
    }
}

struct Lasso {
    points: Vec<Vec2>,
    start: Pos2,
    far: bool,
}

struct CutView {
    key: String,
    rev: u64,
    marks: Vec<Mark>,
    lassoed: Vec<u32>,
    chosen: Vec<u32>,
    more: Vec<PartView>,
}

struct PartView {
    layer: String,
    cel: String,
    marks: Vec<Mark>,
    lassoed: Vec<u32>,
    chosen: Vec<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Grab {
    Move,
    Scale,
    Rotate,
    Stretch(usize),
}

fn grab(screen: [Pos2; 4], pos: Pos2, edges: bool) -> Option<Grab> {
    let reach = crate::theme::GRAB;
    let top = screen[0].lerp(screen[1], 0.5);
    let center = screen.iter().fold(Vec2::ZERO, |a, p| a + p.to_vec2()) / 4.0;
    let knob = top + (top.to_vec2() - center).normalized() * KNOB;
    if pos.distance(knob) <= reach {
        return Some(Grab::Rotate);
    }
    if screen.iter().any(|c| pos.distance(*c) <= reach) {
        return Some(Grab::Scale);
    }
    let flat = screen.map(|p| p.to_vec2());
    if let Some(side) = (0..4).find(|&side| edges && pos.distance(edge(&flat, side).to_pos2()) <= reach) {
        return Some(Grab::Stretch(side));
    }
    let turns: Vec<f32> = (0..4)
        .map(|i| {
            let (side, to) = (screen[(i + 1) % 4] - screen[i], pos - screen[i]);
            side.x * to.y - side.y * to.x
        })
        .collect();
    let inside = turns.iter().all(|t| *t >= 0.0) || turns.iter().all(|t| *t <= 0.0);
    inside.then_some(Grab::Move)
}

fn upright(corners: &[Vec2; 4]) -> bool {
    let level = |a: f32, b: f32| (a - b).abs() < 1e-3 * (1.0 + a.abs());
    level(corners[0].y, corners[1].y) && level(corners[1].x, corners[2].x)
}

fn edge(corners: &[Vec2; 4], side: usize) -> Vec2 {
    (corners[side] + corners[(side + 1) % 4]) / 2.0
}

fn transform_props(t: &klad_core::doc::Transform) -> Map<String, Value> {
    [
        ("transform.x", json!(t.x)),
        ("transform.y", json!(t.y)),
        ("transform.rotate", json!(t.rotate)),
        ("transform.sx", json!(t.sx)),
        ("transform.sy", json!(t.sy)),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

#[derive(Clone)]
struct Drag {
    grab: Grab,
    node: String,
    strokes: bool,
    whole: bool,
    fresh: bool,
    copy: bool,
    start: Vec2,
    now: Vec2,
    center: Vec2,
    corners: [Vec2; 4],
    slop: f32,
    crossed: bool,
    outside: bool,
    shift: bool,
    alt: bool,
    loose: bool,
    free: bool,
    toggle: bool,
}

fn factor(k: f32, stick: Option<f32>) -> f32 {
    let k = if k.abs() < 0.02 {
        0.02f32.copysign(k)
    } else {
        k.clamp(-50.0, 50.0)
    };
    let Some(extent) = stick else {
        return k;
    };
    let whole = k.abs().round().max(1.0);
    match (k.abs() - whole).abs() * extent.max(1.0) < 1.0 {
        true => whole.copysign(k),
        false => k,
    }
}

fn ratio(now: f32, start: f32, pivot: f32) -> f32 {
    let span = start - pivot;
    match span.abs() < 1.0 {
        true => 1.0,
        false => (now - pivot) / span,
    }
}

impl Drag {
    fn to(&mut self, at: Vec2) {
        self.now = at;
        self.crossed |= (self.now - self.start).length() > self.slop;
    }

    fn keys(&mut self, m: egui::Modifiers) {
        self.shift = m.shift;
        self.alt = m.alt && self.grab != Grab::Move;
        self.loose = m.command;
    }

    fn extent(&self, axis: usize) -> f32 {
        let [a, b, _, d] = self.corners;
        match axis {
            0 => (b - a).length(),
            _ => (d - a).length(),
        }
    }

    fn stick(&self, axis: Option<usize>) -> Option<f32> {
        (self.whole && !self.loose).then(|| match axis {
            Some(axis) => self.extent(axis),
            None => self.extent(0).max(self.extent(1)),
        })
    }

    fn readout(&self) -> Option<String> {
        let (kx, ky) = match (self.grab, self.stretch()) {
            (_, Some((0, _, k))) => (k, 1.0),
            (_, Some((_, _, k))) => (1.0, k),
            (Grab::Scale, None) => self.factors(),
            _ => return None,
        };
        let percent = format!("{:.0}% by {:.0}%", kx.abs() * 100.0, ky.abs() * 100.0);
        Some(match self.whole {
            true => format!(
                "{percent}, {:.0} by {:.0} px",
                (self.extent(0) * kx.abs()).round(),
                (self.extent(1) * ky.abs()).round()
            ),
            false => percent,
        })
    }

    fn hint(&self) -> &'static str {
        match self.grab {
            Grab::Move if self.copy => "shift keeps the copy level or upright",
            Grab::Move => "shift keeps it level or upright; alt as you start a drag moves a copy",
            Grab::Scale if self.whole && self.free => {
                "shift frees the proportions; alt scales from the middle; cmd lets go of whole sizes"
            }
            Grab::Scale if self.whole => "alt scales from the middle; cmd lets go of whole sizes",
            Grab::Scale if self.free => "shift frees the proportions; alt scales from the middle",
            Grab::Scale => "alt scales from the middle",
            Grab::Rotate => "shift turns in steps of 15 degrees",
            Grab::Stretch(_) if self.whole => "alt stretches both sides from the middle; cmd lets go of whole sizes",
            Grab::Stretch(_) => "alt stretches both sides from the middle",
        }
    }

    fn pivot(&self) -> Vec2 {
        if self.alt || self.grab != Grab::Scale {
            return self.center;
        }
        let near = (0..4)
            .min_by(|&a, &b| {
                (self.corners[a] - self.start)
                    .length()
                    .total_cmp(&(self.corners[b] - self.start).length())
            })
            .unwrap_or(0);
        self.corners[(near + 2) % 4]
    }

    fn factors(&self) -> (f32, f32) {
        if self.grab != Grab::Scale {
            return (1.0, 1.0);
        }
        let p = self.pivot();
        if self.shift && self.free && upright(&self.corners) {
            return (
                factor(ratio(self.now.x, self.start.x, p.x), self.stick(Some(0))),
                factor(ratio(self.now.y, self.start.y, p.y), self.stick(Some(1))),
            );
        }
        let free = ((self.now - p).length() / (self.start - p).length().max(1.0)).clamp(0.02, 50.0);
        let k = factor(free, self.stick(None));
        (k, k)
    }

    fn scale(&self) -> f32 {
        self.factors().0
    }

    fn uneven(&self) -> Option<(f32, f32)> {
        let (kx, ky) = self.factors();
        (kx != ky).then_some((kx, ky))
    }

    fn angle(&self) -> f32 {
        if self.grab != Grab::Rotate {
            return 0.0;
        }
        let free = (self.now - self.center).angle() - (self.start - self.center).angle();
        let step = match (self.whole, self.shift) {
            (true, _) => std::f32::consts::FRAC_PI_2,
            (false, true) => std::f32::consts::PI / 12.0,
            (false, false) => return free,
        };
        (free / step).round() * step
    }

    fn slide(&self) -> Vec2 {
        let d = self.now - self.start;
        let d = match self.shift {
            true if d.x.abs() >= d.y.abs() => vec2(d.x, 0.0),
            true => vec2(0.0, d.y),
            false => d,
        };
        match self.whole {
            true => vec2(d.x.round(), d.y.round()),
            false => d,
        }
    }

    fn stretch(&self) -> Option<(usize, Vec2, f32)> {
        let Grab::Stretch(side) = self.grab else {
            return None;
        };
        let pivot = match self.alt {
            true => self.center,
            false => edge(&self.corners, (side + 2) % 4),
        };
        let axis = if side % 2 == 0 { 1 } else { 0 };
        let k = factor(
            ratio(self.now[axis], self.start[axis], pivot[axis]),
            self.stick(Some(axis)),
        );
        Some((axis, pivot, k))
    }

    fn affine(&self) -> [f32; 6] {
        let c = self.center;
        if let Some((axis, pivot, k)) = self.stretch() {
            return match axis {
                0 => [k, 0.0, 0.0, 1.0, pivot.x * (1.0 - k), 0.0],
                _ => [1.0, 0.0, 0.0, k, 0.0, pivot.y * (1.0 - k)],
            };
        }
        match self.grab {
            Grab::Move => {
                let d = self.slide();
                [1.0, 0.0, 0.0, 1.0, d.x, d.y]
            }
            Grab::Scale => {
                let (kx, ky) = self.factors();
                let p = self.pivot();
                [kx, 0.0, 0.0, ky, p.x * (1.0 - kx), p.y * (1.0 - ky)]
            }
            Grab::Rotate => {
                let (s, k) = self.angle().sin_cos();
                [k, -s, s, k, c.x - k * c.x + s * c.y, c.y - s * c.x - k * c.y]
            }
            Grab::Stretch(_) => IDENTITY,
        }
    }

    fn preview(&self) -> [Vec2; 4] {
        let a = self.affine();
        self.corners.map(|c| {
            let (x, y) = lift::affine_apply(a, (c.x, c.y));
            vec2(x, y)
        })
    }

    fn moved(&self) -> bool {
        self.crossed
    }

    fn mark_transform(&self) -> MarkTransform {
        let pivot = Some([self.center.x, self.center.y]);
        if let Some((axis, at, k)) = self.stretch() {
            return MarkTransform {
                sx: if axis == 0 { k } else { 1.0 },
                sy: if axis == 1 { k } else { 1.0 },
                pivot: Some([at.x, at.y]),
                ..Default::default()
            };
        }
        match self.grab {
            Grab::Move => {
                let d = self.slide();
                MarkTransform {
                    x: d.x,
                    y: d.y,
                    ..Default::default()
                }
            }
            Grab::Scale => {
                let (sx, sy) = self.factors();
                let p = self.pivot();
                MarkTransform {
                    sx,
                    sy,
                    pivot: Some([p.x, p.y]),
                    ..Default::default()
                }
            }
            Grab::Rotate => MarkTransform {
                rotate: self.angle().to_degrees(),
                pivot,
                ..Default::default()
            },
            Grab::Stretch(_) => MarkTransform::default(),
        }
    }
}

struct Finish {
    ops: Vec<Op>,
    rev: u64,
    dirty: BTreeSet<usize>,
    since: Instant,
}

struct PreviewState {
    snap: Arc<preview::Snapshot>,
    pending: BTreeSet<usize>,
    cancel: Arc<AtomicBool>,
    asked: Option<Vec<Op>>,
    asked_at: Option<Vec2>,
    job: Option<Receiver<Result<preview::Done>>>,
    shown: BTreeSet<usize>,
    done: Option<(Vec<Op>, bool, BTreeSet<usize>)>,
    finish: Option<Finish>,
    posed: Option<(u64, u32, String, klad_core::rig::Pose)>,
}

impl PreviewState {
    fn release(&mut self) -> Vec<usize> {
        self.cancel.store(true, AtomicOrdering::Relaxed);
        std::mem::take(&mut self.shown)
            .into_iter()
            .chain(self.finish.take().into_iter().flat_map(|f| f.dirty))
            .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PixelMode {
    Draw,
    Lighter,
    Darker,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PixelShape {
    Rect,
    Ellipse,
    Line,
}

struct PixelLive {
    run: Vec<klad_core::pixel::Cell>,
    erase: bool,
}

struct Live {
    stroke: Stroke,
    painter: StrokePainter,
    brush: Brush,
    recorder: Recorder,
    last_at: Instant,
    twin: Option<(f32, Stroke, StrokePainter)>,
    straight: Option<straight::Straight>,
    follow: Option<assists::Follow>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Tuning {
    hardness: Option<f32>,
    feather: f32,
    size_min: f32,
    size_gamma: f32,
    taper: [f32; 2],
    stabilizer: f32,
    smoothing: f32,
}

impl Tuning {
    fn of(def: &BrushDef) -> Self {
        let curve = def.dynamics.size.pressure.unwrap_or(Curve {
            min: 1.0,
            max: 1.0,
            gamma: 1.0,
        });
        Self {
            hardness: def.hardness(),
            feather: def.feather,
            size_min: curve.min,
            size_gamma: curve.gamma,
            taper: def.taper,
            stabilizer: def.stabilizer,
            smoothing: def.smoothing,
        }
    }

    fn shape(&self, base: &BrushDef) -> BrushDef {
        let mut def = base.clone();
        if let (Some(hardness), TipDef::Round { .. }) = (self.hardness, &def.tip) {
            def.tip = TipDef::Round { hardness };
        }
        def.feather = self.feather;
        let old = Tuning::of(base);
        if (self.size_min, self.size_gamma) != (old.size_min, old.size_gamma) {
            def.dynamics.size.pressure = Some(Curve {
                min: self.size_min,
                max: 1.0,
                gamma: self.size_gamma,
            });
        }
        def
    }
}

fn hand_taper(id: &str, def: &BrushDef) -> [f32; 2] {
    match klad_core::brushes::library(klad_core::brushes::base_of(id)) {
        Some(stock) if stock.taper == def.taper => [0.0, 0.0],
        _ => def.taper,
    }
}

#[derive(Default)]
struct Readout {
    pressure: f32,
    device: u64,
    tilt: (f32, f32),
    eraser: bool,
    tablet: bool,
    samples: VecDeque<Instant>,
}

#[derive(Default)]
struct Windows {
    settings: bool,
    brush: bool,
    log: bool,
    onion: bool,
    link: bool,
    clip: bool,
    between: bool,
    import: bool,
    palettes: bool,
    about: bool,
    export: bool,
    pixels: bool,
    tiles: bool,
    report: bool,
    font: bool,
    paintover: bool,
    compare: bool,
    canvas: bool,
    grid: bool,
    assists: bool,
    frames: bool,
    connect: bool,
}

#[derive(Default)]
struct PaletteState {
    query: String,
    selected: usize,
    opened: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Want {
    New,
    Open,
    Recent,
    Close,
    Gone,
    Path(std::path::PathBuf),
    Cycle(i32),
    Welcome,
}

pub(crate) struct Editor {
    pub(crate) scene: Scene,
    brushes: BrushSet,
    embedded: BTreeMap<String, BrushDef>,
    buffer: StrokeBuffer,
    live: Option<Live>,
    dock_room: f32,
    dock_fit: crate::layout::Fit,
    dock_kept: Option<crate::layout::Pane>,
    folded_for_room: Vec<crate::layout::Pane>,
    fitted_to: Option<Vec2>,
    canvas_frames: u32,
    layer_in_sight: Option<String>,
    timeline_still: Option<bool>,
    timeline_rows: f32,
    timeline_bar: f32,
    timeline_shown: f32,
    timeline_dragged: bool,
    tool: Tool,
    brush: String,
    previous_brush: String,
    other_size: Option<(f32, u32)>,
    heard_seq: Option<u64>,
    bar_above: bool,
    text_click: Option<(Instant, usize)>,
    text_pressing: bool,
    job_ended: Option<Instant>,
    tuning: Tuning,
    size: f32,
    color: [f32; 4],
    hsva: Hsva,
    mixing: Option<Mixing>,
    swatch_name: String,
    fill: FillTool,
    lasso: Option<Lasso>,
    modifiers: egui::Modifiers,
    opacity: f32,
    flow: f32,
    pub(crate) settings: Settings,
    pub(crate) settings_path: Option<PathBuf>,
    pub(crate) settings_dirty: bool,
    pub(crate) update: Option<crate::updates::Release>,
    pub(crate) update_feed: String,
    update_check: Option<std::sync::mpsc::Receiver<crate::updates::Checked>>,
    update_said: Option<String>,
    pub(crate) wants: Option<Want>,
    view: Option<View>,
    canvas_rect: Rect,
    held_messages: Vec<Rect>,
    navigating: bool,
    focused: bool,
    pub(crate) serial: u64,
    pub(crate) pen: Option<Vec<PenEvent>>,
    pen_down: Option<Pos2>,
    touch: Option<egui::TouchId>,
    readout: Readout,
    dirty: BTreeSet<usize>,
    pending: Option<Instant>,
    stats: Arc<Mutex<Stats>>,
    watch: Option<Watch>,
    external: Option<u64>,
    rng: Rng,
    spin: bool,
    messages: Messages,
    measured: BTreeMap<&'static str, String>,
    pointer: Option<Pos2>,
    jobs: Jobs,
    windows: Windows,
    palette: Option<PaletteState>,
    queued: Vec<Cmd>,
    tabs: Vec<bool>,
    themed: bool,
    key_shift: bool,
    held: selection::Held,
    sizing: Option<(f32, f32, Pos2)>,
    second: [f32; 4],
    samples: options::Samples,
    swatch_drag: Option<usize>,
    renaming: Option<layers::Rename>,
    queued_activate: Option<Vec<usize>>,
    solo_later: Option<String>,
    layer_drag: Option<(layers::LayerRow, layers::Where)>,
    canvas_wait: Option<timeline::CanvasWait>,
    canvas_gen: u64,
    buffering: buffer::BufferState,
    custom: Option<BrushDef>,
    brush_window: brushwin::BrushWindow,
    look: look::LookState,
    shapes: make::ShapeTool,
    text_tool: text::TextTool,
    text_edit: Option<text::Typing>,
    picker: text::Picker,
    mirror: Option<f32>,
    mirror_buffer: StrokeBuffer,
    typing: BTreeMap<String, String>,
    import_path: String,
    palette_path: String,
    swatch_picked: Option<usize>,
    depth: depth::Parallax,
    maps: maps::MapsView,
    batch: usize,
    pen_path: Option<PenPath>,
    pen_fill: bool,
    pen_stroke: bool,
    pen_width: f32,
    thumbs: layers::Thumbs,
    between: between::Between,
    checks: checks::Checks,
    export: export::Form,
    pixels: pixels::Form,
    tile_size: [u32; 2],
    paintover: paintover::Form,
    compare: Option<compare::Compared>,
    bones: bones::BoneTool,
    shifting: Option<shift::Shifting>,
    first: first::FirstSteps,
    settings_section: prefs::Section,
    report_drawing: bool,
    #[cfg(test)]
    pub(crate) opened: Vec<String>,
    soloed: Option<(String, Vec<(String, bool)>)>,
    strip_tools: [f32; 2],
    strip_foot: [f32; 2],
    key_ease: klad_core::keys::Ease,
    bindings: crate::commands::Bindings,
    capturing: Option<Cmd>,
    keys_busy: bool,
    field_had_focus: bool,
    turned_box: Option<(String, u64, [Vec2; 4])>,
    cells_last: bool,
    focus: Focus,
    shape_clip: Option<Vec<serde_json::Value>>,
    shape_from: Option<std::path::PathBuf>,
    twist: Option<(f32, f64)>,
    key_filter: String,
    selection: Option<String>,
    group: Vec<String>,
    drag: Option<Drag>,
    edits: BTreeMap<String, f32>,
    texts: BTreeMap<String, String>,
    pins: Vec<Note>,
    note_at: Option<Vec2>,
    note_text: String,
    note_open: Option<u32>,
    note_edit: String,
    history: Option<(u64, Vec<history::Row>)>,
    shared: (Option<String>, Vec<String>, u32),
    ctx: egui::Context,
    sel_box: Option<(String, u64, [Vec2; 4])>,
    group_boxes: Vec<[Vec2; 4]>,
    preview: Option<PreviewState>,
    outbox: Arc<Mutex<Outbox>>,
    zoom_px: f32,
    zoom_wanted: Option<f32>,
    pub(crate) pixel_size: u32,
    pub(crate) pixel_perfect: bool,
    pub(crate) pixel_shape: PixelShape,
    pub(crate) brush_figure: straight::Figure,
    brush_last: Option<(String, Vec2)>,
    typed: Option<typed::Typed>,
    own_picture: Option<clip::Own>,
    look_from: Option<String>,
    canvas_form: canvas::Form,
    cropping: Option<canvas::Crop>,
    window_focused: bool,
    connect: crate::connect::Connect,
    guide_drag: Option<snap::GuideDrag>,
    assist_drag: Option<(usize, usize, [f32; 2])>,
    yours: yours::Yours,
    frames_in: framesin::FramesIn,
    naming: Option<drawingfile::Naming>,
    snap_lines: Option<(Option<f32>, Option<f32>)>,
    pub(crate) pixel_filled: bool,
    pub(crate) pixel_diagonal: bool,
    pub(crate) pixel_fill_all: bool,
    pub(crate) marquee: marquee::Marquee,
    pub(crate) wand_tolerance: f32,
    pub(crate) pixel_mode: PixelMode,
    pub(crate) pixel_dither: u32,
    pub(crate) pixel_two: bool,
    pixel_live: Option<PixelLive>,
    pixel_last: Option<klad_core::pixel::Cell>,
    hover_cell: Option<[i32; 2]>,
    painted: u64,
    real: Option<(RealKey, egui::TextureHandle)>,
    picked: Option<Picked>,
    also: Vec<String>,
    direct: Option<Direct>,
    shaped: Option<Shaped>,
    point_drag: Option<PointDrag>,
    pick_later: bool,
    also_shaped: Vec<Shaped>,
    path_commit: Option<PathCommit>,
    simplify_px: f32,
    under: Vec<String>,
    cut_lasso: bool,
    cut_pixels: bool,
    cut_view: Option<CutView>,
    timeline: timeline::Timeline,
    frames: FrameCache,
    clipboard: Option<copy::Copied>,
}

impl Editor {
    pub(crate) fn open(ctx: egui::Context, path: &Path) -> Result<Self> {
        let start = Instant::now();
        let mut scene = Scene::open_lazy(path)?;
        let brushes = BrushSet::for_bundle(&scene.bundle)?;
        let embedded = scene.bundle.brushes()?;
        let as_read = !scene.bundle.changed_on_disk();
        if let Err(e) = klad_core::drawings::keep_opened(&scene.bundle.root) {
            crate::report::log(&format!("no copy of the drawing as opened, so Revert is off: {e:#}"));
        }
        if as_read {
            scene.bundle.settle();
        }
        let ink = brushes
            .def("ink")
            .ok_or_else(|| crate::report::bug("the ink brush is missing from the library"))?;
        let tuning = Tuning {
            taper: hand_taper("ink", ink),
            ..Tuning::of(ink)
        };
        let settings_path = settings::default_path();
        let mut settings = settings_path.as_deref().map(Settings::load).unwrap_or_default();
        let offered = settings.offer_real_size(scene.bundle.doc.pixel_art());
        let open = format!(
            "{} layers, opened in {:.0} ms",
            scene.bundle.doc.all_layers().len(),
            ms(start)
        );
        let size = scene.size();
        let watch = watch(path, ctx.clone());
        let mut messages = Messages::default();
        if let Err(e) = &watch {
            messages.say(
                Tone::Error,
                format!("not watching the bundle, so agent edits will not show: {e}"),
                Instant::now(),
            );
        }
        let measured = BTreeMap::from([("open", open)]);
        let pins = notes::read(&scene.bundle);
        let repaint = ctx.clone();
        let mut frames = FrameCache::new(Arc::new(move || repaint.request_repaint()));
        frames.disk = Some(scene.pictures());
        let mut editor = Self {
            dirty: scene.all_tiles().into_iter().collect(),
            buffer: StrokeBuffer::new(size.0, size.1),
            scene,
            brushes,
            embedded,
            live: None,
            dock_room: 0.0,
            dock_fit: crate::layout::Fit::default(),
            dock_kept: None,
            folded_for_room: Vec::new(),
            fitted_to: None,
            canvas_frames: 0,
            layer_in_sight: None,
            timeline_still: None,
            timeline_rows: f32::INFINITY,
            timeline_bar: crate::layout::TIMELINE_FOLDED,
            timeline_shown: 0.0,
            timeline_dragged: false,
            tool: Tool::Brush,
            brush: "ink".into(),
            previous_brush: "ink".into(),
            other_size: None,
            heard_seq: None,
            bar_above: false,
            text_click: None,
            text_pressing: false,
            job_ended: None,
            tuning,
            size: 12.0,
            color: [0.08, 0.08, 0.1, 1.0],
            hsva: Hsva::from_rgba([0.08, 0.08, 0.1, 1.0], 0.0),
            mixing: None,
            swatch_name: String::new(),
            fill: FillTool::default(),
            lasso: None,
            modifiers: egui::Modifiers::default(),
            opacity: 1.0,
            flow: 1.0,
            settings,
            settings_path,
            settings_dirty: offered,
            update: None,
            update_feed: crate::updates::feed(),
            update_check: None,
            update_said: None,
            wants: None,
            view: None,
            canvas_rect: Rect::NOTHING,
            held_messages: Vec::new(),
            navigating: false,
            focused: false,
            serial: SERIALS.fetch_add(1, AtomicOrdering::Relaxed),
            pen: None,
            pen_down: None,
            touch: None,
            readout: Readout::default(),
            pending: None,
            stats: Arc::default(),
            watch: watch.ok(),
            external: None,
            rng: Rng::new(now_ms()),
            spin: false,
            messages,
            measured,
            pointer: None,
            jobs: Jobs::default(),
            windows: Windows::default(),
            palette: None,
            queued: Vec::new(),
            tabs: Vec::new(),
            themed: false,
            key_shift: false,
            held: selection::Held::default(),
            sizing: None,
            second: [1.0, 1.0, 1.0, 1.0],
            samples: options::Samples::default(),
            swatch_drag: None,
            renaming: None,
            queued_activate: None,
            solo_later: None,
            layer_drag: None,
            canvas_wait: None,
            canvas_gen: 0,
            buffering: buffer::BufferState::default(),
            custom: None,
            brush_window: brushwin::BrushWindow::default(),
            look: look::LookState::default(),
            shapes: make::ShapeTool::default(),
            text_tool: text::TextTool::default(),
            text_edit: None,
            picker: text::Picker::default(),
            mirror: None,
            mirror_buffer: StrokeBuffer::new(size.0, size.1),
            typing: BTreeMap::new(),
            import_path: String::new(),
            palette_path: String::new(),
            swatch_picked: None,
            depth: depth::Parallax::default(),
            maps: maps::MapsView::default(),
            batch: timeline::FILL,
            pen_path: None,
            pen_fill: true,
            pen_stroke: true,
            pen_width: 3.0,
            thumbs: layers::Thumbs::default(),
            between: between::Between::default(),
            checks: checks::Checks::default(),
            export: export::Form::default(),
            pixels: pixels::Form::default(),
            tile_size: [16, 16],
            paintover: paintover::Form::default(),
            compare: None,
            bones: bones::BoneTool::default(),
            shifting: None,
            first: first::FirstSteps::default(),
            settings_section: prefs::Section::default(),
            report_drawing: false,
            #[cfg(test)]
            opened: Vec::new(),
            soloed: None,
            strip_tools: [570.0, 480.0],
            strip_foot: [186.0, 166.0],
            key_ease: klad_core::keys::Ease::Linear,
            bindings: crate::commands::Bindings::default(),
            capturing: None,
            keys_busy: false,
            field_had_focus: false,
            turned_box: None,
            cells_last: false,
            focus: Focus::Canvas,
            shape_clip: None,
            shape_from: None,
            twist: None,
            key_filter: String::new(),
            selection: None,
            group: Vec::new(),
            drag: None,
            edits: BTreeMap::new(),
            texts: BTreeMap::new(),
            pins,
            note_at: None,
            note_text: String::new(),
            note_open: None,
            note_edit: String::new(),
            history: None,
            shared: (None, Vec::new(), 0),
            ctx,
            sel_box: None,
            group_boxes: Vec::new(),
            preview: None,
            outbox: Arc::default(),
            zoom_px: 1.0,
            zoom_wanted: None,
            pixel_size: 1,
            pixel_perfect: true,
            pixel_shape: PixelShape::Rect,
            brush_figure: straight::Figure::Free,
            brush_last: None,
            typed: None,
            own_picture: None,
            look_from: None,
            canvas_form: canvas::Form::default(),
            cropping: None,
            window_focused: true,
            connect: crate::connect::Connect::here(),
            guide_drag: None,
            assist_drag: None,
            frames_in: framesin::FramesIn::fresh(),
            naming: None,
            yours: yours::Yours::at(match cfg!(test) {
                true => None,
                false => klad_render::library::Library::home().ok(),
            }),
            snap_lines: None,
            pixel_filled: false,
            pixel_diagonal: false,
            pixel_fill_all: false,
            marquee: marquee::Marquee::Box,
            wand_tolerance: klad_render::wand::TOLERANCE,
            pixel_mode: PixelMode::Draw,
            pixel_dither: 100,
            pixel_two: false,
            pixel_live: None,
            pixel_last: None,
            hover_cell: None,
            painted: 0,
            real: None,
            picked: None,
            also: Vec::new(),
            direct: None,
            shaped: None,
            point_drag: None,
            pick_later: false,
            also_shaped: Vec::new(),
            path_commit: None,
            simplify_px: 1.5,
            under: Vec::new(),
            cut_lasso: false,
            cut_pixels: true,
            cut_view: None,
            clipboard: None,
            timeline: timeline::Timeline::default(),
            frames,
        };
        editor.use_defaults();
        let newest = klad_core::history::peek(&editor.scene.bundle, Stack::Undo)
            .ok()
            .flatten();
        editor.heard_seq = Some(newest.as_ref().map_or(0, |(entry, _)| entry.seq));
        if newest.is_some_and(|(entry, _)| entry.author != "editor") {
            editor.tool = Tool::Select;
        }
        if editor.scene.active_layer().is_none() && !editor.scene.bundle.doc.layers.is_empty() {
            let top = editor.scene.bundle.doc.layers.len() - 1;
            editor.scene.set_active_later(vec![top]);
        }
        editor.request_canvas();
        editor.tend_canvas(timeline::OPEN_WAIT)?;
        Ok(editor)
    }

    pub(super) fn use_defaults(&mut self) {
        let d = self.settings.defaults.clone();
        if let Some(brush) = d.brush.filter(|b| self.brushes.def(b).is_some()) {
            self.brush = brush;
        }
        if let Some(size) = d.size {
            self.size = size.clamp(0.5, 500.0);
        }
        let tall = self.scene.bundle.doc.canvas.height as f32;
        self.text_tool.size = (tall * TEXT_SHARE).round().clamp(1.0, text::TEXT_SIZE);
        self.timeline.on = match self.scene.bundle.doc.pixel_art() {
            true => 1,
            false => d.exposure.clamp(1, 8),
        };
        self.scene.onion.before = d.onion_before;
        self.scene.onion.after = d.onion_after;
        self.scene.onion.opacity = d.onion_opacity.clamp(0.05, 1.0);
        self.bindings = crate::commands::Bindings::from_settings(&self.settings.keys);
    }

    pub(super) fn capture_key(&mut self, ui: &egui::Ui) {
        let Some(cmd) = self.capturing else {
            return;
        };
        let pressed = ui.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => Some((*key, *modifiers)),
                _ => None,
            })
        });
        let Some((key, modifiers)) = pressed else {
            return;
        };
        ui.ctx().input_mut(|i| i.consume_key(modifiers, key));
        self.capturing = None;
        if key == egui::Key::Escape {
            self.say(Tone::Info, "the key stays as it was");
            return;
        }
        let keys = crate::commands::Keys::pressed(key, modifiers);
        match self.bindings.bind(cmd, keys) {
            Ok(()) => {
                self.settings.keys = self.bindings.to_settings();
                self.settings_dirty = true;
                self.say(Tone::Done, format!("{} is now {}", cmd.spec().name, keys.label()));
            }
            Err(why) => self.say(Tone::Refused, why),
        }
    }

    fn canvas_size(&self) -> Vec2 {
        let (w, h) = self.scene.size();
        vec2(w as f32, h as f32)
    }

    pub(crate) fn say(&mut self, tone: Tone, text: impl Into<String>) {
        let text = crate::commands::local_keys(&text.into());
        if tone == Tone::Error {
            crate::report::log(&format!("editor: {text}"));
        }
        self.messages.say(tone, text, Instant::now());
        self.ctx.request_repaint();
    }

    pub(crate) fn error(&mut self, e: impl std::fmt::Display) {
        self.say(Tone::Error, format!("{e:#}"));
    }

    pub(crate) fn failed(&mut self, e: anyhow::Error) {
        if let Some(locked) = e.downcast_ref::<lock::Locked>() {
            self.say(Tone::Refused, locked.to_string());
            return;
        }
        if e.downcast_ref::<klad_core::bundle::Changed>().is_some() {
            self.say(
                Tone::Refused,
                "the drawing was changed from outside at that very moment, so nothing was done: do it again",
            );
            return;
        }
        let text = format!("{e:#}");
        if text.contains(words::RAMP_CLI) {
            self.say(Tone::Refused, self.refusal_words(&text));
            return;
        }
        match crate::report::is_bug(&e) {
            true => self.say(Tone::Error, crate::report::bug_message(&e)),
            false => self.error(e),
        }
    }

    fn measure(&mut self, key: &'static str, text: String) {
        self.measured.insert(key, text);
    }

    fn share_selection(&mut self) {
        let items: Vec<String> = self
            .shapes()
            .iter()
            .chain(self.direct.as_ref().map(|d| &d.node))
            .cloned()
            .chain(self.picked.iter().flat_map(Picked::addresses))
            .collect();
        let now = (self.scene.active_id(), items, self.scene.frame);
        if now != self.shared {
            let nodes = now.1.clone();
            if let Err(e) = notes::write_selection(&self.scene.bundle, now.0.clone(), nodes, self.scene.frame) {
                self.failed(e.context("the selection could not be shared with the agent"));
            }
            self.shared = now;
        }
    }

    pub(super) fn folder_is_there(&mut self) -> bool {
        match self.scene.bundle.whereabouts() {
            Whereabouts::Here => true,
            Whereabouts::Renamed(to) => {
                self.follow(to);
                true
            }
            Whereabouts::Replaced => {
                self.scene.bundle.adopt();
                self.watch = watch(&self.scene.bundle.root, self.ctx.clone()).ok();
                self.external.get_or_insert(now_ms());
                true
            }
            Whereabouts::Gone => {
                self.wants = Some(Want::Gone);
                false
            }
        }
    }

    fn follow(&mut self, to: PathBuf) {
        let from = self.scene.bundle.root.clone();
        self.scene.bundle.follow(to.clone());
        self.frames.disk = Some(self.scene.pictures());
        self.watch = match watch(&to, self.ctx.clone()) {
            Ok(watch) => Some(watch),
            Err(e) => {
                self.say(
                    Tone::Error,
                    format!("not watching the drawing's folder, so agent edits will not show: {e}"),
                );
                None
            }
        };
        self.external.get_or_insert(now_ms());
        self.settings.moved(&from, &to);
        self.settings_dirty = true;
        self.ctx
            .send_viewport_cmd(egui::ViewportCommand::Title(crate::start::title(Some(&to))));
        let name = to
            .file_stem()
            .map_or_else(|| to.display().to_string(), |s| s.to_string_lossy().to_string());
        self.say(
            Tone::Info,
            format!("this drawing's folder was renamed: it is called {name} now, and klad goes on in it"),
        );
    }

    fn sync(&mut self) -> Result<()> {
        if let Some(first) = self.watch.as_ref().and_then(|w| w.events.try_iter().min()) {
            self.external.get_or_insert(first);
            self.pins = notes::read(&self.scene.bundle);
        }
        if self.in_the_middle() || self.external.is_none() {
            return Ok(());
        }
        if !self.scene.bundle.changed_on_disk() {
            self.external = None;
            return Ok(());
        }
        self.reload()
    }

    fn in_the_middle(&self) -> bool {
        self.live.is_some() || self.drag.is_some() || self.point_drag.is_some() || self.path_commit.is_some()
    }

    pub(super) fn follow_the_folder_between_acts(&mut self) -> Result<()> {
        match self.in_the_middle() {
            true => Ok(()),
            false => self.follow_the_folder(),
        }
    }

    pub(super) fn follow_the_folder(&mut self) -> Result<()> {
        match self.scene.bundle.changed_on_disk() {
            true => self.reload(),
            false => Ok(()),
        }
    }

    pub(super) fn write(&mut self, ops: &[Op]) -> Result<(Vec<usize>, klad_core::ops::Applied)> {
        self.follow_the_folder()?;
        lock::refuse(&self.scene.bundle.doc, ops)?;
        let layered = ops
            .iter()
            .any(|op| matches!(op, Op::Move { .. } | Op::AddLayer { .. } | Op::Remove { .. }));
        let bases = layered.then(|| words::clip_bases(&self.scene.bundle.doc));
        let done = self.scene.apply_with(ops, &self.brushes)?;
        if let Some(words) = bases.and_then(|b| self.clips_moved(&b)) {
            self.say(Tone::Refused, words);
        }
        Ok(done)
    }

    fn reload(&mut self) -> Result<()> {
        let seen = self.external.take();
        self.checks.gaps = None;
        self.checks.arcs = None;
        let start = Instant::now();
        self.brushes.refresh(&self.scene.bundle)?;
        self.embedded = self.scene.bundle.brushes()?;
        let dirty = self.scene.reload(&self.brushes)?;
        let (w, h) = self.scene.size();
        if (self.buffer.grid().width, self.buffer.grid().height) != (w, h) {
            self.buffer = StrokeBuffer::new(w, h);
            self.mirror_buffer = StrokeBuffer::new(w, h);
            self.view = None;
        }
        let tiles = dirty.len();
        self.dirty.extend(dirty);
        self.forget_missing_selection();
        let took = ms(start);
        let text = match self.scene.bundle.read_stamp() {
            Some(s) => format!(
                "rev {} by {}\nfile event after {} ms\n{tiles} tiles refreshed after {} ms ({took:.0} ms to reload)",
                s.rev,
                s.author,
                seen.map_or(-1, |t| t as i64 - s.at_ms as i64),
                now_ms() as i64 - s.at_ms as i64
            ),
            None => format!("{tiles} tiles refreshed in {took:.0} ms"),
        };
        self.measure("sync", text);
        self.hear_the_agent();
        Ok(())
    }

    fn hear_the_agent(&mut self) {
        let Some((entry, _)) = klad_core::history::peek(&self.scene.bundle, Stack::Undo).ok().flatten() else {
            return;
        };
        let known = self.heard_seq.replace(entry.seq);
        let own = self.jobs.busy(crate::jobs::Tag::Edit)
            || self.jobs.busy(crate::jobs::Tag::Inbetween)
            || self.job_ended.is_some_and(|t| t.elapsed() < OWN_JOB);
        if known.is_some_and(|seq| entry.seq > seq) && entry.author != "editor" && !own {
            let who = match entry.author.as_str() {
                "cli" => "the agent".to_string(),
                other => history::who(other),
            };
            self.say(
                Tone::Done,
                format!("{who} changed the drawing: {}", self.step_words(&entry.summary)),
            );
        }
    }

    pub(super) fn shapes(&self) -> Vec<String> {
        match &self.selection {
            Some(id) if self.group.len() > 1 && self.group.contains(id) => self.group.clone(),
            Some(id) => vec![id.clone()],
            None => Vec::new(),
        }
    }

    pub(super) fn select_shapes(&mut self, ids: Vec<String>) {
        let doc = &self.scene.bundle.doc;
        let home = ids.last().and_then(|id| match doc.item(id) {
            Some(Item::Node(layer, _, _)) => match doc.locate(&layer.id) {
                Some(Loc::Layer(path)) => Some(path),
                _ => None,
            },
            _ => None,
        });
        if let Some(path) = home.filter(|p| *p != self.scene.active && self.shifting.is_none()) {
            let _ = self.activate(path);
        }
        self.selection = ids.last().cloned();
        self.group = if ids.len() > 1 { ids } else { Vec::new() };
        self.refresh_box();
    }

    fn forget_missing_selection(&mut self) {
        let doc = &self.scene.bundle.doc;
        self.group.retain(|id| matches!(doc.item(id), Some(Item::Node(..))));
        if self.group.len() < 2 || !self.selection.as_ref().is_some_and(|id| self.group.contains(id)) {
            self.group.clear();
        }
        if self
            .selection
            .as_ref()
            .is_some_and(|id| !matches!(doc.item(id), Some(Item::Node(..))))
        {
            self.selection = None;
        }
        let gone = |id: &str| match markedit::parse_address(id) {
            Some((cel, mark)) => {
                self.scene.active_cel().as_deref() != Some(cel)
                    || !self.scene.active_marks().iter().any(|m| m.id() == mark)
            }
            None => !matches!(doc.item(id), Some(Item::Node(..))),
        };
        if self.direct.as_ref().is_some_and(|d| gone(&d.node)) {
            self.direct = None;
        }
        let active = self.scene.active_cel();
        let active_id = self.scene.active_id();
        let present: BTreeSet<u32> = self.scene.active_marks().iter().map(Mark::id).collect();
        let frame = self.scene.frame;
        let scene = &self.scene;
        let shown = |layer: &str| {
            scene
                .bundle
                .doc
                .layer(layer)
                .and_then(|l| l.content_at(frame))
                .map(|c| c.id.clone())
        };
        self.picked = self.picked.take().and_then(|mut p| {
            p.ids.retain(|id| present.contains(id));
            let cut = p.cut.is_some();
            p.more.retain_mut(|part| {
                if shown(&part.layer).as_deref() != Some(part.cel.as_str()) {
                    return false;
                }
                let have: BTreeSet<u32> = scene.marks_of(&part.cel).iter().map(Mark::id).collect();
                part.ids.retain(|id| have.contains(id));
                !part.ids.is_empty() || cut
            });
            (Some(&p.cel) == active.as_ref() && (!p.ids.is_empty() || cut || !p.more.is_empty())).then_some(p)
        });
        self.also.retain(|id| {
            Some(id) != active_id.as_ref() && scene.bundle.doc.layer(id).is_some_and(|l| l.kind() == Kind::Paint)
        });
    }

    fn apply(&mut self, ops: Vec<Op>) -> Result<()> {
        let dirty = self.commit(ops)?;
        self.dirty.extend(dirty);
        Ok(())
    }

    fn commit(&mut self, ops: Vec<Op>) -> Result<Vec<usize>> {
        self.checks.gaps = None;
        self.checks.arcs = None;
        if ops.is_empty() {
            return Ok(Vec::new());
        }
        self.end_look_preview()?;
        let start = Instant::now();
        let (dirty, _) = self.write(&ops)?;
        self.measure(
            "edit",
            format!("edit applied, {} tiles refreshed in {:.0} ms", dirty.len(), ms(start)),
        );
        self.forget_missing_selection();
        Ok(dirty)
    }

    fn set(&mut self, id: &str, props: Vec<(String, Value)>) -> Result<()> {
        if props.is_empty() {
            return Ok(());
        }
        self.apply(vec![Op::Set {
            id: id.to_string(),
            props: props.into_iter().collect::<Map<String, Value>>(),
        }])
    }

    fn step(&mut self, stack: Stack) -> Result<()> {
        if self.live.is_some() || self.drag.is_some() || self.point_drag.is_some() || self.path_commit.is_some() {
            return Ok(());
        }
        self.follow_the_folder()?;
        let start = Instant::now();
        let word = if stack == Stack::Undo { "undid" } else { "redid" };
        let author = klad_core::history::peek(&self.scene.bundle, stack)
            .ok()
            .flatten()
            .map(|(entry, _)| entry.author);
        match self.scene.step(stack, &self.brushes)? {
            Some((summary, dirty)) => {
                self.measure("undo", format!("{word} {summary} in {:.0} ms", ms(start)));
                let whose = match author.as_deref() {
                    Some("editor") | None => String::new(),
                    Some("cli") => " the agent's step".to_string(),
                    Some(other) => format!(" {}'s step", history::who(other)),
                };
                let tone = if whose.is_empty() { Tone::Info } else { Tone::Done };
                self.say(tone, format!("{word}{whose}: {}", self.step_words(&summary)));
                self.dirty.extend(dirty);
                self.forget_missing_selection();
            }
            None => self.say(
                Tone::Refused,
                format!("nothing to {}", if stack == Stack::Undo { "undo" } else { "redo" }),
            ),
        }
        Ok(())
    }

    pub(super) fn plain(&self, text: &str) -> String {
        let doc = &self.scene.bundle.doc;
        let drawing = |cel: &str| match doc.locate(cel) {
            Some(Loc::Cel(path, c)) => Some(format!(
                "the drawing at frame {} on {}",
                doc.layer_at(&path).cels()[c].frame,
                doc.layer_at(&path).name
            )),
            _ => None,
        };
        let word = |w: &str| -> Option<String> {
            if let Some((cel, _)) = klad_core::markedit::parse_address(w) {
                return Some(drawing(cel).map_or("a mark".to_string(), |d| format!("a mark of {d}")));
            }
            let digits = w.len() > 1 && w[1..].chars().all(|c| c.is_ascii_digit());
            match w.chars().next() {
                Some('c') if digits => drawing(w),
                Some('l') if digits => doc.layer(w).map(|l| l.name.clone()),
                Some('n') if digits => match doc.item(w) {
                    Some(Item::Node(_, _, node)) => Some(format!("a {}", node.type_name())),
                    _ => None,
                },
                _ => None,
            }
        };
        text.split(' ')
            .map(|w| {
                let bare = w.trim_end_matches([',', ')', ':', '.', ';']);
                let lead = bare.trim_start_matches('(');
                let (head, tail) = (&bare[..bare.len() - lead.len()], &w[bare.len()..]);
                word(lead).map_or(w.to_string(), |p| format!("{head}{p}{tail}"))
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn refresh_box(&mut self) {
        let rev = self.scene.bundle.doc.rev;
        let ids = self.shapes();
        if ids.is_empty() {
            self.sel_box = None;
            self.group_boxes.clear();
            return;
        }
        let key = ids.join(" ");
        if self.sel_box.as_ref().is_some_and(|(s, r, _)| *s == key && *r == rev) {
            return;
        }
        let boxes: Vec<[f32; 4]> = ids
            .iter()
            .filter_map(|id| overlay::node_world_box(&self.scene.bundle.doc, id).ok().flatten())
            .collect();
        let corners = |[x, y, w, h]: [f32; 4]| [vec2(x, y), vec2(x + w, y), vec2(x + w, y + h), vec2(x, y + h)];
        self.group_boxes = match boxes.len() {
            0 | 1 => Vec::new(),
            _ => boxes.iter().map(|b| corners(*b)).collect(),
        };
        self.sel_box = boxes
            .iter()
            .copied()
            .reduce(|a, b| {
                let (x, y) = (a[0].min(b[0]), a[1].min(b[1]));
                [
                    x,
                    y,
                    (a[0] + a[2]).max(b[0] + b[2]) - x,
                    (a[1] + a[3]).max(b[1] + b[3]) - y,
                ]
            })
            .map(|all| (key, rev, corners(all)));
    }

    fn selection_corners(&self) -> Option<(String, [Vec2; 4])> {
        let (key, _, corners) = self.sel_box.as_ref()?;
        let id = self.selection.clone()?;
        (self.shapes().join(" ") == *key).then_some((id, *corners))
    }

    fn refresh_cut(&mut self) {
        let rev = self.scene.bundle.doc.rev;
        let Some((key, cel, polygon, ids, more)) = self.picked.as_ref().and_then(|p| {
            p.cut
                .clone()
                .map(|polygon| (p.key(), p.cel.clone(), polygon, p.ids.clone(), p.more.clone()))
        }) else {
            self.cut_view = None;
            return;
        };
        if self.cut_view.as_ref().is_some_and(|v| v.key == key && v.rev == rev) {
            return;
        }
        let doc = &self.scene.bundle.doc;
        let split = |cel: &str, source: &[Mark], ids: Vec<u32>| {
            let mut marks = source.to_vec();
            let lassoed = markedit::split(&mut marks, &polygon, true, doc.mark_ids(cel));
            let chosen: Vec<u32> = lassoed
                .iter()
                .copied()
                .chain(ids.into_iter().filter(|id| !lassoed.contains(id)))
                .collect();
            (marks, lassoed, chosen)
        };
        let (marks, lassoed, chosen) = split(&cel, self.scene.active_marks(), ids);
        let more = more
            .into_iter()
            .map(|p| {
                let (marks, lassoed, chosen) = split(&p.cel, self.scene.marks_of(&p.cel), p.ids);
                PartView {
                    layer: p.layer,
                    cel: p.cel,
                    marks,
                    lassoed,
                    chosen,
                }
            })
            .collect();
        self.cut_view = Some(CutView {
            key,
            rev,
            marks,
            lassoed,
            chosen,
            more,
        });
    }

    fn picked_parts(&self) -> Vec<PartMarks<'_>> {
        let Some(picked) = self.picked.as_ref() else {
            return Vec::new();
        };
        let view = self
            .cut_view
            .as_ref()
            .filter(|v| picked.cut.is_some() && v.key == picked.key());
        if picked.cut.is_some() && view.is_none() {
            return Vec::new();
        }
        let active = self.scene.active_id().unwrap_or_default();
        let first: PartMarks = match view {
            Some(v) => (active, picked.cel.clone(), &v.marks[..], &v.chosen[..]),
            None => (active, picked.cel.clone(), self.scene.active_marks(), &picked.ids[..]),
        };
        std::iter::once(first)
            .chain(
                picked
                    .more
                    .iter()
                    .enumerate()
                    .map(|(k, p)| match view.and_then(|v| v.more.get(k)) {
                        Some(pv) => (p.layer.clone(), p.cel.clone(), &pv.marks[..], &pv.chosen[..]),
                        None => (p.layer.clone(), p.cel.clone(), self.scene.marks_of(&p.cel), &p.ids[..]),
                    }),
            )
            .collect()
    }

    fn picked_corners(&self) -> Option<(String, [Vec2; 4])> {
        let picked = self.picked.as_ref()?;
        if let Some((_, _, corners)) = self
            .turned_box
            .as_ref()
            .filter(|(key, rev, _)| *key == picked.key() && *rev == self.scene.bundle.doc.rev)
        {
            return Some((picked.key(), *corners));
        }
        let area = picked.cut.as_ref().filter(|_| self.scene.on_grid()).map(|polygon| {
            polygon.iter().fold([f32::MAX, f32::MAX, f32::MIN, f32::MIN], |b, p| {
                [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0]), b[3].max(p[1])]
            })
        });
        let [x0, y0, x1, y1] = self
            .picked_parts()
            .iter()
            .map(|(_, _, marks, ids)| markedit::bounds_of(marks, ids))
            .chain(std::iter::once(area))
            .fold(None, union_box)?;
        Some((picked.key(), [vec2(x0, y0), vec2(x1, y0), vec2(x1, y1), vec2(x0, y1)]))
    }

    fn across(&self, action: LassoAction) -> LassoAction {
        let spans = self.picked.as_ref().is_some_and(|p| !p.more.is_empty());
        match action {
            LassoAction::Transform(t) if spans && t.pivot.is_none() => {
                let pivot = self
                    .picked_corners()
                    .map(|(_, c)| [(c[0].x + c[2].x) / 2.0, (c[0].y + c[2].y) / 2.0]);
                LassoAction::Transform(MarkTransform { pivot, ..t })
            }
            other => other,
        }
    }

    fn picked_ops(&self, action: LassoAction) -> Option<(Vec<Op>, Option<Picked>)> {
        let picked = self.picked.clone()?;
        let action = self.across(action);
        let Some(polygon) = picked.cut.clone() else {
            return Some(match action {
                LassoAction::Transform(transform) => (
                    vec![Op::Transform {
                        ids: picked.addresses(),
                        transform,
                    }],
                    Some(picked),
                ),
                LassoAction::Remove => (
                    vec![Op::Remove {
                        ids: picked.addresses(),
                    }],
                    None,
                ),
            });
        };
        let view = self.cut_view.as_ref().filter(|v| v.key == picked.key())?;
        let layer = self.scene.active_id()?;
        let frame = self.scene.frame;
        let views: Vec<(String, &str, &[u32], &[u32])> =
            std::iter::once((layer, picked.cel.as_str(), &view.lassoed[..], &view.chosen[..]))
                .chain(
                    view.more
                        .iter()
                        .map(|v| (v.layer.clone(), v.cel.as_str(), &v.lassoed[..], &v.chosen[..])),
                )
                .collect();
        let lassos: Vec<Op> = views
            .iter()
            .filter(|(_, _, lassoed, _)| !lassoed.is_empty())
            .map(|(layer, ..)| {
                Op::Lasso(LassoSpec {
                    layer: layer.clone(),
                    frame,
                    polygon: polygon.clone(),
                    cut: true,
                    action: action.clone(),
                })
            })
            .collect();
        let extras: Vec<String> = views
            .iter()
            .flat_map(|(_, cel, lassoed, chosen)| {
                chosen
                    .iter()
                    .filter(|id| !lassoed.contains(id))
                    .map(|id| address(cel, *id))
            })
            .collect();
        let rest = (!extras.is_empty()).then_some(match &action {
            LassoAction::Transform(transform) => Op::Transform {
                ids: extras,
                transform: *transform,
            },
            LassoAction::Remove => Op::Remove { ids: extras },
        });
        let after = match action {
            LassoAction::Remove => None,
            LassoAction::Transform(_) => Some(Picked {
                cel: picked.cel.clone(),
                ids: view.chosen.clone(),
                cut: None,
                more: view
                    .more
                    .iter()
                    .map(|v| Part {
                        layer: v.layer.clone(),
                        cel: v.cel.clone(),
                        ids: v.chosen.clone(),
                    })
                    .collect(),
            }),
        };
        Some((lassos.into_iter().chain(rest).collect(), after))
    }

    fn edit_picked(&mut self, action: LassoAction) -> Result<()> {
        let Some((ops, after)) = self.picked_ops(action) else {
            return Ok(());
        };
        self.apply(ops)?;
        self.picked = after;
        self.forget_missing_selection();
        Ok(())
    }

    fn on_marks(&self) -> bool {
        match self.tool {
            Tool::Strokes => true,
            Tool::Select => self.selection.is_none() && self.picked.is_some(),
            _ => false,
        }
    }

    fn handles(&self) -> Option<(String, [Vec2; 4])> {
        if let Some(found) = self.shift_handles() {
            return Some(found);
        }
        match self.tool {
            Tool::Direct => None,
            _ if self.on_marks() => self.picked_corners(),
            _ => self.selection_corners(),
        }
    }

    fn node_ops(&self, drag: &Drag) -> Result<Option<Vec<Op>>> {
        let ids = match self.shapes() {
            all if all.len() > 1 && all.contains(&drag.node) => all,
            _ => vec![drag.node.clone()],
        };
        let together = ids.len() > 1;
        let affine = drag.affine();
        let ops = ids
            .into_iter()
            .filter_map(|id| match self.scene.bundle.doc.item(&id) {
                Some(Item::Node(_, _, node)) => Some((id, node.clone())),
                _ => None,
            })
            .map(|(id, node)| {
                let t = match (
                    drag.stretch().filter(|_| !together),
                    drag.uneven().filter(|_| !together),
                ) {
                    (Some((axis, _, k)), _) => lift::stretched(&node, affine, axis, k)?,
                    (None, Some((kx, ky))) => lift::scaled(&node, affine, kx, ky)?,
                    (None, None) => lift::committed(&node, affine, drag.scale(), drag.angle().to_degrees())?,
                };
                Ok(Op::Set {
                    id,
                    props: transform_props(&t),
                })
            })
            .collect::<Result<Vec<Op>>>()?;
        Ok((!ops.is_empty()).then_some(ops))
    }

    fn drag_ops(&self) -> Option<Vec<Op>> {
        let drag = self.drag.as_ref().filter(|d| d.moved() && !d.copy)?;
        if self.shifting.is_some() {
            return self.shift_ops(drag, false).ok().flatten();
        }
        match drag.strokes {
            true => self
                .picked_ops(LassoAction::Transform(drag.mark_transform()))
                .map(|(ops, _)| ops),
            false => self.node_ops(drag).ok().flatten(),
        }
    }

    fn preview_cels(&self) -> Vec<String> {
        if self.shifting.is_some() {
            return self.shift_cels();
        }
        match self.drag.as_ref().is_some_and(|d| d.strokes) {
            true => self
                .picked
                .iter()
                .flat_map(|p| std::iter::once(p.cel.clone()).chain(p.more.iter().map(|m| m.cel.clone())))
                .collect(),
            false => Vec::new(),
        }
    }

    fn drop_preview(&mut self) {
        if let Some(mut state) = self.preview.take() {
            self.dirty.extend(state.release());
        }
    }

    fn settle(&mut self, pending: &BTreeSet<usize>, settled: &BTreeSet<usize>, rest: Vec<usize>) {
        self.dirty.retain(|i| !(settled.contains(i) && pending.contains(i)));
        self.dirty.extend(rest.into_iter().filter(|i| !settled.contains(i)));
    }

    fn finish_preview(&mut self, ops: Vec<Op>, dirty: Vec<usize>, before: u64) {
        let Some(mut state) = self.preview.take() else {
            self.dirty.extend(dirty);
            return;
        };
        if state.snap.rev != before {
            self.dirty.extend(dirty.into_iter().chain(state.release()));
            return;
        }
        match state.done.take() {
            Some((done, exact, composed)) if state.job.is_none() && done == ops => {
                let settled = match exact && !self.scene.uses_caches() {
                    true => composed,
                    false => BTreeSet::new(),
                };
                let pending = std::mem::take(&mut state.pending);
                let rest = dirty.into_iter().chain(state.release()).collect();
                self.settle(&pending, &settled, rest);
            }
            Some((_, false, _)) => self.dirty.extend(dirty.into_iter().chain(state.release())),
            done => {
                state.done = done;
                state.finish = Some(Finish {
                    ops,
                    rev: self.scene.bundle.doc.rev,
                    dirty: dirty.into_iter().collect(),
                    since: Instant::now(),
                });
                self.preview = Some(state);
            }
        }
    }

    fn tend_preview(&mut self, visible: BTreeSet<usize>) -> Result<Tiles> {
        if self.drag.is_some() && !matches!(self.tool, Tool::Select | Tool::Strokes) {
            self.drag = None;
        }
        let mut uploads = Vec::new();
        let rev = self.scene.bundle.doc.rev;
        let posing = self.live_pose();
        if let Some(mut state) = self.preview.take() {
            let mut keep = true;
            match state.job.as_ref().and_then(|rx| rx.try_recv().ok()) {
                Some(Ok(done)) => {
                    state.job = None;
                    let composed: BTreeSet<usize> = done.tiles.iter().map(|(i, _)| *i).collect();
                    let earlier = std::mem::replace(
                        &mut state.shown,
                        done.changed.intersection(&composed).copied().collect(),
                    );
                    state.shown.extend(earlier.difference(&composed));
                    let current = state.finish.as_ref().map_or(state.snap.rev, |f| f.rev) == rev;
                    let last = state.finish.as_ref().is_some_and(|f| f.ops == done.ops);
                    if current {
                        uploads = done.tiles;
                    }
                    if !current {
                        keep = false;
                        self.dirty.extend(earlier.into_iter().chain(done.hidden));
                        self.dirty.extend(state.release());
                    } else if last {
                        keep = false;
                        let settled = match done.exact && !self.scene.uses_caches() {
                            true => composed,
                            false => BTreeSet::new(),
                        };
                        let pending = std::mem::take(&mut state.pending);
                        let rest = earlier.into_iter().chain(done.hidden).chain(state.release()).collect();
                        self.settle(&pending, &settled, rest);
                    } else {
                        state.done = Some((done.ops, done.exact, composed));
                    }
                }
                Some(Err(e)) => {
                    keep = false;
                    self.dirty.extend(state.release());
                    self.failed(e.context("the moved picture could not be drawn"));
                }
                None => {}
            }
            if keep {
                let gone = match &state.posed {
                    Some(_) => posing.is_none(),
                    None => self.drag.is_none(),
                };
                let stale = state.finish.is_none() && (gone || state.snap.rev != rev || !self.canvas_current());
                let late = state.finish.as_ref().is_some_and(|f| f.since.elapsed() > FINISH_WAIT);
                match stale || late {
                    true => {
                        uploads.clear();
                        self.dirty.extend(state.release());
                    }
                    false => self.preview = Some(state),
                }
            }
        }
        if self.preview.is_none()
            && self.live.is_none()
            && (posing.is_some() || self.drag.as_ref().is_some_and(|d| d.moved() && !d.copy))
            && self.canvas_current()
            && self.dirty.len() <= timeline::FILL
        {
            let cels = match &posing {
                Some(p) => p.cels.to_vec(),
                None => self.preview_cels(),
            };
            self.preview = self
                .scene
                .preview_snapshot(&cels, &self.brushes)
                .map(|snap| PreviewState {
                    snap: Arc::new(snap),
                    pending: self.dirty.clone(),
                    cancel: Arc::default(),
                    asked: None,
                    asked_at: None,
                    job: None,
                    shown: BTreeSet::new(),
                    done: None,
                    finish: None,
                    posed: None,
                });
        }
        let position = self.drag.as_ref().map(|d| d.now);
        let ready = self
            .preview
            .as_ref()
            .filter(|p| p.job.is_none())
            .map(|p| p.finish.as_ref().map(|f| f.ops.clone()).ok_or(p.asked_at != position));
        let wanted = match (ready, &posing) {
            (Some(Ok(finish)), _) => Some(finish),
            (Some(Err(_)), Some(p)) => match self.preview.as_ref().is_some_and(|s| s.posed.as_ref() == Some(&p.key)) {
                true => None,
                false => Some(p.ops.to_vec()),
            },
            (Some(Err(true)), None) => self.drag_ops(),
            _ => None,
        };

        if let (Some(state), Some(ops)) = (self.preview.as_mut(), wanted) {
            state.asked_at = position;
            if state.asked.as_ref() != Some(&ops) {
                let job = preview::Job {
                    ops: ops.clone(),
                    snap: state.snap.clone(),
                    shown: state.shown.clone(),
                    visible,
                    cancel: state.cancel.clone(),
                };
                state.asked = Some(ops);
                state.posed = posing.as_ref().map(|p| p.key.clone());
                let (tx, rx) = channel();
                let ctx = self.ctx.clone();
                crate::background().spawn(move || {
                    let _ = tx.send(job.run());
                    ctx.request_repaint();
                });
                state.job = Some(rx);
            }
        }
        Ok(uploads)
    }

    fn screen(&self, view: View, rect: Rect, c: Vec2) -> Pos2 {
        rect.min + view.to_screen(c, rect.size())
    }

    fn visible_tiles(&self, view: View, rect: Rect) -> BTreeSet<usize> {
        if self.scene.bundle.doc.canvas.wrap {
            return self.scene.all_tiles().into_iter().collect();
        }
        let size = rect.size();
        let corners = [vec2(0.0, 0.0), vec2(size.x, 0.0), vec2(0.0, size.y), size].map(|p| view.to_canvas(p, size));
        let range = |lo: f32, hi: f32, count: usize| {
            let at = |v: f32| ((v / klad_brush::TILE as f32).floor() as i64).clamp(0, count as i64 - 1) as usize;
            at(lo).saturating_sub(1)..=(at(hi) + 1).min(count - 1)
        };
        let (xs, ys) = (corners.map(|c| c.x), corners.map(|c| c.y));
        let low = |v: [f32; 4]| v.into_iter().fold(f32::MAX, f32::min);
        let high = |v: [f32; 4]| v.into_iter().fold(f32::MIN, f32::max);
        let cols = self.scene.cols();
        range(low(ys), high(ys), self.scene.rows())
            .flat_map(|ty| range(low(xs), high(xs), cols).map(move |tx| ty * cols + tx))
            .collect()
    }

    fn drag_keys(&self) -> Option<String> {
        self.drag.as_ref().filter(|d| d.moved()).map(|d| match d.readout() {
            Some(size) => format!("{size}; {}", d.hint()),
            None => d.hint().to_string(),
        })
    }

    fn edges(&self) -> bool {
        match self.tool {
            _ if self.on_marks() => true,
            Tool::Select if self.group.len() > 1 => false,
            Tool::Select => self
                .selection
                .as_ref()
                .and_then(|id| match self.scene.bundle.doc.item(id) {
                    Some(Item::Node(_, _, node)) => Some(lift::square(node)),
                    _ => None,
                })
                .unwrap_or(false),
            _ => false,
        }
    }

    fn grab_at(&self, view: View, rect: Rect, pos: Pos2) -> Option<(String, Grab, [Vec2; 4])> {
        let (id, corners) = self.handles()?;
        let screen = corners.map(|c| self.screen(view, rect, c));
        grab(screen, pos, self.edges()).map(|g| (id, g, corners))
    }

    fn start_drag(&mut self, id: String, grab: Grab, corners: [Vec2; 4], at: Vec2, fresh: bool) {
        self.drop_preview();
        let strokes = self.on_marks() && self.shifting.is_none();
        if !strokes && self.shifting.is_none() && !matches!(self.scene.bundle.doc.item(&id), Some(Item::Node(..))) {
            return;
        }
        let whole = strokes && self.scene.on_grid();
        let center = corners.iter().fold(Vec2::ZERO, |a, c| a + *c) / 4.0;
        let center = match whole {
            true => vec2(center.x.round(), center.y.round()),
            false => center,
        };
        self.drag = Some(Drag {
            grab,
            node: id,
            strokes,
            whole,
            fresh,
            copy: grab == Grab::Move && self.modifiers.alt,
            start: at,
            now: at,
            center,
            corners,
            slop: DRAG_SLOP / self.view.map_or(1.0, |v| v.zoom),
            crossed: false,
            outside: false,
            shift: self.modifiers.shift,
            alt: self.modifiers.alt && grab != Grab::Move,
            loose: self.modifiers.command,
            free: self.edges() && self.shifting.is_none(),
            toggle: false,
        });
    }

    fn erased(&self) -> impl Fn(&[Mark], &Mark) -> bool + Copy + '_ {
        move |marks: &[Mark], m: &Mark| {
            let eraser = m.as_stroke().is_some_and(|s| {
                self.brushes
                    .get(&s.brush)
                    .is_ok_and(|b| b.blend == klad_brush::brush::StrokeBlend::Erase)
            });
            eraser || !self.wiped(marks, &[m.id()]).is_empty()
        }
    }

    fn wiped(&self, marks: &[Mark], among: &[u32]) -> BTreeSet<u32> {
        let attached = klad_core::stroke::strokes_of(marks).any(|s| s.on.iter().any(|t| among.contains(t)));
        match attached {
            false => BTreeSet::new(),
            true => {
                let (w, h) = self.scene.size();
                klad_brush::scope::hidden(w, h, marks, &self.brushes, Some(among))
            }
        }
    }

    fn layer_rank(&self, id: &str) -> Option<usize> {
        self.scene.bundle.doc.all_layers().iter().position(|l| l.id == id)
    }

    fn shape_hits(&self, at: Vec2) -> Result<Vec<(String, String)>> {
        let active = self.scene.active_id();
        Ok(overlay::hits(&self.scene.bundle.doc, self.scene.frame, (at.x, at.y))?
            .into_iter()
            .filter(|(_, layer)| self.settings.pick_all_layers || active.as_deref() == Some(layer.as_str()))
            .collect())
    }

    pub(super) fn mark_at(&self, at: Vec2, slack: f32) -> Option<(Vec<usize>, u32)> {
        self.scene
            .marks_at((at.x, at.y), slack, self.settings.pick_all_layers, self.erased())
    }

    fn select_at(&mut self, at: Vec2, cycle: bool) -> Result<()> {
        let hits = self.shape_hits(at)?;
        let current = self
            .selection
            .as_ref()
            .and_then(|id| hits.iter().position(|(n, _)| n == id));
        let next = match (current, cycle) {
            (Some(i), true) => hits.get((i + 1) % hits.len().max(1)),
            _ => hits.first(),
        };
        let picked: Vec<String> = next.map(|(n, _)| n.clone()).into_iter().collect();
        self.group.clear();
        self.select_shapes(picked);
        Ok(())
    }

    fn toggle_shape(&mut self, at: Vec2) -> Result<bool> {
        let Some((hit, _)) = self.shape_hits(at)?.into_iter().next() else {
            return Ok(false);
        };
        let held = self.shapes();
        let next: Vec<String> = match held.contains(&hit) {
            true => held.into_iter().filter(|id| *id != hit).collect(),
            false => held.into_iter().chain([hit]).collect(),
        };
        self.picked = None;
        self.select_shapes(next);
        Ok(true)
    }

    fn lasso_shapes(&mut self) -> Result<bool> {
        let vector = self
            .scene
            .active_layer()
            .is_some_and(|l| l.kind() == klad_core::doc::Kind::Vector);
        let Some(polygon) = self
            .lasso
            .as_ref()
            .filter(|l| vector && l.far && l.points.len() >= 3)
            .map(|l| l.points.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>())
        else {
            return Ok(false);
        };
        let active = self.scene.active_id();
        let found: Vec<String> = overlay::shapes_inside(&self.scene.bundle.doc, self.scene.frame, &polygon)?
            .into_iter()
            .filter(|(_, layer)| self.settings.pick_all_layers || active.as_deref() == Some(layer.as_str()))
            .map(|(node, _)| node)
            .collect();
        self.lasso = None;
        self.picked = None;
        if found.is_empty() && self.refuse_locked_in_hand() {
            return Ok(true);
        }
        self.select_shapes(found);
        Ok(true)
    }

    fn finish_drag(&mut self) -> Result<()> {
        let Some(drag) = self.drag.take() else {
            return Ok(());
        };
        if self.shifting.is_some() {
            return self.finish_shift_drag(&drag);
        }
        if drag.strokes {
            return self.finish_stroke_drag(&drag);
        }
        if !drag.moved() {
            if drag.toggle && self.toggle_shape(drag.start)? {
                return Ok(());
            }
            if drag.grab == Grab::Move && !drag.fresh {
                self.select_at(drag.start, true)?;
            }
            return Ok(());
        }
        let Some(ops) = self.node_ops(&drag)? else {
            self.drop_preview();
            return Ok(());
        };
        let before = self.scene.bundle.doc.rev;
        let dirty = self.commit(ops.clone())?;
        self.finish_preview(ops, dirty, before);
        Ok(())
    }

    fn grab_under(&mut self, c: Vec2, pos: Pos2) -> Result<()> {
        let zoom = self.view.map_or(1.0, |v| v.zoom);
        let slack = 5.0 / zoom;
        let on_picked = self.picked.as_ref().is_some_and(|p| !p.more.is_empty())
            && self
                .picked_parts()
                .iter()
                .any(|(_, _, marks, ids)| markedit::hit(marks, (c.x, c.y), slack).is_some_and(|id| ids.contains(&id)));
        if on_picked && let Some((key, corners)) = self.picked_corners() {
            self.selection = None;
            self.start_drag(key, Grab::Move, corners, c, true);
            return Ok(());
        }
        let stroke = self.mark_at(c, slack);
        if stroke.as_ref().is_some_and(|(path, _)| *path == self.scene.active)
            && let Some(why) = self.take_target()
        {
            self.say(Tone::Refused, why);
            return Ok(());
        }
        let shape = self
            .shape_hits(c)?
            .first()
            .and_then(|(_, layer)| self.layer_rank(layer));
        let stroke_first = match (&stroke, shape) {
            (Some((path, _)), Some(shape)) => self
                .scene
                .bundle
                .doc
                .locate_path(path)
                .and_then(|l| self.layer_rank(&l.id))
                .is_some_and(|rank| rank > shape),
            (_, None) => true,
            (None, Some(_)) => false,
        };
        if !stroke_first && self.modifiers.shift && self.selection.is_some() && self.toggle_shape(c)? {
            return Ok(());
        }
        if !stroke_first {
            self.select_at(c, false)?;
            if let Some((id, corners)) = self.selection_corners() {
                self.picked = None;
                self.start_drag(id, Grab::Move, corners, c, true);
                return Ok(());
            }
        }
        self.selection = None;
        let Some((path, id)) = stroke else {
            self.lasso = Some(Lasso {
                points: vec![c],
                start: pos,
                far: false,
            });
            return Ok(());
        };
        if path != self.scene.active {
            self.activate(path)?;
            let name = self.scene.active_layer().map(|l| l.name.clone()).unwrap_or_default();
            self.say(Tone::Info, format!("on layer {name}"));
            self.scene.rebuild_caches(None)?;
            if !self.ensure_canvas()? {
                self.say(Tone::Info, format!("{name} is still loading; press again in a moment"));
                return Ok(());
            }
        }
        let Some(cel) = self.scene.active_cel() else {
            return Ok(());
        };
        let held: Vec<u32> = self
            .picked
            .as_ref()
            .filter(|p| p.cel == cel && p.cut.is_none())
            .map_or(Vec::new(), |p| p.ids.clone());
        let ids = match (self.modifiers.shift, held.contains(&id)) {
            (true, true) => held.into_iter().filter(|x| *x != id).collect(),
            (true, false) => held.into_iter().chain([id]).collect(),
            (false, true) => held,
            (false, false) => vec![id],
        };
        self.picked = (!ids.is_empty()).then(|| Picked::whole(cel, ids));
        if let Some((key, corners)) = self.picked_corners() {
            self.start_drag(key, Grab::Move, corners, c, true);
        }
        Ok(())
    }

    fn copy_for_drag(&mut self) -> Result<()> {
        let Some(drag) = self.drag.as_mut().filter(|d| d.copy && d.moved()) else {
            return Ok(());
        };
        drag.copy = false;
        let strokes = drag.strokes;
        self.drop_preview();
        self.duplicate_in_place()?;
        if !strokes && let (Some(drag), Some(id)) = (self.drag.as_mut(), self.selection.clone()) {
            drag.node = id;
        }
        Ok(())
    }

    fn finish_stroke_drag(&mut self, drag: &Drag) -> Result<()> {
        let Some(picked) = self.picked.clone() else {
            return Ok(());
        };
        if !drag.moved() && drag.fresh {
            return Ok(());
        }
        if !drag.moved() {
            self.lasso = Some(Lasso {
                points: vec![drag.start],
                start: Pos2::ZERO,
                far: false,
            });
            return self.finish_pick();
        }
        let start = Instant::now();
        match self.picked_ops(LassoAction::Transform(drag.mark_transform())) {
            Some((ops, after)) => {
                let before = self.scene.bundle.doc.rev;
                let dirty = self.commit(ops.clone())?;
                self.picked = after;
                self.forget_missing_selection();
                self.finish_preview(ops, dirty, before);
                self.turned_box = self
                    .picked
                    .as_ref()
                    .map(|p| (p.key(), self.scene.bundle.doc.rev, drag.preview()))
                    .filter(|(_, _, corners)| !upright(corners));
            }
            None => self.drop_preview(),
        }
        let text = format!(
            "{} strokes and fills {} and redrawn in {:.0} ms",
            self.picked.as_ref().map_or(0, Picked::count),
            if picked.cut.is_some() { "cut, moved" } else { "moved" },
            ms(start)
        );
        self.measure("strokes", text);
        Ok(())
    }

    fn pick_cels(&self, every: bool) -> Vec<(String, String)> {
        let doc = &self.scene.bundle.doc;
        let active = self.scene.active_id();
        let layers: Vec<&klad_core::doc::Layer> = match every {
            true => doc.all_layers(),
            false => self.also.iter().filter_map(|id| doc.layer(id)).collect(),
        };
        layers
            .into_iter()
            .filter(|l| l.kind() == Kind::Paint && l.visible && !l.locked)
            .filter(|l| !every || active.as_deref() != Some(l.id.as_str()))
            .filter_map(|l| l.content_at(self.scene.frame).map(|c| (l.id.clone(), c.id.clone())))
            .collect()
    }

    fn marquee_here(&self) -> marquee::Marquee {
        let grid = self.scene.on_grid();
        match self.tool == Tool::Strokes || (grid && self.tool == Tool::Select) {
            true => self.marquee,
            false => marquee::Marquee::Loop,
        }
    }

    fn lasso_outline(&self, lasso: &Lasso) -> Vec<Vec2> {
        let grid = self.scene.on_grid();
        match (self.marquee_here(), lasso.points.first(), lasso.points.last()) {
            (marquee::Marquee::Box, Some(a), Some(b)) => {
                let (lo, hi) = (a.min(*b), a.max(*b));
                let (lo, hi) = match grid {
                    true => (lo.floor(), hi.ceil()),
                    false => (lo, hi),
                };
                vec![lo, vec2(hi.x, lo.y), hi, vec2(lo.x, hi.y)]
            }
            (marquee::Marquee::Ellipse, Some(a), Some(b)) => marquee::ellipse(*a, *b, self.modifiers.shift),
            _ => lasso.points.clone(),
        }
    }

    fn finish_pick(&mut self) -> Result<()> {
        let Some(lasso) = self.lasso.take() else {
            return Ok(());
        };
        let zoom = self.view.map_or(1.0, |v| v.zoom);
        let wand = self.marquee_here() == marquee::Marquee::Wand;
        let click = !wand && (!lasso.far || lasso.points.len() < 3);
        let reaches = click || (wand && self.settings.pick_all_layers && !self.modifiers.alt);
        if reaches
            && !self.modifiers.shift
            && let Some((path, _)) = self.mark_at(lasso.points[0], 5.0 / zoom)
            && path != self.scene.active
        {
            self.activate(path)?;
        }
        let every = !click && self.settings.pick_all_layers;
        if every
            && self.scene.active_cel().is_none()
            && let Some((layer, _)) = self.pick_cels(true).into_iter().next()
            && let Some(klad_core::doc::Loc::Layer(path)) = self.scene.bundle.doc.locate(&layer)
        {
            let from = self.scene.active_layer().map(|l| l.name.clone());
            self.activate(path)?;
            if let (Some(from), Some(to)) = (from, self.scene.active_layer().map(|l| l.name.clone())) {
                self.say(
                    Tone::Info,
                    format!("{from} has no strokes on this frame, so the loop picks on {to}"),
                );
            }
        }
        let Some(cel) = self.scene.active_cel() else {
            let why = match self.scene.active_layer() {
                Some(l) if l.kind() == Kind::Paint => format!("{} has no drawing on this frame: K makes one", l.name),
                Some(l) if l.kind() == Kind::Bones => "bones are picked with the bone tool (J)".to_string(),
                _ => "a loop selects strokes of the active layer: pick a paint layer in the layers panel".to_string(),
            };
            self.say(Tone::Refused, why);
            return Ok(());
        };
        let at = lasso.points[0];
        let erased = self.erased();
        let outline = self.lasso_outline(&lasso);
        let polygon: Option<Vec<[f32; 2]>> = match wand {
            true => self.wand_polygon(at),
            false => (lasso.far && outline.len() >= 3).then(|| outline.iter().map(|p| [p.x, p.y]).collect()),
        };
        if wand && polygon.is_none() {
            self.say(
                Tone::Refused,
                match self.settings.pick_all_layers {
                    true => "the wand takes an area of one colour: click on something painted",
                    false => "the wand takes an area of one colour: click on what is painted on this layer",
                },
            );
            return Ok(());
        }
        let pick = |marks: &[Mark]| -> (Vec<u32>, bool) {
            match &polygon {
                Some(polygon) => {
                    let sides = markedit::sides(marks, polygon);
                    let crossing = sides.iter().any(|(_, side)| *side == markedit::Side::Crossing);
                    let mut inside: Vec<u32> = sides
                        .into_iter()
                        .filter(|(_, side)| *side == markedit::Side::Inside)
                        .map(|(id, _)| id)
                        .collect();
                    let wiped = self.wiped(marks, &inside);
                    inside.retain(|id| !wiped.contains(id));
                    (inside, crossing)
                }
                None => (
                    markedit::pick(marks, (at.x, at.y), 5.0 / zoom, |m| erased(marks, m))
                        .into_iter()
                        .collect(),
                    false,
                ),
            }
        };
        let (found, crossing) = match self.take_target() {
            Some(_) => (Vec::new(), false),
            None => pick(self.scene.active_marks()),
        };
        let others: Vec<(String, String, Vec<u32>, bool)> = self
            .pick_cels(every)
            .into_iter()
            .map(|(layer, cel)| {
                let (ids, crossing) = pick(self.scene.marks_of(&cel));
                (layer, cel, ids, crossing)
            })
            .collect();
        let taking = self.modifiers.alt && !click;
        let adding = self.modifiers.shift && !(wand && taking);
        let previous = self.picked.clone().filter(|p| (adding || taking) && p.cel == cel);
        let crossed = crossing || others.iter().any(|o| o.3);
        let cut = match taking {
            true => previous.as_ref().and_then(|p| p.cut.clone()),
            false => polygon
                .clone()
                .filter(|_| (self.cuts() || wand) && crossed)
                .or_else(|| previous.as_ref().and_then(|p| p.cut.clone())),
        };
        let far = lasso.far || wand;
        let merge = |held: Vec<u32>, found: &[u32]| match taking {
            true => held.into_iter().filter(|id| !found.contains(id)).collect(),
            false => found.iter().fold(held, |mut ids, id| {
                if adding && !far && ids.contains(id) {
                    ids.retain(|x| x != id);
                } else if !ids.contains(id) {
                    ids.push(*id);
                }
                ids
            }),
        };
        let ids = merge(previous.as_ref().map_or(Vec::new(), |p| p.ids.clone()), &found);
        let more: Vec<Part> = others
            .into_iter()
            .filter_map(|(layer, cel, found, crossing)| {
                let held = previous
                    .as_ref()
                    .and_then(|p| p.more.iter().find(|q| q.cel == cel))
                    .map_or(Vec::new(), |q| q.ids.clone());
                let ids = merge(held, &found);
                (!ids.is_empty() || (cut.is_some() && crossing)).then_some(Part { layer, cel, ids })
            })
            .collect();
        self.picked = (!ids.is_empty() || cut.is_some() || !more.is_empty()).then_some(Picked { cel, ids, cut, more });
        if self.picked.is_none() {
            if let Some(why) = self.take_target() {
                self.say(Tone::Refused, why);
                return Ok(());
            }
            if self.refuse_locked_in_hand() {
                return Ok(());
            }
        }
        if self.picked.is_none() && polygon.is_some() && crossed {
            let shape = self.marquee_here().label();
            self.say(
                Tone::Info,
                format!("the {shape} holds no whole stroke: tick \"a box or loop cuts strokes at its edge\" in the bar to take the parts inside"),
            );
        }
        Ok(())
    }

    fn shape_of(&self, id: &str) -> Option<Shaped> {
        let rev = self.scene.bundle.doc.rev;
        if let Some(subs) = self.stroke_path(id) {
            return Some(Shaped {
                node: id.to_string(),
                rev,
                subs,
                matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            });
        }
        let doc = &self.scene.bundle.doc;
        match doc.item(id) {
            Some(Item::Node(_, _, node)) => pathedit::from_shape(&node.shape)
                .ok()
                .zip(overlay::local_to_canvas(doc, id).ok().flatten())
                .map(|(subs, matrix)| Shaped {
                    node: id.to_string(),
                    rev,
                    subs,
                    matrix,
                }),
            _ => None,
        }
    }

    fn refresh_shape(&mut self) {
        let rev = self.scene.bundle.doc.rev;
        let Some(direct) = self.direct.as_ref() else {
            self.shaped = None;
            self.also_shaped.clear();
            return;
        };
        let (id, also): (String, Vec<String>) = (direct.node.clone(), direct.also.keys().cloned().collect());
        if !self.shaped.as_ref().is_some_and(|s| s.node == id && s.rev == rev) {
            self.shaped = self.shape_of(&id);
        }
        let kept = std::mem::take(&mut self.also_shaped);
        self.also_shaped = also
            .iter()
            .filter_map(|id| {
                kept.iter()
                    .find(|s| s.node == *id && s.rev == rev)
                    .cloned()
                    .or_else(|| self.shape_of(id))
            })
            .collect();
        let alive: Vec<&String> = self.also_shaped.iter().map(|s| &s.node).collect();
        if let Some(d) = self.direct.as_mut() {
            d.also.retain(|id, points| alive.contains(&id) && !points.is_empty());
        }
    }

    fn point_edit(&self, drag: &PointDrag) -> Option<PathEdit> {
        let shaped = self.shaped.as_ref()?;
        let direct = self.direct.as_ref()?;
        let (a, b) = (to_local(shaped.matrix, drag.start), to_local(shaped.matrix, drag.now));
        match drag.grab {
            PointGrab::Anchors => Some(PathEdit::Move {
                points: direct.points.iter().copied().collect(),
                by: [b[0] - a[0], b[1] - a[1]],
            }),
            PointGrab::Handle(point, side) => Some(PathEdit::Handle {
                point,
                side,
                to: b,
                free: drag.free,
            }),
            PointGrab::Marquee { .. } => None,
        }
    }

    fn spread(&self, edit: &PathEdit) -> Vec<(Shaped, PathEdit)> {
        let (Some(direct), Some(first)) = (self.direct.as_ref(), self.shaped.as_ref()) else {
            return Vec::new();
        };
        let all = |points: &[usize]| points.iter().copied().collect::<BTreeSet<usize>>() == direct.points;
        self.also_shaped
            .iter()
            .filter_map(|shaped| {
                let points: Vec<usize> = direct.also.get(&shaped.node)?.iter().copied().collect();
                let theirs = match edit {
                    PathEdit::Move { points: of, by } if all(of) => {
                        let moved = to_canvas(first.matrix, *by) - to_canvas(first.matrix, [0.0, 0.0]);
                        let by = to_local(shaped.matrix, to_canvas(shaped.matrix, [0.0, 0.0]) + moved);
                        PathEdit::Move { points, by }
                    }
                    PathEdit::Remove { points: of } if all(of) => PathEdit::Remove { points },
                    PathEdit::Corner { points: of } if all(of) => PathEdit::Corner { points },
                    PathEdit::Smooth { points: of } if all(of) => PathEdit::Smooth { points },
                    _ => return None,
                };
                Some((shaped.clone(), theirs))
            })
            .collect()
    }

    fn shown_subs(&self) -> Option<Vec<Subpath>> {
        if let Some(commit) = self.path_commit.as_ref() {
            return Some(commit.preview.clone());
        }
        let mut subs = self.shaped.as_ref()?.subs.clone();
        if let Some(edit) = self.point_drag.as_ref().and_then(|d| self.point_edit(d)) {
            let _ = edit.apply(&mut subs);
        }
        Some(subs)
    }

    fn shown_also(&self) -> Vec<Shaped> {
        if let Some(commit) = self.path_commit.as_ref() {
            return commit.also.clone();
        }
        let moved = self
            .point_drag
            .as_ref()
            .and_then(|d| self.point_edit(d))
            .map_or(Vec::new(), |edit| self.spread(&edit));
        self.also_shaped
            .iter()
            .map(|shaped| {
                let mut shown = shaped.clone();
                if let Some((_, edit)) = moved.iter().find(|(s, _)| s.node == shaped.node) {
                    let _ = edit.apply(&mut shown.subs);
                }
                shown
            })
            .collect()
    }

    fn promote(&mut self, view: View, rect: Rect, pos: Pos2) {
        let Some(direct) = self.direct.as_ref() else {
            return;
        };
        let found = self.also_shaped.iter().find_map(|shaped| {
            let chosen = direct.also.get(&shaped.node)?;
            pathedit::anchors(&shaped.subs)
                .iter()
                .enumerate()
                .any(|(i, (_, a))| {
                    chosen.contains(&i)
                        && self.screen(view, rect, to_canvas(shaped.matrix, a.at)).distance(pos) <= HANDLE
                })
                .then(|| shaped.node.clone())
        });
        let (Some(id), Some(d)) = (found, self.direct.as_mut()) else {
            return;
        };
        let points = d.also.remove(&id).unwrap_or_default();
        let before = (
            std::mem::replace(&mut d.node, id),
            std::mem::replace(&mut d.points, points),
        );
        d.also.insert(before.0, before.1);
        self.refresh_shape();
    }

    fn direct_down(&mut self, view: View, rect: Rect, pos: Pos2, c: Vec2) -> Result<()> {
        let (alt, shift) = (self.modifiers.alt, self.modifiers.shift);
        self.pick_later = false;
        self.promote(view, rect, pos);
        enum Action {
            Handle(usize, Side),
            Anchor(usize, bool),
            Insert(usize, f32),
            Elsewhere,
        }
        let action = match self.shaped.as_ref() {
            Some(shaped) => {
                let m = shaped.matrix;
                let screen = |p: [f32; 2]| self.screen(view, rect, to_canvas(m, p));
                let anchors = pathedit::anchors(&shaped.subs);
                let selected: Vec<usize> = self
                    .direct
                    .as_ref()
                    .map_or(Vec::new(), |d| d.points.iter().copied().collect());
                let handle = selected.iter().find_map(|&i| {
                    let a = anchors.get(i)?.1;
                    [(Side::In, a.inn, a.has_in()), (Side::Out, a.out, a.has_out())]
                        .into_iter()
                        .find(|(_, p, has)| *has && screen(*p).distance(pos) <= HANDLE)
                        .map(|(side, _, _)| (i, side))
                });
                let anchor = anchors.iter().position(|(_, a)| screen(a.at).distance(pos) <= HANDLE);
                let scale = (m[0] * m[3] - m[1] * m[2]).abs().sqrt() * view.zoom;
                let near = pathedit::nearest(&shaped.subs, to_local(m, c)).filter(|(_, _, d)| d * scale <= 8.0);
                match (handle, anchor, near) {
                    (Some((i, side)), _, _) => Action::Handle(i, side),
                    (None, Some(i), _) => Action::Anchor(i, anchors[i].1.has_in() || anchors[i].1.has_out()),
                    (None, None, Some((segment, t, _))) if alt => Action::Insert(segment, t),
                    _ => Action::Elsewhere,
                }
            }
            None => Action::Elsewhere,
        };
        match action {
            Action::Handle(i, side) => {
                self.point_drag = Some(PointDrag {
                    grab: PointGrab::Handle(i, side),
                    start: c,
                    now: c,
                    free: alt,
                });
            }
            Action::Anchor(i, curved) if alt => {
                let points = vec![i];
                return self.commit_path(if curved {
                    PathEdit::Corner { points }
                } else {
                    PathEdit::Smooth { points }
                });
            }
            Action::Anchor(i, _) => {
                if let Some(d) = self.direct.as_mut() {
                    if shift {
                        if !d.points.remove(&i) {
                            d.points.insert(i);
                        }
                    } else if !d.points.contains(&i) {
                        d.points = BTreeSet::from([i]);
                        d.also.clear();
                    }
                    if d.points.contains(&i) {
                        self.point_drag = Some(PointDrag {
                            grab: PointGrab::Anchors,
                            start: c,
                            now: c,
                            free: false,
                        });
                    }
                }
            }
            Action::Insert(segment, t) => return self.commit_path(PathEdit::Insert { segment, t }),
            Action::Elsewhere => {
                let on_shape = match shift || (self.direct.is_some() && !self.on_picked_shape(c)?) {
                    true => {
                        self.pick_later = true;
                        Some(false)
                    }
                    false => self.pick_elsewhere(c)?,
                };
                if let Some(on_shape) = on_shape {
                    self.point_drag = Some(PointDrag {
                        grab: PointGrab::Marquee { on_shape },
                        start: c,
                        now: c,
                        free: false,
                    });
                }
            }
        }
        Ok(())
    }

    fn on_picked_shape(&self, c: Vec2) -> Result<bool> {
        let Some(current) = self.direct.as_ref().map(|d| &d.node) else {
            return Ok(false);
        };
        let under = overlay::leaf_hits(&self.scene.bundle.doc, self.scene.frame, (c.x, c.y))?;
        if !under.is_empty() {
            return Ok(under.contains(current));
        }
        let zoom = self.view.map_or(1.0, |v| v.zoom);
        Ok(self.mark_at(c, 5.0 / zoom).is_some_and(|(path, mark)| {
            path == self.scene.active
                && self
                    .scene
                    .active_cel()
                    .is_some_and(|cel| markedit::address(&cel, mark) == *current)
        }))
    }

    fn refuse_locked_in_hand(&mut self) -> bool {
        let Some(name) = self.scene.active_layer().filter(|l| l.locked).map(|l| l.name.clone()) else {
            return false;
        };
        self.say(Tone::Refused, lock::Locked(vec![name]).to_string());
        true
    }

    fn pick_elsewhere(&mut self, c: Vec2) -> Result<Option<bool>> {
        self.under = overlay::leaf_hits(&self.scene.bundle.doc, self.scene.frame, (c.x, c.y))?;
        let zoom = self.view.map_or(1.0, |v| v.zoom);
        if self.under.is_empty() && self.mark_at(c, 5.0 / zoom).is_none() && self.refuse_locked_in_hand() {
            return Ok(None);
        }
        let mut on_stroke = false;
        if self.under.is_empty()
            && let Some((path, mark)) = self.mark_at(c, 5.0 / zoom)
        {
            if path != self.scene.active {
                self.activate(path)?;
            }
            let picked = self
                .scene
                .active_marks()
                .iter()
                .find(|m| m.id() == mark)
                .map(|m| m.as_stroke().is_some());
            match (picked, self.scene.active_cel()) {
                (Some(true), Some(cel)) => {
                    let address = markedit::address(&cel, mark);
                    if self.direct.as_ref().map(|d| &d.node) != Some(&address) {
                        self.pick_shape(address);
                        return Ok(None);
                    }
                    on_stroke = true;
                }
                (Some(false), _) if self.scene.on_grid() => self.say(
                    Tone::Info,
                    "pixels have no points to reshape: S selects them to move; A reshapes strokes and vector shapes",
                ),
                (Some(false), _) => self.say(Tone::Info, "that is a fill: A reshapes strokes and vector shapes"),
                _ => {}
            }
        }
        let current = self.direct.as_ref().map(|d| d.node.clone());
        let hit = match &current {
            Some(id) if self.under.contains(id) => Some(id.clone()),
            _ => self.under.first().cloned(),
        };
        match hit {
            Some(id) if Some(&id) != current.as_ref() => {
                self.pick_shape(id);
                Ok(None)
            }
            hit => Ok(Some(hit.is_some() || on_stroke)),
        }
    }

    fn pen_down(&mut self, view: View, rect: Rect, pos: Pos2, c: Vec2) -> Result<()> {
        let first = self
            .pen_path
            .as_ref()
            .filter(|p| p.anchors.len() >= 2)
            .and_then(|p| p.anchors.first())
            .map(|a| self.screen(view, rect, vec2(a.at[0], a.at[1])));
        if first.is_some_and(|f| f.distance(pos) <= HANDLE + 2.0) {
            return self.commit_pen(true);
        }
        let path = self.pen_path.get_or_insert_with(|| PenPath {
            anchors: Vec::new(),
            dragging: false,
        });
        path.anchors.push(pathedit::Anchor::corner([c.x, c.y]));
        path.dragging = true;
        Ok(())
    }

    fn pen_move(&mut self, c: Vec2) {
        let zoom = self.view.map_or(1.0, |v| v.zoom);
        let Some(last) = self
            .pen_path
            .as_mut()
            .filter(|p| p.dragging)
            .and_then(|p| p.anchors.last_mut())
        else {
            return;
        };
        let pull = [c.x - last.at[0], c.y - last.at[1]];
        let (out, inn) = match pull[0].hypot(pull[1]) * zoom > 3.0 {
            true => ([c.x, c.y], [last.at[0] - pull[0], last.at[1] - pull[1]]),
            false => (last.at, last.at),
        };
        last.out = out;
        last.inn = inn;
    }

    fn pen_keys(&mut self, ui: &mut egui::Ui) -> Result<()> {
        if self.pen_path.is_none() {
            return Ok(());
        }
        if self.tool != Tool::Pen {
            return self.commit_pen(false);
        }
        if self.keys_busy {
            return Ok(());
        }
        let (finish, cancel) = ui.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, Key::Enter),
                i.consume_key(egui::Modifiers::NONE, Key::Escape),
            )
        });
        if cancel {
            self.pen_path = None;
            self.say(Tone::Info, "path dropped");
        }
        if finish {
            self.commit_pen(false)?;
        }
        Ok(())
    }

    fn commit_pen(&mut self, closed: bool) -> Result<()> {
        let Some(path) = self.pen_path.take() else {
            return Ok(());
        };
        if path.anchors.len() < 2 {
            self.say(Tone::Info, "a path needs two points: click again before Return");
            return Ok(());
        }
        let home = self.vector_home("paths");
        let name = match &home.made {
            Some(_) => home.said(),
            None => self
                .scene
                .active_layer()
                .map_or_else(String::new, |l| format!(" to {}", l.name)),
        };
        let d = pathedit::to_d(&[Subpath {
            anchors: path.anchors,
            closed,
        }]);
        let (colour, outline) = (color::hex(self.color), color::hex(self.second));
        let fill = (closed && self.pen_fill).then(|| colour.clone());
        let mut node = json!({ "type": "path", "d": d });
        if let Some(fill) = &fill {
            node["fill"] = json!(fill);
        }
        if self.pen_stroke || fill.is_none() {
            node["stroke"] = json!({ "paint": outline, "width": self.pen_width, "cap": "round", "join": "round" });
        }
        let place = home.place(self.scene.frame);
        let ops = home.ops.iter().cloned().chain([Op::AddNode { node, place }]).collect();
        self.apply(ops)?;
        self.move_in(&home)?;
        self.used_colour();
        self.say(
            Tone::Done,
            format!(
                "{} path added{name}: V moves it, A edits its points",
                if closed { "closed" } else { "open" }
            ),
        );
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn decorate_pen(
        &self,
        painter: &egui::Painter,
        path: &PenPath,
        hover: Option<Pos2>,
        view: View,
        rect: Rect,
        accent: Color32,
        dark: egui::Stroke,
    ) {
        let to_screen = |p: [f32; 2]| self.screen(view, rect, vec2(p[0], p[1]));
        let mut anchors = path.anchors.clone();
        if let Some(hover) = hover.filter(|h| !path.dragging && rect.contains(*h)) {
            let c = view.to_canvas(hover - rect.min, rect.size());
            anchors.push(pathedit::Anchor::corner([c.x, c.y]));
        }
        let sub = Subpath { anchors, closed: false };
        pathedit::flatten(&[sub], 2.0 / view.zoom.max(1e-3))
            .into_iter()
            .for_each(|line| {
                let points: Vec<Pos2> = line.into_iter().map(to_screen).collect();
                painter.add(egui::Shape::line(points.clone(), dark));
                painter.add(egui::Shape::line(points, egui::Stroke::new(1.5, accent)));
            });
        if let Some(last) = path.anchors.last().filter(|a| a.has_out()) {
            [last.inn, last.out].into_iter().for_each(|h| {
                painter.line_segment([to_screen(last.at), to_screen(h)], egui::Stroke::new(1.0, accent));
                painter.circle_filled(to_screen(h), 3.5, accent);
            });
        }
        path.anchors.iter().enumerate().for_each(|(i, a)| {
            let p = to_screen(a.at);
            let size = if i == 0 && path.anchors.len() >= 2 {
                HANDLE + 3.0
            } else {
                HANDLE
            };
            let square = Rect::from_center_size(p, vec2(size, size));
            painter.rect_filled(square, 1.0, Color32::WHITE);
            painter.rect_stroke(square, 1.0, egui::Stroke::new(1.0, accent), egui::StrokeKind::Middle);
        });
    }

    fn pick_shape(&mut self, id: String) {
        self.direct = Some(Direct {
            node: id,
            points: BTreeSet::new(),
            also: BTreeMap::new(),
        });
        self.refresh_shape();
    }

    fn direct_up(&mut self) -> Result<()> {
        let Some(drag) = self.point_drag.take() else {
            return Ok(());
        };
        let zoom = self.view.map_or(1.0, |v| v.zoom);
        let moved = (drag.now - drag.start).length() * zoom > 2.0;
        match drag.grab {
            PointGrab::Marquee { on_shape } => {
                let later = std::mem::take(&mut self.pick_later);
                if !moved && later {
                    if self.pick_elsewhere(drag.start)? == Some(false) {
                        self.direct = None;
                    }
                    return Ok(());
                }
                if !moved {
                    let current = self.direct.as_ref().map(|d| d.node.clone());
                    let below = current
                        .and_then(|id| self.under.iter().position(|u| *u == id))
                        .filter(|_| self.under.len() > 1)
                        .map(|i| self.under[(i + 1) % self.under.len()].clone());
                    match (on_shape, below) {
                        (true, Some(id)) => self.pick_shape(id),
                        (true, None) => {
                            if let Some(d) = self.direct.as_mut() {
                                d.points.clear();
                                d.also.clear();
                            }
                        }
                        (false, _) => self.direct = None,
                    }
                    return Ok(());
                }
                let (lo, hi) = (drag.start.min(drag.now), drag.start.max(drag.now));
                let boxed = |shaped: &Shaped| -> BTreeSet<usize> {
                    pathedit::anchors(&shaped.subs)
                        .iter()
                        .enumerate()
                        .filter(|(_, (_, a))| {
                            let p = to_canvas(shaped.matrix, a.at);
                            p.x >= lo.x && p.x <= hi.x && p.y >= lo.y && p.y <= hi.y
                        })
                        .map(|(i, _)| i)
                        .collect()
                };
                if self.modifiers.shift {
                    let found: Vec<(String, BTreeSet<usize>)> = self
                        .layer_shapes([lo.x, lo.y, hi.x, hi.y])
                        .iter()
                        .filter_map(|id| self.shape_of(id))
                        .map(|shaped| (boxed(&shaped), shaped.node))
                        .filter(|(points, _)| !points.is_empty())
                        .map(|(points, id)| (id, points))
                        .collect();
                    if self.direct.is_none()
                        && let Some((id, _)) = found.first()
                    {
                        self.pick_shape(id.clone());
                    }
                    if let Some(d) = self.direct.as_mut() {
                        found.into_iter().for_each(|(id, points)| match id == d.node {
                            true => d.points.extend(points),
                            false => d.also.entry(id).or_default().extend(points),
                        });
                    }
                    self.refresh_shape();
                    return Ok(());
                }
                let inside = self.shaped.as_ref().map(boxed);
                if let (Some(d), Some(inside)) = (self.direct.as_mut(), inside) {
                    d.points = inside;
                    d.also.clear();
                }
                Ok(())
            }
            _ if !moved => Ok(()),
            _ => match self.point_edit(&drag) {
                Some(edit) => self.commit_path(edit),
                None => Ok(()),
            },
        }
    }

    fn stroke_path(&self, id: &str) -> Option<Vec<Subpath>> {
        let (cel, mark) = markedit::parse_address(id)?;
        if self.scene.active_cel().as_deref() != Some(cel) {
            return None;
        }
        let stroke = self
            .scene
            .active_marks()
            .iter()
            .find(|m| m.id() == mark)
            .and_then(|m| m.as_stroke())?;
        let points: Vec<[f32; 2]> = stroke
            .samples
            .iter()
            .map(|s| {
                let (x, y) = s.pos();
                [x, y]
            })
            .collect();
        Some(vec![pathedit::fit_polyline(
            &points,
            (stroke.size * 0.12).clamp(1.0, 6.0),
        )])
    }

    fn reshape_stroke(&mut self, id: String, edit: PathEdit, also: Vec<(Shaped, PathEdit)>) -> Result<()> {
        let Some(shaped) = self.shaped.as_ref().filter(|s| s.node == id) else {
            return Ok(());
        };
        let mut subs = shaped.subs.clone();
        if let Err(e) = edit.apply(&mut subs) {
            self.say(Tone::Refused, format!("{e:#}"));
            return Ok(());
        }
        let points = |subs: &[Subpath]| subs.iter().map(|s| s.anchors.len()).sum::<usize>();
        if points(&subs) != points(&shaped.subs)
            && let Some(d) = self.direct.as_mut()
        {
            d.points.clear();
        }
        let removing = matches!(edit, PathEdit::Remove { .. });
        let bent: Vec<Shaped> = also
            .into_iter()
            .filter_map(|(mut shaped, edit)| edit.apply(&mut shaped.subs).is_ok().then_some(shaped))
            .collect();
        let set = |id: &str, subs: &[Subpath]| Op::Set {
            id: id.to_string(),
            props: [("path".to_string(), json!(pathedit::to_d(subs)))]
                .into_iter()
                .collect(),
        };
        let ops = std::iter::once(set(&id, &subs))
            .chain(bent.iter().map(|shaped| set(&shaped.node, &shaped.subs)))
            .collect();
        self.apply(ops)?;
        self.first.reshaped = true;
        let rev = self.scene.bundle.doc.rev;
        self.shaped = Some(Shaped {
            node: id,
            rev,
            subs,
            matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        });
        self.also_shaped = std::mem::take(&mut self.also_shaped)
            .into_iter()
            .map(|shaped| {
                let now = bent.iter().find(|b| b.node == shaped.node).cloned().unwrap_or(shaped);
                Shaped { rev, ..now }
            })
            .collect();
        if removing && let Some(d) = self.direct.as_mut() {
            d.also.clear();
        }
        Ok(())
    }

    fn commit_path(&mut self, edit: PathEdit) -> Result<()> {
        let Some(id) = self.direct.as_ref().map(|d| d.node.clone()) else {
            return Ok(());
        };
        if self.path_commit.is_some() {
            return Ok(());
        }
        let also = self.spread(&edit);
        let removing = matches!(edit, PathEdit::Remove { .. });
        if markedit::parse_address(&id).is_some() {
            return self.reshape_stroke(id, edit, also);
        }
        let doc = &self.scene.bundle.doc;
        let Some(Loc::Node(path, c, _)) = doc.locate(&id) else {
            return Ok(());
        };
        let layer = doc.layer_at(&path);
        if layer.locked {
            self.say(
                Tone::Refused,
                format!(
                    "{} is locked: unlock it with the lock in the layers panel to change it",
                    layer.name
                ),
            );
            return Ok(());
        }
        let cel = layer.cels()[c].id.clone();
        let edits: Vec<Op> = std::iter::once(Op::Path { id: id.clone(), edit })
            .chain(also.iter().map(|(shaped, edit)| Op::Path {
                id: shaped.node.clone(),
                edit: edit.clone(),
            }))
            .collect();
        let ops = vector::pinned(doc, &edits)?;
        let (next, _) = apply_all(doc, &ops)?;
        let nodes = match next.locate(&cel) {
            Some(Loc::Cel(p, c)) => next.layer_at(&p).cels()[c].nodes.clone(),
            _ => Vec::new(),
        };
        let preview = match next.item(&id) {
            Some(Item::Node(_, _, n)) => pathedit::from_shape(&n.shape).unwrap_or_default(),
            _ => Vec::new(),
        };
        let count = |subs: &[Subpath]| subs.iter().map(|s| s.anchors.len()).sum::<usize>();
        if Some(count(&preview)) != self.shaped.as_ref().map(|s| count(&s.subs))
            && let Some(d) = self.direct.as_mut()
        {
            d.points.clear();
        }
        let also = self
            .also_shaped
            .iter()
            .map(|shaped| Shaped {
                subs: match next.item(&shaped.node) {
                    Some(Item::Node(_, _, n)) => pathedit::from_shape(&n.shape).unwrap_or_default(),
                    _ => Vec::new(),
                },
                ..shaped.clone()
            })
            .collect();
        if removing && let Some(d) = self.direct.as_mut() {
            d.also.clear();
        }
        let job = lift::render(nodes, self.scene.size(), self.ctx.clone());
        self.path_commit = Some(PathCommit {
            job,
            ops,
            cel,
            preview,
            also,
            node: id,
        });
        Ok(())
    }

    fn tend_path(&mut self) -> Result<()> {
        let Some(result) = self.path_commit.as_ref().and_then(|p| p.job.try_recv().ok()) else {
            return Ok(());
        };
        let Some(commit) = self.path_commit.take() else {
            return Ok(());
        };
        let start = Instant::now();
        let stale = self.scene.bundle.changed_on_disk();
        self.follow_the_folder()?;
        let dirty = match result {
            Ok((hash, raster)) if !stale => {
                self.scene
                    .apply_rendered(&commit.ops, (commit.cel, hash, raster), &self.brushes)?
            }
            Ok(_) => self.scene.apply(&commit.ops, &self.brushes)?,
            Err(e) => {
                self.failed(e.context("the edited path could not be drawn"));
                self.scene.apply(&commit.ops, &self.brushes)?
            }
        };
        self.dirty.extend(dirty);
        self.measure(
            "point edit",
            format!("point edit on {} written in {:.0} ms", commit.node, ms(start)),
        );
        self.forget_missing_selection();
        Ok(())
    }

    fn on_pen(&mut self, ctx: &egui::Context, e: PenEvent, rect: Rect, was_focused: bool) -> Result<()> {
        self.pen_down = match e.phase {
            Phase::Down => Some(e.pos),
            Phase::Move => self.pen_down.map(|_| e.pos),
            Phase::Up => None,
        };
        self.readout.pressure = e.pressure;
        self.readout.tilt = e.tilt;
        self.readout.eraser = e.eraser;
        self.readout.tablet = e.tablet;
        if e.tablet {
            self.readout.device = e.device;
            if e.phase != Phase::Up && self.settings.note_peak(&device_key(e.device), e.pressure) {
                self.settings_dirty = true;
            }
        }
        if e.phase == Phase::Move {
            self.readout.samples.push_back(e.at);
            while self
                .readout
                .samples
                .front()
                .is_some_and(|t| e.at.duration_since(*t) > Duration::from_secs(1))
            {
                self.readout.samples.pop_front();
            }
        }
        let Some(view) = self.view else {
            return Ok(());
        };
        let c = view.to_canvas(e.pos - rect.min, rect.size());
        let on_canvas = rect.contains(e.pos)
            && !self.held_messages.iter().any(|m| m.contains(e.pos))
            && ctx
                .layer_id_at(e.pos)
                .is_none_or(|l| l.order == egui::Order::Background);
        if self.cropping.is_some() {
            self.crop_pen(e.phase, c, on_canvas);
            return Ok(());
        }
        if self.guide_pen(&e, view, rect, c, on_canvas)? {
            return Ok(());
        }
        if self.assist_pen(&e, view, rect, c)? {
            return Ok(());
        }
        let c = self.snapped(&e, c);
        let picking = self.modifiers.alt && !self.modifiers.command && matches!(self.tool, Tool::Brush | Tool::Fill);
        let sizing_now = self.modifiers.alt && self.modifiers.command && self.tool == Tool::Brush;
        match (e.phase, self.sizing) {
            (Phase::Down, None) if sizing_now && on_canvas && self.live.is_none() => {
                self.sizing = Some((e.pos.x, self.size, e.pos));
                return Ok(());
            }
            (Phase::Move, Some((x, start, _))) => {
                self.size = options::size_from_drag(start, e.pos.x - x);
                return Ok(());
            }
            (Phase::Up, Some(_)) => {
                self.sizing = None;
                return Ok(());
            }
            _ => {}
        }
        let pressing = e.phase == Phase::Down && was_focused && on_canvas && !self.navigating;
        if pressing {
            self.focus = Focus::Canvas;
        }
        if pressing && self.holding_frame_keys() {
            return Ok(());
        }
        if pressing && !picking && matches!(self.tool, Tool::Brush | Tool::Fill) {
            let erasing = self.tool == Tool::Brush && (e.eraser || self.is_on(Cmd::Eraser) == Some(true));
            match self.paint_target(erasing) {
                Target::Here => {}
                Target::NewLayer => {
                    let under = self.scene.active_layer().map(|l| l.name.clone());
                    self.add_layer("paint")?;
                    self.scene.rebuild_caches(None)?;
                    self.say(
                        Tone::Done,
                        match under {
                            Some(name) => format!("on a new paint layer above {name}"),
                            None => "on a new paint layer".to_string(),
                        },
                    );
                }
                Target::Refused(why) => {
                    self.say(Tone::Refused, why);
                    return Ok(());
                }
            }
        }
        let this_layer = match self.tool {
            Tool::Strokes | Tool::Direct => true,
            Tool::Select => !self.settings.pick_all_layers,
            _ => false,
        };
        if pressing
            && this_layer
            && matches!(self.tool, Tool::Select | Tool::Direct)
            && self.scene.active_layer().is_some_and(|l| l.kind() == Kind::Bones)
        {
            self.say(Tone::Refused, "bones are picked and moved with the bone tool (J)");
            return Ok(());
        }
        if pressing
            && this_layer
            && let Some(why) = self.take_target()
        {
            self.say(Tone::Refused, why);
            return Ok(());
        }
        let needs_canvas = matches!(self.tool, Tool::Brush | Tool::Fill | Tool::Strokes | Tool::Select);
        if pressing && !needs_canvas {
            self.hold_still();
        }
        if pressing && needs_canvas && !self.ensure_canvas()? {
            self.say(
                Tone::Info,
                format!("frame {} is still loading; draw again in a moment", self.scene.frame),
            );
            return Ok(());
        }
        match (self.tool, e.phase) {
            (_, Phase::Down) if !was_focused || !on_canvas || self.navigating => {}
            (_, Phase::Down) if picking => self.pick_colour(c, self.modifiers.shift)?,
            (Tool::Fill, Phase::Down) if self.scene.on_grid() => {
                if self.scene.can_paint() {
                    self.fill_pixels(klad_core::pixel::cell_at(c.x, c.y))?;
                }
            }
            (Tool::Fill, Phase::Move | Phase::Up) if self.scene.on_grid() => {}
            (Tool::Brush, Phase::Down) if self.scene.on_grid() => {
                if self.pixel_live.is_none() && self.scene.can_paint() {
                    let at = klad_core::pixel::cell_at(c.x, c.y);
                    let run = match self.pixel_last.filter(|_| self.modifiers.shift) {
                        Some(last) => klad_core::pixel::line(last, at),
                        None => vec![at],
                    };
                    let erase = e.eraser || self.brush == "eraser";
                    self.pixel_live = Some(PixelLive { run, erase });
                }
            }
            (Tool::Brush, Phase::Move) if self.pixel_live.is_some() => {
                if let Some(live) = self.pixel_live.as_mut() {
                    klad_core::pixel::extend(&mut live.run, klad_core::pixel::cell_at(c.x, c.y));
                }
            }
            (Tool::Brush, Phase::Up) if self.pixel_live.is_some() => self.finish_pixels()?,
            (Tool::Fill, Phase::Down) => {
                if self.scene.can_paint() {
                    self.lasso = Some(Lasso {
                        points: vec![c],
                        start: e.pos,
                        far: false,
                    });
                }
            }
            (Tool::Strokes, Phase::Down) => match self.grab_at(view, rect, e.pos) {
                Some((id, grab, corners)) if !(self.modifiers.alt && grab == Grab::Move) => {
                    self.start_drag(id, grab, corners, c, false)
                }
                _ => {
                    self.lasso = Some(Lasso {
                        points: vec![c],
                        start: e.pos,
                        far: false,
                    });
                }
            },
            (Tool::Strokes, Phase::Up) if self.drag.is_some() => self.finish_drag()?,
            (Tool::Strokes, Phase::Up) => self.finish_pick()?,
            (Tool::Strokes, Phase::Move) if self.drag.is_some() => {
                if let Some(drag) = self.drag.as_mut() {
                    drag.to(c);
                }
                self.copy_for_drag()?;
            }
            (Tool::Direct, Phase::Down) if self.path_commit.is_some() => {}
            (Tool::Direct, Phase::Down) => self.direct_down(view, rect, e.pos, c)?,
            (Tool::Direct, Phase::Move) => {
                if let Some(drag) = self.point_drag.as_mut() {
                    drag.now = c;
                    drag.free |= self.modifiers.alt;
                }
            }
            (Tool::Direct, Phase::Up) => self.direct_up()?,
            (Tool::Fill | Tool::Strokes | Tool::Select, Phase::Move) if self.drag.is_none() => {
                if let Some(lasso) = self.lasso.as_mut() {
                    lasso.far |= (e.pos - lasso.start).length() > 6.0;
                    if lasso.points.last().is_none_or(|p| (*p - c).length() >= 1.0) {
                        lasso.points.push(c);
                    }
                }
            }
            (Tool::Fill, Phase::Up) => self.finish_fill()?,
            (Tool::Brush, Phase::Down) => {
                if self.live.is_none() && self.scene.can_paint() {
                    self.begin(&e, c)?;
                }
            }
            (Tool::Brush, Phase::Move) if self.live.as_ref().is_some_and(|l| l.straight.is_some()) => {
                let m = self.modifiers;
                if let Some(s) = self.live.as_mut().and_then(|l| l.straight.as_mut()) {
                    s.to(c, m);
                }
                self.show_straight();
                self.pending.get_or_insert(e.at);
            }
            (Tool::Brush, Phase::Move) => {
                if let Some(live) = self.live.as_mut() {
                    let dt = e.at.duration_since(live.last_at).as_secs_f32() * 1000.0;
                    let sample = live.recorder.push(raw_of(&e, c, dt));
                    let ready = match live.follow.as_mut() {
                        Some(follow) => follow.take(sample),
                        None => vec![sample],
                    };
                    ready.into_iter().for_each(|sample| {
                        live.painter
                            .push_on(&sample, &mut self.buffer, self.scene.active_raster());
                        live.stroke.samples.push(sample);
                        if let Some((axis, copy, painter)) = live.twin.as_mut() {
                            let twin = make::mirrored(&sample, *axis);
                            painter.push_on(&twin, &mut self.mirror_buffer, self.scene.active_raster());
                            copy.samples.push(twin);
                        }
                    });
                    live.last_at = e.at;
                    self.pending.get_or_insert(e.at);
                }
            }
            (Tool::Brush, Phase::Up) => self.finish()?,
            (Tool::Select, Phase::Down) => match self.grab_at(view, rect, e.pos) {
                Some((id, Grab::Move, corners))
                    if self.modifiers.shift && self.selection.is_some() && !self.on_marks() =>
                {
                    self.start_drag(id, Grab::Move, corners, c, false);
                    if let Some(drag) = self.drag.as_mut() {
                        drag.toggle = true;
                    }
                }
                Some((id, grab, corners)) => self.start_drag(id, grab, corners, c, false),
                None if self.shifting.is_some() => {
                    if let Some((id, corners)) = self.handles() {
                        self.start_drag(id, Grab::Rotate, corners, c, false);
                        if let Some(drag) = self.drag.as_mut() {
                            drag.outside = true;
                        }
                    }
                }
                None => self.grab_under(c, e.pos)?,
            },
            (Tool::Select, Phase::Move) => {
                if let Some(drag) = self.drag.as_mut() {
                    drag.to(c);
                }
                self.copy_for_drag()?;
            }
            (Tool::Select, Phase::Up) if self.lasso.is_some() && self.lasso_shapes()? => {}
            (Tool::Select, Phase::Up) if self.lasso.is_some() => match self.scene.active_cel() {
                Some(_) => self.finish_pick()?,
                None => {
                    let far = self.lasso.take().is_some_and(|l| l.far);
                    self.picked = None;
                    if self.scene.active_layer().is_some_and(|l| l.kind() == Kind::Bones) {
                        self.say(Tone::Refused, "bones are picked and moved with the bone tool (J)");
                    } else if !self.refuse_locked_in_hand() && far {
                        self.say(
                            Tone::Info,
                            "a loop takes strokes of the active layer: pick a paint layer, or click a shape to take it",
                        );
                    }
                }
            },
            (Tool::Select, Phase::Up) => self.finish_drag()?,
            (Tool::Note, Phase::Down) => self.press_note(c),
            (Tool::Shape, Phase::Down) => self.shape_down(c),
            (Tool::Shape, Phase::Move) => self.shape_move(c),
            (Tool::Shape, Phase::Up) => self.shape_up()?,
            (Tool::Text, Phase::Down) => self.text_down(c)?,
            (Tool::Text, Phase::Move) if self.text_pressing => self.text_drag(c),
            (Tool::Text, Phase::Up) => self.text_pressing = false,
            (Tool::Bone, Phase::Down) => self.bone_down(c),
            (Tool::Bone, Phase::Move) => {
                self.bones.pen = match e.tablet {
                    true => e.pressure,
                    false => 1.0,
                };
                self.bone_move(c)
            }
            (Tool::Bone, Phase::Up) => self.bone_up()?,
            (Tool::Pen, Phase::Down) => self.pen_down(view, rect, e.pos, c)?,
            (Tool::Pen, Phase::Move) => self.pen_move(c),
            (Tool::Pen, Phase::Up) => {
                if let Some(path) = self.pen_path.as_mut() {
                    path.dragging = false;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn shaped(&self) -> Result<(String, BrushDef)> {
        let base = self.brushes.def(&self.brush).ok_or_else(|| {
            anyhow!(
                "the brush {} is not there any more: pick another one in the brush panel",
                self.brush
            )
        })?;
        let mut def = match &self.custom {
            Some(def) => def.clone(),
            None => self.tuning.shape(base),
        };
        if self.tilted_brush() {
            def.color = Some(klad_core::brushes::ColorDef::tilted());
        }
        Ok((pick_id(&self.brush, &def, &self.embedded), def))
    }

    fn pick(&mut self, id: &str) {
        if let Some(def) = self.brushes.def(id) {
            self.tuning = self.tuned(id, def);
            self.brush = id.to_string();
            self.custom = None;
            self.brush_window.name = def.name.clone();
        }
    }

    fn tuned(&self, id: &str, def: &BrushDef) -> Tuning {
        let tuning = Tuning {
            taper: hand_taper(id, def),
            ..Tuning::of(def)
        };
        match (id, self.settings.eraser_feather, tuning.hardness) {
            ("eraser", Some(feather), Some(_)) => Tuning { feather, ..tuning },
            _ => tuning,
        }
    }

    pub(super) fn eraser_feather(&self) -> Option<f32> {
        if self.is_on(Cmd::Eraser) != Some(true) {
            return None;
        }
        let def = self.brushes.def("eraser").filter(|d| d.hardness().is_some())?;
        Some(self.settings.eraser_feather.unwrap_or(def.feather))
    }

    pub(super) fn set_eraser_feather(&mut self, feather: f32) {
        let feather = feather.clamp(0.0, 1.0);
        self.settings.eraser_feather = Some(feather);
        self.settings_dirty = true;
        if self.brush == "eraser" {
            self.tuning.feather = feather;
        }
    }

    fn begin(&mut self, e: &PenEvent, c: Vec2) -> Result<()> {
        let (name, def, tuning) = if e.eraser {
            let base = self
                .brushes
                .def("eraser")
                .cloned()
                .ok_or_else(|| crate::report::bug("the eraser brush is missing from the library"))?;
            let tuning = self.tuned("eraser", &base);
            let def = tuning.shape(&base);
            (pick_id("eraser", &def, &self.embedded), def, tuning)
        } else {
            let (name, def) = self.shaped()?;
            let tuning = match &self.custom {
                Some(custom) => Tuning {
                    taper: custom.taper,
                    stabilizer: custom.stabilizer,
                    smoothing: custom.smoothing,
                    ..self.tuning
                },
                None => self.tuning,
            };
            (name, def, tuning)
        };
        if self.brushes.def(&name) != Some(&def) {
            self.brushes.insert(&name, def);
        }
        let brush = self.brushes.get(&name)?.clone();
        let lock = self.scene.active_locks_alpha();
        if lock && stroke_blend(&brush, true).is_none() {
            self.say(
                Tone::Refused,
                "alpha is locked on this layer, so the eraser cannot change it (untick lock alpha)",
            );
            return Ok(());
        }
        let id = self.scene.next_mark_id();
        let mut stroke = Stroke::new(id, &name, self.color, self.size, self.rng.next_u64() as u32);
        stroke.opacity = self.opacity;
        stroke.flow = self.flow;
        stroke.taper = tuning.taper;
        stroke.lock = lock;
        let curve = if e.tablet {
            self.settings.curve(&device_key(e.device))
        } else {
            PressureCurve::default()
        };
        let mut recorder = Recorder::new(curve, tuning.stabilizer, tuning.smoothing);
        let sample = recorder.push(raw_of(e, c, 0.0));
        if brush.def.color.is_some_and(|c| c.uses_second()) {
            stroke.second = Some(klad_core::color::to_u16(self.second));
        }
        let zoom = self.view.map_or(1.0, |v| v.zoom);
        let straight = self
            .straight_figure()
            .map(|(figure, from)| straight::Straight::new(figure, c, from, sample, zoom));
        let follow = (straight.is_none() && self.following())
            .then(|| assists::Follow::new(self.scene.bundle.doc.guides.assists.clone(), sample, zoom));
        let first: Vec<klad_core::stroke::Sample> = match follow {
            Some(_) => Vec::new(),
            None => vec![sample],
        };
        let mut painter = StrokePainter::live(&brush, &stroke);
        first
            .iter()
            .for_each(|s| painter.push_on(s, &mut self.buffer, self.scene.active_raster()));
        stroke.samples.extend(first.iter().copied());
        let twin = self.mirror.map(|axis| {
            let mut copy = stroke.clone();
            copy.id = id + 1;
            copy.seed = stroke.seed.wrapping_add(1);
            copy.samples = first.iter().map(|s| make::mirrored(s, axis)).collect();
            let mut painter = StrokePainter::live(&brush, &copy);
            copy.samples
                .iter()
                .for_each(|s| painter.push_on(s, &mut self.mirror_buffer, self.scene.active_raster()));
            (axis, copy, painter)
        });
        self.live = Some(Live {
            stroke,
            painter,
            brush,
            recorder,
            last_at: e.at,
            twin,
            straight,
            follow,
        });
        if straight.is_some() {
            self.show_straight();
        }
        self.pending.get_or_insert(e.at);
        Ok(())
    }

    fn finish(&mut self) -> Result<()> {
        let Some(mut live) = self.live.take() else {
            return Ok(());
        };
        let start = Instant::now();
        let straight = live.straight.take();
        let tail = match straight {
            Some(s) => {
                live.stroke.samples = s.samples();
                Vec::new()
            }
            None => live.recorder.finish(),
        };
        let tail = match live.follow.take() {
            Some(follow) => follow.flush(tail),
            None => tail,
        };
        tail.iter()
            .for_each(|s| live.painter.push_on(s, &mut self.buffer, self.scene.active_raster()));
        live.painter.end(&mut self.buffer, self.scene.active_raster());
        if let Some((axis, copy, painter)) = live.twin.as_mut() {
            tail.iter().map(|s| make::mirrored(s, *axis)).for_each(|s| {
                painter.push_on(&s, &mut self.mirror_buffer, self.scene.active_raster());
                copy.samples.push(s);
            });
            painter.end(&mut self.mirror_buffer, self.scene.active_raster());
        }
        live.stroke.samples.extend(tail);
        if straight.is_some()
            && let Some((axis, copy, _)) = live.twin.as_mut()
        {
            copy.samples = live.stroke.samples.iter().map(|s| make::mirrored(s, *axis)).collect();
        }
        let repainted = needs_repaint(&live.stroke) || straight.is_some();
        if repainted {
            self.dirty.extend(self.buffer.touched().iter().copied());
            self.buffer.reset();
            klad_brush::draw_stroke_on(&mut self.buffer, &live.brush, &live.stroke, self.scene.active_raster());
            if let Some((_, copy, _)) = &live.twin {
                self.dirty.extend(self.mirror_buffer.touched().iter().copied());
                self.mirror_buffer.reset();
                klad_brush::draw_stroke_on(&mut self.mirror_buffer, &live.brush, copy, self.scene.active_raster());
            }
        }
        let ended = ms(start);
        if live.brush.blend == klad_brush::brush::StrokeBlend::Erase
            && !erases_any(&self.buffer, self.scene.active_raster())
            && !erases_any(&self.mirror_buffer, self.scene.active_raster())
        {
            self.dirty.extend(self.buffer.touched().iter().copied());
            self.dirty.extend(self.mirror_buffer.touched().iter().copied());
            self.buffer.reset();
            self.mirror_buffer.reset();
            self.say(Tone::Info, "nothing there to erase");
            return Ok(());
        }
        self.follow_the_folder()?;
        if self.scene.bundle.embed_brush(&live.stroke.brush, &live.brush.def)? {
            self.embedded.insert(live.stroke.brush.clone(), live.brush.def.clone());
        }
        let samples = live.stroke.samples.len();
        let dabs = live.painter.dabs();
        let alone = encode_cel(&[Mark::Stroke(live.stroke.clone())])?.len();
        let brush = live.stroke.brush.clone();
        let erasing = live.brush.blend == klad_brush::brush::StrokeBlend::Erase;
        let blend = stroke_blend(&live.brush, live.stroke.lock).unwrap_or(live.brush.blend);
        let ending = live.stroke.samples.last().map(|s| {
            let (x, y) = s.pos();
            vec2(x, y)
        });
        let attaching = Instant::now();
        live.stroke.on = self.attaching(&live.stroke, &live.brush.def);
        let attach = ms(attaching);
        let mut touched = self.scene.commit_stroke(live.stroke, &mut self.buffer, blend)?;
        if let Some((_, mut copy, _)) = live.twin {
            copy.id = self.scene.next_mark_id();
            copy.on = self.attaching(&copy, &live.brush.def);
            touched.extend(self.scene.commit_stroke(copy, &mut self.mirror_buffer, blend)?);
        }
        let commit = ms(start);
        self.brush_last = self.scene.active_id().zip(ending);
        self.dirty.extend(touched);
        let saving = Instant::now();
        let bytes = self.scene.save_active("stroke")?;
        match erasing {
            true => self.first.erased = true,
            false => self.first.painted += 1,
        }
        if !erasing {
            self.used_colour();
        }
        let count = self.scene.active_marks().len();
        let text = format!(
            "{brush}: {samples} samples, {dabs} live dabs, {alone} B packed alone\nlayer file {bytes} B for {count} strokes and fills ({:.0} B each)\npen up {ended:.1} ms{}, attach {attach:.1} ms, commit {commit:.1} ms, save {:.1} ms",
            bytes as f64 / count.max(1) as f64,
            if repainted { " (tapered repaint)" } else { "" },
            ms(saving)
        );
        self.measure("stroke", text);
        Ok(())
    }

    fn attaching(&self, stroke: &Stroke, def: &BrushDef) -> Vec<u32> {
        let marks = self.scene.active_marks();
        klad_core::attach::targets(marks, marks.len(), stroke, |id| match id == stroke.brush {
            true => Some(def),
            false => self.brushes.def(id).or_else(|| self.embedded.get(id)),
        })
    }

    fn set_colour(&mut self, rgba: [f32; 4]) {
        self.color = rgba;
        self.hsva = Hsva::from_rgba(rgba, self.hsva.h);
    }

    fn used_colour(&mut self) {
        if self.settings.used(&color::hex(self.color)) {
            self.settings_dirty = true;
        }
    }

    fn pick_colour(&mut self, at: Vec2, layer_only: bool) -> Result<()> {
        let where_from = if layer_only { "this layer" } else { "the image" };
        match self.scene.sample((at.x, at.y), layer_only)? {
            Some(rgba) => {
                self.pick_new_colour(rgba);
                self.say(Tone::Done, format!("picked {} from {where_from}", color::hex(rgba)));
            }
            None => self.say(Tone::Refused, format!("nothing to pick from {where_from} there")),
        }
        Ok(())
    }

    fn fill_reference(&self) -> FillRef {
        match &self.fill.reference {
            FillRef::Layer(id) if self.scene.active_id().as_ref() == Some(id) => FillRef::This,
            FillRef::Layer(id) if self.scene.bundle.doc.layer(id).is_none() => FillRef::This,
            other => other.clone(),
        }
    }

    fn pixel_cells(&self, live: &PixelLive) -> Vec<klad_core::pixel::Cell> {
        let run = match self.pixel_perfect && self.pixel_size == 1 {
            true => klad_core::pixel::perfect(&live.run),
            false => live.run.clone(),
        };
        klad_core::pixel::stamp(&run, self.pixel_size)
    }

    pub(super) fn pixel_spec(&self, erase: bool) -> Option<klad_core::ops::PixelSpec> {
        let shade = match (erase, self.pixel_mode) {
            (false, PixelMode::Lighter) => Some(1),
            (false, PixelMode::Darker) => Some(-1),
            _ => None,
        };
        let dither = (!erase && shade.is_none() && self.pixel_dither < 100).then_some(self.pixel_dither);
        Some(klad_core::ops::PixelSpec {
            layer: self.scene.active_id()?,
            frame: self.scene.frame,
            color: (!erase && shade.is_none()).then(|| color::hex(self.color)),
            erase,
            shade,
            dither,
            second: dither.filter(|_| self.pixel_two).map(|_| color::hex(self.second)),
            mirror_x: self.mirror.map(|axis| (axis * 2.0).round() / 2.0),
            by_hand: true,
            ..klad_core::ops::PixelSpec::default()
        })
    }

    fn pixel_preview(&self, cells: Vec<klad_core::pixel::Cell>) -> Vec<klad_core::pixel::Cell> {
        let twinned = match self.mirror {
            Some(axis) => klad_core::pixel::mirrored(&cells, Some((axis * 2.0).round() / 2.0), None),
            None => cells,
        };
        let canvas = &self.scene.bundle.doc.canvas;
        match canvas.wrap {
            true => klad_core::pixel::wrapped(&twinned, canvas.width, canvas.height),
            false => twinned,
        }
    }

    fn erases_nothing(&self, ops: &[Op]) -> bool {
        let (w, h) = self.scene.size();
        let grid = klad_core::pixel::grid_of(self.scene.active_marks(), w, h);
        let painted = |c: &klad_core::pixel::Cell| {
            c[0] >= 0 && c[1] >= 0 && c[0] < w as i64 && c[1] < h as i64 && grid.at(c[0] as u32, c[1] as u32).is_some()
        };
        ops.iter().all(|op| match op {
            Op::Pixel(spec) => spec.erase && !spec.cells.iter().any(painted),
            _ => false,
        })
    }

    pub(super) fn apply_pixels(&mut self, spec: klad_core::ops::PixelSpec) -> Result<()> {
        let drew = spec.color.is_some();
        match self.scene.bundle.settle_pixels(&[Op::Pixel(spec)]) {
            Ok(ops) if self.erases_nothing(&ops) => {}
            Ok(ops) => {
                self.apply(ops)?;
                if drew {
                    self.used_colour();
                }
            }
            Err(e) => {
                let why = e.chain().last().map_or_else(|| format!("{e:#}"), ToString::to_string);
                self.say(Tone::Refused, self.refusal_words(&why));
            }
        }
        Ok(())
    }

    fn step_picked(&mut self, by: i32) -> Result<()> {
        let doc = &self.scene.bundle.doc;
        let sets: Vec<(String, u32, &'static str, Value)> = self.chosen_marks().map_or(Vec::new(), |(_, chosen)| {
            chosen
                .iter()
                .filter_map(|(cel, m)| {
                    let fill = m.as_fill().filter(|f| f.on_grid() && !f.erases())?;
                    let at = fill
                        .entry()
                        .and_then(|entry| doc.swatch(entry))
                        .or_else(|| doc.swatch(&klad_core::color::to_hex(fill.color())))?;
                    let to = doc.ramp_step(at, by)?;
                    Some((cel.clone(), m.id(), "color", json!(to.to_string())))
                })
                .collect()
        });
        if sets.is_empty() {
            self.say(
                Tone::Refused,
                match by > 0 {
                    true => "nothing to step lighter: these colours are in no ramp, or at its light end (the ramp button makes one)",
                    false => "nothing to step darker: these colours are in no ramp, or at its dark end (the ramp button makes one)",
                },
            );
            return Ok(());
        }
        self.set_picked(sets)
    }

    fn fill_pixels(&mut self, at: klad_core::pixel::Cell) -> Result<()> {
        let (w, h) = self.scene.size();
        if at[0] < 0 || at[1] < 0 || at[0] >= w as i64 || at[1] >= h as i64 {
            return Ok(());
        }
        let Some(spec) = self.pixel_spec(false) else {
            return Ok(());
        };
        self.apply_pixels(klad_core::ops::PixelSpec {
            fill: Some(at),
            diagonal: self.pixel_diagonal,
            reference: self.pixel_fill_all.then(|| "all".to_string()),
            ..spec
        })
    }

    pub(super) fn pixel_shape_spec(&self) -> Option<klad_core::ops::PixelSpec> {
        let (a, b) = self.shapes.drag?;
        let (a, b) = (klad_core::pixel::cell_at(a.x, a.y), klad_core::pixel::cell_at(b.x, b.y));
        let b = match (self.modifiers.shift, self.pixel_shape) {
            (true, PixelShape::Rect | PixelShape::Ellipse) => {
                let side = (b[0] - a[0]).abs().max((b[1] - a[1]).abs());
                let sign = |v: i64| if v < 0 { -1 } else { 1 };
                [a[0] + side * sign(b[0] - a[0]), a[1] + side * sign(b[1] - a[1])]
            }
            _ => b,
        };
        let bounds = [
            a[0].min(b[0]),
            a[1].min(b[1]),
            (b[0] - a[0]).abs() + 1,
            (b[1] - a[1]).abs() + 1,
        ];
        let spec = self.pixel_spec(false)?;
        Some(match self.pixel_shape {
            PixelShape::Rect => klad_core::ops::PixelSpec {
                rect: Some(bounds),
                filled: self.pixel_filled,
                size: self.pixel_size,
                ..spec
            },
            PixelShape::Ellipse => klad_core::ops::PixelSpec {
                ellipse: Some(bounds),
                filled: self.pixel_filled,
                size: self.pixel_size,
                ..spec
            },
            PixelShape::Line => klad_core::ops::PixelSpec {
                line: Some([a[0], a[1], b[0], b[1]]),
                size: self.pixel_size,
                ..spec
            },
        })
    }

    fn finish_pixels(&mut self) -> Result<()> {
        let Some(live) = self.pixel_live.take() else {
            return Ok(());
        };
        self.pixel_last = live.run.last().copied();
        let cells = self.pixel_cells(&live);
        let Some(spec) = self.pixel_spec(live.erase).filter(|_| !cells.is_empty()) else {
            return Ok(());
        };
        self.apply_pixels(klad_core::ops::PixelSpec { cells, ..spec })
    }

    fn finish_fill(&mut self) -> Result<()> {
        let Some(lasso) = self.lasso.take() else {
            return Ok(());
        };
        let start = Instant::now();
        let (w, h) = self.scene.size();
        let (source, coverage) = if lasso.far && lasso.points.len() >= 3 {
            let points: Vec<[f32; 2]> = lasso.points.iter().map(|p| [p.x, p.y]).collect();
            let coverage = polygon(&points, w, h);
            (FillSource::Polygon(points), coverage)
        } else {
            let at = lasso.points[0];
            let seed = Seed {
                x: at.x,
                y: at.y,
                tolerance: self.fill.tolerance,
                gap: self.fill.gap,
                expand: self.fill.expand,
            };
            let reference = self.fill_reference();
            let image = self.scene.fill_reference(&reference, &self.brushes)?;
            let coverage = flood(&image, &seed);
            (FillSource::Seed { seed, reference }, coverage)
        };
        let computed = ms(start);
        if coverage.is_empty() {
            self.say(
                Tone::Refused,
                "nothing to fill there: click inside closed lines, or raise the gap so small openings close",
            );
            return Ok(());
        }
        self.follow_the_folder()?;
        let pixels = coverage.pixels();
        let lassoed = matches!(source, FillSource::Polygon(_));
        let mut fill = Fill::new(self.scene.next_mark_id(), self.color, source, coverage);
        fill.opacity = self.fill.opacity;
        fill.lock = self.scene.active_locks_alpha();
        let committing = Instant::now();
        let touched = self.scene.commit_fill(fill, &self.brushes)?;
        self.dirty.extend(touched);
        let bytes = self.scene.save_active("fill")?;
        self.first.filled = true;
        self.used_colour();
        let text = format!(
            "{}: {pixels} px computed in {computed:.0} ms, painted and saved in {:.0} ms\nlayer file {bytes} B",
            if lassoed { "lasso fill" } else { "fill" },
            ms(committing)
        );
        self.measure("fill", text);
        Ok(())
    }

    fn navigate(&mut self, ui: &egui::Ui, response: &egui::Response, rect: Rect) {
        let area = rect.size();
        let (zoom, scroll, rotation, hover, space, middle, primary, delta, time, snap) = ui.input(|i| {
            (
                i.zoom_delta(),
                i.smooth_scroll_delta,
                i.rotation_delta(),
                i.pointer.hover_pos(),
                i.key_down(Key::Space),
                i.pointer.middle_down(),
                i.pointer.primary_down(),
                i.pointer.delta(),
                i.time,
                i.modifiers.shift,
            )
        });
        self.navigating = (space && !ui.ctx().egui_wants_keyboard_input()) || middle;
        let Some(view) = self.view.as_mut() else {
            return;
        };
        let at = hover.filter(|p| rect.contains(*p)).map_or(area / 2.0, |p| p - rect.min);
        if response.hovered() || response.dragged() {
            if zoom != 1.0 && self.scene.bundle.doc.pixel_art() {
                let wanted = (self.zoom_wanted.unwrap_or(view.zoom) * zoom).clamp(0.02, 64.0);
                self.zoom_wanted = Some(wanted);
                let target = View::whole(wanted, ui.ctx().pixels_per_point());
                if target != view.zoom {
                    view.zoom_at(target / view.zoom, at, area);
                }
            } else if zoom != 1.0 {
                view.zoom_at(zoom, at, area);
            }
            if scroll != Vec2::ZERO {
                view.pan(scroll);
            }
            if rotation != 0.0 {
                let raw = match self.twist {
                    Some((raw, last)) if time - last < TWIST_PAUSE => raw,
                    _ => view.angle,
                } + rotation;
                self.twist = Some((raw, time));
                view.rotate_at(crate::view::twisted(raw, snap) - view.angle, at, area);
            }
        }
        if self.navigating && (middle || primary) && self.live.is_none() {
            view.pan(delta);
        }
        if self.spin {
            view.rotate_at(0.01, area / 2.0, area);
            view.zoom_at(1.0 + 0.012 * (time as f32 * 1.5).sin(), area / 2.0, area);
            ui.ctx().request_repaint();
        }
    }

    fn canvas(&mut self, ui: &mut egui::Ui) -> Result<()> {
        let rect = ui.max_rect();
        let response = ui.allocate_rect(rect, Sense::click_and_drag());
        self.canvas_rect = rect;
        let area = rect.size();
        let canvas = self.canvas_size();
        self.canvas_frames = self.canvas_frames.saturating_add(1);
        let settling = self.canvas_frames <= SETTLING_FRAMES;
        let refit = match (self.view, self.fitted_to) {
            (None, _) => true,
            (Some(view), Some(was)) => {
                let moved =
                    (area.x - was.x).abs() > REFIT_CHANGE * was.x || (area.y - was.y).abs() > REFIT_CHANGE * was.y;
                was != area && view == self.fitted(was) && (settling || moved)
            }
            (Some(_), None) => false,
        };
        if refit {
            self.view = Some(self.fitted(area));
            self.fitted_to = Some(area);
        }
        self.navigate(ui, &response, rect);
        self.keep_typing_preview()?;
        self.keep_typing_focus(ui, &response)?;
        self.typing_keys(ui)?;
        self.pen_keys(ui)?;
        self.bone_keys(ui);
        self.shift_keys(ui)?;
        self.crop_keys(ui)?;
        self.fit_buffers();
        self.command_keys(ui)?;
        let ctx = ui.ctx().clone();
        let was_focused = self.focused;
        self.focused = ctx.input(|i| i.focused);
        self.modifiers = ctx.input(|i| i.modifiers);
        if let Some(drag) = self.drag.as_mut() {
            drag.keys(self.modifiers);
        }
        self.refresh_shape();
        let events = match self.pen.take() {
            Some(events) => events,
            None => pen::from_egui(&ui.input(|i| i.events.clone()), &mut self.touch),
        };
        let holding = self.writing() && matches!(self.tool, Tool::Select | Tool::Direct);
        self.held
            .admit(holding, events)
            .into_iter()
            .try_for_each(|e| self.on_pen(&ctx, e, rect, was_focused))?;
        if self.tool == Tool::Bone && response.secondary_clicked() {
            self.end_chain();
        }
        if response.secondary_clicked() {
            self.focus = Focus::Canvas;
        }
        if !matches!(self.tool, Tool::Bone | Tool::Note) {
            response.context_menu(|ui| self.canvas_menu(ui));
        }
        if self.tool == Tool::Note
            && response.secondary_clicked()
            && let (Some(p), Some(view)) = (response.interact_pointer_pos(), self.view)
        {
            self.select_pin(view.to_canvas(p - rect.min, rect.size()))?;
        }
        let view = self.view.unwrap_or_else(|| self.fitted(area));
        self.double_click_text(&response, view, rect)?;
        self.zoom_px = view.zoom * ctx.pixels_per_point();
        self.refresh_box();
        self.refresh_cut();
        self.refresh_shape();
        self.tend_path()?;
        let level = match self.zoom_px >= 1.0 {
            true => 0,
            false => (1.0 / self.zoom_px).log2().floor().clamp(0.0, 4.0) as u32,
        };
        self.frames.set_level(level);
        let show = self.tend_frames()?;
        let view_uniform = view.uniform(area, ctx.pixels_per_point(), canvas, self.scene.bundle.doc.canvas.wrap);
        let (w, h) = self.scene.size();
        let visible = self.visible_tiles(view, rect);
        let previewed = self.tend_preview(visible)?;
        let mut tiles = std::mem::take(&mut self.dirty);
        if let Some(state) = self.preview.as_ref() {
            let held: BTreeSet<usize> = tiles.intersection(&state.shown).copied().collect();
            tiles.retain(|i| !held.contains(i));
            self.dirty.extend(held);
        }
        let mut stroke: Vec<usize> = self.buffer.take_dirty();
        stroke.extend(self.mirror_buffer.take_dirty());
        if !self.canvas_current() {
            self.dirty.extend(std::mem::take(&mut tiles));
            self.dirty.extend(stroke.drain(..));
        }
        let refill = self.live.is_none() && tiles.len() > self.batch;
        if refill {
            let later = tiles.split_off(&tiles.iter().nth(self.batch).copied().unwrap_or(usize::MAX));
            self.dirty.extend(later);
            ctx.request_repaint();
        }
        let tiles: Vec<usize> = tiles.into_iter().collect();
        let start = Instant::now();
        let preview = self.live.as_ref().map(|l| Preview {
            twin: l.twin.as_ref().map(|_| &self.mirror_buffer),
            buffer: &self.buffer,
            color: l.stroke.color(),
            opacity: l.stroke.opacity,
            blend: stroke_blend(&l.brush, l.stroke.lock).unwrap_or(l.brush.blend),
        });
        let uploads = if tiles.is_empty() && stroke.is_empty() {
            Vec::new()
        } else {
            self.scene.compose_painting(&stroke, &tiles, preview.as_ref())?
        };
        if !uploads.is_empty() {
            let spent = ms(start);
            if refill {
                self.batch = ((uploads.len() as f64 * REFILL_MS / spent.max(0.1)) as usize).clamp(8, timeline::FILL);
            }
            let mut stats = lock(&self.stats);
            stats.compose_ms.push(spent as f32);
            stats.tiles.push(uploads.len() as f32);
        }
        let uploads: Tiles = previewed.into_iter().chain(uploads).collect();
        if !uploads.is_empty() {
            self.painted += 1;
        }
        let (frame_uploads, frame_drops) = self.frames.take();
        lock(&self.outbox).send(uploads, frame_uploads, frame_drops);
        ui.painter().add(egui_wgpu::Callback::new_paint_callback(
            rect,
            CanvasPaint {
                show: show.filter(|_| self.preview.is_none()),
                outbox: self.outbox.clone(),
                uniform: view_uniform,
                size: (w, h),
                cols: self.scene.cols(),
                pending: self.pending.take(),
                stats: self.stats.clone(),
            },
        ));
        self.map_overlay(ui, view, rect);
        self.occluder_overlay(ui, view, rect);
        self.parallax(ui, view, rect);
        self.prepare_typing(&ctx, view, rect);
        let hover = ui
            .input(|i| i.pointer.hover_pos())
            .filter(|p| rect.contains(*p))
            .map(|p| view.to_canvas(p - rect.min, rect.size()));
        self.hover_cell = hover
            .filter(|_| self.scene.bundle.doc.pixel_art())
            .filter(|p| p.x >= 0.0 && p.y >= 0.0 && p.x < canvas.x && p.y < canvas.y)
            .map(|p| [p.x.floor() as i32, p.y.floor() as i32]);
        self.decorate_grid(ui, view, rect);
        self.decorate(ui, view, rect);
        let painter = ui.painter().with_clip_rect(rect);
        self.decorate_bones(&painter, |c| rect.min + view.to_screen(c, rect.size()), hover);
        if self.live.is_some()
            || self.lasso.is_some()
            || self.drag.is_some()
            || self.point_drag.is_some()
            || self.path_commit.is_some()
            || self
                .preview
                .as_ref()
                .is_some_and(|p| p.job.is_some() || p.finish.is_some())
        {
            ctx.request_repaint();
        }
        self.share_selection();
        Ok(())
    }

    pub(super) fn fitted(&self, area: Vec2) -> View {
        match self.scene.bundle.doc.pixel_art() {
            true => View::fit_whole(self.canvas_size(), area, self.ctx.pixels_per_point()),
            false => View::fit(self.canvas_size(), area),
        }
    }

    pub(super) fn grid_shows(&self) -> bool {
        self.settings.pixel_grid && self.zoom_px >= GRID_FROM && self.scene.bundle.doc.pixel_art()
    }

    fn decorate_grid(&self, ui: &egui::Ui, view: View, rect: Rect) {
        let painter = ui.painter().with_clip_rect(rect);
        let canvas = self.canvas_size();
        if self.grid_shows() {
            let corners = [
                rect.left_top(),
                rect.right_top(),
                rect.right_bottom(),
                rect.left_bottom(),
            ]
            .map(|p| view.to_canvas(p - rect.min, rect.size()));
            let low = |f: fn(&Vec2) -> f32| corners.iter().map(f).fold(f32::MAX, f32::min);
            let high = |f: fn(&Vec2) -> f32| corners.iter().map(f).fold(f32::MIN, f32::max);
            let (x0, x1) = (low(|p| p.x).floor().max(0.0), high(|p| p.x).ceil().min(canvas.x));
            let (y0, y1) = (low(|p| p.y).floor().max(0.0), high(|p| p.y).ceil().min(canvas.y));
            let dark = egui::Stroke::new(1.0, Color32::from_black_alpha(70));
            let light = egui::Stroke::new(1.0, Color32::from_white_alpha(90));
            let to_screen = |x: f32, y: f32| self.screen(view, rect, vec2(x, y));
            let rule = |a: Pos2, b: Pos2| {
                painter.line_segment([a, b], dark);
                painter.extend(egui::Shape::dashed_line(&[a, b], light, 2.0, 2.0));
            };
            (x0 as i32..=x1 as i32)
                .map(|x| x as f32)
                .for_each(|x| rule(to_screen(x, y0), to_screen(x, y1)));
            (y0 as i32..=y1 as i32)
                .map(|y| y as f32)
                .for_each(|y| rule(to_screen(x0, y), to_screen(x1, y)));
        }
        if let Some([tile_width, tile_height]) = self.scene.bundle.doc.canvas.tile {
            let edge = egui::Stroke::new(1.5, Color32::from_black_alpha(150));
            let glow = egui::Stroke::new(1.0, Color32::from_white_alpha(140));
            let to_screen = |x: f32, y: f32| self.screen(view, rect, vec2(x, y));
            let rule = |a: Pos2, b: Pos2| {
                painter.line_segment([a, b], edge);
                painter.extend(egui::Shape::dashed_line(&[a, b], glow, 4.0, 4.0));
            };
            (0..=(canvas.x as u32 / tile_width.max(1)))
                .map(|column| (column * tile_width) as f32)
                .for_each(|x| rule(to_screen(x, 0.0), to_screen(x, canvas.y)));
            (0..=(canvas.y as u32 / tile_height.max(1)))
                .map(|row| (row * tile_height) as f32)
                .for_each(|y| rule(to_screen(0.0, y), to_screen(canvas.x, y)));
        }
        let shaped = self
            .pixel_shape_spec()
            .filter(|_| self.tool == Tool::Shape && self.scene.on_grid())
            .and_then(|spec| {
                klad_core::ops::PixelSpec {
                    shade: None,
                    dither: None,
                    ..spec.clone()
                }
                .drawn()
                .ok()
                .map(|cells| (self.pixel_preview(cells), spec.erase))
            });
        let drawing = self
            .pixel_live
            .as_ref()
            .map(|live| (self.pixel_preview(self.pixel_cells(live)), live.erase))
            .or(shaped);
        if let Some((cells, erase)) = drawing {
            let shading = !erase && self.pixel_mode != PixelMode::Draw;
            let dither = (!erase && !shading && self.pixel_dither < 100).then_some(self.pixel_dither);
            cells.iter().for_each(|c| {
                let fill = match (erase, self.pixel_mode, dither) {
                    (true, _, _) => Color32::from_rgba_unmultiplied(255, 255, 255, 120),
                    (false, PixelMode::Lighter, _) => Color32::from_white_alpha(90),
                    (false, PixelMode::Darker, _) => Color32::from_black_alpha(90),
                    (false, PixelMode::Draw, Some(percent)) if !klad_core::pixel::dithered(*c, percent) => {
                        match self.pixel_two {
                            true => color::color32(self.second),
                            false => Color32::TRANSPARENT,
                        }
                    }
                    (false, PixelMode::Draw, _) => color::color32(self.color),
                };
                let (x, y) = (c[0] as f32, c[1] as f32);
                let corners = [(x, y), (x + 1.0, y), (x + 1.0, y + 1.0), (x, y + 1.0)]
                    .map(|(x, y)| self.screen(view, rect, vec2(x, y)))
                    .to_vec();
                painter.add(egui::Shape::convex_polygon(corners, fill, egui::Stroke::NONE));
            });
        }
        let pencil = self.tool == Tool::Brush && self.scene.on_grid();
        if let Some([x, y]) = self.hover_cell.filter(|_| self.scene.on_grid() && self.zoom_px >= 3.0) {
            let side = if pencil { self.pixel_size.max(1) as f32 } else { 1.0 };
            let low = ((side - 1.0) / 2.0).floor();
            let (x, y) = (x as f32 - low, y as f32 - low);
            let cell = [(x, y), (x + side, y), (x + side, y + side), (x, y + side)]
                .map(|(x, y)| self.screen(view, rect, vec2(x, y)))
                .to_vec();
            painter.add(egui::Shape::closed_line(
                cell.clone(),
                egui::Stroke::new(3.0, Color32::from_black_alpha(150)),
            ));
            painter.add(egui::Shape::closed_line(cell, egui::Stroke::new(1.0, Color32::WHITE)));
        }
    }

    fn ring(&self) -> Option<f32> {
        (self.tool == Tool::Brush && !self.scene.on_grid()).then_some(self.size)
    }

    fn decorate(&self, ui: &egui::Ui, view: View, rect: Rect) {
        let painter = ui.painter().with_clip_rect(rect);
        let accent = Color32::from_rgb(64, 196, 255);
        let dark = egui::Stroke::new(3.0, Color32::from_black_alpha(140));
        if let Some(picked) = self.picked.as_ref().filter(|_| self.on_marks()) {
            let thin = egui::Stroke::new(1.0, accent);
            let moving = self.drag.as_ref().filter(|d| d.strokes && d.moved()).map(Drag::affine);
            let to_screen = |x: f32, y: f32| {
                let (x, y) = moving.map_or((x, y), |a| lift::affine_apply(a, (x, y)));
                self.screen(view, rect, vec2(x, y))
            };
            let parts = self.picked_parts();
            let chosen = parts
                .iter()
                .flat_map(|(_, _, marks, ids)| marks.iter().filter(|m| ids.contains(&m.id())));
            chosen.for_each(|m| match m {
                Mark::Stroke(s) => {
                    let points: Vec<(f32, f32)> = s.samples.iter().map(|v| v.pos()).collect();
                    points
                        .chunk_by(|a, b| klad_core::cut::keeps(&s.cuts, *a) == klad_core::cut::keeps(&s.cuts, *b))
                        .filter(|run| klad_core::cut::keeps(&s.cuts, run[0]))
                        .for_each(|run| {
                            let line: Vec<Pos2> = run.iter().map(|&(x, y)| to_screen(x, y)).collect();
                            painter.add(egui::Shape::line(line, thin));
                        });
                }
                Mark::Fill(f) => {
                    if let Some([x0, y0, x1, y1]) = m.bounds().or(f.bounds()) {
                        let corners = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)].map(|(x, y)| to_screen(x, y));
                        painter.add(egui::Shape::closed_line(corners.to_vec(), thin));
                    }
                }
            });
            if let Some(polygon) = &picked.cut {
                let line: Vec<Pos2> = polygon.iter().map(|p| to_screen(p[0], p[1])).collect();
                painter.add(egui::Shape::dashed_line(
                    &[line.clone(), line[..1].to_vec()].concat(),
                    egui::Stroke::new(1.5, accent),
                    6.0,
                    4.0,
                ));
            }
        }
        if self.tool == Tool::Direct {
            self.decorate_points(&painter, view, rect, accent);
            self.decorate_point_box(&painter, view, rect, accent, dark);
        }
        self.decorate_shape(&painter, &|x, y| self.screen(view, rect, vec2(x, y)), accent);
        self.decorate_typing(&painter, view, rect, accent);
        let mirrors =
            self.tool == Tool::Brush || (self.scene.on_grid() && matches!(self.tool, Tool::Shape | Tool::Fill));
        if let Some(axis) = self.mirror.filter(|_| mirrors) {
            let h = self.scene.size().1 as f32;
            painter.add(egui::Shape::dashed_line(
                &[
                    self.screen(view, rect, vec2(axis, 0.0)),
                    self.screen(view, rect, vec2(axis, h)),
                ],
                egui::Stroke::new(1.0, accent),
                8.0,
                6.0,
            ));
        }
        if let Some(path) = self.pen_path.as_ref() {
            let hover = ui.input(|i| i.pointer.hover_pos());
            self.decorate_pen(&painter, path, hover, view, rect, accent, dark);
        }
        if let Some((_, corners)) = self.handles() {
            if self.drag.is_none() && self.tool == Tool::Select && !self.on_marks() {
                self.group_boxes.iter().for_each(|member| {
                    let outline: Vec<Pos2> = member.iter().map(|c| self.screen(view, rect, *c)).collect();
                    painter.add(egui::Shape::closed_line(outline, egui::Stroke::new(1.0, accent)));
                });
            }
            let corners = match &self.drag {
                Some(drag) => drag.preview(),
                None => corners,
            };
            let screen: Vec<Pos2> = corners.iter().map(|c| self.screen(view, rect, *c)).collect();
            painter.add(egui::Shape::closed_line(screen.clone(), dark));
            painter.add(egui::Shape::closed_line(screen.clone(), egui::Stroke::new(1.5, accent)));
            screen.iter().for_each(|p| {
                painter.rect_filled(Rect::from_center_size(*p, vec2(HANDLE, HANDLE)), 1.0, Color32::WHITE);
                painter.rect_stroke(
                    Rect::from_center_size(*p, vec2(HANDLE, HANDLE)),
                    1.0,
                    egui::Stroke::new(1.0, accent),
                    egui::StrokeKind::Middle,
                );
            });
            if self.edges() {
                let flat = [0, 1, 2, 3].map(|i| screen[i].to_vec2());
                (0..4).for_each(|side| {
                    let p = edge(&flat, side).to_pos2();
                    painter.circle_filled(p, HANDLE / 2.0 - 0.5, Color32::WHITE);
                    painter.circle_stroke(p, HANDLE / 2.0 - 0.5, egui::Stroke::new(1.0, accent));
                });
            }
            let top = screen[0].lerp(screen[1], 0.5);
            let center = screen.iter().fold(Vec2::ZERO, |a, p| a + p.to_vec2()) / 4.0;
            let knob = top + (top.to_vec2() - center).normalized() * KNOB;
            painter.line_segment([top, knob], egui::Stroke::new(1.5, accent));
            painter.circle_filled(knob, HANDLE / 2.0 + 1.0, Color32::WHITE);
            painter.circle_stroke(knob, HANDLE / 2.0 + 1.0, egui::Stroke::new(1.0, accent));
        }
        self.draw_canvas_edge(&painter, |c| self.screen(view, rect, c));
        self.draw_pins(&painter, |c| self.screen(view, rect, c));
        self.crop_overlay(&painter, |c| self.screen(view, rect, c));
        self.draw_guides(&painter, rect, |c| self.screen(view, rect, c));
        self.draw_assists(&painter, rect, |c| self.screen(view, rect, c));
        self.draw_checks(&painter, |c| self.screen(view, rect, c));
        if let Some(lasso) = self.lasso.as_ref().filter(|l| l.far) {
            let line: Vec<Pos2> = self
                .lasso_outline(lasso)
                .iter()
                .map(|p| self.screen(view, rect, *p))
                .collect();
            let tint = if matches!(self.tool, Tool::Strokes | Tool::Select) {
                accent
            } else {
                color::color32(self.color)
            };
            painter.add(egui::Shape::closed_line(line.clone(), dark));
            painter.add(egui::Shape::closed_line(line, egui::Stroke::new(1.5, tint)));
        }
        let hover = ui.input(|i| i.pointer.hover_pos()).filter(|p| rect.contains(*p));
        let dropper = self.modifiers.alt && !self.modifiers.command && matches!(self.tool, Tool::Brush | Tool::Fill);
        if let Some(hover) = hover.filter(|_| dropper) {
            let font = egui::FontId::proportional(DROPPER);
            let tip = hover + vec2(-DROPPER * 0.12, DROPPER * 0.2);
            [vec2(-1.0, 0.0), vec2(1.0, 0.0), vec2(0.0, -1.0), vec2(0.0, 1.0)]
                .iter()
                .for_each(|d| {
                    painter.text(
                        tip + *d,
                        egui::Align2::LEFT_BOTTOM,
                        crate::theme::icon::EYEDROPPER,
                        font.clone(),
                        Color32::from_black_alpha(220),
                    );
                });
            painter.text(
                tip,
                egui::Align2::LEFT_BOTTOM,
                crate::theme::icon::EYEDROPPER,
                font,
                Color32::WHITE,
            );
            ui.ctx().set_cursor_icon(egui::CursorIcon::None);
            return;
        }
        if self.tool == Tool::Fill
            && let Some(hover) = hover
        {
            let arm = |d: Vec2| [hover + d * 3.0, hover + d * 9.0];
            [vec2(1.0, 0.0), vec2(-1.0, 0.0), vec2(0.0, 1.0), vec2(0.0, -1.0)]
                .iter()
                .for_each(|d| {
                    painter.line_segment(arm(*d), egui::Stroke::new(3.0, Color32::from_black_alpha(160)));
                    painter.line_segment(arm(*d), egui::Stroke::new(1.5, Color32::WHITE));
                });
        }
        if let Some((_, _, origin)) = self.sizing {
            let radius = (self.size * 0.5 * view.zoom).max(1.5);
            painter.circle_stroke(origin, radius, egui::Stroke::new(1.0, Color32::from_gray(30)));
            painter.circle_stroke(origin, radius + 1.0, egui::Stroke::new(1.0, Color32::from_gray(235)));
            painter.text(
                origin + vec2(radius + 8.0, 0.0),
                egui::Align2::LEFT_CENTER,
                format!("{:.1} px", self.size),
                crate::theme::regular(crate::theme::BODY),
                Color32::WHITE,
            );
            return;
        }
        if let (Some(size), Some(hover)) = (self.ring(), hover) {
            let radius = (size * 0.5 * view.zoom).max(1.5);
            painter.circle_stroke(hover, radius, egui::Stroke::new(1.0, Color32::from_gray(30)));
            painter.circle_stroke(hover, radius + 1.0, egui::Stroke::new(1.0, Color32::from_gray(235)));
        }
    }

    fn decorate_points(&self, painter: &egui::Painter, view: View, rect: Rect, accent: Color32) {
        let (Some(shaped), Some(direct)) = (self.shaped.as_ref(), self.direct.as_ref()) else {
            return;
        };
        let Some(subs) = self.shown_subs() else {
            return;
        };
        let m = shaped.matrix;
        let screen = |p: [f32; 2]| self.screen(view, rect, to_canvas(m, p));
        let thin = egui::Stroke::new(1.0, accent);
        let none = BTreeSet::new();
        self.shown_also().iter().for_each(|shown| {
            self.outline_points(painter, view, rect, accent, &shown.subs, shown.matrix);
            let chosen = direct.also.get(&shown.node).unwrap_or(&none);
            self.square_points(painter, view, rect, accent, &shown.subs, shown.matrix, chosen);
        });
        self.outline_points(painter, view, rect, accent, &subs, m);
        let anchors = pathedit::anchors(&subs);
        direct.points.iter().filter_map(|&i| anchors.get(i)).for_each(|(_, a)| {
            let at = screen(a.at);
            [(a.inn, a.has_in()), (a.out, a.has_out())]
                .into_iter()
                .filter(|(_, has)| *has)
                .for_each(|(p, _)| {
                    let h = screen(p);
                    painter.line_segment([at, h], thin);
                    painter.circle_filled(h, HANDLE / 2.0 - 0.5, Color32::WHITE);
                    painter.circle_stroke(h, HANDLE / 2.0 - 0.5, thin);
                });
        });
        self.square_points(painter, view, rect, accent, &subs, m, &direct.points);
    }

    fn point_box(&self) -> Option<(Vec2, Vec2)> {
        self.point_drag
            .filter(|d| matches!(d.grab, PointGrab::Marquee { .. }))
            .map(|d| (d.start, d.now))
    }

    fn decorate_point_box(&self, painter: &egui::Painter, view: View, rect: Rect, accent: Color32, dark: egui::Stroke) {
        let Some((start, now)) = self.point_box() else {
            return;
        };
        let area = Rect::from_two_pos(self.screen(view, rect, start), self.screen(view, rect, now));
        painter.rect_filled(area, 0.0, accent.gamma_multiply(0.12));
        painter.rect_stroke(area, 0.0, dark, egui::StrokeKind::Outside);
        painter.rect_stroke(area, 0.0, egui::Stroke::new(1.5, accent), egui::StrokeKind::Middle);
    }

    fn outline_points(
        &self,
        painter: &egui::Painter,
        view: View,
        rect: Rect,
        accent: Color32,
        subs: &[Subpath],
        m: [f32; 6],
    ) {
        let screen = |p: [f32; 2]| self.screen(view, rect, to_canvas(m, p));
        let step = (2.0 / (view.zoom * (m[0] * m[3] - m[1] * m[2]).abs().sqrt()).max(1e-3)).clamp(0.05, 8.0);
        pathedit::flatten(subs, step).iter().zip(subs).for_each(|(line, sub)| {
            let points: Vec<Pos2> = line.iter().map(|p| screen(*p)).collect();
            let shape = if sub.closed {
                egui::Shape::closed_line(points, egui::Stroke::new(1.5, accent))
            } else {
                egui::Shape::line(points, egui::Stroke::new(1.5, accent))
            };
            painter.add(shape);
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn square_points(
        &self,
        painter: &egui::Painter,
        view: View,
        rect: Rect,
        accent: Color32,
        subs: &[Subpath],
        m: [f32; 6],
        chosen: &BTreeSet<usize>,
    ) {
        let size = HANDLE - 2.0;
        let thin = egui::Stroke::new(1.0, accent);
        pathedit::anchors(subs).iter().enumerate().for_each(|(i, (_, a))| {
            let at = self.screen(view, rect, to_canvas(m, a.at));
            let square = Rect::from_center_size(at, vec2(size, size));
            let fill = if chosen.contains(&i) { accent } else { Color32::WHITE };
            painter.rect_filled(square, 0.0, fill);
            painter.rect_stroke(square, 0.0, thin, egui::StrokeKind::Middle);
        });
    }

    fn layer_shapes(&self, bounds: [f32; 4]) -> Vec<String> {
        fn leaves(node: &Node, out: &mut Vec<String>) {
            match &node.shape {
                Shape::Group { children } => children.iter().for_each(|child| leaves(child, out)),
                Shape::Rect { .. } | Shape::Ellipse { .. } | Shape::Path { .. } => out.push(node.id.clone()),
                _ => {}
            }
        }
        let doc = &self.scene.bundle.doc;
        let of_cel = |nodes: &[Node]| {
            let mut out = Vec::new();
            nodes.iter().for_each(|node| leaves(node, &mut out));
            out
        };
        if let Some(Loc::Node(path, c, _)) = self.direct.as_ref().and_then(|d| doc.locate(&d.node)) {
            let layer = doc.layer_at(&path);
            return match layer.locked {
                true => Vec::new(),
                false => of_cel(&layer.cels()[c].nodes),
            };
        }
        let Some(layer) = self.scene.active_layer().filter(|l| !l.locked) else {
            return Vec::new();
        };
        match (layer.kind(), self.scene.active_cel()) {
            (Kind::Vector, _) => layer
                .content_at(self.scene.frame)
                .map_or(Vec::new(), |cel| of_cel(&cel.nodes)),
            (_, Some(cel)) => self
                .scene
                .active_marks()
                .iter()
                .filter_map(|m| m.as_stroke())
                .filter(|stroke| {
                    stroke.samples.iter().any(|sample| {
                        let (x, y) = sample.pos();
                        x >= bounds[0] && y >= bounds[1] && x <= bounds[2] && y <= bounds[3]
                    })
                })
                .map(|stroke| markedit::address(&cel, stroke.id))
                .collect(),
            _ => Vec::new(),
        }
    }

    fn chosen_marks(&self) -> Option<(Picked, Vec<(String, Mark)>)> {
        let picked = self.picked.clone()?;
        let parts = self.picked_parts();
        if parts.is_empty() {
            return None;
        }
        let chosen = parts
            .iter()
            .flat_map(|(_, cel, marks, ids)| {
                marks
                    .iter()
                    .filter(|m| ids.contains(&m.id()))
                    .map(|m| (cel.clone(), m.clone()))
            })
            .collect();
        Some((picked, chosen))
    }

    fn set_picked(&mut self, sets: Vec<(String, u32, &'static str, Value)>) -> Result<()> {
        let Some(picked) = self.picked.clone().filter(|_| !sets.is_empty()) else {
            return Ok(());
        };
        let start = Instant::now();
        let (mut ops, after) = match &picked.cut {
            Some(_) => self
                .picked_ops(LassoAction::Transform(MarkTransform::default()))
                .ok_or_else(|| anyhow!("the selection changed underneath: select the strokes again"))?,
            None => (Vec::new(), Some(picked.clone())),
        };
        ops.extend(sets.into_iter().map(|(cel, id, key, value)| Op::Set {
            id: address(&cel, id),
            props: Map::from_iter([(key.to_string(), value)]),
        }));
        self.apply(ops)?;
        self.picked = after;
        self.forget_missing_selection();
        self.measure("stroke edit", format!("stroke edit redrawn in {:.0} ms", ms(start)));
        Ok(())
    }

    pub(super) fn recolourable(&self) -> Option<String> {
        let id = self
            .selection
            .clone()
            .or_else(|| self.direct.as_ref().map(|d| d.node.clone()))?;
        match self.scene.bundle.doc.item(&id) {
            Some(Item::Node(_, _, node))
                if matches!(
                    node.shape,
                    Shape::Rect { .. } | Shape::Ellipse { .. } | Shape::Path { .. } | Shape::Text(_)
                ) =>
            {
                Some(id)
            }
            _ => None,
        }
    }

    fn recolour_shape(&mut self, outline: bool) -> Result<()> {
        self.recolour_shape_to(outline, self.color)
    }

    fn recolour_shape_to(&mut self, outline: bool, rgba: [f32; 4]) -> Result<()> {
        let Some(id) = self.recolourable() else {
            return Ok(());
        };
        let hex = color::hex(rgba);
        let node = match self.scene.bundle.doc.item(&id) {
            Some(Item::Node(_, _, node)) => Some(node.clone()),
            _ => None,
        };
        let has_outline = node.as_ref().is_some_and(|n| n.stroke.is_some());
        let now = node.as_ref().and_then(|n| match outline {
            true => n.stroke.as_ref().map(|s| s.paint.clone()),
            false => n.fill.clone(),
        });
        let paint = match now.and_then(|p| regraded(p, rgba)) {
            Some(graded) => serde_json::to_value(graded)?,
            None => json!(hex),
        };
        let props = match (outline, has_outline) {
            (false, _) => json!({"fill": paint}),
            (true, true) => json!({"stroke.paint": paint}),
            (true, false) => json!({"stroke": {"paint": hex, "width": self.pen_width}}),
        };
        let props = props.as_object().cloned().unwrap_or_default();
        self.apply(vec![Op::Set { id, props }])
    }

    pub(super) fn solo(&mut self, id: &str) -> Result<()> {
        let ops = match self.soloed.take() {
            Some((was, before)) if was == id => before
                .into_iter()
                .filter(|(layer, visible)| {
                    self.scene
                        .bundle
                        .doc
                        .layer(layer)
                        .is_some_and(|l| l.visible != *visible)
                })
                .map(|(layer, visible)| Op::Set {
                    id: layer,
                    props: Map::from_iter([("visible".to_string(), json!(visible))]),
                })
                .collect(),
            _ => {
                let before = self
                    .scene
                    .bundle
                    .doc
                    .all_layers()
                    .iter()
                    .map(|l| (l.id.clone(), l.visible))
                    .collect();
                let ops = klad_core::ops::solo(&self.scene.bundle.doc, id)?;
                self.soloed = Some((id.to_string(), before));
                ops
            }
        };
        if ops.is_empty() {
            return Ok(());
        }
        self.apply(ops)
    }

    fn recolour_picked(&mut self) -> Result<()> {
        self.recolour_picked_to(self.color)
    }

    fn dyed(&self, dye: Dye) -> Option<[f32; 4]> {
        let of_node = |pick: &dyn Fn(&klad_core::doc::Node) -> Option<String>| {
            let id = self.recolourable()?;
            match self.scene.bundle.doc.item(&id) {
                Some(Item::Node(_, _, node)) => pick(node).and_then(|hex| color::rgba_of(&hex)),
                _ => None,
            }
        };
        match dye {
            Dye::Strokes => self
                .chosen_marks()
                .and_then(|(_, chosen)| chosen.first().map(|(_, m)| m.color())),
            Dye::Fill => of_node(&|node| node.fill.as_ref().and_then(paint_colour)),
            Dye::Outline => of_node(&|node| node.stroke.as_ref().and_then(|s| paint_colour(&s.paint))),
        }
    }

    pub(super) fn undyeable(&self, outline: bool) -> Vec<String> {
        let picked = match self.shapes() {
            ids if !ids.is_empty() => ids,
            _ => self.direct.as_ref().map(|d| d.node.clone()).into_iter().collect(),
        };
        picked
            .into_iter()
            .filter(|id| match self.scene.bundle.doc.item(id) {
                Some(Item::Node(_, _, node)) => {
                    matches!(
                        node.shape,
                        Shape::Rect { .. } | Shape::Ellipse { .. } | Shape::Path { .. } | Shape::Text(_)
                    ) && match outline {
                        true => node.stroke.is_some(),
                        false => !matches!(&node.fill, Some(Paint::Solid(c)) if c == "none"),
                    }
                }
                _ => false,
            })
            .collect()
    }

    pub(super) fn undye(&mut self, outline: bool) -> Result<()> {
        let (key, value) = match outline {
            true => ("stroke", Value::Null),
            false => ("fill", json!("none")),
        };
        let ops = self
            .undyeable(outline)
            .into_iter()
            .map(|id| Op::Set {
                id,
                props: Map::from_iter([(key.to_string(), value.clone())]),
            })
            .collect();
        self.apply(ops)
    }

    fn dye(&mut self, dye: Dye, rgba: [f32; 4]) -> Result<()> {
        if self.dyed(dye).is_some_and(|now| color::hex(now) == color::hex(rgba)) {
            return Ok(());
        }
        match dye {
            Dye::Strokes => self.recolour_picked_to(rgba)?,
            Dye::Fill => self.recolour_shape_to(false, rgba)?,
            Dye::Outline => self.recolour_shape_to(true, rgba)?,
        }
        if self.settings.used(&color::hex(rgba)) {
            self.settings_dirty = true;
        }
        Ok(())
    }

    fn dye_button(&mut self, ui: &mut egui::Ui, cmd: Cmd, dye: Dye) {
        let spec = cmd.spec();
        let refused = self.can_run(cmd);
        let now = self.dyed(dye);
        let hint = match (&refused, now) {
            (Some(why), _) => format!("{}\n\nnot now: {why}", dye.what()),
            (None, Some(rgba)) => format!(
                "{}, {}: click to choose another, from the wheel, the palette or the colour you paint with",
                dye.what(),
                color::hex(rgba)
            ),
            (None, None) => format!(
                "{}: click to choose one, from the wheel, the palette or the colour you paint with",
                dye.what()
            ),
        };
        let button = egui::Button::new(egui::RichText::new(spec.icon).size(crate::theme::ICON))
            .min_size(vec2(crate::theme::CONTROL, crate::theme::CONTROL));
        let enabled = refused.is_none();
        let response = ui.add_enabled(enabled, button).hint(hint);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, dye.what()));
        ui.painter().rect_filled(
            Rect::from_min_size(response.rect.right_bottom() - vec2(9.0, 9.0), vec2(7.0, 7.0)),
            1.0,
            color::color32(now.unwrap_or(self.color)),
        );
        if !enabled {
            return;
        }
        let named = |hex: &str, name: &str| match name.is_empty() {
            true => hex.to_string(),
            false => format!("{name}  {hex}"),
        };
        let palette = self
            .scene
            .bundle
            .doc
            .palette
            .iter()
            .filter_map(|s| color::rgba_of(&s.color).map(|rgba| (rgba, named(&s.color, &s.name))))
            .collect();
        let recent = self
            .settings
            .recent
            .iter()
            .filter_map(|hex| color::rgba_of(hex).map(|rgba| (rgba, format!("{hex}, used lately"))))
            .collect();
        let none = match dye {
            Dye::Fill => Some((Cmd::NoFill, "no fill", "the shape gets no fill: only its outline shows")),
            Dye::Outline => Some((Cmd::NoOutline, "no outline", "the shape's outline comes off")),
            Dye::Strokes => None,
        }
        .filter(|(cmd, _, _)| self.can_run(*cmd).is_none());
        let square = color::Square::new(ui, ("dye", dye.what()), now.unwrap_or(self.color), dye.what())
            .current(self.color)
            .upward(self.bar_above)
            .colours("palette", palette)
            .colours("recent", recent);
        let square = match none {
            Some((_, label, hint)) => square.removable_as(label, hint),
            None => square,
        };
        match square.under(ui, &response) {
            Some(color::Pick::Chosen(rgba)) => self.check(|e| e.dye(dye, rgba)),
            Some(color::Pick::Remove) => {
                if let Some((cmd, _, _)) = none {
                    self.queued.push(cmd);
                }
            }
            _ => {}
        }
    }

    fn recolour_picked_to(&mut self, target: [f32; 4]) -> Result<()> {
        let sets = self.chosen_marks().map_or(Vec::new(), |(_, chosen)| {
            let colours: Vec<_> = chosen.iter().map(|(_, m)| m.color()).collect();
            klad_core::color::recolour(&colours, target)
                .into_iter()
                .zip(&chosen)
                .map(|(shade, (cel, m))| {
                    let colour = match m.as_fill().is_some_and(|f| f.on_grid()) {
                        true => target,
                        false => shade,
                    };
                    (cel.clone(), m.id(), "color", json!(klad_core::color::to_hex(colour)))
                })
                .collect()
        });
        self.set_picked(sets)
    }

    fn select_same_colour(&mut self) {
        let Some(cel) = self.scene.active_cel() else {
            return;
        };
        let marks = self.scene.active_marks();
        let key = |m: &Mark| {
            m.as_fill().filter(|f| f.on_grid() && !f.erases()).map(|f| {
                f.entry()
                    .map_or_else(|| klad_core::color::to_hex(f.color()), String::from)
            })
        };
        let held: Vec<u32> = self
            .picked
            .as_ref()
            .filter(|p| p.cel == cel)
            .map_or(Vec::new(), |p| p.ids.clone());
        let wanted: BTreeSet<String> = match held.is_empty() {
            false => marks
                .iter()
                .filter(|m| held.contains(&m.id()))
                .filter_map(key)
                .collect(),
            true => {
                let current = klad_core::color::to_hex(self.color);
                marks
                    .iter()
                    .filter(|m| klad_core::color::to_hex(m.color()).eq_ignore_ascii_case(&current))
                    .filter_map(key)
                    .collect()
            }
        };
        let ids: Vec<u32> = marks
            .iter()
            .filter(|m| key(m).is_some_and(|k| wanted.contains(&k)))
            .map(Mark::id)
            .collect();
        match ids.is_empty() {
            true => self.say(
                Tone::Info,
                "no pixels of this drawing have that colour: click a palette colour, or select a mark first",
            ),
            false => {
                let count = ids.len();
                self.tool = Tool::Strokes;
                self.picked = Some(Picked::whole(cel, ids));
                self.say(Tone::Done, format!("{count} marks of that colour selected"));
            }
        }
    }

    fn attached_parts(&self) -> Vec<(String, String, Vec<u32>)> {
        self.picked_parts()
            .into_iter()
            .map(|(layer, cel, marks, ids)| {
                let chosen: BTreeSet<u32> = ids.iter().copied().collect();
                let attached = klad_core::stroke::strokes_of(marks)
                    .filter(|s| !chosen.contains(&s.id) && s.on.iter().any(|t| chosen.contains(t)))
                    .map(|s| s.id)
                    .collect();
                (layer, cel, attached)
            })
            .collect()
    }

    pub(super) fn attached_count(&self) -> usize {
        self.attached_parts().iter().map(|(_, _, ids)| ids.len()).sum()
    }

    pub(super) fn detachable(&self) -> bool {
        self.attached_count() > 0
            || self.picked_parts().iter().any(|(_, _, marks, ids)| {
                let live: BTreeSet<u32> = marks.iter().map(Mark::id).collect();
                klad_core::stroke::strokes_of(marks)
                    .any(|s| ids.contains(&s.id) && s.on.iter().any(|t| live.contains(t)))
            })
    }

    fn select_attached(&mut self) -> Result<()> {
        let Some(picked) = self.picked.clone() else {
            return Ok(());
        };
        if picked.cut.is_some() {
            self.say(
                Tone::Refused,
                "the lasso cut has not happened yet: move or edit the strokes first, then select what is attached",
            );
            return Ok(());
        }
        let parts = self.attached_parts();
        let count: usize = parts.iter().map(|(_, _, ids)| ids.len()).sum();
        let mut parts = parts.into_iter();
        let Some((_, cel, ids)) = parts.next() else {
            return Ok(());
        };
        self.picked = Some(Picked {
            more: parts
                .filter(|(_, _, ids)| !ids.is_empty())
                .map(|(layer, cel, ids)| Part { layer, cel, ids })
                .collect(),
            ..Picked::whole(cel, ids)
        });
        self.say(
            Tone::Done,
            format!("{count} attached selected: they move and go on their own now, and attach to what they land on"),
        );
        Ok(())
    }

    fn detach_picked(&mut self) -> Result<()> {
        let Some(picked) = self.picked.clone() else {
            return Ok(());
        };
        let count = self.attached_count();
        let (mut ops, after) = match &picked.cut {
            Some(_) => self
                .picked_ops(LassoAction::Transform(MarkTransform::default()))
                .ok_or_else(|| anyhow!("the selection changed underneath: select the strokes again"))?,
            None => (Vec::new(), Some(picked.clone())),
        };
        let ids: Vec<String> = after
            .iter()
            .flat_map(|p| {
                std::iter::once((p.cel.clone(), p.ids.clone()))
                    .chain(p.more.iter().map(|m| (m.cel.clone(), m.ids.clone())))
            })
            .flat_map(|(cel, ids)| ids.into_iter().map(move |id| address(&cel, id)))
            .collect();
        ops.push(Op::Detach { ids });
        self.apply(ops)?;
        self.picked = after;
        self.forget_missing_selection();
        self.say(
            Tone::Done,
            format!("{count} detached: they stay where they are and act on whatever is under them"),
        );
        Ok(())
    }

    fn size_picked(&mut self, k: f32) -> Result<()> {
        let sets = self.chosen_marks().map_or(Vec::new(), |(_, chosen)| {
            chosen
                .iter()
                .filter_map(|(cel, m)| {
                    m.as_stroke()
                        .map(|s| (cel.clone(), s.id, "size", json!((s.size * k).max(0.1))))
                })
                .collect()
        });
        self.set_picked(sets)
    }

    fn flip_picked(&mut self, sx: f32, sy: f32) -> Result<()> {
        let middle = self
            .picked_corners()
            .map(|(_, c)| [(c[0].x + c[2].x) / 2.0, (c[0].y + c[2].y) / 2.0]);
        let start = Instant::now();
        self.edit_picked(LassoAction::Transform(MarkTransform {
            sx,
            sy,
            pivot: middle,
            ..Default::default()
        }))?;
        self.measure("stroke edit", format!("stroke edit redrawn in {:.0} ms", ms(start)));
        Ok(())
    }

    fn select_all_strokes(&mut self) -> Result<()> {
        let pickable = |marks: &[Mark]| -> Vec<u32> {
            let riding = klad_core::attach::following(marks);
            marks.iter().map(Mark::id).filter(|id| !riding.contains(id)).collect()
        };
        let every = self.settings.pick_all_layers;
        if every
            && pickable(self.scene.active_marks()).is_empty()
            && let Some((layer, _)) = self
                .pick_cels(true)
                .into_iter()
                .find(|(_, cel)| !pickable(self.scene.marks_of(cel)).is_empty())
            && let Some(klad_core::doc::Loc::Layer(path)) = self.scene.bundle.doc.locate(&layer)
        {
            self.activate(path)?;
        }
        let ids = pickable(self.scene.active_marks());
        let more: Vec<Part> = self
            .pick_cels(every)
            .into_iter()
            .map(|(layer, cel)| Part {
                ids: pickable(self.scene.marks_of(&cel)),
                layer,
                cel,
            })
            .filter(|p| !p.ids.is_empty())
            .collect();
        if let Some(cel) = self.scene.active_cel().filter(|_| !ids.is_empty() || !more.is_empty()) {
            if self.tool != Tool::Select {
                self.tool = Tool::Strokes;
            }
            self.picked = Some(Picked {
                more,
                ..Picked::whole(cel, ids)
            });
        }
        Ok(())
    }

    fn strokes_inspector(&mut self, ui: &mut egui::Ui) -> Result<()> {
        let Some((picked, chosen)) = self.chosen_marks() else {
            ui.label(crate::commands::local_keys(&match self.also.is_empty() {
                true => "click a stroke or drag a loop around strokes on the active paint layer (shift adds); cmd click layers in the layers panel to select on several at once".to_string(),
                false => format!("click a stroke or drag a loop: it selects on the active layer and {} more (cmd clicked in the layers panel)", self.also.len()),
            }));
            return Ok(());
        };
        let fills = chosen.iter().filter(|(_, m)| m.as_fill().is_some()).count();
        let place = match picked.more.len() {
            0 => self
                .scene
                .bundle
                .doc
                .paint_cels()
                .into_iter()
                .find(|(_, c)| c.id == picked.cel)
                .map_or_else(String::new, |(l, c)| format!("on {}, frame {}", l.name, c.frame)),
            n => format!("on {} layers", n + 1),
        };
        let attached = match self.attached_count() {
            0 => String::new(),
            n => format!(", {n} attached"),
        };
        ui.label(format!(
            "{} strokes, {fills} fills{attached} selected {place}{}",
            chosen.len() - fills,
            if picked.cut.is_some() {
                "\n(with the parts crossing the lasso edge)"
            } else {
                ""
            }
        ));
        ui.small("the bar under the selection edits it; drag inside the box to move, a corner to scale, an edge to stretch (past the other edge flips), the knob to turn; arrows nudge");
        ui.separator();
        ui.label(format!("part: {}", self.picked_part_names().join(", ")));
        let mut name = false;
        ui.horizontal(|ui| {
            let label = ui.label("name");
            let field = ui
                .add(
                    egui::TextEdit::singleline(&mut self.checks.part)
                        .desired_width(120.0)
                        .hint_text("leaf, near leg"),
                )
                .labelled_by(label.id)
                .hint("the part's name; Return or the button names the selected marks");
            name = ui
                .button("make them this part")
                .hint("name the selected marks as this part, as klad part does")
                .clicked()
                || (field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
        });
        ui.small(
            "in-betweens pair marks by part and klad arcs tracks them; an empty name takes the marks out of their part",
        );
        if name {
            self.name_part()?;
        }
        Ok(())
    }

    fn path_inspector(&mut self, ui: &mut egui::Ui) -> Result<()> {
        let Some(direct) = self.direct.clone() else {
            ui.label("click a stroke, path, rect or ellipse to edit its points");
            return Ok(());
        };
        if markedit::parse_address(&direct.node).is_some() {
            let points = self
                .shaped
                .as_ref()
                .map_or(0, |s| s.subs.iter().map(|p| p.anchors.len()).sum());
            ui.label(format!("a stroke: {points} points fitted to it"));
            ui.small("drag a point or a handle to bend the stroke (it keeps its pressure); alt click the line adds a point; Delete removes the selected points");
            return Ok(());
        }
        let Some(Item::Node(layer, _, node)) = self.scene.bundle.doc.item(&direct.node) else {
            self.direct = None;
            return Ok(());
        };
        let (kind, layer_name, is_path) = (
            node.type_name(),
            layer.name.clone(),
            matches!(node.shape, Shape::Path { .. }),
        );
        let total = self
            .shaped
            .as_ref()
            .map_or(0, |s| s.subs.iter().map(|p| p.anchors.len()).sum::<usize>());
        ui.label(format!(
            "{kind}{} on {layer_name}: {total} points, {} selected",
            if node.name.is_empty() {
                String::new()
            } else {
                format!(" \"{}\"", node.name)
            },
            direct.points.len()
        ));
        if !direct.also.is_empty() {
            ui.label(format!(
                "and {} points on {} more on this layer: drag one to move them all",
                direct.also.values().map(BTreeSet::len).sum::<usize>(),
                direct.also.len()
            ));
        }
        if !is_path {
            ui.label(format!("any point edit turns this {kind} into a path"));
        }
        if self.under.len() > 1 && self.under.contains(&direct.node) {
            ui.label("under the last click (clicking the shape again steps down):");
            let doc = &self.scene.bundle.doc;
            let rows: Vec<(String, String)> = self
                .under
                .iter()
                .take(12)
                .map(|id| {
                    let what = match doc.item(id) {
                        Some(Item::Node(_, _, n)) => format!("{id} {} {}", n.type_name(), n.name),
                        _ => id.clone(),
                    };
                    (id.clone(), what)
                })
                .collect();
            let mut pick = None;
            rows.into_iter().for_each(|(id, what)| {
                if ui.selectable_label(id == direct.node, what).clicked() {
                    pick = Some(id);
                }
            });
            if let Some(id) = pick {
                self.pick_shape(id);
                return Ok(());
            }
        }
        ui.small(
            "drag points or handles; drag a box to select points of this shape, shift and drag a box for the points of every shape on the layer; drag one to move them all\n\
             alt click the outline: new point; alt click a point: corner or smooth; alt drag a handle: break it",
        );
        if self.path_commit.is_some() {
            ui.label("writing the last edit...");
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn number(
        &mut self,
        ui: &mut egui::Ui,
        key: &str,
        label: &str,
        current: f32,
        range: RangeInclusive<f32>,
        speed: f64,
        hint: &str,
    ) -> Option<f32> {
        let mut value = self.edits.get(key).copied().unwrap_or(current);
        let response = ui
            .add(egui::DragValue::new(&mut value).speed(speed).range(range).prefix(label))
            .hint(hint);
        if response.dragged() || response.has_focus() {
            self.edits.insert(key.to_string(), value);
            return None;
        }
        let pending = self.edits.remove(key);
        if response.drag_stopped() || response.lost_focus() || response.changed() {
            return Some(value);
        }
        pending.filter(|p| (p - current).abs() > f32::EPSILON)
    }

    fn fraction(&mut self, ui: &mut egui::Ui, key: &str, label: &str, current: f32, hint: &str) -> Option<f32> {
        let mut value = self.edits.get(key).copied().unwrap_or(current);
        let response = ui
            .add(
                egui::DragValue::new(&mut value)
                    .speed(0.01)
                    .range(0.0..=1.0)
                    .prefix(label)
                    .custom_formatter(|v, _| format!("{:.0}%", v * 100.0))
                    .custom_parser(|text| {
                        let trimmed = text.trim();
                        let shown = trimmed.trim_end_matches('%').trim().parse::<f64>().ok()?;
                        Some(match trimmed.ends_with('%') || shown > 1.0 {
                            true => shown / 100.0,
                            false => shown,
                        })
                    }),
            )
            .hint(hint);
        if response.dragged() || response.has_focus() {
            self.edits.insert(key.to_string(), value);
            return None;
        }
        let pending = self.edits.remove(key);
        if response.drag_stopped() || response.lost_focus() || response.changed() {
            return Some(value);
        }
        pending.filter(|p| (p - current).abs() > f32::EPSILON)
    }

    fn text(&mut self, ui: &mut egui::Ui, key: &str, label: &str, current: &str, hint: &str) -> Option<String> {
        let mut value = self.texts.get(key).cloned().unwrap_or_else(|| current.to_string());
        let label = ui.label(label);
        let response = ui.text_edit_singleline(&mut value).labelled_by(label.id).hint(hint);
        if response.has_focus() {
            self.texts.insert(key.to_string(), value);
            return None;
        }
        self.texts.remove(key);
        (response.lost_focus() && value != current).then_some(value)
    }

    fn node_inspector(&mut self, ui: &mut egui::Ui, id: &str) -> Result<()> {
        let Some(Item::Node(layer, _, node)) = self.scene.bundle.doc.item(id) else {
            self.selection = None;
            return Ok(());
        };
        let (node, name) = (node.clone(), layer.name.clone());
        ui.label(format!("{} on {name}", node.type_name()));
        let mut props: Vec<(String, Value)> = Vec::new();
        ui.horizontal(|ui| {
            if let Some(name) = self.text(
                ui,
                &format!("{id}.name"),
                "name",
                &node.name,
                "the shape's name; Return keeps it",
            ) {
                props.push(("name".into(), json!(name)));
            }
        });
        let t = node.transform;
        ui.horizontal_wrapped(|ui| {
            if let Some(v) = self.number(
                ui,
                &format!("{id}.x"),
                "x ",
                t.x,
                -1e5..=1e5,
                1.0,
                "move it across, in canvas px",
            ) {
                props.push(("transform.x".into(), json!(v)));
            }
            if let Some(v) = self.number(
                ui,
                &format!("{id}.y"),
                "y ",
                t.y,
                -1e5..=1e5,
                1.0,
                "move it down, in canvas px",
            ) {
                props.push(("transform.y".into(), json!(v)));
            }
            if let Some(v) = self.number(
                ui,
                &format!("{id}.r"),
                "rotate ",
                t.rotate,
                -3600.0..=3600.0,
                0.5,
                "turn it, in degrees clockwise",
            ) {
                props.push(("transform.rotate".into(), json!(v)));
            }
            if let Some(v) = self.number(
                ui,
                &format!("{id}.sx"),
                "sx ",
                t.sx,
                -100.0..=100.0,
                0.01,
                "scale across; negative mirrors",
            ) {
                props.push(("transform.sx".into(), json!(v)));
            }
            if let Some(v) = self.number(
                ui,
                &format!("{id}.sy"),
                "sy ",
                t.sy,
                -100.0..=100.0,
                0.01,
                "scale up and down; negative mirrors",
            ) {
                props.push(("transform.sy".into(), json!(v)));
            }
        });
        if let Some(paint) = self.paint_editor(ui, id, &node) {
            props.push(("fill".into(), serde_json::to_value(paint)?));
        }
        if let Shape::Rays(rays) = &node.shape {
            let changed = ui.horizontal_wrapped(|ui| self.rays_section(ui, id, rays)).inner;
            if let Some(Value::Object(fields)) = changed {
                props.extend(fields);
            }
        }
        props.extend(self.shape_fields(ui, id, &node));
        ui.horizontal(|ui| {
            if let Some(v) = self.number(
                ui,
                &format!("{id}.o"),
                "opacity ",
                node.opacity,
                0.0..=1.0,
                0.01,
                "the shape's opacity",
            ) {
                props.push(("opacity".into(), json!(v)));
            }
            if let Some(v) = self.number(
                ui,
                &format!("{id}.b"),
                "blur ",
                node.blur,
                0.0..=200.0,
                0.2,
                "soften the whole shape, in px",
            ) {
                props.push(("blur".into(), json!(v)));
            }
        });
        ui.horizontal(|ui| self.check(|e| e.outline_row(ui)));
        let mut remove = false;
        ui.horizontal(|ui| {
            let mut visible = node.visible;
            if ui
                .checkbox(&mut visible, "visible")
                .hint("show or hide this shape")
                .changed()
            {
                props.push(("visible".into(), json!(visible)));
            }
            remove = ui.button("delete").hint("remove this shape (Delete)").clicked();
            if ui.button("deselect").hint("clear the selection (Esc)").clicked() {
                self.selection = None;
            }
        });
        self.set(id, props)?;
        if remove {
            self.selection = None;
            self.apply(vec![Op::Remove {
                ids: vec![id.to_string()],
            }])?;
            return Ok(());
        }
        self.animation_section(ui, id)
    }

    fn paint_editor(&mut self, ui: &mut egui::Ui, id: &str, node: &Node) -> Option<Paint> {
        let first = |stops: &[Stop]| stops.first().map_or("#000000".to_string(), |s| s.1.clone());
        let (kind, base) = match &node.fill {
            Some(Paint::Solid(hex)) if hex == "none" => ("none", "#000000".to_string()),
            Some(Paint::Solid(hex)) => ("solid", hex.clone()),
            None => ("solid", "#000000".to_string()),
            Some(Paint::Linear { linear }) => ("linear", first(&linear.stops)),
            Some(Paint::Radial { radial }) => ("radial", first(&radial.stops)),
        };
        let mut chosen = kind;
        let mut out: Option<Paint> = None;
        ui.horizontal(|ui| {
            ui.label("fill");
            egui::ComboBox::from_id_salt(format!("{id}.fillkind"))
                .selected_text(kind)
                .show_ui(ui, |ui| {
                    ["none", "solid", "linear", "radial"].iter().for_each(|k| {
                        ui.selectable_value(&mut chosen, *k, *k);
                    });
                })
                .response
                .hint("no fill, one colour, or a linear or radial gradient");
            if kind == "solid"
                && let Some(rgba) = color::Square::new(
                    ui,
                    format!("{id}.fill"),
                    color::rgba_of(&base).unwrap_or([0.0, 0.0, 0.0, 1.0]),
                    "the fill colour",
                )
                .current(self.color)
                .alpha()
                .pick(ui)
            {
                out = Some(Paint::Solid(color::hex(rgba)));
            }
        });
        if chosen != kind {
            let [x, y, w, h] = klad_render::vector::local_bounds(node)
                .ok()
                .flatten()
                .map_or([0.0, 0.0, 100.0, 100.0], |r| [r.left, r.top, r.width(), r.height()]);
            let stops = vec![Stop(0.0, base.clone()), Stop(1.0, "#ffffff".into())];
            return Some(match chosen {
                "none" => Paint::Solid("none".into()),
                "solid" => Paint::Solid(base),
                "linear" => Paint::Linear {
                    linear: Linear {
                        from: [x + w / 2.0, y],
                        to: [x + w / 2.0, y + h],
                        stops,
                    },
                },
                _ => Paint::Radial {
                    radial: Radial {
                        center: [x + w / 2.0, y + h / 2.0],
                        radius: (w.max(h) / 2.0).max(1.0),
                        stops,
                    },
                },
            });
        }
        match &node.fill {
            Some(Paint::Linear { linear }) => {
                let mut next = linear.clone();
                let mut changed = false;
                ui.horizontal(|ui| {
                    [("fx", 0, "from x "), ("fy", 1, "y ")]
                        .iter()
                        .for_each(|(k, c, label)| {
                            if let Some(v) = self.number(
                                ui,
                                &format!("{id}.g.{k}"),
                                label,
                                linear.from[*c],
                                -1e5..=1e5,
                                1.0,
                                "where the gradient starts, in the shape's own px",
                            ) {
                                next.from[*c] = v;
                                changed = true;
                            }
                        });
                });
                ui.horizontal(|ui| {
                    [("tx", 0, "to x "), ("ty", 1, "y ")].iter().for_each(|(k, c, label)| {
                        if let Some(v) = self.number(
                            ui,
                            &format!("{id}.g.{k}"),
                            label,
                            linear.to[*c],
                            -1e5..=1e5,
                            1.0,
                            "where the gradient ends, in the shape's own px",
                        ) {
                            next.to[*c] = v;
                            changed = true;
                        }
                    });
                });
                if let Some(stops) = self.stops_editor(ui, id, &linear.stops) {
                    next.stops = stops;
                    changed = true;
                }
                changed.then_some(Paint::Linear { linear: next }).or(out)
            }
            Some(Paint::Radial { radial }) => {
                let mut next = radial.clone();
                let mut changed = false;
                ui.horizontal(|ui| {
                    [("cx", 0, "centre x "), ("cy", 1, "y ")]
                        .iter()
                        .for_each(|(k, c, label)| {
                            if let Some(v) = self.number(
                                ui,
                                &format!("{id}.g.{k}"),
                                label,
                                radial.center[*c],
                                -1e5..=1e5,
                                1.0,
                                "the centre of the gradient, in the shape's own px",
                            ) {
                                next.center[*c] = v;
                                changed = true;
                            }
                        });
                    if let Some(v) = self.number(
                        ui,
                        &format!("{id}.g.r"),
                        "radius ",
                        radial.radius,
                        0.1..=1e5,
                        1.0,
                        "how far the gradient reaches",
                    ) {
                        next.radius = v;
                        changed = true;
                    }
                });
                if let Some(stops) = self.stops_editor(ui, id, &radial.stops) {
                    next.stops = stops;
                    changed = true;
                }
                changed.then_some(Paint::Radial { radial: next }).or(out)
            }
            _ => out,
        }
    }

    fn stops_editor(&mut self, ui: &mut egui::Ui, id: &str, stops: &[Stop]) -> Option<Vec<Stop>> {
        let mut next = stops.to_vec();
        let mut changed = false;
        let mut remove = None;
        stops.iter().enumerate().for_each(|(i, Stop(at, hex))| {
            ui.horizontal(|ui| {
                if let Some(v) = self.number(
                    ui,
                    &format!("{id}.g.s{i}"),
                    "stop ",
                    *at,
                    0.0..=1.0,
                    0.005,
                    "where this colour sits along the gradient, 0 to 1",
                ) {
                    next[i].0 = v;
                    changed = true;
                }
                if let Some(rgba) = color::Square::new(
                    ui,
                    format!("{id}.g.c{i}"),
                    color::rgba_of(hex).unwrap_or([0.0, 0.0, 0.0, 1.0]),
                    "this stop's colour",
                )
                .current(self.color)
                .alpha()
                .pick(ui)
                {
                    next[i].1 = color::hex(rgba);
                    changed = true;
                }
                if stops.len() > 1 && ui.button(crate::theme::icon::CLOSE).hint("remove this stop").clicked() {
                    remove = Some(i);
                }
            });
        });
        if let Some(i) = remove {
            next.remove(i);
            changed = true;
        }
        if ui
            .button("add stop")
            .hint("a stop between the last two, in the current colour")
            .clicked()
        {
            let at = match stops {
                [.., a, b] => (a.0 + b.0) / 2.0,
                _ => 0.5,
            };
            next.push(Stop(at, color::hex(self.color)));
            changed = true;
        }
        if changed {
            next.sort_by(|a, b| a.0.total_cmp(&b.0));
        }
        changed.then_some(next)
    }

    fn activate(&mut self, path: Vec<usize>) -> Result<()> {
        if self.live.is_some() {
            return Ok(());
        }
        let id = self.scene.bundle.doc.locate_path(&path).map(|l| l.id.clone());
        let tinted = self.scene.tint_mask && (self.scene.active_is_mask() || path.last() == Some(&MASK));
        if path != self.scene.active {
            self.timeline.kept = (self.canvas_current() && !self.scene.onion_on && !tinted).then_some((
                self.scene.frame,
                self.scene.bundle.doc.rev,
                self.canvas_gen,
            ));
        }
        self.scene.set_active_later(path);
        if self.scene.onion_on {
            self.frames_changed();
        }
        self.also.retain(|a| Some(a) != id.as_ref());
        self.selection = None;
        self.picked = None;
        Ok(())
    }

    fn mask_inspector(&mut self, ui: &mut egui::Ui) -> Result<()> {
        if self.scene.active_layer().is_none() {
            return Ok(());
        }
        let owner_path = self.scene.active[..self.scene.active.len() - 1].to_vec();
        let owner = self.scene.bundle.doc.layer_at(&owner_path).clone();
        ui.label(format!("the mask of {}", owner.name));
        ui.colored_label(Color32::from_rgb(255, 150, 150), self.mask_hint());
        ui.horizontal_wrapped(|ui| {
            self.command_check(ui, Cmd::ToggleMask, "mask on");
            self.command_check(ui, Cmd::TintMask, "show the mask in red");
        });
        ui.horizontal_wrapped(|ui| {
            self.command_button(ui, Cmd::BackToLayer, Some("back to the layer"));
            self.command_button(ui, Cmd::RemoveMask, Some("remove mask"));
        });
        Ok(())
    }

    fn mask_hint(&self) -> &'static str {
        if self.brush == "eraser" {
            "the eraser shows the layer again where the mask hid it (E switches back to the brush)"
        } else {
            "the brush hides the layer where you paint (any colour), the red marks what is hidden; the eraser (E) shows it again; untick show the mask in red, in Layer settings, to see the layer without it"
        }
    }

    fn layer_inspector(&mut self, ui: &mut egui::Ui) -> Result<()> {
        if self.scene.active_is_mask() {
            return self.mask_inspector(ui);
        }
        let Some(layer) = self.scene.active_layer().cloned() else {
            ui.label("no layer selected");
            return Ok(());
        };
        let id = layer.id.clone();
        ui.label(match layer.map {
            Some(role) => format!("{} map layer", role.name()),
            None => format!("{} layer", layer.kind_name()),
        });
        let mut props: Vec<(String, Value)> = Vec::new();
        ui.horizontal(|ui| {
            if let Some(name) = self.text(
                ui,
                &format!("{id}.name"),
                "name",
                &layer.name,
                "the layer's name; Return keeps it",
            ) {
                props.push(("name".into(), json!(name)));
            }
        });
        if layer.kind() == Kind::Bones {
            self.command_check(ui, Cmd::ToggleLocked, "locked");
            let linked = klad_core::rig::painted(&self.scene.bundle.doc, &id).len();
            ui.label(
                egui::RichText::new(format!(
                    "{} bones, {linked} layers rigged to them; draw and pose them with the bone tool (J)",
                    layer.bones.len()
                ))
                .color(crate::theme::WEAK),
            );
            return self.set(&id, props);
        }
        self.set(&id, std::mem::take(&mut props))?;
        self.adjust_section(ui, &layer)?;
        self.map_section(ui, &layer)?;
        ui.horizontal_wrapped(|ui| {
            self.command_check(ui, Cmd::ToggleLocked, "locked");
            self.command_check(ui, Cmd::ToggleClip, "clip to layer below");
            if layer.kind() == Kind::Paint {
                self.command_check(ui, Cmd::ToggleLockAlpha, "lock alpha");
            }
        });
        self.light_section(ui, &layer)?;
        ui.horizontal_wrapped(|ui| match &layer.mask {
            Some(_) => {
                self.command_check(ui, Cmd::ToggleMask, "mask on");
                self.command_button(ui, Cmd::PaintMask, Some("paint on the mask"));
                self.command_button(ui, Cmd::RemoveMask, Some("remove mask"));
            }
            None => {
                self.command_button(ui, Cmd::AddMask, Some("add mask"));
                self.command_button(ui, Cmd::AddMaskHiding, Some("add mask hiding all"));
            }
        });
        ui.horizontal_wrapped(|ui| {
            let mut reference = layer.reference;
            if ui
                .checkbox(&mut reference, "reference")
                .hint("shown here to trace over, left out of renders and exports (klad render --references shows it)")
                .changed()
            {
                props.push(("reference".into(), json!(reference)));
            }
        });
        self.set(&id, props)?;
        if layer.map.is_none() {
            self.depth_section(ui, &layer)?;
        }
        self.effects_section(ui, &layer)?;
        self.animation_section(ui, &id)
    }

    fn colour_panel(&mut self, ui: &mut egui::Ui) -> Result<()> {
        if self.on_normal_layer() {
            return self.normal_panel(ui);
        }
        if self.on_height_layer() {
            return self.height_panel(ui);
        }
        let mut picked: Option<[f32; 4]> = None;
        let edit = match self.scene.bundle.doc.pixel_art() {
            true => {
                let edit = self.palette_part(ui);
                self.colour_mixer(ui, &mut picked);
                edit
            }
            false => {
                self.colour_mixer(ui, &mut picked);
                self.palette_part(ui)
            }
        };
        if !self.settings.recent.is_empty() {
            crate::widgets::form_label(ui, "recent");
            let recent = self.settings.recent.clone();
            let current = color::hex(self.color);
            ui.horizontal_wrapped(|ui| {
                recent.iter().for_each(|hex| {
                    if let Some(rgba) = color::rgba_of(hex)
                        && color::swatch(ui, rgba, hex.eq_ignore_ascii_case(&current), crate::theme::CONTROL)
                            .hint(format!("{hex}, used lately"))
                            .clicked()
                    {
                        picked = Some(rgba);
                    }
                });
            });
        }
        if let Some(rgba) = picked {
            self.pick_new_colour(rgba);
        }
        match edit {
            Some(edit) => self.apply(vec![Op::Palette(edit)]),
            None => Ok(()),
        }
    }

    fn colour_mixer(&mut self, ui: &mut egui::Ui, picked: &mut Option<[f32; 4]>) {
        let mut hsva = self.hsva;
        ui.horizontal_top(|ui| {
            if color::picker(ui, &mut hsva, 150.0).changed() {
                self.color = hsva.rgba();
                self.hsva = hsva;
            }
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                ui.horizontal(|ui| {
                    color::swatch(ui, self.color, false, crate::theme::CONTROL).hint("the current colour");
                    if let Some(text) = self.text(
                        ui,
                        "colour.hex",
                        "hex",
                        &color::hex(self.color),
                        "type #rrggbb or #rrggbbaa and press Return",
                    ) {
                        match color::rgba_of(&text) {
                            Some(rgba) => *picked = Some(rgba),
                            None => self.say(
                                Tone::Refused,
                                format!("{text} is not a colour: type it as #rrggbb or #rrggbbaa, for example #c8a070"),
                            ),
                        }
                    }
                });
                self.colour_channels(ui);
            });
        });
    }

    fn palette_part(&mut self, ui: &mut egui::Ui) -> Option<klad_core::ops::PaletteEdit> {
        crate::widgets::form_label(ui, "palette");
        let edit = self.palette_row(ui);
        let variants: Vec<String> = self
            .scene
            .bundle
            .doc
            .variant_names()
            .into_iter()
            .map(String::from)
            .collect();
        let shown = self.scene.bundle.doc.shown_variant().to_string();
        let mut show = None;
        if variants.len() > 1 {
            ui.horizontal_wrapped(|ui| {
                ui.label("variant");
                variants.iter().for_each(|name| {
                    let hint = match name == klad_core::doc::BASE_VARIANT {
                        true => "show the palette itself".to_string(),
                        false => format!(
                            "show the drawing in the colours of {name}; colours changed while it is shown are its own"
                        ),
                    };
                    if ui.selectable_label(*name == shown, name).hint(hint).clicked() && *name != shown {
                        show = Some(name.clone());
                    }
                });
            });
        }
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = crate::theme::space::XS;
            let label = crate::widgets::form_label(ui, "name");
            ui.add(crate::widgets::field(&mut self.swatch_name, 88.0).hint_text("optional"))
                .labelled_by(label.id)
                .hint("a name for the colour; agents can use it in place of the hex");
            self.command_plain(ui, Cmd::AddSwatch, "add colour");
            if self.can_run(Cmd::UpdateSwatch).is_none() {
                self.command_plain(ui, Cmd::UpdateSwatch, "update");
            }
            self.command_plain(ui, Cmd::AddRamp, "ramp");
            self.command_plain(ui, Cmd::LoadPalette, "palettes");
            if variants.len() > 1 || self.can_run(Cmd::NewVariant).is_none() {
                self.command_plain(ui, Cmd::NewVariant, "new variant");
            }
            if !self.scene.bundle.doc.variants.shown.is_empty() {
                self.command_plain(ui, Cmd::RenameVariant, "rename variant");
                self.command_plain(ui, Cmd::RemoveVariant, "remove variant");
            }
        });
        edit.or(show.map(|name| klad_core::ops::PaletteEdit::Variant { name }))
    }

    fn colour_channels(&mut self, ui: &mut egui::Ui) {
        let mut model = self.settings.colour_model;
        let options: Vec<(color::Model, &str, &str)> = color::Model::ALL
            .into_iter()
            .map(|m| (m, m.name(), m.about()))
            .collect();
        if let Some(m) = crate::widgets::segmented_wide(ui, model, &options) {
            model = m;
        }
        if model != self.settings.colour_model {
            self.settings.colour_model = model;
            self.settings_dirty = true;
        }
        let mut mixing = match self.mixing {
            Some(m) if m.model == model && m.of == self.color => m,
            kept => {
                let keep = match kept {
                    Some(m) if m.model == model => m.values,
                    _ => [self.hsva.h * 360.0, self.hsva.s * 100.0, self.hsva.v * 100.0],
                };
                Mixing {
                    model,
                    values: model.values(self.color, keep),
                    of: self.color,
                }
            }
        };
        let mut alpha = self.color[3] * 100.0;
        let [r, g, b, _] = self.color;
        let row = |ui: &mut egui::Ui, channel: &color::Channel, value: &mut f32, ramp: &dyn Fn(f32) -> [f32; 3]| {
            ui.horizontal(|ui| {
                let gap = crate::theme::space::XS;
                let value_box = 44.0;
                ui.spacing_mut().item_spacing.x = gap;
                ui.add_sized(
                    [10.0, crate::theme::CONTROL],
                    egui::Label::new(egui::RichText::new(channel.name).color(crate::theme::WEAK)),
                );
                let width = (ui.available_width() - value_box - gap).max(CHANNEL_TRACK);
                let slid = color::channel_slider(ui, channel, value, width, ramp).changed();
                let typed = ui
                    .add_sized(
                        [value_box, crate::theme::CONTROL],
                        egui::DragValue::new(value)
                            .range(channel.lo..=channel.hi)
                            .speed((channel.hi - channel.lo) / 200.0)
                            .fixed_decimals(0),
                    )
                    .hint(channel.about)
                    .changed();
                slid || typed
            })
            .inner
        };
        let values = mixing.values;
        let rows: Vec<bool> = model
            .channels()
            .iter()
            .enumerate()
            .map(|(i, channel)| {
                let ramp = |v: f32| {
                    let mut at = values;
                    at[i] = v;
                    model.rgb(at)
                };
                row(ui, channel, &mut mixing.values[i], &ramp)
            })
            .collect();
        let mixed = rows.contains(&true);
        let faded = row(ui, &ALPHA, &mut alpha, &|v| {
            let t = v / 100.0;
            [r, g, b].map(|c| 0.5 + (c - 0.5) * t)
        });
        if mixed {
            let [r, g, b] = model.rgb(mixing.values).map(|c| c.clamp(0.0, 1.0));
            self.set_colour([r, g, b, self.color[3]]);
            if matches!(model, color::Model::Hsv | color::Model::Hsl) {
                self.hsva.h = (mixing.values[0] / 360.0).rem_euclid(1.0);
            }
            mixing.of = self.color;
        }
        if faded {
            let [r, g, b, _] = self.color;
            self.set_colour([r, g, b, alpha / 100.0]);
            mixing.of = self.color;
        }
        self.mixing = Some(mixing);
    }

    fn fill_panel(&mut self, ui: &mut egui::Ui) {
        let layers: Vec<(String, String)> = self
            .scene
            .bundle
            .doc
            .all_layers()
            .iter()
            .rev()
            .map(|l| (l.id.clone(), l.name.clone()))
            .collect();
        let name_of = |r: &FillRef| match r {
            FillRef::This => "this layer".to_string(),
            FillRef::All => "all visible layers".to_string(),
            FillRef::Layer(id) => layers
                .iter()
                .find(|(l, _)| l == id)
                .map_or_else(|| "a layer that is gone".to_string(), |(_, name)| name.clone()),
        };
        let mut reference = self.fill.reference.clone();
        ui.horizontal_wrapped(|ui| {
            ui.label("inside the lines of");
            egui::ComboBox::from_id_salt("fill.ref")
                .selected_text(name_of(&reference))
                .width(170.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut reference, FillRef::This, name_of(&FillRef::This));
                    ui.selectable_value(&mut reference, FillRef::All, name_of(&FillRef::All));
                    layers.iter().for_each(|(id, _)| {
                        let r = FillRef::Layer(id.clone());
                        let text = name_of(&r);
                        ui.selectable_value(&mut reference, r, text);
                    });
                })
                .response
                .hint("the lines the fill stops at: this layer, everything visible, or a layer such as your lines");
        });
        self.fill.reference = reference;
        let f = &mut self.fill;
        options::whole(ui, "close gaps px", |ui| {
            crate::widgets::labelled_slider(ui, "close gaps px", egui::Slider::new(&mut f.gap, 0.0..=fill::MAX_GAP))
        })
        .hint("openings in the lines up to this wide count as closed");
        options::whole(ui, "tolerance", |ui| {
            crate::widgets::labelled_slider(ui, "tolerance", egui::Slider::new(&mut f.tolerance, 0.0..=1.0))
        })
        .hint("how different a pixel may be from where you click and still fill");
        options::whole(ui, "grow under lines px", |ui| {
            crate::widgets::labelled_slider(
                ui,
                "grow under lines px",
                egui::Slider::new(&mut f.expand, 0.0..=fill::MAX_EXPAND),
            )
        })
        .hint("the fill reaches this far under the lines, so no paper shows between line and colour");
        options::whole(ui, "opacity", |ui| {
            crate::widgets::labelled_slider(
                ui,
                "opacity",
                crate::widgets::percent(egui::Slider::new(&mut f.opacity, 0.05..=1.0)),
            )
        })
        .hint("the fill's opacity");
        options::note(
            ui,
            egui::RichText::new("click inside lines to fill, drag a loop to lasso fill")
                .small()
                .color(crate::theme::WEAK),
        );
        ui.horizontal_wrapped(|ui| {
            self.command_button(ui, Cmd::Refill, Some("refill this drawing"));
            self.command_button(ui, Cmd::ShowGaps, Some("show gaps"));
        });
        self.paint_note(ui);
    }

    fn pen_panel(&mut self, ui: &mut egui::Ui) {
        let key = device_key(self.readout.device);
        let mut curve = self.settings.curve(&key);
        let raw = self.readout.pressure;
        let peak = self.settings.peak(&key);
        let before = curve;
        ui.spacing_mut().slider_width = 220.0;
        prefs::grid(ui, "settings pen", |ui| {
            prefs::row_label(ui, "Pen");
            ui.label(if self.readout.device == 0 && cfg!(windows) {
                "not identified yet: draw with it. No pressure? Turn on Windows Ink in the tablet's driver".to_string()
            } else if self.readout.device == 0 {
                "not identified yet: move it away from the tablet and back".to_string()
            } else {
                key.clone()
            });
            ui.end_row();
            prefs::row_label(ui, "Pressure");
            ui.vertical(|ui| {
                ui.add(
                    egui::ProgressBar::new(raw)
                        .desired_width(280.0)
                        .text(format!("pen {raw:.3}")),
                );
                ui.add(
                    egui::ProgressBar::new(curve.apply(raw))
                        .desired_width(280.0)
                        .text(format!("recorded {:.3}", curve.apply(raw))),
                );
                ui.label(egui::RichText::new(format!("heaviest press seen {peak:.3}")).color(crate::theme::WEAK));
            });
            ui.end_row();
            prefs::row_label(ui, "Tilt");
            let (tx, ty) = self.readout.tilt;
            let lean = (tx * tx + ty * ty).sqrt().min(1.0);
            ui.vertical(|ui| {
                ui.add(egui::ProgressBar::new(lean).desired_width(280.0).text(format!("tilt {lean:.2}")));
                ui.label(
                    egui::RichText::new(
                        "lean the pen while it touches: the bar follows when the pen sends tilt. The Pencil grows and lightens as it leans",
                    )
                    .color(crate::theme::WEAK),
                );
            });
            ui.end_row();
            prefs::row_label(ui, "Lightest");
            ui.add(egui::Slider::new(&mut curve.low, 0.0..=0.5))
                .hint("the pen's raw pressure that records as the lightest touch");
            ui.end_row();
            prefs::row_label(ui, "Heaviest");
            ui.add(egui::Slider::new(&mut curve.high, 0.05..=1.0))
                .hint("the pen's raw pressure that records as full pressure");
            ui.end_row();
            prefs::row_label(ui, "Curve");
            ui.add(egui::Slider::new(&mut curve.gamma, 0.3..=3.0).logarithmic(true))
                .hint("below 1 light presses record heavier, above 1 lighter");
            ui.end_row();
            prefs::row_label(ui, "");
            ui.horizontal_wrapped(|ui| {
                if ui
                    .button("heaviest = my peak")
                    .hint("set heaviest to the hardest press seen with this pen")
                    .clicked()
                    && peak > 0.05
                {
                    curve.high = peak;
                }
                if ui.button("reset").hint("the default curve").clicked() {
                    curve = PressureCurve::default();
                }
                if ui
                    .button("forget peak")
                    .hint("start measuring the hardest press again")
                    .clicked()
                    && let Some(pen) = self.settings.pens.get_mut(&key)
                {
                    pen.peak = 0.0;
                    self.settings_dirty = true;
                }
            });
            ui.end_row();
            prefs::row_label(ui, "Response");
            let (rect, _) = ui.allocate_exact_size(vec2(220.0, 120.0), Sense::hover());
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 2.0, Color32::from_gray(28));
            let at = |x: f32, y: f32| egui::pos2(rect.left() + x * rect.width(), rect.bottom() - y * rect.height());
            let line: Vec<Pos2> = (0..=40)
                .map(|i| i as f32 / 40.0)
                .map(|x| at(x, curve.apply(x)))
                .collect();
            painter.add(egui::Shape::line(line, egui::Stroke::new(1.5, Color32::LIGHT_BLUE)));
            painter.circle_filled(at(raw, curve.apply(raw)), 3.5, Color32::YELLOW);
            ui.end_row();
        });
        if curve != before {
            curve.high = curve.high.max(curve.low + 0.05);
            self.settings.set_curve(&key, curve);
            self.settings_dirty = true;
        }
    }
}

impl Editor {
    #[cfg(test)]
    pub(crate) fn redraw_canvas(&mut self) {
        self.dirty.extend(self.scene.all_tiles());
    }

    pub(crate) fn clear(&self) -> [f32; 4] {
        crate::theme::surround(self.settings.defaults.surround)
    }

    pub(crate) fn hand_over(&self, to: &mut Editor) {
        to.clipboard.clone_from(&self.clipboard);
        to.shape_clip.clone_from(&self.shape_clip);
        to.shape_from.clone_from(&self.shape_from);
        to.cells_last = self.cells_last;
    }

    pub(crate) fn hand_settings(&self, to: &mut Editor) {
        to.settings = self.settings.clone();
        to.settings_dirty = true;
    }

    #[cfg(test)]
    pub(crate) fn screen_at(&self, x: f32, y: f32) -> Pos2 {
        self.view.map_or(self.canvas_rect.center(), |view| {
            self.screen(view, self.canvas_rect, vec2(x, y))
        })
    }

    pub(crate) fn leave(&mut self, ctx: &egui::Context) {
        self.held = selection::Held::default();
        let Some(pos) = self.pen_down.take() else {
            return;
        };
        let up = PenEvent {
            phase: Phase::Up,
            pos,
            pressure: 0.0,
            tilt: (0.0, 0.0),
            eraser: false,
            tablet: false,
            device: 0,
            at: Instant::now(),
        };
        let rect = self.canvas_rect;
        self.check(|e| e.on_pen(ctx, up, rect, true));
    }

    pub(crate) fn reshown(&mut self) {
        self.yours.reload();
        self.dirty = self.scene.all_tiles().into_iter().collect();
        self.frames.forget_all();
        self.external.get_or_insert(now_ms());
    }

    pub(crate) fn take_tabs(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        if !ctx.egui_wants_keyboard_input() {
            self.tabs.extend(crate::layout::take_tabs(raw_input));
        }
    }
}

fn regraded(paint: klad_core::doc::Paint, target: [f32; 4]) -> Option<klad_core::doc::Paint> {
    use klad_core::doc::Paint;
    let regrade = |stops: &mut Vec<klad_core::doc::Stop>| {
        let colours: Vec<[f32; 4]> = stops.iter().map(|s| color::rgba_of(&s.1).unwrap_or(target)).collect();
        let moved = klad_core::color::recolour(&colours, [target[0], target[1], target[2], 1.0]);
        stops
            .iter_mut()
            .zip(colours.iter().zip(moved))
            .for_each(|(stop, (was, now))| {
                stop.1 = klad_core::color::to_hex([now[0], now[1], now[2], was[3]]);
            });
    };
    match paint {
        Paint::Solid(_) => None,
        Paint::Linear { mut linear } => {
            regrade(&mut linear.stops);
            Some(Paint::Linear { linear })
        }
        Paint::Radial { mut radial } => {
            regrade(&mut radial.stops);
            Some(Paint::Radial { radial })
        }
    }
}

fn erases_any(buffer: &StrokeBuffer, raster: Option<&Raster>) -> bool {
    let Some(raster) = raster else {
        return false;
    };
    buffer.touched().iter().any(|&i| {
        buffer.grid().at(i).zip(raster.at(i)).is_some_and(|(cover, px)| {
            cover
                .iter()
                .zip(px.as_chunks::<4>().0)
                .any(|(c, p)| *c > 0.0 && f32::from(p[3]) > 0.0)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixel_scales_are_free_but_land_on_whole_sizes_within_a_pixel() {
        assert_eq!(factor(2.04, Some(10.0)), 2.0, "0.4 px from twice the size lands on it");
        assert_eq!(factor(2.2, Some(10.0)), 2.2, "2 px away stays where it is");
        assert_eq!(factor(0.75, Some(24.0)), 0.75, "a squash stays");
        assert_eq!(factor(0.98, Some(24.0)), 1.0, "half a pixel from the size it had");
        assert_eq!(factor(0.98, Some(100.0)), 0.98, "two pixels on a big one");
        assert_eq!(factor(-1.03, Some(20.0)), -1.0, "a flip lands on a whole flip");
        assert_eq!(factor(2.04, None), 2.04, "cmd held, or not on the grid: free");
    }

    #[test]
    fn grab_finds_every_handle_where_the_press_lands() {
        use egui::pos2;
        let screen = [
            pos2(100.0, 100.0),
            pos2(300.0, 100.0),
            pos2(300.0, 200.0),
            pos2(100.0, 200.0),
        ];
        assert_eq!(grab(screen, pos2(200.0, 100.0 - KNOB), true), Some(Grab::Rotate));
        assert_eq!(
            grab(screen, pos2(200.0 + 9.0, 100.0 - KNOB - 6.0), true),
            Some(Grab::Rotate)
        );
        assert_eq!(grab(screen, pos2(311.0, 104.0), true), Some(Grab::Scale));
        assert_eq!(grab(screen, pos2(92.0, 207.0), false), Some(Grab::Scale));
        assert_eq!(grab(screen, pos2(200.0, 111.0), true), Some(Grab::Stretch(0)));
        assert_eq!(grab(screen, pos2(309.0, 150.0), true), Some(Grab::Stretch(1)));
        assert_eq!(grab(screen, pos2(200.0, 189.0), true), Some(Grab::Stretch(2)));
        assert_eq!(grab(screen, pos2(91.0, 150.0), true), Some(Grab::Stretch(3)));
        assert_eq!(grab(screen, pos2(309.0, 150.0), false), None);
        assert_eq!(grab(screen, pos2(295.0, 150.0), false), Some(Grab::Move));
        assert_eq!(grab(screen, pos2(200.0, 150.0), true), Some(Grab::Move));
        assert_eq!(grab(screen, pos2(330.0, 150.0), true), None);
    }

    #[test]
    fn stroke_drags_commit_what_the_preview_shows() {
        [
            Grab::Move,
            Grab::Scale,
            Grab::Rotate,
            Grab::Stretch(0),
            Grab::Stretch(1),
            Grab::Stretch(3),
        ]
        .iter()
        .flat_map(|&grab| [(false, false), (true, false), (false, true)].map(|(shift, alt)| (grab, shift, alt)))
        .for_each(|(grab, shift, alt)| {
            let drag = Drag {
                grab,
                node: "c1:1,2".into(),
                strokes: true,
                whole: false,
                fresh: false,
                copy: false,
                start: vec2(140.0, 80.0),
                now: vec2(190.0, 20.0),
                center: vec2(100.0, 100.0),
                corners: [
                    vec2(50.0, 50.0),
                    vec2(150.0, 50.0),
                    vec2(150.0, 150.0),
                    vec2(50.0, 150.0),
                ],
                slop: 0.5,
                crossed: true,
                outside: false,
                shift,
                alt,
                loose: false,
                free: true,
                toggle: false,
            };
            let committed = drag.mark_transform();
            let a = committed.affine(committed.pivot.unwrap_or([0.0, 0.0]));
            drag.affine().iter().zip(a).for_each(|(p, c)| {
                assert!(
                    (p - c).abs() < 1e-3,
                    "{grab:?} shift {shift} alt {alt}: {:?} vs {a:?}",
                    drag.affine()
                )
            });
        });
    }

    #[test]
    fn shift_and_alt_change_how_a_drag_moves_scales_and_turns() {
        let base = Drag {
            grab: Grab::Move,
            node: "n1".into(),
            strokes: false,
            whole: false,
            fresh: false,
            copy: false,
            start: vec2(150.0, 150.0),
            now: vec2(250.0, 170.0),
            center: vec2(100.0, 100.0),
            corners: [
                vec2(50.0, 50.0),
                vec2(150.0, 50.0),
                vec2(150.0, 150.0),
                vec2(50.0, 150.0),
            ],
            slop: 0.5,
            crossed: true,
            outside: false,
            shift: false,
            alt: false,
            loose: false,
            free: true,
            toggle: false,
        };
        assert_eq!(base.slide(), vec2(100.0, 20.0));
        assert_eq!(
            Drag {
                shift: true,
                ..base.clone()
            }
            .slide(),
            vec2(100.0, 0.0),
            "shift keeps a move level"
        );
        let scale = Drag {
            grab: Grab::Scale,
            now: vec2(250.0, 250.0),
            ..base.clone()
        };
        assert_eq!(
            scale.pivot(),
            vec2(50.0, 50.0),
            "a corner scales from the opposite corner"
        );
        assert_eq!(scale.factors(), (2.0, 2.0));
        assert_eq!(
            Drag {
                alt: true,
                ..scale.clone()
            }
            .pivot(),
            vec2(100.0, 100.0),
            "alt from the middle"
        );
        let free = Drag {
            shift: true,
            now: vec2(250.0, 150.0),
            ..scale.clone()
        };
        assert_eq!(free.factors(), (2.0, 1.0), "shift frees the proportions");
        assert_eq!(
            Drag {
                free: false,
                ..free.clone()
            }
            .factors()
            .0,
            Drag {
                free: false,
                ..free.clone()
            }
            .factors()
            .1
        );
        let turn = Drag {
            grab: Grab::Rotate,
            start: vec2(200.0, 100.0),
            now: vec2(100.0 + 100.0 * 0.4f32.cos(), 100.0 + 100.0 * 0.4f32.sin()),
            shift: true,
            ..base.clone()
        };
        assert!(
            (turn.angle().to_degrees() - 30.0).abs() < 1e-3,
            "shift turns in 15 degree steps: {}",
            turn.angle().to_degrees()
        );
        let stretch = Drag {
            grab: Grab::Stretch(1),
            start: vec2(150.0, 100.0),
            now: vec2(200.0, 100.0),
            alt: true,
            ..base.clone()
        };
        let (_, pivot, k) = stretch.stretch().unwrap();
        assert_eq!(
            (pivot, k),
            (vec2(100.0, 100.0), 2.0),
            "alt stretches both sides from the middle"
        );
    }

    #[test]
    fn points_map_between_canvas_and_the_shape() {
        let m = [0.0, -2.0, 2.0, 0.0, 100.0, 50.0];
        let local = to_local(m, vec2(80.0, 70.0));
        let back = to_canvas(m, local);
        assert!((back - vec2(80.0, 70.0)).length() < 1e-3);
        let picked = Picked::whole("c4".into(), vec![3, 12]);
        assert_eq!(picked.addresses(), ["c4:s3", "c4:s12"]);
        assert_eq!(picked.key(), "c4:3,12");
        let cut = Picked {
            cut: Some(vec![[0.0, 0.0], [5.0, 0.0], [5.0, 5.0]]),
            ..picked.clone()
        };
        assert!(cut.key().starts_with("c4:3,12:cut"));
    }

    #[test]
    fn a_library_brush_starts_without_its_taper_by_hand_and_a_brush_of_the_drawing_keeps_its_own() {
        let set = BrushSet::presets();
        let ink = set.def("ink").unwrap();
        assert!(ink.taper[0] > 0.0, "the ink brush tapers strokes drawn from a path");
        assert_eq!(hand_taper("ink", ink), [0.0, 0.0]);
        assert_eq!(hand_taper("ink-1a2b3c", ink), [0.0, 0.0], "a variant of it too");
        let own = BrushDef {
            taper: [30.0, 5.0],
            ..ink.clone()
        };
        assert_eq!(hand_taper("ink-1a2b3c", &own), [30.0, 5.0]);
        assert_eq!(hand_taper("quill", &own), [30.0, 5.0]);
        assert_eq!(
            hand_taper("quill", ink),
            ink.taper,
            "a brush of the drawing keeps what it was saved with"
        );
    }

    #[test]
    fn hardness_and_curve_make_a_variant_but_feel_settings_do_not() {
        let set = BrushSet::presets();
        let ink = set.def("ink").unwrap();
        let tuning = Tuning::of(ink);
        assert_eq!(&tuning.shape(ink), ink);
        let soft = set.def("soft").unwrap();
        assert_eq!(&Tuning::of(soft).shape(soft), soft);
        let feel = Tuning {
            taper: [0.0, 0.0],
            stabilizer: 0.0,
            smoothing: 80.0,
            ..tuning
        };
        assert_eq!(&feel.shape(ink), ink);
        let harder = Tuning {
            hardness: Some(1.0),
            ..tuning
        };
        assert_eq!(harder.shape(ink).hardness(), Some(1.0));
        let feathered = Tuning { feather: 0.5, ..tuning };
        assert_eq!(feathered.shape(ink).feather, 0.5);
        let lighter = Tuning {
            size_min: 0.3,
            ..tuning
        };
        let shaped = lighter.shape(ink);
        assert_eq!(shaped.dynamics.size.pressure.map(|c| c.min), Some(0.3));
        let none = BTreeMap::new();
        assert_eq!(pick_id("ink", &tuning.shape(ink), &none), "ink");
        assert!(pick_id("ink", &shaped, &none).starts_with("ink-"));
    }
}
