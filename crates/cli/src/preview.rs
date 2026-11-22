use crate::layers;
use anyhow::{Context, Result};
use klad_brush::BrushSet;
use klad_core::bundle::{Bundle, write_atomic};
use klad_core::here;
use klad_core::preview::{self, FOLDER, PREVIEW, PREVIEW_SIDE, THUMBNAIL, THUMBNAIL_SIDE};
use klad_render::{View, compose_view};
use std::path::{Path, PathBuf};

pub enum Done {
    Written(PathBuf),
    Unchanged(PathBuf),
}

fn scale(bundle: &Bundle, side: f32) -> f32 {
    let longest = bundle.doc.canvas.width.max(bundle.doc.canvas.height).max(1) as f32;
    match bundle.doc.pixel_art() && longest <= side {
        true => (side / longest).floor().clamp(1.0, 64.0),
        false => (side / longest).min(1.0),
    }
}

pub fn write(path: &Path, only_changed: bool) -> Result<Done> {
    if only_changed && !preview::stale(path) {
        return Ok(Done::Unchanged(path.to_path_buf()));
    }
    let bundle = Bundle::open(path)?;
    let fingerprint = here::fingerprint(&bundle.root);
    let frame = preview::frame(&bundle.doc);
    let loaded = layers::load(&bundle, frame, &BrushSet::for_bundle(&bundle)?, true)?;
    let folder = bundle.root.join(FOLDER);
    [(PREVIEW, PREVIEW_SIDE), (THUMBNAIL, THUMBNAIL_SIDE)]
        .into_iter()
        .try_for_each(|(name, side)| {
            let view = View {
                scale: scale(&bundle, side),
                region: None,
                references: false,
            };
            let mut image = compose_view(&bundle.doc, frame, &loaded.rasters, view, None, None)?;
            write_atomic(&folder.join(name), &image.png()?)
        })
        .with_context(|| format!("writing the preview of {}", bundle.root.display()))?;
    preview::made(&bundle.root, fingerprint)?;
    Ok(Done::Written(folder.join(PREVIEW)))
}

pub fn drawings_in(folder: &Path) -> Result<Vec<PathBuf>> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(folder)
        .with_context(|| format!("reading {}", folder.display()))?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.join("doc.json").is_file())
        .collect();
    found.sort();
    Ok(found)
}
