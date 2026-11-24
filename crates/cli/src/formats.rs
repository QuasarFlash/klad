use crate::layers;
use anyhow::{Result, anyhow, bail};
use klad_brush::BrushSet;
use klad_core::bundle::{Bundle, write_atomic};
use klad_core::stroke::Mark;
use klad_render::pdf::{Page, Paper, Pdf};
use std::path::{Path, PathBuf};

pub struct PdfArgs {
    pub frame: u32,
    pub pages: Option<String>,
    pub dpi: Option<f32>,
    pub paper: Option<String>,
    pub background: bool,
    pub references: bool,
    pub variant: Option<String>,
    pub out: Option<PathBuf>,
}

fn page_frames(doc: &klad_core::doc::Document, frame: u32, pages: Option<&str>) -> Result<Vec<u32>> {
    match pages.map(str::trim) {
        None => Ok(vec![frame]),
        Some("clips") => match doc.timeline.clips.is_empty() {
            true => bail!("the drawing has no clips: --pages all gives a page for each frame"),
            false => Ok(doc.timeline.clips.iter().map(|c| c.from).collect()),
        },
        Some(spec) => Ok(klad_render::sprites::animations(doc, spec)?
            .iter()
            .flat_map(|a| a.from..=a.to)
            .collect()),
    }
}

pub fn pdf(path: &Path, a: PdfArgs) -> Result<()> {
    let bundle = Bundle::open(path)?;
    let doc = &bundle.doc;
    let frames = page_frames(doc, a.frame, a.pages.as_deref())?;
    if let Some(f) = frames.iter().find(|f| **f >= doc.timeline.frames) {
        bail!("frame {f} is past the last frame {}", doc.timeline.frames - 1);
    }
    let brushes = BrushSet::for_bundle(&bundle)?;
    let dress = layers::dress_of(doc, a.variant.as_deref())?;
    let rasters = frames
        .iter()
        .map(|f| layers::dressed(&bundle, &brushes, layers::load_at(&bundle, *f, &brushes, 1.0)?, &dress))
        .collect::<Result<Vec<_>>>()?;
    let pages: Vec<Page> = frames
        .iter()
        .zip(&rasters)
        .map(|(frame, rasters)| Page { frame: *frame, rasters })
        .collect();
    let opts = Pdf {
        dpi: a.dpi.unwrap_or(72.0),
        paper: a.paper.as_deref().map(Paper::parse).transpose()?,
        background: a.background,
        references: a.references,
    };
    let bytes = klad_render::pdf::write(doc, &pages, &opts)?;
    let usual = crate::export::default_path(&bundle, None, a.frame, 1.0, a.variant.as_deref()).with_extension("pdf");
    let usual = match a.pages.is_some() {
        true => {
            let stem = bundle
                .root
                .file_stem()
                .map_or("export".into(), |s| s.to_string_lossy().to_string());
            usual.with_file_name(format!("{stem}{}.pdf", layers::variant_tag(a.variant.as_deref())))
        }
        false => usual,
    };
    let out = match a.out {
        Some(folder) if folder.is_dir() => folder.join(usual.file_name().unwrap_or_default()),
        Some(file) => file,
        None => usual,
    };
    write_atomic(&out, &bytes)?;
    println!("{}", out.display());
    let (w, h) = klad_render::pdf::page_size(doc, &opts);
    let vectors = doc
        .all_layers()
        .iter()
        .filter(|l| matches!(l.content, klad_core::doc::LayerContent::Vector { .. }))
        .count();
    eprintln!(
        "{} page{} of {w:.0}x{h:.0} pt, {} KB; {vectors} vector layer{} kept as shapes, the others as pictures",
        pages.len(),
        if pages.len() == 1 { "" } else { "s" },
        bytes.len() / 1024,
        if vectors == 1 { "" } else { "s" }
    );
    Ok(())
}

enum Pick {
    Node(String),
    Strokes(String, Vec<u32>),
}

fn picks(refs: &[String]) -> Result<Vec<Pick>> {
    let mut out: Vec<Pick> = Vec::new();
    refs.iter().try_for_each(|r| {
        match r.split_once(":s") {
            Some((cel, id)) => {
                let id: u32 = id.parse().map_err(|_| anyhow!("{r:?}: a stroke is <cel>:s<number>"))?;
                match out.iter_mut().find_map(|p| match p {
                    Pick::Strokes(c, ids) if c == cel => Some(ids),
                    _ => None,
                }) {
                    Some(ids) => ids.push(id),
                    None => out.push(Pick::Strokes(cel.to_string(), vec![id])),
                }
            }
            None => out.push(Pick::Node(r.trim().to_string())),
        }
        Ok::<_, anyhow::Error>(())
    })?;
    Ok(out)
}

fn layer_of<'a>(doc: &'a klad_core::doc::Document, cel: &str) -> Result<&'a klad_core::doc::Layer> {
    doc.all_layers()
        .into_iter()
        .find(|l| l.cels().iter().any(|c| c.id == cel))
        .ok_or_else(|| anyhow!("no cel {cel} in the drawing"))
}

pub fn only_image(bundle: &Bundle, frame: u32, refs: &[String], scale: f32) -> Result<klad_render::formats::Picture> {
    let doc = &bundle.doc;
    let brushes = BrushSet::for_bundle(bundle)?;
    let mut nodes = Vec::new();
    let mut rasters = Vec::new();
    picks(refs)?.iter().try_for_each(|pick| {
        match pick {
            Pick::Node(id) => match doc.item(id) {
                Some(klad_core::doc::Item::Node(_, _, node)) => nodes.push(node.clone()),
                _ => bail!("no shape {id} in the drawing"),
            },
            Pick::Strokes(cel, ids) => {
                let layer = layer_of(doc, cel)?;
                let marks: Vec<Mark> = bundle
                    .read_marks(&layer.id, cel)?
                    .into_iter()
                    .filter(|m| ids.contains(&m.id()))
                    .collect();
                if marks.len() != ids.len() {
                    bail!("cel {cel} has not all of the strokes {ids:?}");
                }
                rasters.push(klad_brush::replay(doc.canvas.width, doc.canvas.height, &marks, &brushes)?.raster);
            }
        }
        Ok(())
    })?;
    let nodes = klad_core::doc::nodes_at(&nodes, frame);
    klad_render::formats::alone(doc.canvas.width, doc.canvas.height, &nodes, &rasters, scale)
}
