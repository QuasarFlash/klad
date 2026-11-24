use anyhow::{Result, bail};
use klad_core::bundle::Bundle;
use klad_core::doc::Document;
use klad_core::markedit::MarkTransform;
use klad_core::ops::{Op, ShiftSpec};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Reshape {
    Size { width: u32, height: u32, anchor: [f32; 2] },
    Crop { x: f32, y: f32, width: u32, height: u32 },
    Scale { sx: f32, sy: f32 },
    Turn(i32),
    Flip { level: bool },
}

pub const ANCHORS: [(&str, [f32; 2]); 9] = [
    ("top-left", [0.0, 0.0]),
    ("top", [0.5, 0.0]),
    ("top-right", [1.0, 0.0]),
    ("left", [0.0, 0.5]),
    ("center", [0.5, 0.5]),
    ("right", [1.0, 0.5]),
    ("bottom-left", [0.0, 1.0]),
    ("bottom", [0.5, 1.0]),
    ("bottom-right", [1.0, 1.0]),
];

pub const MOST: u32 = 16384;

pub fn anchor(word: &str) -> Result<[f32; 2]> {
    let word = if word == "centre" { "center" } else { word };
    ANCHORS
        .iter()
        .find(|(w, _)| *w == word)
        .map(|(_, a)| *a)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "anchor {word:?}: use {}",
                ANCHORS.iter().map(|(w, _)| *w).collect::<Vec<_>>().join(", ")
            )
        })
}

pub fn plan(doc: &Document, r: &Reshape) -> Result<(MarkTransform, u32, u32)> {
    let planned = plan_size(doc, r)?;
    let fit = klad_core::memory::fit(planned.1, planned.2);
    if let (klad_core::memory::Fit::TooBig { .. }, Some(words)) = (fit, fit.words(planned.1, planned.2)) {
        anyhow::bail!(words);
    }
    Ok(planned)
}

fn plan_size(doc: &Document, r: &Reshape) -> Result<(MarkTransform, u32, u32)> {
    let (w, h) = (doc.canvas.width as f32, doc.canvas.height as f32);
    let pixel = doc.pixel_art();
    let whole = |v: f32| if pixel { v.round() } else { v };
    let mut t = MarkTransform {
        pivot: Some([0.0, 0.0]),
        ..MarkTransform::default()
    };
    let (nw, nh) = match *r {
        Reshape::Size { width, height, anchor } => {
            t.x = whole((width as f32 - w) * anchor[0]);
            t.y = whole((height as f32 - h) * anchor[1]);
            (width, height)
        }
        Reshape::Crop { x, y, width, height } => {
            t.x = whole(-x);
            t.y = whole(-y);
            (width, height)
        }
        Reshape::Scale { sx, sy } => {
            if sx <= 0.0 || sy <= 0.0 {
                bail!("scale by more than 0");
            }
            if pixel && (sx.fract() != 0.0 || sy.fract() != 0.0) {
                bail!("a pixel drawing scales by whole numbers only (2, 3, 4…), so every pixel stays a sharp square");
            }
            t.sx = sx;
            t.sy = sy;
            ((w * sx).round() as u32, (h * sy).round() as u32)
        }
        Reshape::Turn(degrees) => match degrees.rem_euclid(360) {
            90 => {
                t.rotate = 90.0;
                t.x = h;
                (h as u32, w as u32)
            }
            270 => {
                t.rotate = -90.0;
                t.y = w;
                (h as u32, w as u32)
            }
            180 => {
                t.rotate = 180.0;
                t.x = w;
                t.y = h;
                (w as u32, h as u32)
            }
            other => bail!("turn by 90, -90 or 180 degrees, not {other}"),
        },
        Reshape::Flip { level } => {
            match level {
                true => {
                    t.sx = -1.0;
                    t.x = w;
                }
                false => {
                    t.sy = -1.0;
                    t.y = h;
                }
            }
            (w as u32, h as u32)
        }
    };
    if nw == 0 || nh == 0 || nw > MOST || nh > MOST {
        bail!("the canvas would be {nw} by {nh} px: it takes 1 to {MOST} px a side");
    }
    if let Some([tw, th]) = doc.canvas.tile
        && (nw % tw != 0 || nh % th != 0)
    {
        bail!(
            "the drawing is cut into tiles of {tw} by {th} px, so its sides stay whole numbers of tiles: {nw} by {nh} is not (try {} by {})",
            (nw / tw).max(1) * tw,
            (nh / th).max(1) * th
        );
    }
    Ok((t, nw, nh))
}

fn point(t: &MarkTransform, p: [f32; 2]) -> [f32; 2] {
    let a = t.affine(t.pivot.unwrap_or([0.0, 0.0]));
    [a[0] * p[0] + a[1] * p[1] + a[4], a[2] * p[0] + a[3] * p[1] + a[5]]
}

pub fn ops(bundle: &Bundle, r: &Reshape) -> Result<Vec<Op>> {
    let doc = &bundle.doc;
    let (t, width, height) = plan(doc, r)?;
    let still = t.x == 0.0 && t.y == 0.0 && t.rotate == 0.0 && t.sx == 1.0 && t.sy == 1.0;
    let mut ops: Vec<Op> = match still {
        true => Vec::new(),
        false => doc
            .layers
            .iter()
            .filter(|l| !l.cels().is_empty() || !l.children().is_empty() || !l.bones.is_empty())
            .map(|l| {
                Op::Shift(ShiftSpec {
                    layer: l.id.clone(),
                    transform: t,
                    only: None,
                    resolved: None,
                })
            })
            .collect(),
    };
    crate::shift::resolve(bundle, &mut ops)?;
    ops.push(Op::Canvas {
        wrap: None,
        tile: None,
        size: Some([width, height]),
        background: None,
    });
    if let Some(origin) = doc.timeline.origin.filter(|_| !still) {
        ops.push(Op::Timeline {
            fps: None,
            frames: None,
            origin: Some(Some(point(&t, origin))),
        });
    }
    if let Some(mut maps) = doc.maps.clone().filter(|m| !still && !m.lights.is_empty()) {
        let k = (t.sx.abs() + t.sy.abs()) / 2.0;
        let a = t.affine([0.0, 0.0]);
        maps.lights.iter_mut().for_each(|light| {
            light.at = match light.kind {
                klad_core::maps::LightKind::Point => point(&t, light.at),
                klad_core::maps::LightKind::Directional => [
                    a[0] * light.at[0] + a[1] * light.at[1],
                    a[2] * light.at[0] + a[3] * light.at[1],
                ],
            };
            light.range *= k;
        });
        ops.push(Op::Maps(Some(maps)));
    }
    Ok(ops)
}

pub fn drawn(bundle: &Bundle) -> Result<Option<[f32; 4]>> {
    let doc = &bundle.doc;
    let mut boxes: Vec<[f32; 4]> = Vec::new();
    doc.all_layers()
        .into_iter()
        .filter(|l| l.visible && !l.reference)
        .try_for_each(|layer| {
            layer.cels().iter().filter(|c| c.link.is_none()).try_for_each(|cel| {
                match layer.kind() {
                    klad_core::doc::Kind::Paint => bundle.read_marks(&layer.id, &cel.id)?.iter().for_each(|m| {
                        let pad = m.as_stroke().map_or(0.0, |s| s.size / 2.0 + 1.0);
                        if let Some(b) = m.bounds() {
                            boxes.push([b[0] - pad, b[1] - pad, b[2] + pad, b[3] + pad]);
                        }
                    }),
                    klad_core::doc::Kind::Vector => {
                        if let Some(r) = crate::vector::nodes_bounds(&cel.nodes)? {
                            boxes.push([r.left, r.top, r.right, r.bottom]);
                        }
                    }
                    _ => {}
                }
                Ok::<_, anyhow::Error>(())
            })
        })?;
    let (w, h) = (doc.canvas.width as f32, doc.canvas.height as f32);
    Ok(boxes
        .into_iter()
        .reduce(|a, b| [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])])
        .map(|b| [b[0].max(0.0), b[1].max(0.0), b[2].min(w), b[3].min(h)])
        .filter(|b| b[2] > b[0] && b[3] > b[1]))
}
