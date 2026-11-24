use crate::layers;
use anyhow::{Context, Result, bail};
use klad_brush::BrushSet;
use klad_core::bundle::{Blank, Bundle, write_atomic};
use klad_core::doc::{Blend, Layer, LayerContent};
use klad_core::ops::Op;
use klad_render::export::{Options, Target, render};
use klad_render::psd::{self, Item, Look, Pixels, Psd};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub fn is_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("psd"))
}

fn cropped(frame: &klad_render::Frame) -> Pixels {
    let (w, h) = (frame.width as usize, frame.height as usize);
    let solid = |x: usize, y: usize| frame.rgba[(y * w + x) * 4 + 3] > 0;
    let rows: Vec<usize> = (0..h).filter(|y| (0..w).any(|x| solid(x, *y))).collect();
    let cols: Vec<usize> = (0..w).filter(|x| (0..h).any(|y| solid(*x, y))).collect();
    match (rows.first(), rows.last(), cols.first(), cols.last()) {
        (Some(&top), Some(&bottom), Some(&left), Some(&right)) => {
            let (cw, ch) = (right - left + 1, bottom - top + 1);
            let rgba = (top..=bottom)
                .flat_map(|y| frame.rgba[(y * w + left) * 4..(y * w + left + cw) * 4].to_vec())
                .collect();
            Pixels {
                left: left as i32,
                top: top as i32,
                width: cw as u32,
                height: ch as u32,
                rgba,
            }
        }
        _ => Pixels {
            left: 0,
            top: 0,
            width: 0,
            height: 0,
            rgba: Vec::new(),
        },
    }
}

fn look_of(layer: &Layer, notes: &mut BTreeSet<String>) -> Look {
    let name = match layer.name.is_empty() {
        true => layer.id.clone(),
        false => layer.name.clone(),
    };
    if !matches!(layer.content, LayerContent::Paint { .. } | LayerContent::Group { .. }) {
        notes.insert(match &layer.content {
            LayerContent::Vector { .. } => "vector layers went in as pixels".to_string(),
            _ => "other layers went in as pixels".to_string(),
        });
    }
    if layer.mask_at(0).is_some() {
        notes.insert("masks were applied to the pixels".into());
    }
    Look {
        name,
        visible: layer.visible,
        opacity: layer.opacity,
        blend: psd::blend_key(layer.blend.name()).to_string(),
        clip: layer.clip,
    }
}

struct Walk<'a> {
    doc: &'a klad_core::doc::Document,
    frame: u32,
    rasters: &'a klad_render::Rasters,
    references: bool,
    notes: BTreeSet<String>,
}

impl Walk<'_> {
    fn items(&mut self, layers: &[Layer]) -> Result<Vec<Item>> {
        let references = self.references;
        let kept: Vec<&Layer> = layers
            .iter()
            .filter(|l| references || !l.reference)
            .filter(|l| !matches!(l.content, LayerContent::Bones {}))
            .collect();
        if kept.iter().any(|l| matches!(l.content, LayerContent::Adjust { .. })) {
            self.notes.insert("adjustment layers were left out".into());
        }
        kept.into_iter()
            .filter(|l| !matches!(l.content, LayerContent::Adjust { .. }))
            .map(|l| match &l.content {
                LayerContent::Group { children } => Ok(Item::Group {
                    look: look_of(l, &mut self.notes),
                    open: true,
                    children: self.items(children)?,
                }),
                _ => {
                    let mut alone = l.clone();
                    alone.opacity = 1.0;
                    alone.clip = false;
                    let options = Options {
                        scale: 1.0,
                        background: false,
                        references: true,
                        detail: 1.0,
                    };
                    let image = render(self.doc, self.frame, self.rasters, &Target::Layer(&alone), options)?;
                    Ok(Item::Layer {
                        look: look_of(l, &mut self.notes),
                        pixels: cropped(&klad_render::read_rgba8(&image)?),
                    })
                }
            })
            .collect()
    }
}

pub fn export(path: &Path, frame: u32, references: bool, background: bool, out: Option<PathBuf>) -> Result<()> {
    let bundle = Bundle::open(path)?;
    let doc = &bundle.doc;
    if frame >= doc.timeline.frames {
        bail!("frame {frame} is past the last frame {}", doc.timeline.frames - 1);
    }
    let brushes = BrushSet::for_bundle(&bundle)?;
    let rasters = layers::load_at(&bundle, frame, &brushes, 1.0)?;
    let mut walk = Walk {
        doc,
        frame,
        rasters: &rasters,
        references,
        notes: BTreeSet::new(),
    };
    let mut items = walk.items(&doc.layers)?;
    if let Some(colour) = doc.canvas.background.as_deref().filter(|_| background) {
        let [r, g, b, _] = klad_core::color::parse_hex(colour)?;
        let px = [r, g, b].map(|c| (c * 255.0).round() as u8);
        let (w, h) = (doc.canvas.width, doc.canvas.height);
        items.insert(
            0,
            Item::Layer {
                look: Look::named("Background"),
                pixels: Pixels {
                    left: 0,
                    top: 0,
                    width: w,
                    height: h,
                    rgba: (0..w * h).flat_map(|_| [px[0], px[1], px[2], 255]).collect(),
                },
            },
        );
    }
    let options = Options {
        scale: 1.0,
        background,
        references,
        detail: 1.0,
    };
    let merged = klad_render::read_rgba8(&render(doc, frame, &rasters, &Target::Frame, options)?)?;
    let file = Psd {
        width: doc.canvas.width,
        height: doc.canvas.height,
        items,
        notes: Vec::new(),
    };
    let bytes = psd::write(&file, &merged.rgba)?;
    let usual = crate::export::default_path(&bundle, None, frame, 1.0, None).with_extension("psd");
    let out = match out {
        Some(folder) if folder.is_dir() => folder.join(usual.file_name().unwrap_or_default()),
        Some(file) => file,
        None => usual,
    };
    write_atomic(&out, &bytes)?;
    println!("{}", out.display());
    walk.notes.iter().for_each(|n| eprintln!("note: {n}"));
    eprintln!(
        "{}x{}  PSD with layers, {} KB",
        doc.canvas.width,
        doc.canvas.height,
        bytes.len() / 1024
    );
    Ok(())
}

fn blend_of(key: &str, notes: &mut BTreeSet<String>) -> &'static str {
    match psd::blend_name(key).filter(|n| *n == "pass-through" || Blend::ALL.iter().any(|b| b.name() == *n)) {
        Some("pass-through") => "normal",
        Some(name) => name,
        None => {
            notes.insert(format!(
                "blend modes klad does not have ({}) blend as normal",
                psd::blend_name(key).unwrap_or(key.trim())
            ));
            "normal"
        }
    }
}

struct Build<'a> {
    target: &'a Path,
    notes: BTreeSet<String>,
    images: usize,
}

impl Build<'_> {
    fn layers(&mut self, items: &[Item]) -> Result<Vec<Value>> {
        items
            .iter()
            .rev()
            .map(|item| match item {
                Item::Group { look, children, .. } => {
                    let children = self.layers(children)?;
                    let mut v = json!({
                        "kind": "group",
                        "name": look.name,
                        "visible": look.visible,
                        "opacity": look.opacity,
                        "children": children,
                    });
                    if look.blend != "pass" {
                        v["blend"] = json!(blend_of(&look.blend, &mut self.notes));
                    }
                    Ok(v)
                }
                Item::Layer { look, pixels } => {
                    let nodes = match pixels.width == 0 || pixels.height == 0 {
                        true => Vec::new(),
                        false => {
                            let picture = klad_render::pixelise::Picture {
                                width: pixels.width,
                                height: pixels.height,
                                rgba: pixels.rgba.clone(),
                            };
                            let png = klad_render::clipboard::png(&picture);
                            let (src, w, h) = klad_render::assets::import_image(self.target, &png)?;
                            self.images += 1;
                            vec![
                                json!({"type": "image", "src": src, "x": pixels.left, "y": pixels.top, "w": w, "h": h}),
                            ]
                        }
                    };
                    Ok(json!({
                        "kind": "vector",
                        "name": look.name,
                        "visible": look.visible,
                        "opacity": look.opacity,
                        "blend": blend_of(&look.blend, &mut self.notes),
                        "clip": look.clip,
                        "cels": [{"frame": 0, "nodes": nodes}],
                    }))
                }
            })
            .collect()
    }
}

pub fn import(file: &Path, out: Option<PathBuf>) -> Result<PathBuf> {
    let bytes = std::fs::read(file).with_context(|| format!("reading {}", file.display()))?;
    let read = psd::read(&bytes).with_context(|| file.display().to_string())?;
    let target = out.unwrap_or_else(|| file.with_extension("klad"));
    if target.exists() {
        bail!(
            "{} is there already: give the drawing another name with -o",
            target.display()
        );
    }
    let bundle = Bundle::blank(
        &target,
        &Blank {
            width: read.width,
            height: read.height,
            ..Blank::default()
        },
        "cli",
    )?;
    let mut build = Build {
        target: &target,
        notes: read.notes.iter().cloned().collect(),
        images: 0,
    };
    let layers = build.layers(&read.items)?;
    let ops: Vec<Op> = layers
        .into_iter()
        .enumerate()
        .map(|(i, layer)| Op::AddLayer {
            layer,
            place: klad_core::ops::Place {
                index: Some(i),
                ..Default::default()
            },
        })
        .collect();
    let name = file
        .file_name()
        .map_or(String::new(), |n| n.to_string_lossy().to_string());
    let bundle = crate::pixelise::finished(bundle, &ops, &target, &format!("bringing in {name}"))?;
    eprintln!(
        "{}x{}, {} layers with pictures, under a paint layer",
        bundle.doc.canvas.width, bundle.doc.canvas.height, build.images
    );
    build.notes.iter().for_each(|n| eprintln!("note: {n}"));
    Ok(target)
}
