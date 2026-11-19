use klad_brush::{Raster, TILE};
use klad_core::region::{self, Ring};
use std::collections::VecDeque;

pub const TOLERANCE: f32 = 0.1;

const SEEN: f32 = 0.02;
const GROW: usize = 2;

fn colour(raster: &Raster, x: usize, y: usize) -> Option<[f32; 3]> {
    let tile = raster.get(x / TILE, y / TILE)?;
    let at = ((y % TILE) * TILE + x % TILE) * 4;
    let a = tile[at + 3].to_f32();
    (a > SEEN).then(|| [0, 1, 2].map(|i| tile[at + i].to_f32() / a))
}

pub fn painted_at(raster: &Raster, x: f32, y: f32) -> bool {
    x >= 0.0
        && y >= 0.0
        && x < raster.width as f32
        && y < raster.height as f32
        && colour(raster, x as usize, y as usize).is_some()
}

fn flood(raster: &Raster, x: usize, y: usize, tolerance: f32) -> Option<(Vec<bool>, [usize; 4])> {
    let (w, h) = (raster.width as usize, raster.height as usize);
    let seed = colour(raster, x, y)?;
    let near = |c: [f32; 3]| {
        let d = (0..3).map(|i| (c[i] - seed[i]).powi(2)).sum::<f32>().sqrt() / 3f32.sqrt();
        d <= tolerance + 1e-4
    };
    let mut taken = vec![false; w * h];
    let mut queue = VecDeque::from([(x, y)]);
    taken[y * w + x] = true;
    let mut span = [x, y, x, y];
    while let Some((px, py)) = queue.pop_front() {
        span = [span[0].min(px), span[1].min(py), span[2].max(px), span[3].max(py)];
        [(-1i64, 0i64), (1, 0), (0, -1), (0, 1)].iter().for_each(|(dx, dy)| {
            let (nx, ny) = (px as i64 + dx, py as i64 + dy);
            if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                return;
            }
            let (nx, ny) = (nx as usize, ny as usize);
            if !taken[ny * w + nx] && colour(raster, nx, ny).is_some_and(near) {
                taken[ny * w + nx] = true;
                queue.push_back((nx, ny));
            }
        });
    }
    Some((taken, span))
}

pub fn region(raster: &Raster, x: f32, y: f32, tolerance: f32, cells: bool) -> Option<Ring> {
    if !painted_at(raster, x, y) {
        return None;
    }
    let (taken, [x0, y0, x1, y1]) = flood(raster, x as usize, y as usize, tolerance.clamp(0.0, 1.0))?;
    let (full, tall) = (raster.width as usize, raster.height as usize);
    let grow = if cells { 0 } else { GROW };
    let (x0, y0) = (x0.saturating_sub(grow), y0.saturating_sub(grow));
    let (x1, y1) = ((x1 + grow).min(full - 1), (y1 + grow).min(tall - 1));
    let (w, h) = (x1 - x0 + 1, y1 - y0 + 1);
    let at = |i: usize| (x0 + i % w, y0 + i / w);
    let mask: Vec<bool> = (0..w * h)
        .map(|i| {
            let (px, py) = at(i);
            let (lo_x, lo_y) = (px.saturating_sub(grow), py.saturating_sub(grow));
            let (hi_x, hi_y) = ((px + grow).min(full - 1), (py + grow).min(tall - 1));
            (lo_y..=hi_y).any(|qy| (lo_x..=hi_x).any(|qx| taken[qy * full + qx]))
        })
        .collect();
    let ring = match cells {
        true => region::joined(region::cell_rings(&mask, w, h)),
        false => {
            let alpha: Vec<f32> = mask.iter().map(|m| if *m { 1.0 } else { 0.0 }).collect();
            region::joined(crate::occluders::polygons(&alpha, w, h, 0.75))
        }
    }?;
    (ring.len() >= 3).then(|| {
        ring.into_iter()
            .map(|[px, py]| [px + x0 as f32, py + y0 as f32])
            .collect()
    })
}
