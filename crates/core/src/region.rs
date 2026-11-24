use std::collections::BTreeMap;

pub type Ring = Vec<[f32; 2]>;

pub fn cell_rings(mask: &[bool], width: usize, height: usize) -> Vec<Ring> {
    let inside = |x: i64, y: i64| {
        x >= 0 && y >= 0 && (x as usize) < width && (y as usize) < height && mask[y as usize * width + x as usize]
    };
    let mut next: BTreeMap<(i64, i64), Vec<(i64, i64)>> = BTreeMap::new();
    let mut add = |a: (i64, i64), b: (i64, i64)| next.entry(a).or_default().push(b);
    (0..height as i64).for_each(|y| {
        (0..width as i64).filter(|&x| inside(x, y)).for_each(|x| {
            if !inside(x, y - 1) {
                add((x, y), (x + 1, y));
            }
            if !inside(x + 1, y) {
                add((x + 1, y), (x + 1, y + 1));
            }
            if !inside(x, y + 1) {
                add((x + 1, y + 1), (x, y + 1));
            }
            if !inside(x - 1, y) {
                add((x, y + 1), (x, y));
            }
        })
    });
    let mut rings = Vec::new();
    while let Some(start) = next.keys().next().copied() {
        let mut ring = vec![start];
        let mut here = start;
        while let Some(list) = next.get_mut(&here) {
            let Some(to) = list.pop() else {
                break;
            };
            if list.is_empty() {
                next.remove(&here);
            }
            if to == start {
                break;
            }
            ring.push(to);
            here = to;
        }
        rings.push(straighten(&ring));
    }
    rings
}

fn straighten(ring: &[(i64, i64)]) -> Ring {
    let n = ring.len();
    ring.iter()
        .enumerate()
        .filter(|(i, p)| {
            let (a, b) = (ring[(i + n - 1) % n], ring[(i + 1) % n]);
            (p.0 - a.0) * (b.1 - p.1) != (p.1 - a.1) * (b.0 - p.0)
        })
        .map(|(_, p)| [p.0 as f32, p.1 as f32])
        .collect()
}

pub fn area(ring: &[[f32; 2]]) -> f32 {
    let n = ring.len();
    (0..n)
        .map(|i| {
            let (a, b) = (ring[i], ring[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f32>()
        / 2.0
}

pub fn outer(rings: Vec<Ring>) -> Option<Ring> {
    rings.into_iter().max_by(|a, b| area(a).abs().total_cmp(&area(b).abs()))
}

pub fn joined(mut rings: Vec<Ring>) -> Option<Ring> {
    rings.sort_by(|a, b| area(b).abs().total_cmp(&area(a).abs()));
    let mut rings = rings.into_iter().filter(|r| r.len() >= 3);
    let outer = rings.next()?;
    let start = outer[0];
    Some(rings.fold(outer, |mut all, hole| {
        all.push(start);
        all.extend(hole.iter().copied());
        all.push(hole[0]);
        all.push(start);
        all
    }))
}

pub fn ellipse(x: f32, y: f32, w: f32, h: f32, points: usize) -> Ring {
    let (cx, cy, rx, ry) = (x + w / 2.0, y + h / 2.0, w / 2.0, h / 2.0);
    (0..points)
        .map(|i| {
            let t = core::f32::consts::TAU * i as f32 / points as f32;
            [cx + rx * libm::cosf(t), cy + ry * libm::sinf(t)]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rings_follow_the_cell_edges_and_the_outer_one_is_the_largest() {
        let (w, h) = (5, 4);
        let cells = [(1, 1), (2, 1), (3, 1), (1, 2), (3, 2)];
        let mask: Vec<bool> = (0..w * h).map(|i| cells.contains(&(i % w, i / w))).collect();
        let rings = cell_rings(&mask, w, h);
        assert_eq!(rings.len(), 1);
        let ring = outer(rings).unwrap();
        assert_eq!(area(&ring).abs(), 5.0);
        assert!(ring.iter().all(|p| p[0].fract() == 0.0 && p[1].fract() == 0.0));
        assert_eq!(ring.len(), 8);
        let hole: Vec<bool> = (0..9).map(|i| i != 4).collect();
        let rings = cell_rings(&hole, 3, 3);
        assert_eq!(rings.len(), 2);
        assert_eq!(area(&outer(rings).unwrap()).abs(), 9.0);
        let rings = cell_rings(&hole, 3, 3);
        let both = joined(rings).unwrap();
        assert!(crate::cut::inside_polygon(&both, (0.5, 0.5)));
        assert!(!crate::cut::inside_polygon(&both, (1.5, 1.5)), "the hole stays out");
        let e = ellipse(0.0, 0.0, 20.0, 10.0, 32);
        assert!((area(&e).abs() - core::f32::consts::PI * 50.0).abs() < 3.0);
    }
}
