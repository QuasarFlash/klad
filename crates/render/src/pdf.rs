use crate::{Ctx, Rasters, background, draw_layers};
use anyhow::{Result, bail};
use klad_core::doc::Document;
use skia_safe::{FilterMode, MipmapMode, Paint, Rect, SamplingOptions};

pub const PAPERS: &str = "a3, a4, a5, letter, legal or tabloid, with ,landscape";
const MARGIN: f32 = 36.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Paper {
    pub width: f32,
    pub height: f32,
}

impl Paper {
    pub fn parse(text: &str) -> Result<Self> {
        let mut parts = text.split(',').map(|p| p.trim().to_ascii_lowercase());
        let name = parts.next().unwrap_or_default();
        let (w, h) = match name.as_str() {
            "a3" => (841.89, 1190.55),
            "a4" => (595.28, 841.89),
            "a5" => (419.53, 595.28),
            "letter" => (612.0, 792.0),
            "legal" => (612.0, 1008.0),
            "tabloid" => (792.0, 1224.0),
            other => bail!("{other:?} is not a paper size: {PAPERS}"),
        };
        match parts.next().as_deref() {
            None | Some("portrait") => Ok(Paper { width: w, height: h }),
            Some("landscape") => Ok(Paper { width: h, height: w }),
            Some(other) => bail!("{other:?}: a paper is portrait or landscape"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pdf {
    pub dpi: f32,
    pub paper: Option<Paper>,
    pub background: bool,
    pub references: bool,
}

impl Default for Pdf {
    fn default() -> Self {
        Self {
            dpi: 72.0,
            paper: None,
            background: true,
            references: false,
        }
    }
}

pub struct Page<'a> {
    pub frame: u32,
    pub rasters: &'a Rasters,
}

pub fn page_size(doc: &Document, opts: &Pdf) -> (f32, f32) {
    match opts.paper {
        Some(p) => (p.width, p.height),
        None => {
            let k = 72.0 / opts.dpi.max(1.0);
            (doc.canvas.width as f32 * k, doc.canvas.height as f32 * k)
        }
    }
}

fn placement(doc: &Document, opts: &Pdf) -> (f32, f32, f32) {
    let (w, h) = (doc.canvas.width as f32, doc.canvas.height as f32);
    match opts.paper {
        None => (0.0, 0.0, 72.0 / opts.dpi.max(1.0)),
        Some(p) => {
            let k = ((p.width - 2.0 * MARGIN) / w).min((p.height - 2.0 * MARGIN) / h);
            ((p.width - w * k) / 2.0, (p.height - h * k) / 2.0, k)
        }
    }
}

pub fn write(doc: &Document, pages: &[Page], opts: &Pdf) -> Result<Vec<u8>> {
    if !(1.0..=2400.0).contains(&opts.dpi) {
        bail!("--dpi takes 1 to 2400, not {}", opts.dpi);
    }
    let mut out = Vec::new();
    let metadata = skia_safe::pdf::Metadata {
        creator: "klad".into(),
        producer: "klad".into(),
        raster_dpi: Some(opts.dpi.max(72.0)),
        ..Default::default()
    };
    let mut document = skia_safe::pdf::new_document(&mut out, Some(&metadata));
    let size = page_size(doc, opts);
    let (x, y, k) = placement(doc, opts);
    let sampling = match doc.pixel_art() {
        true => Some(SamplingOptions::new(FilterMode::Nearest, MipmapMode::None)),
        false => Some(SamplingOptions::new(FilterMode::Linear, MipmapMode::Linear)),
    };
    for page in pages {
        let mut on = document.begin_page(size, None);
        let canvas = on.canvas();
        canvas.translate((x, y));
        canvas.scale((k, k));
        let bounds = Rect::from_wh(doc.canvas.width as f32, doc.canvas.height as f32);
        canvas.clip_rect(bounds, None, true);
        if opts.background && doc.canvas.background.is_some() {
            let fill = background(doc.canvas.background.as_deref())?;
            canvas.draw_rect(bounds, &Paint::new(fill, None));
        }
        let ctx = Ctx {
            sampling,
            doc: Some(doc),
            references: opts.references,
            ..Ctx::full(page.frame, page.rasters)
        };
        draw_layers(canvas, &doc.layers, ctx)?;
        document = on.end_page();
    }
    document.close();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use klad_core::doc::{Layer, Node, Shape};

    pub(crate) fn streams(pdf: &[u8]) -> Vec<Vec<u8>> {
        let text = pdf;
        let mut found = Vec::new();
        let mut at = 0;
        while let Some(start) = text[at..].windows(6).position(|w| w == b"stream").map(|p| p + at) {
            let body = start + 6 + if text.get(start + 6) == Some(&b'\r') { 2 } else { 1 };
            let Some(end) = text[body..]
                .windows(9)
                .position(|w| w == b"endstream")
                .map(|p| p + body)
            else {
                break;
            };
            let raw = &text[body..end];
            let mut inflated = Vec::new();
            let mut z = flate2::read::ZlibDecoder::new(raw);
            match std::io::Read::read_to_end(&mut z, &mut inflated) {
                Ok(_) => found.push(inflated),
                Err(_) => found.push(raw.to_vec()),
            }
            at = end + 9;
        }
        found
    }

    #[test]
    fn vector_layers_stay_paths_and_pages_take_a_paper() {
        let mut doc = Document::new(200, 100);
        let square = Node::new(
            "n1",
            Shape::Rect {
                x: 10.0,
                y: 10.0,
                w: 80.0,
                h: 40.0,
                radius: 0.0,
            },
        )
        .filled("#c0392b");
        doc.layers.push(Layer::vector("v", "v", "c1", vec![square]));
        let rasters = Rasters::new();
        let pdf = write(
            &doc,
            &[Page {
                frame: 0,
                rasters: &rasters,
            }],
            &Pdf::default(),
        )
        .unwrap();
        assert_eq!(&pdf[..5], b"%PDF-");
        let text = String::from_utf8_lossy(&pdf);
        assert!(
            text.contains("/MediaBox [0 0 200 100]"),
            "a page of the canvas at 72 dpi"
        );
        assert!(
            !text.contains("/Subtype /Image"),
            "no pictures: the rectangle is a path"
        );
        let content = streams(&pdf)
            .into_iter()
            .map(|s| String::from_utf8_lossy(&s).to_string())
            .collect::<String>();
        assert!(
            content.contains(" re") || content.contains(" l"),
            "the rectangle is drawn as a path: {content}"
        );
        let a4 = Pdf {
            paper: Some(Paper::parse("a4,landscape").unwrap()),
            ..Pdf::default()
        };
        let two = write(
            &doc,
            &[
                Page {
                    frame: 0,
                    rasters: &rasters,
                },
                Page {
                    frame: 0,
                    rasters: &rasters,
                },
            ],
            &a4,
        )
        .unwrap();
        let text = String::from_utf8_lossy(&two);
        assert!(text.contains("/Count 2"), "two pages");
        assert!(text.contains("/MediaBox [0 0 842 595]"), "an A4 page on its side");
        assert!(Paper::parse("b9").is_err());
    }
}
