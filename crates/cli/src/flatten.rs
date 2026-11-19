use crate::layers;
use anyhow::{Result, bail};
use klad_brush::BrushSet;
use klad_core::bundle::Bundle;
use klad_core::doc::{Kind, Layer};
use klad_core::ops::{Op, Place};
use klad_render::export::{Options, Target, png8, render};
use serde_json::json;
use std::collections::BTreeSet;

fn leaves<'a>(layers: &'a [Layer], out: &mut Vec<&'a Layer>) {
    layers.iter().filter(|l| l.visible).for_each(|l| match l.kind() {
        Kind::Group => leaves(l.children(), out),
        _ => out.push(l),
    });
}

pub fn visible(bundle: &Bundle) -> Vec<String> {
    let mut out = Vec::new();
    leaves(&bundle.doc.layers, &mut out);
    out.into_iter()
        .filter(|l| !l.reference && matches!(l.kind(), Kind::Paint | Kind::Vector | Kind::Adjust))
        .map(|l| l.id.clone())
        .collect()
}

pub fn picture(bundle: &Bundle, brushes: &BrushSet, ids: &[String]) -> Result<Vec<Op>> {
    let doc = &bundle.doc;
    if ids.is_empty() {
        bail!("no visible layers to merge");
    }
    let mut shown = doc.clone();
    let keep: BTreeSet<&str> = ids.iter().map(String::as_str).collect();
    fn hide(layers: &mut [Layer], keep: &BTreeSet<&str>) {
        layers.iter_mut().for_each(|l| {
            let keeps = keep.contains(l.id.as_str());
            match &mut l.content {
                klad_core::doc::LayerContent::Group { children } => hide(children, keep),
                _ => l.visible = l.visible && keeps,
            }
        });
    }
    hide(&mut shown.layers, &keep);
    let frames = doc.timeline.frames;
    let starts: BTreeSet<u32> = doc
        .all_layers()
        .iter()
        .filter(|l| keep.contains(l.id.as_str()))
        .flat_map(|l| l.cels().iter().map(|c| c.frame))
        .filter(|f| *f < frames)
        .chain(std::iter::once(0))
        .collect();
    let (w, h) = (doc.canvas.width as f32, doc.canvas.height as f32);
    let cels = starts
        .iter()
        .map(|&frame| {
            let rasters = layers::load_at(bundle, frame, brushes, 1.0)?;
            let image = render(
                &shown,
                frame,
                &rasters,
                &Target::Frame,
                Options {
                    scale: 1.0,
                    background: false,
                    references: false,
                    detail: 1.0,
                },
            )?;
            let (src, _, _) = klad_render::assets::import_image(&bundle.root, &png8(&image)?)?;
            Ok(json!({"frame": frame, "nodes": [{"type": "image", "src": src, "x": 0, "y": 0, "w": w, "h": h}]}))
        })
        .collect::<Result<Vec<_>>>()?;
    let top = doc.layers.last().map(|l| l.id.clone());
    let place = match top {
        Some(id) => Place {
            above: Some(id),
            ..Place::default()
        },
        None => Place::default(),
    };
    let mut gone: Vec<String> = ids.to_vec();
    let mut groups: Vec<String> = Vec::new();
    doc.all_layers()
        .iter()
        .filter(|g| g.kind() == Kind::Group)
        .filter(|g| {
            let mut inside = Vec::new();
            fn all<'a>(layers: &'a [Layer], out: &mut Vec<&'a Layer>) {
                layers.iter().for_each(|l| {
                    out.push(l);
                    all(l.children(), out);
                });
            }
            all(g.children(), &mut inside);
            inside
                .iter()
                .filter(|l| l.kind() != Kind::Group)
                .all(|l| keep.contains(l.id.as_str()))
        })
        .for_each(|g| groups.push(g.id.clone()));
    gone.extend(groups.into_iter().rev());
    Ok(vec![
        Op::AddLayer {
            layer: json!({"kind": "vector", "name": "merged", "cels": cels}),
            place,
        },
        Op::Remove { ids: gone },
    ])
}
