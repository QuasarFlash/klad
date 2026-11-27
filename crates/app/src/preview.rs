use crate::composite::{compose_items, dilate, effect_reach};
use anyhow::{Result, anyhow, bail};
use half::f16;
use klad_brush::grain::GRAIN_SIZE;
use klad_brush::tiles::{Heights, TILE, TILE_PX, Tile};
use klad_brush::{BrushSet, CelReplay, Raster};
use klad_core::bundle::Bundle;
use klad_core::doc::{Document, Kind, Layer, Node};
use klad_core::markedit::{Affine, MarkTransform, apply_affine, invert, parse_address, transform_mark, union};
use klad_core::ops::{Op, apply_all};
use klad_core::stroke::Mark;
use klad_render::onion::Onions;
use klad_render::{Rasters, changed_tiles, rasterize_nodes, rasterize_tiles};
use rayon::prelude::*;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

pub struct Snapshot {
    pub rev: u64,
    pub bundle: Bundle,
    pub frame: u32,
    pub rasters: Rasters,
    pub cels: BTreeMap<String, CelReplay>,
    pub onions: Option<Onions>,
    pub brushes: BrushSet,
    pub vectors: OnceLock<BTreeMap<String, Vec<Node>>>,
    pub lifts: OnceLock<Option<Lifts>>,
    pub slow: AtomicBool,
}

pub struct Lifts {
    ids: Vec<String>,
    cels: BTreeMap<String, Lift>,
}

struct Lift {
    rest: Raster,
    alone: Raster,
    offset: [f32; 2],
    centre: [f32; 2],
    reach: [f32; 4],
}

pub struct Job {
    pub ops: Vec<Op>,
    pub snap: Arc<Snapshot>,
    pub shown: BTreeSet<usize>,
    pub visible: BTreeSet<usize>,
    pub cancel: Arc<AtomicBool>,
}

pub struct Done {
    pub ops: Vec<Op>,
    pub tiles: Vec<(usize, Vec<f16>)>,
    pub changed: BTreeSet<usize>,
    pub hidden: BTreeSet<usize>,
    pub exact: bool,
}

fn vector_nodes(doc: &klad_core::doc::Document, frame: u32) -> BTreeMap<String, Vec<Node>> {
    doc.all_layers()
        .into_iter()
        .filter(|l| l.kind() == Kind::Vector)
        .filter_map(|l| l.content_at(frame))
        .map(|c| (c.id.clone(), c.nodes.clone()))
        .collect()
}

const MANY: usize = 32;
const BIG: usize = 1000;
const SLOW: std::time::Duration = std::time::Duration::from_millis(12);

fn liftable(mark: &Mark, brushes: &BrushSet) -> bool {
    match mark {
        Mark::Stroke(s) => !s.lock && brushes.def(&s.brush).is_some_and(|d| !d.reads_pixels()),
        Mark::Fill(_) => true,
    }
}

fn split(marks: &[Mark], chosen: &BTreeSet<u32>) -> (Vec<Mark>, Vec<Mark>) {
    let live: BTreeSet<u32> = marks.iter().map(Mark::id).collect();
    let rides = |m: &Mark| m.as_stroke().is_some_and(|s| s.on.iter().any(|t| chosen.contains(t)));
    let only = |m: &Mark| {
        m.as_stroke()
            .is_some_and(|s| s.on.iter().filter(|t| live.contains(t)).all(|t| chosen.contains(t)))
    };
    let moving = marks
        .iter()
        .filter(|m| chosen.contains(&m.id()) || rides(m))
        .cloned()
        .collect();
    let staying = marks
        .iter()
        .filter(|m| !chosen.contains(&m.id()) && !(rides(m) && only(m)))
        .cloned()
        .collect();
    (moving, staying)
}

fn chosen_by_cel(ids: &[String]) -> Option<BTreeMap<String, BTreeSet<u32>>> {
    ids.iter()
        .try_fold(BTreeMap::<String, BTreeSet<u32>>::new(), |mut by, id| {
            let (cel, n) = parse_address(id)?;
            by.entry(cel.to_string()).or_default().insert(n);
            Some(by)
        })
}

fn big(snap: &Snapshot, ids: &[String]) -> bool {
    ids.len() >= MANY
        || chosen_by_cel(ids).is_some_and(|by| {
            by.keys()
                .any(|cel| snap.cels.get(cel).is_some_and(|replay| replay.marks().len() >= BIG))
        })
}

fn cut(full: &Raster, rest: &Raster, wide: [u32; 2], offset: [usize; 2]) -> Raster {
    let (dx, dy) = (offset[0] / TILE, offset[1] / TILE);
    let empty = Heights::new(full.width, full.height);
    let tiles: BTreeSet<usize> = full
        .differs(rest)
        .into_iter()
        .chain(
            full.heights()
                .unwrap_or(&empty)
                .differs(rest.heights().unwrap_or(&empty)),
        )
        .collect();
    let pieces: Vec<(usize, Tile)> = tiles
        .into_par_iter()
        .filter_map(|i| {
            let (colour, under) = (full.at(i), rest.at(i));
            let (height, below) = (full.height_tile(i), rest.height_tile(i));
            let mut picture = vec![f16::ZERO; TILE_PX * 4];
            let mut body = height.map(|_| vec![f16::ZERO; TILE_PX]);
            let at = |t: Option<&[f16]>, k: usize| t.map_or(f16::ZERO, |t| t[k]);
            let mut any = false;
            (0..TILE_PX).for_each(|p| {
                let differs =
                    (0..4).any(|c| at(colour, p * 4 + c) != at(under, p * 4 + c)) || at(height, p) != at(below, p);
                if differs {
                    (0..4).for_each(|c| picture[p * 4 + c] = at(colour, p * 4 + c));
                    if let Some(body) = body.as_mut() {
                        body[p] = at(height, p);
                    }
                    any = true;
                }
            });
            any.then(|| (i, (Some(Arc::from(picture)), body.map(Arc::from))))
        })
        .collect();
    let mut alone = Raster::new(wide[0], wide[1]);
    pieces.iter().for_each(|(i, piece)| {
        let to = alone.index(i % full.cols + dx, i / full.cols + dy);
        alone.restore(to, piece);
    });
    alone
}

fn whole_cels(snap: &Snapshot, ids: &[String]) -> bool {
    ids.len() >= MANY
        && chosen_by_cel(ids).is_some_and(|by| {
            by.iter().all(|(cel, chosen)| {
                snap.cels
                    .get(cel)
                    .is_some_and(|replay| split(replay.marks(), chosen).1.is_empty())
            })
        })
}

fn lift_whole(snap: &Snapshot, cel: &str, picked: [f32; 4], offset: [usize; 2]) -> Option<Lift> {
    let raster = snap.rasters.get(cel)?;
    let (w, h) = (snap.bundle.doc.canvas.width, snap.bundle.doc.canvas.height);
    let mut alone = Raster::new(w + 2 * offset[0] as u32, h + 2 * offset[1] as u32);
    let (dx, dy) = (offset[0] / TILE, offset[1] / TILE);
    let occupied: Vec<usize> = raster.occupied().map(|(i, _)| i).collect();
    occupied.iter().for_each(|&i| {
        let (tx, ty) = (i % raster.cols, i / raster.cols);
        let to = alone.index(tx + dx, ty + dy);
        alone.restore(to, &raster.tile(i));
    });
    let low = occupied.iter().fold((usize::MAX, usize::MAX), |a, &i| {
        (a.0.min(i % raster.cols), a.1.min(i / raster.cols))
    });
    let high = occupied.iter().fold((0, 0), |a, &i| {
        (a.0.max(i % raster.cols + 1), a.1.max(i / raster.cols + 1))
    });
    if occupied.is_empty() {
        return None;
    }
    let [x0, y0, x1, y1] = picked;
    Some(Lift {
        rest: Raster::new(w, h),
        alone,
        offset: offset.map(|v| v as f32),
        centre: [(x0 + x1) / 2.0, (y0 + y1) / 2.0],
        reach: [low.0 + dx, low.1 + dy, high.0 + dx, high.1 + dy].map(|v| (v * TILE) as f32),
    })
}

fn lift_cel(snap: &Snapshot, cel: &str, chosen: &BTreeSet<u32>) -> Option<Lift> {
    let replay = snap.cels.get(cel)?;
    let marks = replay.marks();
    let (moving, staying) = split(marks, chosen);
    let picked: Vec<&Mark> = marks.iter().filter(|m| chosen.contains(&m.id())).collect();
    let [x0, y0, x1, y1] = union(&picked)?;
    let (w, h) = (snap.bundle.doc.canvas.width, snap.bundle.doc.canvas.height);
    let offset = [w, h].map(|v| (v as usize).div_ceil(GRAIN_SIZE) * GRAIN_SIZE);
    if staying.is_empty() {
        return lift_whole(snap, cel, [x0, y0, x1, y1], offset);
    }
    if moving.is_empty() {
        return None;
    }
    let full = snap.rasters.get(cel).cloned().unwrap_or_else(|| Raster::new(w, h));
    let wide = [w + 2 * offset[0] as u32, h + 2 * offset[1] as u32];
    let rested = || {
        let mut rest = full.clone();
        replay.clone().update(&mut rest, staying, &snap.brushes).map(|_| rest)
    };
    let (alone, rest) = match moving.iter().all(|m| liftable(m, &snap.brushes)) {
        true => {
            let around: Vec<Mark> = moving
                .into_iter()
                .map(|mut m| {
                    transform_mark(&mut m, [1.0, 0.0, 0.0, 1.0, offset[0] as f32, offset[1] as f32]);
                    m
                })
                .collect();
            let (alone, rest) = rayon::join(
                || CelReplay::replay_tiled(wide[0], wide[1], around, &snap.brushes).map(|(_, raster, _)| raster),
                rested,
            );
            (alone.ok()?, rest.ok()?)
        }
        false => {
            let rest = rested().ok()?;
            (cut(&full, &rest, wide, offset), rest)
        }
    };
    let tiles: Vec<(usize, usize)> = alone
        .occupied()
        .map(|(i, _)| (i % alone.cols, i / alone.cols))
        .collect();
    if tiles.is_empty() {
        return None;
    }
    let low = tiles
        .iter()
        .fold((usize::MAX, usize::MAX), |a, t| (a.0.min(t.0), a.1.min(t.1)));
    let high = tiles.iter().fold((0, 0), |a, t| (a.0.max(t.0 + 1), a.1.max(t.1 + 1)));
    let reach = [low.0, low.1, high.0, high.1].map(|v| (v * TILE) as f32);
    Some(Lift {
        rest,
        alone,
        offset: offset.map(|v| v as f32),
        centre: [(x0 + x1) / 2.0, (y0 + y1) / 2.0],
        reach,
    })
}

fn lift(snap: &Snapshot, ids: &[String], cancel: &AtomicBool) -> Option<Lifts> {
    let by_cel = chosen_by_cel(ids)?;
    let cels = by_cel
        .into_par_iter()
        .map(|(cel, chosen)| {
            let lift = lift_cel(snap, &cel, &chosen).filter(|_| !cancel.load(Ordering::Relaxed))?;
            Some((cel, lift))
        })
        .collect::<Option<BTreeMap<_, _>>>()?;
    Some(Lifts {
        ids: ids.to_vec(),
        cels,
    })
}

fn tiles_over([x0, y0, x1, y1]: [f32; 4], cols: usize, rows: usize) -> Vec<usize> {
    let span = |a: f32, b: f32, n: usize| {
        let at = |v: f32| ((v / TILE as f32).floor().max(0.0) as usize).min(n - 1);
        at(a)..=at(b - 0.01)
    };
    span(y0, y1, rows)
        .flat_map(|ty| span(x0, x1, cols).map(move |tx| ty * cols + tx))
        .collect()
}

fn texel(raster: &Raster, x: i64, y: i64) -> [f32; 4] {
    let (wide, tall) = ((raster.cols * TILE) as i64, (raster.rows * TILE) as i64);
    if x < 0 || y < 0 || x >= wide || y >= tall {
        return [0.0; 4];
    }
    let (x, y) = (x as usize, y as usize);
    raster.get(x / TILE, y / TILE).map_or([0.0; 4], |t| {
        let k = ((y % TILE) * TILE + x % TILE) * 4;
        [t[k].to_f32(), t[k + 1].to_f32(), t[k + 2].to_f32(), t[k + 3].to_f32()]
    })
}

fn sample(raster: &Raster, x: f32, y: f32) -> [f32; 4] {
    let (fx, fy) = (x.floor(), y.floor());
    let (u, v) = (x - fx, y - fy);
    let (ix, iy) = (fx as i64, fy as i64);
    let corners = [
        (texel(raster, ix, iy), (1.0 - u) * (1.0 - v)),
        (texel(raster, ix + 1, iy), u * (1.0 - v)),
        (texel(raster, ix, iy + 1), (1.0 - u) * v),
        (texel(raster, ix + 1, iy + 1), u * v),
    ];
    std::array::from_fn(|c| corners.iter().map(|(t, w)| t[c] * w).sum())
}

fn laid(lift: &Lift, back: Affine, index: usize) -> Option<Tile> {
    let (tx, ty) = (index % lift.rest.cols, index / lift.rest.cols);
    let corners = [(0, 0), (1, 0), (0, 1), (1, 1)]
        .map(|(i, j)| apply_affine(back, (((tx + i) * TILE) as f32, ((ty + j) * TILE) as f32)));
    let low = corners
        .iter()
        .fold((f32::MAX, f32::MAX), |m, p| (m.0.min(p.0), m.1.min(p.1)));
    let high = corners
        .iter()
        .fold((f32::MIN, f32::MIN), |m, p| (m.0.max(p.0), m.1.max(p.1)));
    let (cols, rows) = (lift.alone.cols, lift.alone.rows);
    let source = tiles_over([low.0 - 2.0, low.1 - 2.0, high.0 + 2.0, high.1 + 2.0], cols, rows);
    if !source.iter().any(|&i| lift.alone.at(i).is_some()) {
        return None;
    }
    let under = lift.rest.at(index);
    let mut tile: Box<[f16]> = under.map_or_else(|| vec![f16::ZERO; TILE_PX * 4].into_boxed_slice(), Box::from);
    let mut body: Option<Box<[f16]>> = match (lift.rest.height_tile(index), lift.alone.has_height()) {
        (Some(below), _) => Some(Box::from(below)),
        (None, true) => Some(vec![f16::ZERO; TILE_PX].into_boxed_slice()),
        (None, false) => None,
    };
    let any = match whole_shift(back) {
        Some(by) => shift_tile(lift, &mut tile, body.as_deref_mut(), (tx, ty), by),
        None => sample_tile(lift, &mut tile, body.as_deref_mut(), (tx, ty), back),
    };
    any.then(|| (Some(Arc::from(tile)), body.map(Arc::from)))
}

fn whole_shift(back: Affine) -> Option<(i64, i64)> {
    let near = |v: f32, to: f32| (v - to).abs() < 1e-5;
    (near(back[0], 1.0) && near(back[1], 0.0) && near(back[2], 0.0) && near(back[3], 1.0))
        .then(|| (back[4].round() as i64, back[5].round() as i64))
}

fn over(tile: &mut [f16], p: usize, top: [f32; 4]) {
    let keep = 1.0 - top[3];
    (0..4).for_each(|c| tile[p + c] = f16::from_f32(top[c] + tile[p + c].to_f32() * keep));
}

fn raise(body: Option<&mut [f16]>, p: usize, top: f32, alpha: f32) {
    if let Some(body) = body {
        body[p] = f16::from_f32(top * alpha + body[p].to_f32() * (1.0 - alpha));
    }
}

fn height_texel(raster: &Raster, x: i64, y: i64) -> f32 {
    let (wide, tall) = ((raster.cols * TILE) as i64, (raster.rows * TILE) as i64);
    if x < 0 || y < 0 || x >= wide || y >= tall {
        return 0.0;
    }
    let (x, y) = (x as usize, y as usize);
    raster
        .height_tile(raster.index(x / TILE, y / TILE))
        .map_or(0.0, |t| t[(y % TILE) * TILE + x % TILE].to_f32())
}

fn height_at(raster: &Raster, x: f32, y: f32) -> f32 {
    let (fx, fy) = (x.floor(), y.floor());
    let (u, v) = (x - fx, y - fy);
    let (ix, iy) = (fx as i64, fy as i64);
    height_texel(raster, ix, iy) * (1.0 - u) * (1.0 - v)
        + height_texel(raster, ix + 1, iy) * u * (1.0 - v)
        + height_texel(raster, ix, iy + 1) * (1.0 - u) * v
        + height_texel(raster, ix + 1, iy + 1) * u * v
}

fn shift_tile(
    lift: &Lift,
    tile: &mut [f16],
    mut body: Option<&mut [f16]>,
    (tx, ty): (usize, usize),
    (ox, oy): (i64, i64),
) -> bool {
    let side = TILE as i64;
    let (cols, rows) = (lift.alone.cols as i64, lift.alone.rows as i64);
    let start = tx as i64 * side + ox;
    let mut any = false;
    (0..TILE).for_each(|row| {
        let sy = (ty * TILE + row) as i64 + oy;
        if sy < 0 || sy >= rows * side {
            return;
        }
        let first = start.div_euclid(side);
        [first, first + 1]
            .into_iter()
            .filter(|stx| (0..cols).contains(stx))
            .filter_map(|stx| {
                let index = lift.alone.index(stx as usize, (sy / side) as usize);
                lift.alone
                    .at(index)
                    .map(|src| (stx, src, lift.alone.height_tile(index)))
            })
            .for_each(|(stx, src, heights)| {
                let lo = (stx * side - start).max(0) as usize;
                let hi = ((stx + 1) * side - start).min(side) as usize;
                let line = (sy % side) as usize * TILE;
                (lo..hi).for_each(|col| {
                    let k = (line + (start + col as i64 - stx * side) as usize) * 4;
                    let alpha = src[k + 3].to_f32();
                    if alpha > 0.0 {
                        let top = [src[k].to_f32(), src[k + 1].to_f32(), src[k + 2].to_f32(), alpha];
                        over(tile, (row * TILE + col) * 4, top);
                        let raised = heights.map_or(0.0, |h| h[k / 4].to_f32());
                        raise(body.as_deref_mut(), row * TILE + col, raised, alpha);
                        any = true;
                    }
                });
            });
    });
    any
}

fn sample_tile(
    lift: &Lift,
    tile: &mut [f16],
    mut body: Option<&mut [f16]>,
    (tx, ty): (usize, usize),
    back: Affine,
) -> bool {
    let [x0, y0, x1, y1] = lift.reach;
    let mut any = false;
    (0..TILE_PX).for_each(|p| {
        let (x, y) = ((tx * TILE + p % TILE) as f32 + 0.5, (ty * TILE + p / TILE) as f32 + 0.5);
        let (sx, sy) = apply_affine(back, (x, y));
        if sx < x0 - 1.0 || sy < y0 - 1.0 || sx > x1 + 1.0 || sy > y1 + 1.0 {
            return;
        }
        let top = sample(&lift.alone, sx - 0.5, sy - 0.5);
        if top[3] > 0.0 {
            over(tile, p * 4, top);
            if body.is_some() {
                raise(
                    body.as_deref_mut(),
                    p,
                    height_at(&lift.alone, sx - 0.5, sy - 0.5),
                    top[3],
                );
            }
            any = true;
        }
    });
    any
}

fn shifted(snap: &Snapshot, lifts: &Lifts, transform: &MarkTransform) -> Result<(Rasters, BTreeSet<usize>)> {
    let mut rasters = snap.rasters.clone();
    let mut changed = BTreeSet::new();
    lifts.cels.iter().try_for_each(|(cel, lift)| {
        let a = transform.affine(transform.pivot.unwrap_or(lift.centre));
        let back = invert(a).ok_or_else(|| anyhow!("this transform squashes the strokes flat"))?;
        let [ox, oy] = lift.offset;
        let into = [back[0], back[1], back[2], back[3], back[4] + ox, back[5] + oy];
        let [x0, y0, x1, y1] = [
            lift.reach[0] - ox,
            lift.reach[1] - oy,
            lift.reach[2] - ox,
            lift.reach[3] - oy,
        ];
        let moved = [(x0, y0), (x1, y0), (x0, y1), (x1, y1)].map(|p| apply_affine(a, p));
        let low = moved
            .iter()
            .fold((f32::MAX, f32::MAX), |m, p| (m.0.min(p.0), m.1.min(p.1)));
        let high = moved
            .iter()
            .fold((f32::MIN, f32::MIN), |m, p| (m.0.max(p.0), m.1.max(p.1)));
        let (cols, rows) = (lift.rest.cols, lift.rest.rows);
        let to = tiles_over([low.0 - 1.0, low.1 - 1.0, high.0 + 1.0, high.1 + 1.0], cols, rows);
        let drawn: Vec<(usize, Option<Tile>)> = to.par_iter().map(|&i| (i, laid(lift, into, i))).collect();
        let mut raster = lift.rest.clone();
        drawn
            .into_iter()
            .filter_map(|(i, tile)| tile.map(|t| (i, t)))
            .for_each(|(i, tile)| raster.restore(i, &tile));
        changed.extend(to);
        changed.extend(tiles_over([x0, y0, x1, y1], cols, rows));
        rasters.insert(cel.clone(), raster);
        Ok::<_, anyhow::Error>(())
    })?;
    Ok((rasters, changed))
}

impl Job {
    fn check(&self) -> Result<()> {
        if self.cancel.load(Ordering::Relaxed) {
            bail!("the drag ended");
        }
        Ok(())
    }

    pub fn run(self) -> Result<Done> {
        let lifted = match &self.ops[..] {
            [Op::Transform { ids, transform }]
                if self.snap.lifts.get().is_some()
                    || self.snap.slow.load(Ordering::Relaxed)
                    || whole_cels(&self.snap, ids)
                    || big(&self.snap, ids) =>
            {
                self.snap
                    .lifts
                    .get_or_init(|| lift(&self.snap, ids, &self.cancel))
                    .as_ref()
                    .filter(|l| l.ids == *ids)
                    .map(|l| shifted(&self.snap, l, transform))
                    .transpose()?
            }
            _ => None,
        };
        self.check()?;
        match lifted {
            Some((rasters, changed)) => {
                let snap = self.snap.clone();
                self.composed(&snap.bundle.doc, rasters, changed, false)
            }
            None => {
                let start = std::time::Instant::now();
                let (doc, rasters, changed, exact) = self.replayed()?;
                if start.elapsed() > SLOW {
                    self.snap.slow.store(true, Ordering::Relaxed);
                }
                self.composed(&doc, rasters, changed, exact)
            }
        }
    }

    fn replayed(&self) -> Result<(Document, Rasters, BTreeSet<usize>, bool)> {
        let snap = &self.snap;
        let (doc, mut applied) = apply_all(&snap.bundle.doc, &self.ops)?;
        self.check()?;
        let (w, h) = (doc.canvas.width, doc.canvas.height);
        let mut rasters = snap.rasters.clone();
        let mut changed = BTreeSet::new();
        let before = snap.vectors.get_or_init(|| vector_nodes(&snap.bundle.doc, snap.frame));
        let redrawn = vector_nodes(&doc, snap.frame)
            .into_iter()
            .filter(|(cel, nodes)| before.get(cel) != Some(nodes))
            .collect::<Vec<_>>()
            .into_par_iter()
            .map(|(cel, nodes)| {
                let old = before.get(&cel);
                let drawn = rasters.get(&cel);
                let tiles = match (old, drawn) {
                    (Some(old), Some(_)) => changed_tiles(old, &nodes, w, h)?,
                    _ => None,
                };
                let raster = match (tiles, drawn) {
                    (Some(tiles), Some(drawn)) => {
                        let mut patched = drawn.clone();
                        rasterize_tiles(&nodes, w, h, &tiles)?
                            .into_iter()
                            .for_each(|(i, tile)| match tile {
                                Some(tile) => patched.put(i, tile.into_boxed_slice()),
                                None => patched.clear(i),
                            });
                        patched
                    }
                    _ => rasterize_nodes(&nodes, w, h)?,
                };
                let differs = match drawn {
                    Some(drawn) => drawn.differs(&raster),
                    None => raster.occupied().map(|(i, _)| i).collect(),
                };
                Ok((cel, raster, differs))
            })
            .collect::<Result<Vec<_>>>()?;
        redrawn.into_iter().for_each(|(cel, raster, differs)| {
            changed.extend(differs);
            rasters.insert(cel, raster);
        });
        let plan = applied.marks.clone();
        let exact = applied.marks.is_empty();
        applied.marks.iter_mut().try_for_each(|pending| {
            let replay = snap
                .cels
                .get(&pending.cel)
                .ok_or_else(|| anyhow!("cel {} is not in memory", pending.cel))?;
            let mut replay = replay.clone();
            let mut marks = replay.marks().to_vec();
            pending.changes.iter_mut().try_for_each(|change| {
                snap.bundle
                    .apply_change(&snap.bundle.doc, &plan, &mut marks, change, &pending.cel)
            })?;
            let mut raster = rasters.get(&pending.cel).cloned().unwrap_or_else(|| Raster::new(w, h));
            changed.extend(replay.update(&mut raster, marks, &snap.brushes)?.tiles);
            rasters.insert(pending.cel.clone(), raster);
            Ok::<_, anyhow::Error>(())
        })?;
        self.check()?;
        Ok((doc, rasters, changed, exact))
    }

    fn composed(self, doc: &Document, rasters: Rasters, changed: BTreeSet<usize>, exact: bool) -> Result<Done> {
        let snap = &self.snap;
        let (w, h) = (doc.canvas.width, doc.canvas.height);
        let cols = (w as usize).div_ceil(klad_brush::TILE);
        let rows = (h as usize).div_ceil(klad_brush::TILE);
        let reach = effect_reach(doc, &doc.layers);
        let changed: BTreeSet<usize> = dilate(&changed.into_iter().collect::<Vec<_>>(), reach, cols, rows)
            .into_iter()
            .collect();
        let wanted: Vec<usize> = changed.union(&self.shown).copied().collect();
        let (now, hidden): (Vec<usize>, Vec<usize>) = wanted.into_iter().partition(|i| self.visible.contains(i));
        let layers: Vec<&Layer> = doc.layers.iter().collect();
        let background = doc.canvas.background.as_deref();
        let tiles = now
            .par_iter()
            .map(|&i| {
                self.check()?;
                let tile = compose_items(
                    doc,
                    snap.frame,
                    &rasters,
                    snap.onions.as_ref(),
                    &layers,
                    i,
                    background,
                    None,
                )?;
                Ok((i, tile.unwrap_or_else(|| vec![f16::ZERO; TILE_PX * 4])))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Done {
            ops: self.ops,
            tiles,
            changed,
            hidden: hidden.into_iter().collect(),
            exact,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use klad_core::markedit::address;
    use std::time::Instant;

    fn drawing(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("klad-lift-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut doc = Document::new(600, 300);
        doc.canvas.background = Some("#ffffff".into());
        doc.layers = vec![Layer::paint("a", "a", "ca")];
        let mut bundle = Bundle::create(&dir, doc).unwrap();
        bundle
            .apply(
                &klad_core::ops::parse_lines(
                    r#"{"op":"stroke","layer":"a","path":"M40 40 L300 200","size":12}
{"op":"stroke","layer":"a","path":"M60 260 L400 60","size":9}"#,
                )
                .unwrap(),
            )
            .unwrap();
        dir
    }

    #[test]
    fn a_moved_stroke_previews_as_it_lands_and_a_turned_one_close_to_it() {
        let path = drawing("moved");
        let scene = crate::composite::Scene::open(&path).unwrap();
        let brushes = BrushSet::for_bundle(&scene.bundle).unwrap();
        let cel = "ca".to_string();
        let top = scene.marks_of(&cel).last().unwrap().id();
        let snap = Arc::new(scene.preview_snapshot(std::slice::from_ref(&cel), &brushes).unwrap());
        snap.slow.store(true, Ordering::Relaxed);
        let visible: BTreeSet<usize> = scene.all_tiles().into_iter().collect();
        let job = |transform: MarkTransform| Job {
            ops: vec![Op::Transform {
                ids: vec![address(&cel, top)],
                transform,
            }],
            snap: snap.clone(),
            shown: BTreeSet::new(),
            visible: visible.clone(),
            cancel: Arc::default(),
        };
        let apart = |transform: MarkTransform| -> (f32, f32) {
            let lifted = job(transform).run().unwrap();
            assert!(!lifted.exact, "the move is previewed from the lifted strokes");
            let exact = job(transform);
            let (doc, rasters, changed, _) = exact.replayed().unwrap();
            let exact = exact.composed(&doc, rasters, changed, true).unwrap();
            let gaps: Vec<f32> = exact
                .tiles
                .iter()
                .flat_map(|(i, tile)| {
                    let shown = lifted
                        .tiles
                        .iter()
                        .find(|(j, _)| j == i)
                        .map(|(_, t)| t.clone())
                        .expect("every tile the move changes is previewed");
                    tile.iter()
                        .zip(shown)
                        .map(|(a, b)| (a.to_f32() - b.to_f32()).abs())
                        .collect::<Vec<f32>>()
                })
                .collect();
            let most = gaps.iter().copied().fold(0.0, f32::max);
            (most, gaps.iter().sum::<f32>() / gaps.len() as f32)
        };
        let (most, _) = apart(MarkTransform {
            x: 30.0,
            y: -12.0,
            ..MarkTransform::default()
        });
        assert!(most < 0.01, "a move by whole pixels previews exactly: {most}");
        let (_, mean) = apart(MarkTransform {
            rotate: 20.0,
            ..MarkTransform::default()
        });
        assert!(mean < 0.01, "a turn previews close to where it lands: {mean}");
        let _ = std::fs::remove_dir_all(&path);
    }

    #[test]
    fn a_whole_layer_of_alpha_locked_oil_moves_as_its_picture_from_the_first_step() {
        let dir = std::env::temp_dir().join(format!("klad-lift-whole-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut doc = Document::new(600, 300);
        doc.canvas.background = Some("#ffffff".into());
        doc.layers = vec![Layer::paint("a", "a", "ca")];
        let mut bundle = Bundle::create(&dir, doc).unwrap();
        let strokes: Vec<String> = (0..40)
            .map(|k| {
                let y = 70 + k * 4;
                format!(
                    r##"{{"op":"stroke","layer":"a","path":"M60 {y} L420 {}","size":14,"brush":"oil","color":"#{:02x}5020"}}"##,
                    y + 20,
                    120 + k * 3
                )
            })
            .collect();
        let ops = [
            r##"{"op":"stroke","layer":"a","path":"M120 160 L380 160","size":180,"color":"#e6bf34"}"##.to_string(),
            r##"{"op":"set","id":"a","lockAlpha":true}"##.to_string(),
        ]
        .into_iter()
        .chain(strokes)
        .collect::<Vec<_>>()
        .join("\n");
        bundle.apply(&klad_core::ops::parse_lines(&ops).unwrap()).unwrap();
        let scene = crate::composite::Scene::open(&dir).unwrap();
        let brushes = BrushSet::for_bundle(&scene.bundle).unwrap();
        let cel = "ca".to_string();
        let marks = scene.marks_of(&cel);
        assert!(
            marks.iter().filter_map(Mark::as_stroke).skip(1).all(|s| s.lock),
            "every stroke on the base is alpha-locked"
        );
        let ids: Vec<String> = marks.iter().map(|m| address(&cel, m.id())).collect();
        let snap = Arc::new(scene.preview_snapshot(std::slice::from_ref(&cel), &brushes).unwrap());
        let visible: BTreeSet<usize> = scene.all_tiles().into_iter().collect();
        let transform = MarkTransform {
            x: 40.0,
            y: 16.0,
            ..MarkTransform::default()
        };
        let job = || Job {
            ops: vec![Op::Transform {
                ids: ids.clone(),
                transform,
            }],
            snap: snap.clone(),
            shown: BTreeSet::new(),
            visible: visible.clone(),
            cancel: Arc::default(),
        };
        let lifted = job().run().unwrap();
        assert!(
            !lifted.exact,
            "the first step already moves the layer's picture instead of painting every stroke again"
        );
        let exact = job();
        let (doc, rasters, changed, _) = exact.replayed().unwrap();
        let exact = exact.composed(&doc, rasters, changed, true).unwrap();
        let gaps: Vec<f32> = exact
            .tiles
            .iter()
            .flat_map(|(i, tile)| {
                let shown = lifted
                    .tiles
                    .iter()
                    .find(|(j, _)| j == i)
                    .map(|(_, t)| t.clone())
                    .expect("every tile the move changes is previewed");
                tile.iter()
                    .zip(shown)
                    .map(|(a, b)| (a.to_f32() - b.to_f32()).abs())
                    .collect::<Vec<f32>>()
            })
            .collect();
        let mean = gaps.iter().sum::<f32>() / gaps.len() as f32;
        assert!(mean < 0.02, "the moved picture is where the strokes land: {mean}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn part_of_a_layer_of_alpha_locked_oil_moves_as_its_picture_close_to_where_it_lands() {
        let dir = std::env::temp_dir().join(format!("klad-lift-part-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut doc = Document::new(600, 300);
        doc.canvas.background = Some("#ffffff".into());
        doc.layers = vec![Layer::paint("a", "a", "ca")];
        let mut bundle = Bundle::create(&dir, doc).unwrap();
        let strokes: Vec<String> = (0..80)
            .map(|k| {
                let y = 60 + k * 2;
                format!(
                    r##"{{"op":"stroke","layer":"a","path":"M{} {y} L{} {}","size":12,"brush":"oil","color":"#{:02x}5020"}}"##,
                    80 + (k % 2) * 200,
                    240 + (k % 2) * 200,
                    y + 14,
                    90 + k
                )
            })
            .collect();
        let ops = [
            r##"{"op":"stroke","layer":"a","path":"M60 150 L540 150","size":220,"color":"#e6bf34"}"##.to_string(),
            r##"{"op":"set","id":"a","lockAlpha":true}"##.to_string(),
        ]
        .into_iter()
        .chain(strokes)
        .collect::<Vec<_>>()
        .join("\n");
        bundle.apply(&klad_core::ops::parse_lines(&ops).unwrap()).unwrap();
        let scene = crate::composite::Scene::open(&dir).unwrap();
        let brushes = BrushSet::for_bundle(&scene.bundle).unwrap();
        let cel = "ca".to_string();
        let marks = scene.marks_of(&cel);
        let ids: Vec<String> = marks.iter().skip(1).step_by(2).map(|m| address(&cel, m.id())).collect();
        assert!(ids.len() >= MANY && ids.len() < marks.len() - 1);
        let snap = Arc::new(scene.preview_snapshot(std::slice::from_ref(&cel), &brushes).unwrap());
        let visible: BTreeSet<usize> = scene.all_tiles().into_iter().collect();
        let job = || Job {
            ops: vec![Op::Transform {
                ids: ids.clone(),
                transform: MarkTransform {
                    x: 30.0,
                    y: 12.0,
                    ..MarkTransform::default()
                },
            }],
            snap: snap.clone(),
            shown: BTreeSet::new(),
            visible: visible.clone(),
            cancel: Arc::default(),
        };
        let lifted = job().run().unwrap();
        assert!(
            !lifted.exact,
            "the first step already moves the picked strokes as a picture"
        );
        let exact = job();
        let (doc, rasters, changed, _) = exact.replayed().unwrap();
        let exact = exact.composed(&doc, rasters, changed, true).unwrap();
        let gaps: Vec<f32> = exact
            .tiles
            .iter()
            .flat_map(|(i, tile)| {
                let shown = lifted
                    .tiles
                    .iter()
                    .find(|(j, _)| j == i)
                    .map(|(_, t)| t.clone())
                    .expect("every tile the move changes is previewed");
                tile.iter()
                    .zip(shown)
                    .map(|(a, b)| (a.to_f32() - b.to_f32()).abs())
                    .collect::<Vec<f32>>()
            })
            .collect();
        let mean = gaps.iter().sum::<f32>() / gaps.len() as f32;
        assert!(
            mean < 0.02,
            "the moved picture is close to where the strokes land: {mean}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[ignore = "a timing to read: KLAD_DRAG_BENCH=<drawing> [KLAD_DRAG_LAYER=<layer>] [KLAD_DRAG_AT=<first mark>] [KLAD_DRAG_COUNT=<marks>] cargo test drag_steps -- --ignored --nocapture"]
    fn drag_steps_on_a_large_drawing() {
        let Ok(path) = std::env::var("KLAD_DRAG_BENCH") else {
            return;
        };
        let at: usize = std::env::var("KLAD_DRAG_AT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let count: usize = std::env::var("KLAD_DRAG_COUNT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1);
        let copy = std::env::temp_dir().join(format!("klad-drag-bench-{}.klad", std::process::id()));
        let _ = std::fs::remove_dir_all(&copy);
        copy_dir(std::path::Path::new(&path), &copy);
        let start = Instant::now();
        let mut scene = crate::composite::Scene::open(&copy).unwrap();
        let brushes = BrushSet::for_bundle(&scene.bundle).unwrap();
        eprintln!("opened in {:.0} ms", start.elapsed().as_secs_f64() * 1000.0);
        let layer = std::env::var("KLAD_DRAG_LAYER").ok();
        let cels = scene.bundle.doc.paint_cels();
        let cel = cels
            .iter()
            .find(|(l, _)| layer.as_ref().is_none_or(|name| &l.name == name || &l.id == name))
            .map(|(_, c)| c.id.clone())
            .unwrap();
        let marks = scene.marks_of(&cel);
        let ids: Vec<String> = marks[at.min(marks.len() - 1)..(at + count).min(marks.len())]
            .iter()
            .map(|m| address(&cel, m.id()))
            .collect();
        let snap = Arc::new(scene.preview_snapshot(std::slice::from_ref(&cel), &brushes).unwrap());
        let visible: BTreeSet<usize> = scene.all_tiles().into_iter().collect();
        let mut shown = BTreeSet::new();
        let times: Vec<f64> = (1..=8)
            .map(|step| {
                let job = Job {
                    ops: vec![Op::Transform {
                        ids: ids.clone(),
                        transform: MarkTransform {
                            x: 7.0 * step as f32,
                            y: 3.0 * step as f32,
                            ..MarkTransform::default()
                        },
                    }],
                    snap: snap.clone(),
                    shown: shown.clone(),
                    visible: visible.clone(),
                    cancel: Arc::default(),
                };
                let start = Instant::now();
                let done = job.run().unwrap();
                shown = done.changed;
                start.elapsed().as_secs_f64() * 1000.0
            })
            .collect();
        eprintln!(
            "{} marks from {at} of {}: drag steps {}",
            ids.len(),
            marks.len(),
            times
                .iter()
                .map(|t| format!("{t:.0} ms"))
                .collect::<Vec<_>>()
                .join(", ")
        );
        let ops = vec![Op::Transform {
            ids,
            transform: MarkTransform {
                x: 56.0,
                y: 24.0,
                ..MarkTransform::default()
            },
        }];
        let start = Instant::now();
        let dirty = scene.apply(&ops, &brushes).unwrap();
        eprintln!(
            "letting go: {:.0} ms, {} tiles redrawn",
            start.elapsed().as_secs_f64() * 1000.0,
            dirty.len()
        );
        let last = scene.marks_of(&cel).last().map(|m| address(&cel, m.id())).unwrap();
        let start = Instant::now();
        scene
            .apply(
                &[Op::Transform {
                    ids: vec![last],
                    transform: MarkTransform {
                        x: 5.0,
                        ..MarkTransform::default()
                    },
                }],
                &brushes,
            )
            .unwrap();
        eprintln!(
            "then moving the last stroke: {:.0} ms",
            start.elapsed().as_secs_f64() * 1000.0
        );
        let _ = std::fs::remove_dir_all(&copy);
    }

    fn copy_dir(from: &std::path::Path, to: &std::path::Path) {
        std::fs::create_dir_all(to).unwrap();
        std::fs::read_dir(from).unwrap().flatten().for_each(|entry| {
            let target = to.join(entry.file_name());
            match entry.file_type().unwrap().is_dir() {
                true => copy_dir(&entry.path(), &target),
                false => {
                    std::fs::copy(entry.path(), target).unwrap();
                }
            }
        });
    }
}
