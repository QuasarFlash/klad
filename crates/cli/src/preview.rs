use crate::layers;
use anyhow::{Context, Result};
use klad_brush::BrushSet;
use klad_core::bundle::{Bundle, write_atomic};
use klad_core::here;
use klad_core::preview::{self, EXPLORER, FOLDER, PREVIEW, PREVIEW_SIDE, THUMBNAIL, THUMBNAIL_SIDE};
use klad_render::formats::{self, Quality};
use klad_render::{Frame, View, compose_view};
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
    let render = |side: f32| {
        let view = View {
            scale: scale(&bundle, side),
            region: None,
            references: false,
        };
        compose_view(&bundle.doc, frame, &loaded.rasters, view, None, None)
    };
    let written = (|| -> Result<()> {
        write_atomic(&folder.join(PREVIEW), &render(PREVIEW_SIDE)?.png()?)?;
        let mut thumbnail = render(THUMBNAIL_SIDE)?;
        write_atomic(&folder.join(THUMBNAIL), &thumbnail.png()?)?;
        if cfg!(windows) {
            folder_picture(&bundle.root, &thumbnail)?;
        }
        Ok(())
    })();
    written.with_context(|| format!("writing the preview of {}", bundle.root.display()))?;
    preview::made(&bundle.root, fingerprint)?;
    Ok(Done::Written(folder.join(PREVIEW)))
}

pub fn folder_picture(root: &Path, frame: &Frame) -> Result<()> {
    let quality = Quality {
        quality: 88.0,
        matte: [255, 255, 255],
        ..Quality::default()
    };
    let jpeg = formats::jpeg(frame.width, frame.height, &frame.rgba, quality)?;
    let path = root.join(EXPLORER);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(windows)]
    std::os::windows::fs::OpenOptionsExt::attributes(&mut options, 0x2);
    let mut file = options
        .open(&path)
        .with_context(|| format!("writing {}", path.display()))?;
    std::io::Write::write_all(&mut file, &jpeg)?;
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explorer_gets_the_picture_as_the_folders_jpeg() {
        let dir = std::env::temp_dir().join(format!("klad-folder-picture-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let frame = Frame {
            width: 4,
            height: 2,
            rgba: [[200, 80, 40, 255], [0, 0, 0, 0]].repeat(4).concat(),
        };
        folder_picture(&dir, &frame).unwrap();
        folder_picture(&dir, &frame).unwrap();
        let jpeg = std::fs::read(dir.join(EXPLORER)).unwrap();
        assert_eq!(
            &jpeg[..3],
            &[0xFF, 0xD8, 0xFF],
            "a JPEG, written again over the last one"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
