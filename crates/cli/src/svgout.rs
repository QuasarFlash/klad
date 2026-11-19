use crate::layers;
use crate::sprites::{bundle_dir, bundle_stem, file_safe};
use anyhow::{Context, Result, bail};
use klad_brush::BrushSet;
use klad_core::bundle::{Bundle, write_atomic};
use klad_render::export::{Options, Target, png8, render};
use klad_render::svg::{Svg, write};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub struct Export {
    pub layer: Option<String>,
    pub frame: u32,
    pub frames: Option<String>,
    pub vectors_only: bool,
    pub outline_text: bool,
    pub only: Vec<String>,
    pub out: Option<PathBuf>,
}

fn one(bundle: &Bundle, brushes: &BrushSet, opts: &Export, frame: u32) -> Result<klad_render::svg::Written> {
    let doc = &bundle.doc;
    let layer = opts.layer.as_deref().map(|key| doc.find_layer(key)).transpose()?;
    let mut rasters = None;
    let mut picture = |l: &klad_core::doc::Layer| -> Result<Vec<u8>> {
        if rasters.is_none() {
            rasters = Some(layers::load_at(bundle, frame, brushes, 1.0)?);
        }
        let Some(loaded) = rasters.as_ref() else {
            bail!("the layers could not be read");
        };
        let image = render(
            doc,
            frame,
            loaded,
            &Target::Layer(l),
            Options {
                scale: 1.0,
                background: false,
                references: true,
                detail: 1.0,
            },
        )?;
        png8(&image)
    };
    let only = (!opts.only.is_empty()).then_some(opts.only.as_slice());
    write(
        doc,
        &bundle.root,
        &Svg {
            frame,
            layer,
            vectors_only: opts.vectors_only,
            outline_text: opts.outline_text,
            nodes: only,
        },
        &mut picture,
    )
}

fn told(written: &klad_render::svg::Written) {
    if !written.pictures.is_empty() {
        eprintln!(
            "note: {} went in as pictures (paint and pixel layers have no shapes); --vectors-only leaves them out",
            written.pictures.join(", ")
        );
    }
    if !written.left_out.is_empty() {
        eprintln!("note: left out: {}", written.left_out.join(", "));
    }
}

pub fn run(path: &Path, opts: &Export) -> Result<()> {
    let bundle = Bundle::open(path)?;
    let doc = &bundle.doc;
    let brushes = BrushSet::for_bundle(&bundle)?;
    if opts.layer.is_some() && !opts.only.is_empty() {
        bail!("give --layer or --only, not both");
    }
    let stem = bundle_stem(&bundle);
    let tag = opts
        .layer
        .as_deref()
        .map(|l| format!("-{}", file_safe(l)))
        .unwrap_or_default();
    match &opts.frames {
        None => {
            if opts.frame >= doc.timeline.frames {
                bail!(
                    "frame {} is past the last frame {}",
                    opts.frame,
                    doc.timeline.frames - 1
                );
            }
            let written = one(&bundle, &brushes, opts, opts.frame)?;
            let usual = bundle_dir(&bundle).join(format!("{stem}{tag}.svg"));
            let out = match &opts.out {
                Some(folder) if folder.is_dir() => folder.join(usual.file_name().unwrap_or_default()),
                Some(file) => file.clone(),
                None => usual,
            };
            write_atomic(&out, written.svg.as_bytes())?;
            told(&written);
            println!("{}", out.display());
        }
        Some(spec) => {
            let frames: BTreeSet<u32> = klad_render::sprites::animations(doc, spec)?
                .iter()
                .flat_map(|a| a.from..=a.to)
                .collect();
            let dir = opts
                .out
                .clone()
                .unwrap_or_else(|| bundle_dir(&bundle).join(format!("{stem}{tag}-svg")));
            std::fs::create_dir_all(&dir).with_context(|| format!("making {}", dir.display()))?;
            let mut last = None;
            frames.iter().try_for_each(|&frame| {
                let written = one(&bundle, &brushes, opts, frame)?;
                write_atomic(&dir.join(format!("{stem}-{frame:04}.svg")), written.svg.as_bytes())?;
                last = Some(written);
                Ok::<_, anyhow::Error>(())
            })?;
            if let Some(written) = &last {
                told(written);
            }
            println!("{}", dir.display());
        }
    }
    Ok(())
}
