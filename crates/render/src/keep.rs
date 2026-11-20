use anyhow::Result;
use half::f16;
use klad_brush::cache::{CelCache, Write};
use klad_brush::replay::painted_over;
use klad_brush::{BrushSet, Raster, TILE};
use klad_core::bundle::Bundle;
use klad_core::cut::{Cut, Keep, around_pixels};
use klad_core::ops::{CopyTarget, Op};
use klad_core::stroke::Mark;
use std::collections::BTreeMap;

const MARGIN: f32 = 3.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Area {
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
}

impl Area {
    fn of(b: [f32; 4], width: u32, height: u32) -> Option<Self> {
        let clip = |v: f32, most: u32| (v.max(0.0) as usize).min(most as usize);
        let area = Self {
            x0: clip(b[0].floor(), width),
            y0: clip(b[1].floor(), height),
            x1: clip(b[2].ceil(), width),
            y1: clip(b[3].ceil(), height),
        };
        (area.x0 < area.x1 && area.y0 < area.y1).then_some(area)
    }

    fn join(self, o: Self) -> Self {
        Self {
            x0: self.x0.min(o.x0),
            y0: self.y0.min(o.y0),
            x1: self.x1.max(o.x1),
            y1: self.y1.max(o.y1),
        }
    }

    fn width(&self) -> usize {
        self.x1 - self.x0
    }

    fn index(&self, x: i64, y: i64) -> Option<usize> {
        let inside = x >= self.x0 as i64 && y >= self.y0 as i64 && x < self.x1 as i64 && y < self.y1 as i64;
        inside.then(|| (y as usize - self.y0) * self.width() + x as usize - self.x0)
    }

    fn pixels(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        (self.y0..self.y1).flat_map(move |y| (self.x0..self.x1).map(move |x| (x, y)))
    }
}

fn locked(mark: &Mark) -> bool {
    match mark {
        Mark::Stroke(s) => s.lock,
        Mark::Fill(f) => f.lock && !f.erases(),
    }
}

fn reach(mark: &Mark, brushes: &BrushSet) -> Option<[f32; 4]> {
    match mark {
        Mark::Stroke(s) => {
            let pad = brushes
                .def(&s.brush)
                .map_or(s.size * 0.5 + 2.0, |d| klad_core::attach::reach(s, d))
                + MARGIN;
            s.bounds()
                .map(|[x0, y0, x1, y1]| [x0 - pad, y0 - pad, x1 + pad, y1 + pad])
        }
        Mark::Fill(f) => f.coverage.bounds(),
    }
}

fn pixel(raster: &Raster, x: usize, y: usize) -> [f32; 4] {
    raster.get(x / TILE, y / TILE).map_or([0.0; 4], |t| {
        let i = ((y % TILE) * TILE + x % TILE) * 4;
        std::array::from_fn(|c| t[i + c].to_f32())
    })
}

fn put(raster: &mut Raster, x: usize, y: usize, value: [f32; 4]) {
    let index = raster.index(x / TILE, y / TILE);
    let tile = raster.ensure(index);
    let i = ((y % TILE) * TILE + x % TILE) * 4;
    (0..4).for_each(|c| tile[i + c] = f16::from_f32(value[c]));
}

fn apart(a: [f32; 4], b: [f32; 4]) -> f32 {
    (0..4).map(|c| (a[c] - b[c]).abs()).fold(0.0, f32::max)
}

fn cut_along(keep: &[bool], area: Area, reach: [f32; 4], width: u32, height: u32) -> Option<Cut> {
    let (bx0, by0) = (reach[0].floor() as i64, reach[1].floor() as i64);
    let (bw, bh) = (
        (reach[2].ceil() as i64 - bx0).max(1) as usize,
        (reach[3].ceil() as i64 - by0).max(1) as usize,
    );
    let on_canvas = |x: i64, y: i64| x >= 0 && y >= 0 && x < width as i64 && y < height as i64;
    let mask: Vec<bool> = (0..bw * bh)
        .map(|i| {
            let (x, y) = (bx0 + (i % bw) as i64, by0 + (i / bw) as i64);
            match (on_canvas(x, y), area.index(x, y)) {
                (true, Some(at)) => keep[at],
                _ => true,
            }
        })
        .collect();
    if mask.iter().all(|k| *k) {
        return None;
    }
    let polygon = match around_pixels(&mask, bw, bh, (bx0 as f32, by0 as f32)) {
        none if none.is_empty() => vec![[bx0 as f32, by0 as f32]; 3],
        some => some,
    };
    Some(Cut {
        keep: Keep::Inside,
        polygon,
    })
}

fn trim(fill: &mut klad_core::fill::Fill, keep: &[bool], area: Area) -> bool {
    let c = &mut fill.coverage;
    let (x0, y0, w) = (c.x as i64, c.y as i64, c.width.max(1) as usize);
    let mut trimmed = false;
    c.alpha.iter_mut().enumerate().for_each(|(i, a)| {
        let Some(at) = area.index(x0 + (i % w) as i64, y0 + (i / w) as i64) else {
            return;
        };
        if *a > 0 && !keep[at] {
            *a = 0;
            trimmed = true;
        }
    });
    trimmed
}

pub fn keep_own_paint(
    under: &Raster,
    alone: Option<Raster>,
    upper: &mut [Mark],
    brushes: &BrushSet,
) -> Result<(usize, Raster)> {
    let (width, height) = (under.width, under.height);
    let area = upper
        .iter()
        .filter(|m| locked(m))
        .filter_map(|m| reach(m, brushes).and_then(|r| Area::of(r, width, height)))
        .reduce(Area::join);
    let with = painted_over(under, upper, brushes)?;
    let Some(area) = area else {
        return Ok((0, with));
    };
    let plain: Vec<Mark> = upper.iter().filter(|m| !locked(m)).cloned().collect();
    let without = painted_over(under, &plain, brushes)?;
    let alone = match alone {
        Some(raster) => raster,
        None => painted_over(&Raster::new(width, height), upper, brushes)?,
    };
    let keep: Vec<bool> = area
        .pixels()
        .map(|(x, y)| {
            let (own, below) = (pixel(&alone, x, y), pixel(under, x, y));
            let stacked: [f32; 4] = std::array::from_fn(|c| own[c] + below[c] * (1.0 - own[3]));
            apart(pixel(&with, x, y), stacked) <= apart(pixel(&without, x, y), stacked)
        })
        .collect();
    let kept = upper
        .iter_mut()
        .filter(|m| locked(m))
        .map(|m| {
            let Some(r) = reach(m, brushes) else {
                return false;
            };
            match m {
                Mark::Stroke(s) => cut_along(&keep, area, r, width, height)
                    .map(|cut| s.cuts.push(cut))
                    .is_some(),
                Mark::Fill(f) => trim(f, &keep, area),
            }
        })
        .filter(|changed| *changed)
        .count();
    let mut merged = with;
    area.pixels()
        .zip(&keep)
        .filter(|(_, k)| !**k)
        .for_each(|((x, y), _)| put(&mut merged, x, y, pixel(&without, x, y)));
    Ok((kept, merged))
}

pub fn merged(bundle: &Bundle, brushes: &BrushSet, ops: &mut [Op]) -> Result<usize> {
    let doc = &bundle.doc;
    let (width, height) = (doc.canvas.width, doc.canvas.height);
    let into = |op: &Op| match op {
        Op::Copy(spec) => match &spec.to {
            CopyTarget::Frame {
                frame,
                layer: Some(target),
            } => Some((target.clone(), *frame)),
            _ => None,
        },
        _ => None,
    };
    let locks: Vec<(String, u32)> = ops
        .iter()
        .filter_map(|op| match (into(op), op) {
            (Some(key), Op::Copy(spec)) if spec.held.iter().any(|(_, m)| locked(m)) => Some(key),
            _ => None,
        })
        .collect();
    let cache = CelCache::new(bundle, brushes, Write::Never);
    let stored = |layer: &str, cel: &str| cache.load(bundle, brushes, layer, cel, false).map(|l| l.raster);
    let mut unders: BTreeMap<(String, u32), Raster> = BTreeMap::new();
    ops.iter_mut().try_fold(0usize, |kept, op| {
        let Some(key) = into(op).filter(|k| locks.contains(k)) else {
            return Ok(kept);
        };
        let Op::Copy(spec) = op else {
            return Ok(kept);
        };
        let under = match unders.remove(&key) {
            Some(raster) => raster,
            None => {
                let target = doc.find_layer(&key.0)?;
                match target.content_at(key.1) {
                    Some(cel) => stored(&target.id, &cel.id)?,
                    None => Raster::new(width, height),
                }
            }
        };
        let source = spec.held.first().and_then(|(cel, _)| {
            doc.all_layers()
                .into_iter()
                .find(|l| l.cels().iter().any(|c| &c.id == cel))
                .filter(|l| l.opacity >= 1.0)
                .map(|l| (l.id.clone(), cel.clone()))
        });
        let alone = source.and_then(|(layer, cel)| stored(&layer, &cel).ok());
        let mut marks: Vec<Mark> = spec.held.iter().map(|(_, m)| m.clone()).collect();
        let (more, next) = keep_own_paint(&under, alone, &mut marks, brushes)?;
        spec.held
            .iter_mut()
            .zip(marks)
            .for_each(|((_, held), mark)| *held = mark);
        unders.insert(key, next);
        Ok(kept + more)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use klad_core::fill::{Coverage, Fill, FillSource};
    use klad_core::stroke::Stroke;

    const W: u32 = 220;
    const H: u32 = 140;

    fn disc(id: u32, colour: [f32; 4], centre: (f32, f32), radius: f32) -> Mark {
        let mask: Vec<u8> = (0..(W * H) as usize)
            .map(|i| {
                let (x, y) = ((i % W as usize) as f32 + 0.5, (i / W as usize) as f32 + 0.5);
                let d = libm::hypotf(x - centre.0, y - centre.1);
                ((radius - d + 0.5).clamp(0.0, 1.0) * 255.0).round() as u8
            })
            .collect();
        let ring: Vec<[f32; 2]> = (0..48)
            .map(|k| {
                let t = k as f32 / 48.0 * std::f32::consts::TAU;
                [centre.0 + radius * t.cos(), centre.1 + radius * t.sin()]
            })
            .collect();
        Mark::Fill(Fill::new(
            id,
            colour,
            FillSource::Polygon(ring),
            Coverage::from_mask(W, H, &mask),
        ))
    }

    fn locked_line(id: u32, colour: [f32; 4], from: (f32, f32), to: (f32, f32)) -> Mark {
        let points: Vec<(f32, f32, f32)> = (0..=20)
            .map(|k| {
                let t = k as f32 / 20.0;
                (from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t, 1.0)
            })
            .collect();
        let mut stroke = Stroke::new(id, "ink", colour, 9.0, id).with_points(&points, 4.0);
        stroke.lock = true;
        Mark::Stroke(stroke)
    }

    fn stacked(under: &Raster, alone: &Raster) -> Raster {
        let mut out = Raster::new(W, H);
        (0..H as usize).for_each(|y| {
            (0..W as usize).for_each(|x| {
                let (own, below) = (pixel(alone, x, y), pixel(under, x, y));
                put(
                    &mut out,
                    x,
                    y,
                    std::array::from_fn(|c| own[c] + below[c] * (1.0 - own[3])),
                );
            })
        });
        out
    }

    fn off(a: &Raster, b: &Raster) -> (usize, f32) {
        (0..H as usize)
            .flat_map(|y| (0..W as usize).map(move |x| (x, y)))
            .map(|(x, y)| apart(pixel(a, x, y), pixel(b, x, y)))
            .fold((0, 0.0f32), |(n, most), d| {
                (n + (d > 8.0 / 255.0) as usize, most.max(d))
            })
    }

    #[test]
    fn merged_alpha_locked_strokes_stay_on_their_own_layers_paint() {
        let brushes = BrushSet::presets();
        let under = painted_over(
            &Raster::new(W, H),
            &[disc(1, [0.15, 0.2, 0.55, 1.0], (70.0, 70.0), 60.0)],
            &brushes,
        )
        .unwrap();
        let mut upper = vec![disc(1, [0.9, 0.75, 0.2, 1.0], (140.0, 70.0), 45.0)];
        (0..24).for_each(|k| {
            let y = 30.0 + k as f32 * 3.5;
            let shade = [0.95 - k as f32 * 0.02, 0.6, 0.1 + k as f32 * 0.02, 1.0];
            upper.push(locked_line(k + 2, shade, (80.0, y), (200.0, y + 6.0)));
        });
        let alone = painted_over(&Raster::new(W, H), &upper, &brushes).unwrap();
        let wanted = stacked(&under, &alone);
        let (spilled, _) = off(&painted_over(&under, &upper, &brushes).unwrap(), &wanted);
        assert!(
            spilled > 500,
            "without keeping them, the strokes paint on the layer under: {spilled}"
        );
        let (kept, next) = keep_own_paint(&under, None, &mut upper, &brushes).unwrap();
        assert!(kept > 10, "the strokes that reach the layer under get a cut: {kept}");
        let merged = painted_over(&under, &upper, &brushes).unwrap();
        let (left, most) = off(&merged, &wanted);
        let astray: Vec<(usize, usize)> = (0..H as usize)
            .flat_map(|y| (0..W as usize).map(move |x| (x, y)))
            .filter(|&(x, y)| apart(pixel(&merged, x, y), pixel(&wanted, x, y)) > 8.0 / 255.0)
            .filter(|&(x, y)| {
                let a = pixel(&alone, x, y)[3];
                a <= 0.0 || a >= 1.0
            })
            .collect();
        assert!(
            astray.is_empty(),
            "only the upper layer's soft rim can differ: {astray:?}"
        );
        assert!(
            left < spilled / 5 && most < 0.25,
            "and only a little: {left} pixels, at most {most}"
        );
        assert_eq!(
            off(&merged, &next).0,
            0,
            "the picture handed on is the one the cut strokes paint"
        );
    }
}
