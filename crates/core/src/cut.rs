use crate::varint::{self, Reader};
use anyhow::{Result, bail};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keep {
    Inside,
    Outside,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Cut {
    pub keep: Keep,
    pub polygon: Vec<[f32; 2]>,
}

pub fn inside_polygon(polygon: &[[f32; 2]], (x, y): (f32, f32)) -> bool {
    let n = polygon.len();
    (0..n).fold(false, |inside, i| {
        let (a, b) = (polygon[i], polygon[(i + n - 1) % n]);
        if (a[1] > y) != (b[1] > y) && x < (b[0] - a[0]) * (y - a[1]) / (b[1] - a[1]) + a[0] {
            !inside
        } else {
            inside
        }
    })
}

pub fn crossings(polygon: &[[f32; 2]], y: f32) -> Vec<f32> {
    let n = polygon.len();
    let mut xs: Vec<f32> = (0..n)
        .filter_map(|i| {
            let (a, b) = (polygon[i], polygon[(i + n - 1) % n]);
            ((a[1] > y) != (b[1] > y)).then(|| (b[0] - a[0]) * (y - a[1]) / (b[1] - a[1]) + a[0])
        })
        .collect();
    xs.sort_by(f32::total_cmp);
    xs
}

pub fn polygon_box(polygon: &[[f32; 2]]) -> Option<[f32; 4]> {
    let first = polygon.first()?;
    Some(
        polygon
            .iter()
            .fold([first[0], first[1], first[0], first[1]], |[x0, y0, x1, y1], p| {
                [x0.min(p[0]), y0.min(p[1]), x1.max(p[0]), y1.max(p[1])]
            }),
    )
}

impl Cut {
    pub fn keeps(&self, at: (f32, f32)) -> bool {
        inside_polygon(&self.polygon, at) == (self.keep == Keep::Inside)
    }

    pub fn map(&mut self, f: impl Fn([f32; 2]) -> [f32; 2]) {
        self.polygon.iter_mut().for_each(|p| *p = f(*p));
    }
}

pub fn keeps(cuts: &[Cut], at: (f32, f32)) -> bool {
    cuts.iter().all(|c| c.keeps(at))
}

pub fn clip_bounds(cuts: &[Cut], bounds: [f32; 4]) -> Option<[f32; 4]> {
    cuts.iter()
        .filter(|c| c.keep == Keep::Inside)
        .filter_map(|c| polygon_box(&c.polygon))
        .try_fold(bounds, |[x0, y0, x1, y1], [a0, b0, a1, b1]| {
            let clipped = [x0.max(a0), y0.max(b0), x1.min(a1), y1.min(b1)];
            (clipped[0] < clipped[2] && clipped[1] < clipped[3]).then_some(clipped)
        })
}

pub fn around_pixels(keep: &[bool], width: usize, height: usize, origin: (f32, f32)) -> Vec<[f32; 2]> {
    let kept = |x: i64, y: i64| {
        x >= 0 && y >= 0 && x < width as i64 && y < height as i64 && keep[y as usize * width + x as usize]
    };
    let mut out: BTreeMap<(i64, i64), Vec<(i64, i64)>> = BTreeMap::new();
    let mut edge = |from: (i64, i64), to: (i64, i64)| out.entry(from).or_default().push(to);
    (0..height as i64).for_each(|y| {
        (0..width as i64).filter(|&x| kept(x, y)).for_each(|x| {
            if !kept(x, y - 1) {
                edge((x, y), (x + 1, y));
            }
            if !kept(x + 1, y) {
                edge((x + 1, y), (x + 1, y + 1));
            }
            if !kept(x, y + 1) {
                edge((x + 1, y + 1), (x, y + 1));
            }
            if !kept(x - 1, y) {
                edge((x, y + 1), (x, y));
            }
        })
    });
    let mut rings: Vec<Vec<(i64, i64)>> = Vec::new();
    while let Some(&start) = out.keys().next() {
        let mut ring = vec![start];
        let mut here = start;
        while let Some(list) = out.get_mut(&here) {
            let next = list.pop();
            if list.is_empty() {
                out.remove(&here);
            }
            match next {
                Some(n) if n == start => break,
                Some(n) => {
                    ring.push(n);
                    here = n;
                }
                None => break,
            }
        }
        rings.push(ring);
    }
    let straight = |ring: &[(i64, i64)]| -> Vec<(i64, i64)> {
        let n = ring.len();
        (0..n)
            .filter(|&i| {
                let (a, b, c) = (ring[(i + n - 1) % n], ring[i], ring[(i + 1) % n]);
                (b.0 - a.0) * (c.1 - b.1) != (b.1 - a.1) * (c.0 - b.0)
            })
            .map(|i| ring[i])
            .collect()
    };
    let rings: Vec<Vec<[f32; 2]>> = rings
        .iter()
        .map(|r| straight(r))
        .filter(|r| r.len() >= 3)
        .map(|r| {
            r.iter()
                .map(|&(x, y)| [origin.0 + x as f32, origin.1 + y as f32])
                .collect()
        })
        .collect();
    let Some(anchor) = rings.first().and_then(|r| r.first()).copied() else {
        return Vec::new();
    };
    rings
        .iter()
        .enumerate()
        .flat_map(|(i, ring)| {
            let closing = ring.first().copied().into_iter();
            let back = (i > 0).then_some(anchor).into_iter();
            ring.iter().copied().chain(closing).chain(back)
        })
        .collect()
}

pub fn encode(out: &mut Vec<u8>, cuts: &[Cut]) {
    varint::put(out, cuts.len() as u64);
    cuts.iter().for_each(|cut| {
        out.push(match cut.keep {
            Keep::Inside => 0,
            Keep::Outside => 1,
        });
        varint::put(out, cut.polygon.len() as u64);
        cut.polygon
            .iter()
            .flatten()
            .for_each(|v| out.extend_from_slice(&v.to_le_bytes()));
    });
}

pub fn decode(reader: &mut Reader) -> Result<Vec<Cut>> {
    let count = reader.varint()? as usize;
    (0..count)
        .map(|_| {
            let keep = match reader.u8()? {
                0 => Keep::Inside,
                1 => Keep::Outside,
                other => bail!("unknown cut side {other}"),
            };
            let n = reader.varint()? as usize;
            let polygon = (0..n)
                .map(|_| Ok([reader.f32()?, reader.f32()?]))
                .collect::<Result<Vec<_>>>()?;
            Ok(Cut { keep, polygon })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(keep: Keep) -> Cut {
        Cut {
            keep,
            polygon: vec![[10.0, 10.0], [50.0, 10.0], [50.0, 50.0], [10.0, 50.0]],
        }
    }

    #[test]
    fn a_cut_around_pixels_keeps_exactly_those_pixels_with_holes_and_islands() {
        let rows = [
            "..........",
            ".####.....",
            ".#..#..##.",
            ".####..##.",
            "......#...",
            ".....#.##.",
            "..........",
        ];
        let (w, h) = (rows[0].len(), rows.len());
        let keep: Vec<bool> = rows.iter().flat_map(|r| r.chars().map(|c| c == '#')).collect();
        let polygon = around_pixels(&keep, w, h, (100.0, 50.0));
        assert!(polygon.len() < 60, "straight runs are one edge: {}", polygon.len());
        (0..h).for_each(|y| {
            (0..w).for_each(|x| {
                let at = (100.0 + x as f32 + 0.5, 50.0 + y as f32 + 0.5);
                assert_eq!(inside_polygon(&polygon, at), keep[y * w + x], "pixel {x},{y}");
                [(0.1, 0.1), (0.9, 0.1), (0.1, 0.9), (0.9, 0.9)]
                    .iter()
                    .for_each(|(dx, dy)| {
                        let corner = (100.0 + x as f32 + dx, 50.0 + y as f32 + dy);
                        assert_eq!(
                            inside_polygon(&polygon, corner),
                            keep[y * w + x],
                            "pixel {x},{y} near its corner"
                        );
                    });
            })
        });
        assert!(around_pixels(&[false; 4], 2, 2, (0.0, 0.0)).is_empty());
    }

    #[test]
    fn cuts_keep_one_side_and_round_trip() {
        let inside = square(Keep::Inside);
        let outside = square(Keep::Outside);
        assert!(inside.keeps((20.0, 20.0)) && !inside.keeps((60.0, 20.0)));
        assert!(!outside.keeps((20.0, 20.0)) && outside.keeps((60.0, 20.0)));
        assert!(!keeps(&[inside.clone(), outside.clone()], (20.0, 20.0)));
        assert_eq!(
            clip_bounds(std::slice::from_ref(&inside), [0.0, 0.0, 30.0, 100.0]),
            Some([10.0, 10.0, 30.0, 50.0])
        );
        assert_eq!(clip_bounds(std::slice::from_ref(&inside), [60.0, 0.0, 80.0, 5.0]), None);
        assert_eq!(
            clip_bounds(std::slice::from_ref(&outside), [60.0, 0.0, 80.0, 5.0]),
            Some([60.0, 0.0, 80.0, 5.0])
        );
        assert_eq!(crossings(&inside.polygon, 20.0), vec![10.0, 50.0]);
        let mut bytes = Vec::new();
        encode(&mut bytes, &[inside.clone(), outside.clone()]);
        let mut reader = Reader::new(&bytes);
        assert_eq!(decode(&mut reader).unwrap(), vec![inside, outside]);
        assert!(reader.done());
    }
}
