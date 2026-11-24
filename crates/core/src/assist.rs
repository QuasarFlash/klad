use serde::{Deserialize, Serialize};

pub type P = [f32; 2];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Assist {
    Ruler {
        from: P,
        to: P,
    },
    Perspective {
        points: Vec<P>,
    },
    Ellipse {
        center: P,
        radii: P,
        #[serde(default)]
        turn: f32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Path {
    Line { at: P, dir: P },
    Oval { center: P, radii: P, turn: f32 },
}

const DECIDE: f32 = 6.0;
const NEAR: f32 = 12.0;

fn sub(a: P, b: P) -> P {
    [a[0] - b[0], a[1] - b[1]]
}

fn len(a: P) -> f32 {
    (a[0] * a[0] + a[1] * a[1]).sqrt()
}

fn unit(a: P) -> P {
    let l = len(a).max(1e-6);
    [a[0] / l, a[1] / l]
}

fn dot(a: P, b: P) -> f32 {
    a[0] * b[0] + a[1] * b[1]
}

impl Assist {
    pub fn handles(&self) -> Vec<P> {
        match self {
            Assist::Ruler { from, to } => vec![*from, *to],
            Assist::Perspective { points } => points.clone(),
            Assist::Ellipse { center, radii, turn } => {
                let (s, c) = (libm::sinf(turn.to_radians()), libm::cosf(turn.to_radians()));
                vec![
                    *center,
                    [center[0] + radii[0] * c, center[1] + radii[0] * s],
                    [center[0] - radii[1] * s, center[1] + radii[1] * c],
                ]
            }
        }
    }

    pub fn moved(&self, handle: usize, to: P) -> Assist {
        match self.clone() {
            Assist::Ruler { from, to: end } => match handle {
                0 => Assist::Ruler { from: to, to: end },
                _ => Assist::Ruler { from, to },
            },
            Assist::Perspective { mut points } => {
                if let Some(p) = points.get_mut(handle) {
                    *p = to;
                }
                Assist::Perspective { points }
            }
            Assist::Ellipse { center, radii, turn } => match handle {
                0 => Assist::Ellipse {
                    center: to,
                    radii,
                    turn,
                },
                1 => {
                    let d = sub(to, center);
                    Assist::Ellipse {
                        center,
                        radii: [len(d).max(1.0), radii[1]],
                        turn: libm::atan2f(d[1], d[0]).to_degrees(),
                    }
                }
                _ => Assist::Ellipse {
                    center,
                    radii: [radii[0], len(sub(to, center)).max(1.0)],
                    turn,
                },
            },
        }
    }

    pub fn word(&self) -> String {
        match self {
            Assist::Ruler { .. } => "a ruler".into(),
            Assist::Perspective { points } => format!("{}-point perspective", points.len()),
            Assist::Ellipse { .. } => "an ellipse".into(),
        }
    }

    fn choose(&self, start: P, heading: P) -> Option<(f32, Path)> {
        let fit = |dir: P| 1.0 - dot(unit(dir), unit(heading)).abs();
        match self {
            Assist::Ruler { from, to } => {
                let dir = unit(sub(*to, *from));
                let off = sub(start, *from);
                let across = (off[0] * dir[1] - off[1] * dir[0]).abs();
                let at = match across <= NEAR {
                    true => {
                        let t = dot(off, dir);
                        [from[0] + dir[0] * t, from[1] + dir[1] * t]
                    }
                    false => start,
                };
                Some((fit(dir), Path::Line { at, dir }))
            }
            Assist::Perspective { points } => {
                let mut dirs: Vec<P> = points
                    .iter()
                    .map(|v| sub(*v, start))
                    .filter(|d| len(*d) > 1e-3)
                    .collect();
                if points.len() < 3 {
                    dirs.push([0.0, 1.0]);
                }
                if points.len() < 2 {
                    dirs.push([1.0, 0.0]);
                }
                dirs.into_iter()
                    .map(|d| {
                        (
                            fit(d),
                            Path::Line {
                                at: start,
                                dir: unit(d),
                            },
                        )
                    })
                    .min_by(|a, b| a.0.total_cmp(&b.0))
            }
            Assist::Ellipse { center, radii, turn } => {
                let (s, c) = (libm::sinf(-turn.to_radians()), libm::cosf(-turn.to_radians()));
                let d = sub(start, *center);
                let local = [d[0] * c - d[1] * s, d[0] * s + d[1] * c];
                let k = ((local[0] / radii[0].max(1e-3)).powi(2) + (local[1] / radii[1].max(1e-3)).powi(2)).sqrt();
                let tangent = {
                    let a = libm::atan2f(local[1], local[0] * radii[1] / radii[0].max(1e-3));
                    let t = [-radii[0] * libm::sinf(a), radii[1] * libm::cosf(a)];
                    let (s2, c2) = (libm::sinf(turn.to_radians()), libm::cosf(turn.to_radians()));
                    [t[0] * c2 - t[1] * s2, t[0] * s2 + t[1] * c2]
                };
                Some((
                    fit(tangent) * 0.5,
                    Path::Oval {
                        center: *center,
                        radii: [radii[0] * k.max(1e-3), radii[1] * k.max(1e-3)],
                        turn: *turn,
                    },
                ))
            }
        }
    }
}

pub fn decide(assists: &[Assist], start: P, now: P, zoom: f32) -> Option<Path> {
    let heading = sub(now, start);
    if len(heading) * zoom.max(1e-3) < DECIDE {
        return None;
    }
    assists
        .iter()
        .filter_map(|a| a.choose(start, heading))
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, p)| p)
}

pub fn project(path: &Path, p: P) -> P {
    match path {
        Path::Line { at, dir } => {
            let t = dot(sub(p, *at), *dir);
            [at[0] + dir[0] * t, at[1] + dir[1] * t]
        }
        Path::Oval { center, radii, turn } => {
            let (s, c) = (libm::sinf(-turn.to_radians()), libm::cosf(-turn.to_radians()));
            let d = sub(p, *center);
            let local = [d[0] * c - d[1] * s, d[0] * s + d[1] * c];
            let a = libm::atan2f(local[1] / radii[1].max(1e-3), local[0] / radii[0].max(1e-3));
            let on = [radii[0] * libm::cosf(a), radii[1] * libm::sinf(a)];
            let (s2, c2) = (libm::sinf(turn.to_radians()), libm::cosf(turn.to_radians()));
            [center[0] + on[0] * c2 - on[1] * s2, center[1] + on[0] * s2 + on[1] * c2]
        }
    }
}

pub fn follow(assists: &[Assist], points: &[P]) -> Vec<P> {
    let Some(start) = points.first().copied() else {
        return Vec::new();
    };
    let path = points.iter().find_map(|p| decide(assists, start, *p, 1.0));
    match path {
        Some(path) => points.iter().map(|p| project(&path, *p)).collect(),
        None => points.to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strokes_follow_the_vanishing_point_they_head_for_a_ruler_and_an_ellipse() {
        let two = [Assist::Perspective {
            points: vec![[-500.0, 100.0], [900.0, 100.0]],
        }];
        let wobbly = [[200.0, 300.0], [190.0, 290.0], [170.0, 283.0], [140.0, 268.0]];
        let out = follow(&two, &wobbly);
        out.iter().for_each(|p| {
            let toward = [p[0] - 200.0, p[1] - 300.0];
            let line = [-700.0, -200.0];
            assert!(
                (toward[0] * line[1] - toward[1] * line[0]).abs() < 1e-1,
                "{p:?} is on the line to the left point"
            );
        });
        let up = [[200.0, 300.0], [201.0, 280.0], [199.0, 260.0]];
        let out = follow(&two, &up);
        assert!(
            out.iter().all(|p| (p[0] - 200.0).abs() < 1e-3),
            "two-point perspective keeps uprights upright: {out:?}"
        );
        let ruler = [Assist::Ruler {
            from: [0.0, 50.0],
            to: [100.0, 50.0],
        }];
        let near = follow(&ruler, &[[10.0, 55.0], [40.0, 58.0], [80.0, 44.0]]);
        assert!(
            near.iter().all(|p| (p[1] - 50.0).abs() < 1e-3),
            "near the ruler, on it: {near:?}"
        );
        let far = follow(&ruler, &[[10.0, 150.0], [40.0, 158.0], [80.0, 144.0]]);
        assert!(
            far.iter().all(|p| (p[1] - 150.0).abs() < 1e-3),
            "away from it, parallel: {far:?}"
        );
        let oval = [Assist::Ellipse {
            center: [0.0, 0.0],
            radii: [100.0, 50.0],
            turn: 0.0,
        }];
        let round = follow(&oval, &[[100.0, 0.0], [95.0, 20.0], [70.0, 38.0]]);
        round.iter().for_each(|p| {
            let k = (p[0] / 100.0).powi(2) + (p[1] / 50.0).powi(2);
            assert!((k - 1.0).abs() < 1e-3, "{p:?} is on the ellipse");
        });
        assert_eq!(
            Assist::Ruler {
                from: [0.0, 0.0],
                to: [1.0, 0.0]
            }
            .moved(1, [5.0, 5.0])
            .handles()[1],
            [5.0, 5.0]
        );
    }
}
