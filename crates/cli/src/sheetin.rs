use anyhow::{Context, Result, anyhow, bail};
use klad_core::bundle::{Blank, Bundle};
use klad_core::ops::{Op, parse_lines};
use klad_render::pixelise::Picture;
use serde_json::json;
use std::path::{Path, PathBuf};

pub use klad_render::sheetcut::{Cut, cells, size_of};

fn read(file: &Path) -> Result<Picture> {
    let bytes = std::fs::read(file).with_context(|| format!("reading {}", file.display()))?;
    klad_render::pixelise::decode(&bytes).with_context(|| file.display().to_string())
}

fn numbered(a: &Path, b: &Path) -> std::cmp::Ordering {
    let key = |p: &Path| {
        let name = p.file_stem().map_or(String::new(), |s| s.to_string_lossy().to_string());
        let digits: String = name
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        (
            name[..name.len() - digits.len()].to_string(),
            digits.parse::<u64>().unwrap_or(0),
            name,
        )
    };
    key(a).cmp(&key(b))
}

pub fn files_of(given: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut files: Vec<PathBuf> = given
        .iter()
        .map(|g| match g.is_dir() {
            true => Ok(std::fs::read_dir(g)?
                .flatten()
                .map(|e| e.path())
                .filter(|p| crate::pixelise::is_picture(p))
                .collect::<Vec<_>>()),
            false => Ok(vec![g.clone()]),
        })
        .collect::<Result<Vec<Vec<PathBuf>>>>()?
        .concat();
    files.sort_by(|a, b| numbered(a, b));
    if files.is_empty() {
        bail!("no pictures to bring in");
    }
    Ok(files)
}

fn name_of(path: &Path) -> String {
    let stem = path
        .file_stem()
        .map_or("frames".into(), |s| s.to_string_lossy().to_string());
    let trimmed = stem
        .trim_end_matches(|c: char| c.is_ascii_digit())
        .trim_end_matches(['-', '_', ' ', '.']);
    match trimmed.is_empty() {
        true => stem,
        false => trimmed.to_string(),
    }
}

pub struct Frames {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub pictures: Vec<Picture>,
}

pub fn gather(given: &[PathBuf], cut: Option<&Cut>) -> Result<Frames> {
    let files = files_of(given)?;
    match (cut, files.len()) {
        (Some(cut), 1) => {
            let (w, h, pictures) = cells(&read(&files[0])?, cut)?;
            if pictures.is_empty() {
                bail!("every cell of {} is empty", files[0].display());
            }
            Ok(Frames {
                name: name_of(&files[0]),
                width: w,
                height: h,
                pictures,
            })
        }
        (Some(_), _) => bail!("--cell and --grid cut one sheet: give one picture"),
        (None, _) => {
            let pictures = files.iter().map(|f| read(f)).collect::<Result<Vec<_>>>()?;
            let (w, h) = pictures
                .iter()
                .fold((0, 0), |(w, h), p| (w.max(p.width), h.max(p.height)));
            Ok(Frames {
                name: name_of(&files[0]),
                width: w,
                height: h,
                pictures,
            })
        }
    }
}

fn layer_ops(bundle: &Bundle, frames: &Frames, notes: &mut Vec<String>) -> Result<Vec<Op>> {
    let doc = &bundle.doc;
    let id = format!("l{}{}", doc.next_id + 1, doc.here.tag);
    let pixel = doc.pixel_art();
    let n = frames.pictures.len() as u32;
    let cels: Vec<serde_json::Value> = match pixel {
        true => (0..n).map(|f| json!({"frame": f})).collect(),
        false => frames
            .pictures
            .iter()
            .enumerate()
            .map(|(f, p)| {
                let png = klad_render::clipboard::png(p);
                let (src, w, h) = klad_render::assets::import_image(&bundle.root, &png)?;
                Ok(json!({"frame": f, "nodes": [{"type": "image", "src": src, "x": 0, "y": 0, "w": w, "h": h}]}))
            })
            .collect::<Result<_>>()?,
    };
    let layer = json!({
        "id": id,
        "kind": if pixel { "pixel" } else { "vector" },
        "name": frames.name,
        "cels": cels,
    });
    let mut lines = vec![json!({"op": "add", "layer": layer, "index": 0}).to_string()];
    if doc.timeline.frames < n {
        lines.push(json!({"op": "timeline", "frames": n}).to_string());
    }
    let clip = match doc.timeline.clips.iter().any(|c| c.name == frames.name) {
        true => format!("{} {}", frames.name, doc.timeline.clips.len() + 1),
        false => frames.name.clone(),
    };
    lines.push(json!({"op": "clip", "add": clip, "from": 0, "to": n - 1, "loop": true}).to_string());
    let mut ops = parse_lines(&lines.join("\n"))?;
    if pixel {
        let mut said = Vec::new();
        let pictures: Vec<&Picture> = frames.pictures.iter().collect();
        let keys = crate::pixelise::keys_for(&doc.palette, &pictures, &mut said);
        notes.extend(said);
        frames.pictures.iter().enumerate().for_each(|(f, p)| {
            ops.extend(crate::pixelise::marks_of(
                &id,
                f as u32,
                p,
                &keys,
                (doc.canvas.width, doc.canvas.height),
            ));
        });
    }
    if frames.width > doc.canvas.width || frames.height > doc.canvas.height {
        notes.push(format!(
            "the frames are {}x{} and the canvas {}x{}: they hang over its right and bottom",
            frames.width, frames.height, doc.canvas.width, doc.canvas.height
        ));
    }
    notes.insert(0, format!("{n} frames on a new layer {}, clip {clip}", frames.name));
    Ok(ops)
}

pub fn into(given: &[PathBuf], cut: Option<&Cut>, to: &Path) -> Result<Vec<String>> {
    let frames = gather(given, cut)?;
    let mut bundle = Bundle::open(to)?;
    let mut notes = Vec::new();
    let ops = layer_ops(&bundle, &frames, &mut notes)?;
    bundle
        .apply(&ops)
        .with_context(|| format!("bringing frames into {}", to.display()))?;
    bundle.stamp("cli")?;
    Ok(notes)
}

pub fn new_drawing(given: &[PathBuf], cut: Option<&Cut>, out: Option<PathBuf>) -> Result<(PathBuf, Vec<String>)> {
    let frames = gather(given, cut)?;
    let first = files_of(given)?.remove(0);
    let target = out.unwrap_or_else(|| match given {
        [folder] if folder.is_dir() => folder.with_extension("klad"),
        _ => first.with_file_name(format!("{}.klad", frames.name)),
    });
    if target.exists() {
        bail!(
            "{} is there already: give the drawing another name with -o, or bring the frames into it with --to",
            target.display()
        );
    }
    let pixel = frames.width.max(frames.height) <= 512;
    let bundle = Bundle::blank(
        &target,
        &Blank {
            width: frames.width,
            height: frames.height,
            layers: 0,
            pixel,
            ..Blank::default()
        },
        "cli",
    )?;
    let mut notes = Vec::new();
    let ops = layer_ops(&bundle, &frames, &mut notes)?;
    crate::pixelise::finished(bundle, &ops, &target, "bringing in the frames")?;
    Ok((target, notes))
}

pub fn parse_cut(cell: Option<&str>, grid: Option<&str>, padding: u32, margin: u32) -> Result<Option<Cut>> {
    match (cell, grid) {
        (None, None) if padding == 0 && margin == 0 => Ok(None),
        (None, None) => Err(anyhow!("--padding and --margin go with --cell or --grid")),
        (Some(_), Some(_)) => Err(anyhow!("give --cell or --grid, not both")),
        (cell, grid) => Ok(Some(Cut {
            cell: cell.map(|c| size_of(c, "--cell")).transpose()?,
            grid: grid.map(|g| size_of(g, "--grid")).transpose()?,
            padding,
            margin,
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sheet_is_cut_by_cells_or_a_grid_with_padding_and_margin_and_empty_cells_are_skipped() {
        let (w, h) = (2 + 3 * 8 + 2 + 2, 2 + 2 * 6 + 1 + 2);
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        [(0, 0), (1, 0), (2, 0), (0, 1)].iter().for_each(|(c, r)| {
            let (x0, y0) = (2 + c * 9, 2 + r * 7);
            (y0..y0 + 6).for_each(|y| {
                (x0..x0 + 8).for_each(|x| {
                    let i = ((y * w + x) * 4) as usize;
                    rgba[i..i + 4].copy_from_slice(&[(c * 60) as u8, (r * 90) as u8, 5, 255]);
                })
            })
        });
        let sheet = Picture {
            width: w,
            height: h,
            rgba,
        };
        let by_cell = Cut {
            cell: Some((8, 6)),
            padding: 1,
            margin: 2,
            ..Cut::default()
        };
        let (cw, ch, pictures) = cells(&sheet, &by_cell).unwrap();
        assert_eq!((cw, ch, pictures.len()), (8, 6, 4), "the empty cell is skipped");
        assert_eq!(&pictures[1].rgba[..4], &[60, 0, 5, 255]);
        let by_grid = Cut {
            grid: Some((3, 2)),
            padding: 1,
            margin: 2,
            ..Cut::default()
        };
        let (gw, gh, again) = cells(&sheet, &by_grid).unwrap();
        assert_eq!((gw, gh), (8, 6));
        assert_eq!(again, pictures);
        assert!(
            cells(
                &sheet,
                &Cut {
                    cell: Some((100, 100)),
                    ..Cut::default()
                }
            )
            .is_err()
        );
        let mut names = vec![
            PathBuf::from("run-10.png"),
            PathBuf::from("run-2.png"),
            PathBuf::from("run-1.png"),
        ];
        names.sort_by(|a, b| numbered(a, b));
        assert_eq!(
            names,
            [
                PathBuf::from("run-1.png"),
                PathBuf::from("run-2.png"),
                PathBuf::from("run-10.png")
            ]
        );
        assert_eq!(name_of(Path::new("x/run-0001.png")), "run");
        assert_eq!(size_of("32x16", "--cell").unwrap(), (32, 16));
        assert!(size_of("0x16", "--cell").is_err());
    }
}
