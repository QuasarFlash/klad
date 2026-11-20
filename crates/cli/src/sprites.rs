use crate::export::scale_tag;
use crate::layers::load_cels_at;
use crate::sheet::{batches, frame_cels, shown};
use anyhow::{Context, Result, anyhow, bail};
use klad_brush::BrushSet;
use klad_core::bundle::{Bundle, write_atomic};
use klad_core::ops::Op;
use klad_render::export::{Options, Target, detail_of, render};
use klad_render::maps::Map;
use klad_render::shaders::Effect;
use klad_render::sprites::{self, Animation, Pixels, Placed, Rect, Run, Scene, Sheet, SpriteFrames, Texture};
use rayon::prelude::*;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::Instant;

pub struct Look {
    pub layer: Option<String>,
    pub scale: f32,
    pub depth: u8,
    pub background: bool,
    pub variant: Option<String>,
    pub maps: Vec<Map>,
    pub invert: bool,
}

impl Look {
    pub fn plain(scale: f32, depth: u8) -> Self {
        Self {
            layer: None,
            scale,
            depth,
            background: false,
            variant: None,
            maps: Vec::new(),
            invert: false,
        }
    }
}

pub fn tagged(path: &Path, tag: &str) -> PathBuf {
    let stem = path
        .file_stem()
        .map_or(String::new(), |s| s.to_string_lossy().to_string());
    let name = match path.extension() {
        Some(ext) => format!("{stem}{tag}.{}", ext.to_string_lossy()),
        None => format!("{stem}{tag}"),
    };
    path.with_file_name(name)
}

fn stem_for(bundle: &Bundle, look: &Look) -> String {
    format!(
        "{}{}",
        bundle_stem(bundle),
        crate::layers::variant_tag(look.variant.as_deref())
    )
}

pub struct Packing {
    pub padding: u32,
    pub max: u32,
}

pub struct Pictures {
    pub source: (u32, u32),
    pub trims: Vec<Rect>,
    pub cut: Vec<Pixels>,
    pub maps: Vec<(Map, Vec<Pixels>)>,
    pub animations: Vec<(Animation, Vec<Run>)>,
    pub firsts: Vec<u32>,
    pub frames: usize,
    pub composed: usize,
    pub ms: f64,
}

struct Name {
    dir: PathBuf,
    stem: String,
}

impl Name {
    fn file(&self, suffix: &str) -> PathBuf {
        self.dir.join(format!("{}{suffix}", self.stem))
    }

    fn page(&self, i: usize) -> PathBuf {
        match i {
            0 => self.file(".png"),
            _ => self.file(&format!("-{}.png", i + 1)),
        }
    }

    fn map_page(&self, i: usize, map: Map) -> PathBuf {
        tagged(&self.page(i), map.suffix())
    }
}

pub(crate) fn bundle_stem(bundle: &Bundle) -> String {
    let root = bundle.root.components().collect::<PathBuf>();
    root.file_stem()
        .map_or("export".into(), |s| s.to_string_lossy().to_string())
}

pub(crate) fn bundle_dir(bundle: &Bundle) -> PathBuf {
    let root = bundle.root.components().collect::<PathBuf>();
    match root.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

pub(crate) fn file_safe(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => c,
            _ => '_',
        })
        .collect()
}

pub fn pictures(bundle: &Bundle, animations: Vec<Animation>, look: &Look) -> Result<Pictures> {
    let start = Instant::now();
    let doc = &bundle.doc;
    let layer = look.layer.as_deref().map(|k| doc.find_layer(k)).transpose()?;
    if layer.is_some() && !look.maps.is_empty() {
        bail!("light maps are made for the whole picture: leave out --layer, or export the layer without --maps");
    }
    let target = layer.map_or(Target::Frame, Target::Layer);
    let options = Options {
        scale: look.scale,
        background: look.background,
        references: false,
        detail: detail_of(doc, look.scale),
    };
    let dress = crate::layers::dress_of(doc, look.variant.as_deref())?;
    let frames: BTreeSet<u32> = animations.iter().flat_map(|a| a.from..=a.to).collect();
    let mut firsts: BTreeMap<Vec<String>, u32> = BTreeMap::new();
    let first_of: BTreeMap<u32, u32> = frames
        .iter()
        .map(|&f| (f, *firsts.entry(shown(doc, f)).or_insert(f)))
        .collect();
    let unique: Vec<u32> = firsts.values().copied().collect::<BTreeSet<_>>().into_iter().collect();
    let brushes = BrushSet::for_bundle(bundle)?;
    let (w, h) = (doc.canvas.width, doc.canvas.height);
    let work: Vec<_> = unique.iter().map(|&f| (f, frame_cels(doc, f, &[]))).collect();
    let mut made: BTreeMap<u32, (Rect, Pixels, Vec<Pixels>)> = BTreeMap::new();
    batches(work, w as usize * h as usize * 8)
        .into_iter()
        .try_for_each(|batch| {
            let needed: Vec<_> = batch
                .iter()
                .flat_map(|(_, c)| c.iter().cloned())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            let rasters = crate::layers::dressed(
                bundle,
                &brushes,
                load_cels_at(bundle, &brushes, &needed, options.detail)?,
                &dress,
            )?;
            let done = batch
                .par_iter()
                .map(|(frame, _)| {
                    let image = render(doc, *frame, &rasters, &target, options)?;
                    let pixels = Pixels::of(&image, look.depth)?;
                    let trim = pixels.trim();
                    let maps = match look.maps.is_empty() {
                        true => Vec::new(),
                        false => {
                            let mut set = klad_render::maps::render(doc, *frame, &rasters, look.scale)?;
                            if look.invert {
                                set.invert_depth();
                            }
                            look.maps.iter().map(|m| Pixels::of_map(&set, *m).crop(trim)).collect()
                        }
                    };
                    Ok((*frame, (trim, pixels.crop(trim), maps)))
                })
                .collect::<Result<Vec<_>>>()?;
            made.extend(done);
            Ok::<_, anyhow::Error>(())
        })?;
    let mut by_hash: BTreeMap<(u64, [u32; 4]), usize> = BTreeMap::new();
    let mut trims = Vec::new();
    let mut cut = Vec::new();
    let mut map_cuts: Vec<Vec<Pixels>> = look.maps.iter().map(|_| Vec::new()).collect();
    let mut firsts = Vec::new();
    let index_of: BTreeMap<u32, usize> = made
        .into_iter()
        .map(|(frame, (trim, pixels, maps))| {
            let hash = maps
                .iter()
                .fold(pixels.hash(), |h, m| klad_core::rng::fnv(h, &m.hash().to_le_bytes()));
            let key = (hash, [trim.x, trim.y, trim.w, trim.h]);
            let index = *by_hash.entry(key).or_insert_with(|| {
                trims.push(trim);
                cut.push(pixels);
                map_cuts.iter_mut().zip(maps).for_each(|(list, m)| list.push(m));
                firsts.push(frame);
                trims.len() - 1
            });
            (frame, index)
        })
        .collect();
    let picture = |f: u32| index_of[&first_of[&f]];
    let animations = animations
        .into_iter()
        .map(|a| {
            let runs = sprites::runs(&a, picture);
            (a, runs)
        })
        .collect();
    let source = (
        ((w as f32 * look.scale).round() as u32).max(1),
        ((h as f32 * look.scale).round() as u32).max(1),
    );
    Ok(Pictures {
        source,
        trims,
        cut,
        maps: look.maps.iter().copied().zip(map_cuts).collect(),
        animations,
        firsts,
        frames: frames.len(),
        composed: unique.len(),
        ms: start.elapsed().as_secs_f64() * 1000.0,
    })
}

struct Pages {
    placed: Vec<Placed>,
    sizes: Vec<(u32, u32)>,
    files: Vec<PathBuf>,
    hashes: Vec<u64>,
    maps: Vec<(Map, Vec<PathBuf>, Vec<u64>)>,
}

fn page_set(
    files: Vec<PathBuf>,
    cut: &[Pixels],
    placed: &[Placed],
    sizes: &[(u32, u32)],
    blank: impl Fn(u32, u32) -> Pixels + Sync,
) -> Result<(Vec<PathBuf>, Vec<u64>)> {
    let encoded = sizes
        .par_iter()
        .enumerate()
        .map(|(i, &(w, h))| {
            let mut page = blank(w, h);
            cut.iter()
                .zip(placed)
                .filter(|(_, at)| at.page == i)
                .for_each(|(cut, at)| {
                    let all = Rect {
                        x: 0,
                        y: 0,
                        w: cut.width,
                        h: cut.height,
                    };
                    page.blit(cut, all, at.x, at.y);
                });
            let hash = page.hash();
            Ok((page.png()?, hash))
        })
        .collect::<Result<Vec<_>>>()?;
    files
        .iter()
        .zip(&encoded)
        .try_for_each(|(file, (bytes, _))| write_atomic(file, bytes))?;
    Ok((files, encoded.iter().map(|(_, h)| *h).collect()))
}

fn write_pages(name: &Name, pics: &Pictures, packing: &Packing, depth: u8) -> Result<Pages> {
    let sizes: Vec<(u32, u32)> = pics.trims.iter().map(|t| (t.w, t.h)).collect();
    let room = packing.max.saturating_sub(2 * packing.padding);
    if let Some((t, frame)) = pics
        .trims
        .iter()
        .zip(&pics.firsts)
        .find(|(t, _)| t.w > room || t.h > room)
    {
        bail!(
            "frame {frame} is {}x{} px after trimming, more than a {} px page holds with {} px padding; export at a smaller --scale or raise --max",
            t.w,
            t.h,
            packing.max,
            packing.padding
        );
    }
    let (placed, pages) = sprites::pack(&sizes, packing.padding, packing.max)?;
    let (files, hashes) = page_set(
        (0..pages.len()).map(|i| name.page(i)).collect(),
        &pics.cut,
        &placed,
        &pages,
        |w, h| Pixels::blank(w, h, depth),
    )?;
    let maps = pics
        .maps
        .iter()
        .map(|(map, cut)| {
            let first = cut.first().cloned();
            let (files, hashes) = page_set(
                (0..pages.len()).map(|i| name.map_page(i, *map)).collect(),
                cut,
                &placed,
                &pages,
                |w, h| first.as_ref().map_or(Pixels::blank(w, h, 8), |f| f.blank_like(w, h)),
            )?;
            Ok((*map, files, hashes))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Pages {
        placed,
        sizes: pages,
        hashes,
        files,
        maps,
    })
}

fn remove_stale_pages(name: &Name, kept: usize) {
    let gone = |page: PathBuf| {
        let _ = fs::remove_file(&page);
        let _ = fs::remove_file(page.with_extension("png.import"));
    };
    (kept..).map(|i| name.page(i)).take_while(|p| p.exists()).for_each(gone);
    Map::ALL.iter().for_each(|map| {
        (kept..)
            .map(|i| name.map_page(i, *map))
            .take_while(|p| p.exists())
            .for_each(gone);
    });
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map_or(String::new(), |n| n.to_string_lossy().to_string())
}

fn summary(pics: &Pictures, pages: &Pages) -> String {
    let sizes: Vec<String> = pages.sizes.iter().map(|(w, h)| format!("{w}x{h}")).collect();
    format!(
        "{} pictures from {} frames ({} composed), {} page{} {}",
        pics.trims.len(),
        pics.frames,
        pics.composed,
        pages.sizes.len(),
        if pages.sizes.len() == 1 { "" } else { "s" },
        sizes.join(" ")
    )
}

fn check_look(look: &Look) -> Result<()> {
    if look.depth != 8 && look.depth != 16 {
        bail!("--depth takes 8 or 16, not {}", look.depth);
    }
    Ok(())
}

pub fn sheet(path: &Path, spec: &str, look: &Look, packing: &Packing, out: Option<PathBuf>) -> Result<()> {
    let start = Instant::now();
    check_look(look)?;
    let bundle = Bundle::open(path)?;
    let animations = sprites::animations(&bundle.doc, spec)?;
    let stem = stem_for(&bundle, look);
    let usual = format!(
        "{stem}-{}{}",
        if spec == "all" { "sheet".into() } else { file_safe(spec) },
        scale_tag(look.scale)
    );
    let name = match out {
        Some(folder) if folder.is_dir() => Name {
            dir: folder,
            stem: usual,
        },
        Some(out) => Name {
            dir: out.parent().map_or(PathBuf::from("."), Path::to_path_buf),
            stem: file_name(&out).trim_end_matches(".png").to_string(),
        },
        None => Name {
            dir: bundle_dir(&bundle),
            stem: usual,
        },
    };
    let pics = pictures(&bundle, animations, look)?;
    let pages = write_pages(&name, &pics, packing, look.depth)?;
    remove_stale_pages(&name, pages.files.len());
    let images: Vec<String> = pages.files.iter().map(|f| file_name(f)).collect();
    let json = sprites::aseprite(&Sheet {
        stem: &stem,
        source: pics.source,
        trims: &pics.trims,
        placed: &pages.placed,
        pages: &pages.sizes,
        images: &images,
        animations: &pics.animations,
        scale: look.scale,
        depth: look.depth,
    });
    let mut json = json;
    if !pages.maps.is_empty() {
        json["meta"]["maps"] = pages
            .maps
            .iter()
            .map(|(map, files, _)| {
                (
                    map.name().to_string(),
                    files.iter().map(|f| serde_json::Value::from(file_name(f))).collect(),
                )
            })
            .collect::<serde_json::Map<String, serde_json::Value>>()
            .into();
    }
    let text = serde_json::to_string_pretty(&json)? + "\n";
    let json_path = name.file(".json");
    write_atomic(&json_path, text.as_bytes())?;
    let map_files = || pages.maps.iter().flat_map(|(_, f, h)| f.iter().zip(h));
    pages.files.iter().for_each(|f| println!("{}", f.display()));
    map_files().for_each(|(f, _)| println!("{}", f.display()));
    println!("{}", json_path.display());
    eprintln!("{}", summary(&pics, &pages));
    pages
        .files
        .iter()
        .zip(&pages.hashes)
        .chain(map_files())
        .for_each(|(f, h)| eprintln!("hash {h:016x}  {}", file_name(f)));
    eprintln!(
        "hash {:016x}  {}",
        klad_core::rng::fnv(klad_core::rng::FNV_OFFSET, text.as_bytes()),
        file_name(&json_path)
    );
    eprintln!(
        "pictures {:.0} ms, {:.0} ms in all",
        pics.ms,
        start.elapsed().as_secs_f64() * 1000.0
    );
    Ok(())
}

pub fn frames(path: &Path, spec: &str, look: &Look, out: Option<PathBuf>) -> Result<()> {
    let start = Instant::now();
    check_look(look)?;
    let bundle = Bundle::open(path)?;
    let animations = sprites::animations(&bundle.doc, spec)?;
    let stem = stem_for(&bundle, look);
    let prefix = match spec {
        "all" => stem.clone(),
        other => file_safe(other),
    };
    let dir = out.unwrap_or_else(|| {
        let tag = if spec == "all" {
            "frames".into()
        } else {
            file_safe(spec)
        };
        bundle_dir(&bundle).join(format!("{stem}-{tag}{}", scale_tag(look.scale)))
    });
    fs::create_dir_all(&dir).with_context(|| format!("making {}", dir.display()))?;
    let pics = pictures(&bundle, animations, look)?;
    let (sw, sh) = pics.source;
    let whole = |cut: &[Pixels]| -> Result<Vec<Vec<u8>>> {
        cut.par_iter()
            .zip(&pics.trims)
            .map(|(cut, trim)| {
                let mut full = cut.blank_like(sw, sh);
                let all = Rect {
                    x: 0,
                    y: 0,
                    w: cut.width,
                    h: cut.height,
                };
                full.blit(cut, all, trim.x, trim.y);
                full.png()
            })
            .collect()
    };
    let encoded = whole(&pics.cut)?;
    let maps: Vec<(Map, Vec<Vec<u8>>)> = pics
        .maps
        .iter()
        .map(|(map, cut)| Ok((*map, whole(cut)?)))
        .collect::<Result<_>>()?;
    let sequence: Vec<usize> = pics
        .animations
        .iter()
        .flat_map(|(_, runs)| {
            runs.iter()
                .flat_map(|r| std::iter::repeat_n(r.picture, r.frames as usize))
        })
        .collect();
    sequence.iter().enumerate().try_for_each(|(i, &p)| {
        write_atomic(&dir.join(format!("{prefix}-{i:04}.png")), &encoded[p])?;
        maps.iter().try_for_each(|(map, bytes)| {
            write_atomic(&dir.join(format!("{prefix}-{i:04}{}.png", map.suffix())), &bytes[p])
        })
    })?;
    println!("{}", dir.display());
    eprintln!(
        "{} frames as {prefix}-0000.png to {prefix}-{:04}.png, {} pictures ({} composed), {:.0} ms",
        sequence.len(),
        sequence.len().saturating_sub(1),
        pics.trims.len(),
        pics.composed,
        start.elapsed().as_secs_f64() * 1000.0
    );
    Ok(())
}

pub enum Anim {
    Gif { dither: bool },
    Webp(klad_render::formats::Quality),
    Mp4(klad_render::video::Settings),
}

impl Anim {
    fn extension(&self) -> &'static str {
        match self {
            Anim::Gif { .. } => "gif",
            Anim::Webp(_) => "webp",
            Anim::Mp4(_) => "mp4",
        }
    }

    fn encode(&self, w: u32, h: u32, shots: Vec<(Vec<u8>, u32)>, looping: bool) -> Result<Vec<u8>> {
        match self {
            Anim::Gif { dither } => {
                let shots: Vec<klad_render::gif::Shot> = shots
                    .into_iter()
                    .map(|(rgba, ms)| klad_render::gif::Shot { rgba, ms })
                    .collect();
                klad_render::gif::encode(w, h, &shots, looping, *dither)
            }
            Anim::Webp(q) => {
                let shots: Vec<klad_render::formats::Still> = shots
                    .into_iter()
                    .map(|(rgba, ms)| klad_render::formats::Still { rgba, ms })
                    .collect();
                klad_render::formats::webp_animated(w, h, &shots, *q, looping)
            }
            Anim::Mp4(s) => {
                let shots: Vec<klad_render::video::Shot> = shots
                    .into_iter()
                    .map(|(rgba, ms)| klad_render::video::Shot { rgba, ms })
                    .collect();
                klad_render::video::mp4(w, h, &shots, *s)
            }
        }
    }
}

pub fn gif(path: &Path, spec: &str, look: &Look, dither: bool, out: Option<PathBuf>) -> Result<()> {
    animated(path, spec, look, &Anim::Gif { dither }, out)
}

pub fn animated(path: &Path, spec: &str, look: &Look, kind: &Anim, out: Option<PathBuf>) -> Result<()> {
    let start = Instant::now();
    let bundle = Bundle::open(path)?;
    let animations = sprites::animations(&bundle.doc, spec)?;
    let look = Look {
        layer: look.layer.clone(),
        background: look.background,
        variant: look.variant.clone(),
        ..Look::plain(look.scale, 8)
    };
    let stem = stem_for(&bundle, &look);
    let pics = pictures(&bundle, animations, &look)?;
    let (sw, sh) = pics.source;
    let full: Vec<Vec<u8>> = pics
        .cut
        .par_iter()
        .zip(&pics.trims)
        .map(|(cut, trim)| {
            let mut whole = Pixels::blank(sw, sh, 8);
            let all = Rect {
                x: 0,
                y: 0,
                w: cut.width,
                h: cut.height,
            };
            whole.blit(cut, all, trim.x, trim.y);
            whole.bytes().to_vec()
        })
        .collect();
    let alone = pics.animations.len() == 1;
    let folder = out.as_ref().filter(|_| !alone);
    if let Some(dir) = folder {
        fs::create_dir_all(dir).with_context(|| format!("making {}", dir.display()))?;
    }
    pics.animations.iter().try_for_each(|(animation, runs)| {
        let shots: Vec<(Vec<u8>, u32)> = runs.iter().map(|r| (full[r.picture].clone(), r.ms)).collect();
        let pictures = shots.len();
        let total: u32 = shots.iter().map(|s| s.1).sum();
        let bytes = kind.encode(sw, sh, shots, animation.looping)?;
        let ext = kind.extension();
        let name = match animation.clip {
            true => format!("{stem}-{}{}.{ext}", file_safe(&animation.name), scale_tag(look.scale)),
            false => format!("{stem}{}.{ext}", scale_tag(look.scale)),
        };
        let file = match (&out, folder) {
            (_, Some(dir)) => dir.join(name),
            (Some(given), None) if given.is_dir() => given.join(name),
            (Some(given), None) => given.clone(),
            (None, None) => bundle_dir(&bundle).join(name),
        };
        write_atomic(&file, &bytes)?;
        println!("{}", file.display());
        eprintln!(
            "{sw}x{sh}  {} frames ({} pictures), {} ms, {}  {:.1} KB",
            runs.iter().map(|r| r.frames).sum::<u32>(),
            pictures,
            total,
            match (pictures, animation.looping) {
                (1, _) => "one still picture",
                (_, true) => "loops",
                (_, false) => "plays once",
            },
            bytes.len() as f64 / 1024.0
        );
        Ok::<_, anyhow::Error>(())
    })?;
    eprintln!("{:.0} ms", start.elapsed().as_secs_f64() * 1000.0);
    Ok(())
}

pub fn relative(from: &Path, to: &Path) -> PathBuf {
    let a: Vec<Component> = from.components().collect();
    let b: Vec<Component> = to.components().collect();
    let common = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let ups = std::iter::repeat_n(Component::ParentDir, a.len() - common);
    let path: PathBuf = ups.chain(b[common..].iter().copied()).collect();
    match path.as_os_str().is_empty() {
        true => PathBuf::from("."),
        false => path,
    }
}

pub fn kept_folder(from: &Path, to: &Path) -> String {
    relative(from, to)
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/")
}

pub(crate) fn read(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok()
}

fn godot_folder(bundle: &mut Bundle, dir: &Option<PathBuf>, store: bool) -> Result<PathBuf> {
    let home = bundle_dir(bundle);
    let stored = bundle.doc.export.godot.clone();
    let target = match (dir, &stored) {
        (Some(d), _) => d.clone(),
        (None, Some(s)) => home.join(s),
        (None, None) => bail!(
            "this document has no Godot folder yet: give one with --godot <folder> (it is kept in the document for the next export and the editor's button)"
        ),
    };
    fs::create_dir_all(&target).with_context(|| format!("making {}", target.display()))?;
    let target = target.canonicalize()?;
    let rel = kept_folder(&home.canonicalize()?, &target);
    let keep = match (dir, &stored) {
        (Some(_), None) => true,
        (Some(_), Some(s)) => store && *s != rel,
        _ => false,
    };
    if keep {
        bundle.apply(&[Op::Export {
            godot: Some(rel.clone()),
        }])?;
        bundle.stamp("cli")?;
        eprintln!("kept {rel} as this document's Godot folder (export.godot)");
    } else if let (Some(_), Some(s)) = (dir, &stored)
        && *s != rel
    {
        eprintln!("exported to {rel}; the document's Godot folder stays {s} (add --store to replace it)");
    }
    Ok(target)
}

pub fn tileset(path: &Path, dir: Option<PathBuf>, store: bool, look: &Look, frame: u32) -> Result<()> {
    check_look(look)?;
    let mut bundle = Bundle::open(path)?;
    let Some([tile_width, tile_height]) = bundle.doc.canvas.tile else {
        bail!(
            "this drawing is not cut into tiles: klad canvas <bundle> --tile 16 sets the tile size (the canvas has to be a whole number of tiles each way)"
        );
    };
    if frame >= bundle.doc.timeline.frames {
        bail!(
            "frame {frame} is past the last frame {}",
            bundle.doc.timeline.frames - 1
        );
    }
    let target = godot_folder(&mut bundle, &dir, store)?;
    let doc = &bundle.doc;
    klad_render::export::check_scale(doc, look.scale)?;
    let size = |side: u32| (side as f32 * look.scale).round() as u32;
    if size(tile_width) as f32 != tile_width as f32 * look.scale
        || size(tile_height) as f32 != tile_height as f32 * look.scale
    {
        bail!(
            "at scale {} a tile of {tile_width}x{tile_height} is not a whole number of pixels",
            look.scale
        );
    }
    let layer = look.layer.as_deref().map(|k| doc.find_layer(k)).transpose()?;
    let detail = detail_of(doc, look.scale);
    let brushes = BrushSet::for_bundle(&bundle)?;
    let dress = crate::layers::dress_of(doc, look.variant.as_deref())?;
    let rasters = crate::layers::dressed(
        &bundle,
        &brushes,
        crate::layers::load_at(&bundle, frame, &brushes, detail)?,
        &dress,
    )?;
    let options = Options {
        scale: look.scale,
        background: look.background,
        references: false,
        detail,
    };
    let image = render(
        doc,
        frame,
        &rasters,
        &layer.map_or(Target::Frame, Target::Layer),
        options,
    )?;
    let shown = klad_render::export::dither8(&image)?;
    let (cell_width, cell_height) = (size(tile_width), size(tile_height));
    let (columns, rows) = (image.width() as u32 / cell_width, image.height() as u32 / cell_height);
    let across = image.width() as usize;
    let tiles: Vec<(u32, u32)> = (0..rows)
        .flat_map(|row| (0..columns).map(move |column| (column, row)))
        .filter(|(column, row)| {
            (0..cell_height).any(|y| {
                (0..cell_width).any(|x| {
                    let at = ((row * cell_height + y) as usize * across + (column * cell_width + x) as usize) * 4;
                    shown[at + 3] > 0
                })
            })
        })
        .collect();
    if tiles.is_empty() {
        bail!("frame {frame} shows nothing: there are no tiles to export");
    }
    let stem = format!("{}-tiles", file_safe(&stem_for(&bundle, look)));
    let (png, tres) = (target.join(format!("{stem}.png")), target.join(format!("{stem}.tres")));
    let bytes = match look.depth {
        16 => klad_render::export::png16(&image)?,
        _ => klad_render::export::png8(&image)?,
    };
    let uid = kept_uid(&tres);
    let lighting = match doc.maps.is_some() && layer.is_none() {
        true => {
            let set = klad_render::maps::render(doc, frame, &rasters, look.scale)?;
            let files: Vec<PathBuf> = [Map::Normal, Map::Specular]
                .iter()
                .map(|map| {
                    let file = tagged(&png, map.suffix());
                    write_atomic(&file, &Pixels::of_map(&set, *map).png()?)?;
                    Ok(file)
                })
                .collect::<Result<_>>()?;
            Some((
                sprites::Lighting {
                    normal: texture_of(&files[0]),
                    specular: Some(texture_of(&files[1])),
                },
                files,
            ))
        }
        false => None,
    };
    let text = sprites::tile_set(&sprites::TileSet {
        uid: &uid,
        texture: &texture_of(&png),
        tile: [cell_width, cell_height],
        tiles: &tiles,
        lighting: lighting.as_ref().map(|(l, _)| l),
        shininess: klad_core::maps::godot_shininess(klad_render::maps::settings(doc).shininess),
        nearest: doc.pixel_art(),
    });
    write_atomic(&png, &bytes)?;
    write_atomic(&tres, text.as_bytes())?;
    println!("{}", png.display());
    lighting
        .iter()
        .flat_map(|(_, files)| files)
        .for_each(|f| println!("{}", f.display()));
    println!("{}", tres.display());
    eprintln!(
        "{} tiles of {cell_width}x{cell_height} ({columns} across, {rows} down, {} empty left out)",
        tiles.len(),
        (columns * rows) as usize - tiles.len()
    );
    if doc.pixel_art() {
        eprintln!("note: in Godot, set the TileMapLayer's texture filter to Nearest so the tiles stay sharp");
    }
    Ok(())
}

#[derive(Clone, Debug, Default)]
pub struct Extras {
    pub occluders: Option<f32>,
    pub shaders: Vec<Effect>,
}

pub fn glows(doc: &klad_core::doc::Document) -> bool {
    doc.all_layers()
        .iter()
        .any(|l| l.visible && (l.emissive || !l.glows.is_empty() || l.map == Some(klad_core::maps::Role::Emission)))
}

pub fn lit_maps(doc: &klad_core::doc::Document, shaders: &[Effect]) -> Result<Vec<Map>> {
    if doc.maps.is_none() {
        if !shaders.is_empty() {
            bail!("the shaders read the light maps, and this drawing's maps are off: klad maps <bundle> --on");
        }
        return Ok(Vec::new());
    }
    let mut maps = vec![Map::Normal, Map::Specular];
    if glows(doc) {
        maps.push(Map::Emission);
    }
    maps.extend(shaders.iter().map(|e| e.map()));
    maps.sort();
    maps.dedup();
    Ok(maps)
}

pub fn texture_of(file: &Path) -> Texture {
    Texture {
        path: file_name(file),
        uid: read(&file.with_extension("png.import")).and_then(|t| sprites::import_uid(&t)),
    }
}

pub fn kept_uid(file: &Path) -> String {
    read(file)
        .and_then(|t| sprites::header_uid(&t))
        .unwrap_or_else(|| sprites::uid_for(&file.to_string_lossy()))
}

pub fn shader_file(dir: &Path, stem: &str, effects: &[Effect], nearest: bool) -> Result<Option<(PathBuf, String)>> {
    if effects.is_empty() {
        return Ok(None);
    }
    let file = dir.join(format!("{stem}-maps.gdshader"));
    write_atomic(&file, klad_render::shaders::source(effects, nearest).as_bytes())?;
    let kept = file.with_extension("gdshader.uid");
    let uid = match read(&kept)
        .map(|t| t.trim().to_string())
        .filter(|t| t.starts_with("uid://"))
    {
        Some(uid) => uid,
        None => {
            let uid = sprites::uid_for(&format!("shader {}", file.display()));
            write_atomic(&kept, format!("{uid}\n").as_bytes())?;
            uid
        }
    };
    Ok(Some((file, uid)))
}

pub fn occluder_shapes(cut: &[Pixels], trims: &[Rect], tolerance: f32) -> Vec<Vec<klad_render::occluders::Polygon>> {
    cut.par_iter()
        .zip(trims)
        .map(|(pixels, trim)| {
            let (w, h) = (pixels.width as usize, pixels.height as usize);
            let alpha: Vec<f32> = (0..w * h)
                .map(|i| pixels.alpha((i % w) as u32, (i / w) as u32))
                .collect();
            klad_render::occluders::polygons(&alpha, w, h, tolerance)
                .into_iter()
                .map(|ring| {
                    ring.into_iter()
                        .map(|[x, y]| [x + trim.x as f32, y + trim.y as f32])
                        .collect()
                })
                .collect()
        })
        .collect()
}

pub fn godot(
    path: &Path,
    dir: Option<PathBuf>,
    store: bool,
    look: &Look,
    packing: &Packing,
    extras: &Extras,
) -> Result<()> {
    let start = Instant::now();
    check_look(look)?;
    let mut bundle = Bundle::open(path)?;
    let target = godot_folder(&mut bundle, &dir, store)?;
    let doc = &bundle.doc;
    let animations = sprites::animations(doc, "all")?;
    let stem = file_safe(&stem_for(&bundle, look));
    let name = Name {
        dir: target.clone(),
        stem: stem.clone(),
    };
    let wanted = lit_maps(doc, &extras.shaders)?;
    let look = Look {
        layer: look.layer.clone(),
        variant: look.variant.clone(),
        maps: wanted,
        invert: false,
        ..*look
    };
    let pics = pictures(&bundle, animations, &look)?;
    let pages = write_pages(&name, &pics, packing, look.depth)?;
    remove_stale_pages(&name, pages.files.len());
    let textures: Vec<Texture> = pages.files.iter().map(|f| texture_of(f)).collect();
    let map_pages = |map: Map| pages.maps.iter().find(|(m, _, _)| *m == map).map(|(_, f, _)| f);
    let nearest = doc.pixel_art();
    let lighting: Vec<sprites::Lighting> = match map_pages(Map::Normal) {
        Some(normals) => normals
            .iter()
            .enumerate()
            .map(|(i, n)| sprites::Lighting {
                normal: texture_of(n),
                specular: map_pages(Map::Specular).and_then(|s| s.get(i)).map(|s| texture_of(s)),
            })
            .collect(),
        None => Vec::new(),
    };
    let settings = klad_render::maps::settings(doc);
    let (tres, tscn) = (name.file(".tres"), name.file(".tscn"));
    let (frames_uid, scene_uid) = (kept_uid(&tres), kept_uid(&tscn));
    let frames_text = sprites::sprite_frames(&SpriteFrames {
        uid: &frames_uid,
        textures: &textures,
        source: pics.source,
        trims: &pics.trims,
        placed: &pages.placed,
        animations: &pics.animations,
        lighting: &lighting,
        shininess: klad_core::maps::godot_shininess(settings.shininess),
        nearest,
    });
    let mut written = vec![tres.clone(), tscn.clone()];
    let glow = match map_pages(Map::Emission) {
        Some(files) => {
            let file = name.file("-glow.tres");
            let uid = kept_uid(&file);
            let glow_textures: Vec<Texture> = files.iter().map(|f| texture_of(f)).collect();
            let text = sprites::sprite_frames(&SpriteFrames {
                uid: &uid,
                textures: &glow_textures,
                source: pics.source,
                trims: &pics.trims,
                placed: &pages.placed,
                animations: &pics.animations,
                lighting: &[],
                shininess: 0.0,
                nearest,
            });
            write_atomic(&file, text.as_bytes())?;
            written.push(file.clone());
            Some((file_name(&file), uid))
        }
        None => None,
    };
    let (w, h) = (doc.canvas.width as f32, doc.canvas.height as f32);
    let [ox, oy] = doc.timeline.origin.unwrap_or([w / 2.0, h / 2.0]);
    let (sx, sy) = (pics.source.0 as f32 / w, pics.source.1 as f32 / h);
    let first = pics
        .animations
        .first()
        .map(|(a, _)| a.name.clone())
        .ok_or_else(|| anyhow!("nothing to export"))?;
    let per_frame = |of: &dyn Fn(usize) -> usize| -> Vec<(String, Vec<usize>)> {
        pics.animations
            .iter()
            .map(|(a, runs)| (a.name.clone(), runs.iter().map(|r| of(r.picture)).collect()))
            .collect()
    };
    let shapes = extras
        .occluders
        .map(|tolerance| occluder_shapes(&pics.cut, &pics.trims, tolerance * look.scale.max(1.0)));
    let pictures_by_frame = per_frame(&|p| p);
    let pages_by_frame = per_frame(&|p| pages.placed[p].page);
    let shader = shader_file(&target, &stem, &extras.shaders, nearest)?;
    let shader_textures: Vec<(String, Vec<Texture>)> = extras
        .shaders
        .iter()
        .filter_map(|e| {
            map_pages(e.map()).map(|files| (e.uniform().to_string(), files.iter().map(|f| texture_of(f)).collect()))
        })
        .collect();
    let scene_text = sprites::scene(&Scene {
        uid: &scene_uid,
        node: &sprites::node_name(&stem_for(&bundle, &look)),
        frames_path: &file_name(&tres),
        frames_uid: &frames_uid,
        animation: &first,
        origin: [ox * sx, oy * sy],
        pixel: nearest,
        glow: glow.as_ref().map(|(p, u)| (p.as_str(), u.as_str())),
        shadows: shapes.as_ref().map(|shapes| sprites::Shadows {
            shapes,
            pictures: &pictures_by_frame,
        }),
        material: shader.as_ref().map(|(file, uid)| sprites::Material {
            shader: (
                file.file_name().and_then(|n| n.to_str()).unwrap_or_default(),
                uid.as_str(),
            ),
            params: &[],
            textures: &shader_textures,
            pages: &pages_by_frame,
        }),
    });
    write_atomic(&tres, frames_text.as_bytes())?;
    write_atomic(&tscn, scene_text.as_bytes())?;
    if let Some((file, _)) = &shader {
        written.push(file.clone());
    }
    if let Some(maps) = &doc.maps {
        let file = name.file("_lit.tscn");
        let text = sprites::lit_scene(&sprites::LitScene {
            uid: &kept_uid(&file),
            node: &sprites::node_name(&stem_for(&bundle, &look)),
            sprite: (&file_name(&tscn), &scene_uid),
            origin: [ox, oy],
            size: [w, h],
            scale: look.scale,
            maps,
            outline: shapes
                .as_ref()
                .and_then(|shapes| shapes.get(pics.animations.first()?.1.first()?.picture))
                .map_or(&[], |rings| rings.as_slice()),
        })?;
        write_atomic(&file, text.as_bytes())?;
        written.push(file);
    }
    pages
        .files
        .iter()
        .chain(pages.maps.iter().flat_map(|(_, f, _)| f))
        .chain(&written)
        .for_each(|f| println!("{}", f.display()));
    let names: Vec<String> = pics
        .animations
        .iter()
        .map(|(a, runs)| {
            format!(
                "{} ({} frames, {} entries, {} fps{})",
                a.name,
                a.to - a.from + 1,
                runs.len(),
                a.fps,
                if a.looping { ", loops" } else { "" }
            )
        })
        .collect();
    eprintln!("{}", summary(&pics, &pages));
    eprintln!("animations: {}", names.join(", "));
    if !pages.maps.is_empty() {
        let maps: Vec<&str> = pages.maps.iter().map(|(m, _, _)| m.name()).collect();
        eprintln!(
            "lit: {} maps on every page, a CanvasTexture each; {} to try it with the drawing's lights",
            maps.join(", "),
            file_name(&name.file("_lit.tscn"))
        );
    }
    if let Some(shapes) = &shapes {
        eprintln!(
            "shadows: {} occluder polygons over {} pictures",
            shapes.iter().map(Vec::len).sum::<usize>(),
            shapes.len()
        );
    }
    eprintln!(
        "pictures {:.0} ms, {:.0} ms in all",
        pics.ms,
        start.elapsed().as_secs_f64() * 1000.0
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use klad_core::doc::{Cel, Clip, Document, Layer};
    use klad_core::stroke::{Stroke, marks};
    use serde_json::Value;
    use std::process::Command;

    const CHECK: &str = r#"extends SceneTree

func _init():
	var args = OS.get_cmdline_user_args()
	var frames: SpriteFrames = load("res://art/hop.tres")
	var out = {"animations": {}}
	for anim in frames.get_animation_names():
		var list = []
		for i in frames.get_frame_count(anim):
			var t: AtlasTexture = frames.get_frame_texture(anim, i)
			list.append([frames.get_frame_duration(anim, i), t.get_width(), t.get_height(), t.region.position.x, t.region.position.y, t.margin.position.x, t.margin.position.y])
		out["animations"][anim] = {"speed": frames.get_animation_speed(anim), "loop": frames.get_animation_loop(anim), "frames": list}
	var node = load("res://art/hop.tscn").instantiate()
	out["scene"] = [node.get_class(), String(node.animation), node.centered, node.offset.x, node.offset.y]
	node.free()
	out["uids"] = []
	for uid in args:
		var res = load(uid)
		out["uids"].append(res.resource_path if res else "")
	print("RESULT ", JSON.stringify(out))
	quit()
"#;

    fn godot_binary() -> Option<String> {
        let binary = std::env::var("GODOT").unwrap_or_else(|_| "godot".into());
        let ok = Command::new(&binary)
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success());
        ok.then_some(binary)
    }

    fn godot_run(binary: &str, project: &Path, args: &[&str]) -> String {
        let out = Command::new(binary)
            .args(["--headless", "--quit-after", "600", "--path"])
            .arg(project)
            .args(args)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr)
    }

    fn hop(dir: &Path) -> PathBuf {
        let mut doc = Document::new(64, 48);
        doc.timeline.frames = 10;
        doc.timeline.origin = Some([20.0, 40.0]);
        doc.timeline.clips = vec![
            Clip::new("idle", 0, 3, true),
            Clip {
                fps: Some(24),
                ..Clip::new("hop", 4, 9, false)
            },
        ];
        let mut ball = Layer::paint("l1", "ball", "c1");
        *ball.cels_mut().unwrap() = vec![
            Cel {
                hold: Some(3),
                ..Cel::at("c1", 0)
            },
            Cel::at("c2", 3),
            Cel::at("c3", 4),
            Cel {
                hold: Some(2),
                ..Cel::at("c4", 5)
            },
            Cel {
                link: Some("c1".into()),
                ..Cel::at("c5", 7)
            },
        ];
        doc.layers = vec![ball];
        let path = dir.join("hop.klad");
        let bundle = Bundle::create(&path, doc).unwrap();
        let dot = |x: f32, y: f32| {
            marks(vec![
                Stroke::new(1, "ink", [0.8, 0.2, 0.1, 1.0], 10.0, 1).with_points(&[(x, y, 1.0)], 0.0),
            ])
        };
        [
            ("c1", 20.0, 36.0),
            ("c2", 22.0, 28.0),
            ("c3", 24.0, 16.0),
            ("c4", 26.0, 24.0),
        ]
        .iter()
        .for_each(|(cel, x, y)| {
            bundle.write_marks("l1", cel, &dot(*x, *y)).unwrap();
        });
        path
    }

    fn look(scale: f32) -> Look {
        Look::plain(scale, 8)
    }

    const PACKING: Packing = Packing { padding: 2, max: 4096 };

    const TILES: &str = r#"extends SceneTree

func _init():
	var out = {}
	var tiles = load("res://art/ground-tiles.tres")
	out["class"] = tiles.get_class()
	out["tile_size"] = [tiles.tile_size.x, tiles.tile_size.y]
	out["sources"] = tiles.get_source_count()
	var source = tiles.get_source(tiles.get_source_id(0))
	out["source"] = source.get_class()
	out["region"] = [source.texture_region_size.x, source.texture_region_size.y]
	out["count"] = source.get_tiles_count()
	out["has"] = [source.has_tile(Vector2i(0, 0)), source.has_tile(Vector2i(1, 0)), source.has_tile(Vector2i(2, 1))]
	out["texture"] = [source.texture.get_width(), source.texture.get_height()]
	out["path"] = tiles.resource_path
	print("RESULT ", JSON.stringify(out))
	quit()
"#;

    #[test]
    fn godot_loads_the_tile_set_with_a_tile_for_every_cell_that_is_drawn() {
        let dir = std::env::temp_dir().join(format!("draw-godot-tiles-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let project = dir.join("game");
        fs::create_dir_all(project.join("art")).unwrap();
        let path = dir.join("ground.klad");
        let mut bundle = Bundle::blank(
            &path,
            &klad_core::bundle::Blank {
                width: 48,
                height: 32,
                pixel: true,
                ..klad_core::bundle::Blank::default()
            },
            "cli",
        )
        .unwrap();
        let layer = bundle.doc.layers[0].id.clone();
        assert!(
            tileset(&path, Some(project.join("art")), false, &look(1.0), 0)
                .unwrap_err()
                .to_string()
                .contains("not cut into tiles")
        );
        bundle
            .apply(
                &klad_core::ops::parse_lines(&format!(
                    "{}\n{}\n{}",
                    r##"{"op":"canvas","tile":16}"##,
                    format_args!(
                        r##"{{"op":"pixel","layer":"{layer}","color":"#3a5a40","rect":[0,0,16,16],"filled":true}}"##
                    ),
                    format_args!(r##"{{"op":"pixel","layer":"{layer}","color":"#8b5a2b","cells":[[40,20]]}}"##),
                ))
                .unwrap(),
            )
            .unwrap();
        tileset(&path, Some(project.join("art")), false, &look(2.0), 0).unwrap();
        let tres = fs::read_to_string(project.join("art/ground-tiles.tres")).unwrap();
        assert!(tres.starts_with("[gd_resource type=\"TileSet\" load_steps=3 format=3 uid=\"uid://"));
        assert!(tres.contains("texture_region_size = Vector2i(32, 32)\n0:0/0 = 0\n2:1/0 = 0\n\n[resource]"));
        assert!(tres.contains("path=\"ground-tiles.png\""));
        assert!(!tres.contains("1:0/0"), "an empty cell is no tile");
        assert_eq!(
            Bundle::open(&path).unwrap().doc.export.godot.as_deref(),
            Some("game/art"),
            "the folder is kept, as for the other Godot exports"
        );
        let uid = sprites::header_uid(&tres).unwrap();
        tileset(&path, None, false, &look(2.0), 0).unwrap();
        assert_eq!(
            sprites::header_uid(&fs::read_to_string(project.join("art/ground-tiles.tres")).unwrap()).unwrap(),
            uid,
            "a second export keeps the uid"
        );
        assert!(tileset(&path, None, false, &look(1.5), 0).is_err());
        let Some(binary) = godot_binary() else {
            eprintln!("no godot on PATH (or $GODOT): skipping the import check");
            fs::remove_dir_all(&dir).unwrap();
            return;
        };
        fs::write(
            project.join("project.godot"),
            "config_version=5\n\n[application]\n\nconfig/name=\"klad tiles\"\n",
        )
        .unwrap();
        fs::write(project.join("tiles.gd"), TILES).unwrap();
        godot_run(&binary, &project, &["--import"]);
        let text = godot_run(&binary, &project, &["--script", "res://tiles.gd"]);
        let line = text
            .lines()
            .find_map(|l| l.strip_prefix("RESULT "))
            .unwrap_or_else(|| panic!("no result from godot:\n{text}"));
        let got: Value = serde_json::from_str(line).unwrap();
        assert_eq!(got["class"], "TileSet", "{got}");
        assert_eq!(got["tile_size"], serde_json::json!([32, 32]));
        assert_eq!(
            (got["sources"].as_u64(), got["source"].as_str()),
            (Some(1), Some("TileSetAtlasSource"))
        );
        assert_eq!(got["region"], serde_json::json!([32, 32]));
        assert_eq!(got["count"].as_u64(), Some(2));
        assert_eq!(got["has"], serde_json::json!([true, false, true]));
        assert_eq!(got["texture"], serde_json::json!([96, 64]));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn godot_imports_the_export_with_its_clips_timing_origin_and_uids() {
        let Some(binary) = godot_binary() else {
            eprintln!("no godot on PATH (or $GODOT): skipping the import check");
            return;
        };
        let dir = std::env::temp_dir().join(format!("draw-godot-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let project = dir.join("game");
        fs::create_dir_all(project.join("art")).unwrap();
        fs::write(
            project.join("project.godot"),
            "config_version=5\n\n[application]\n\nconfig/name=\"klad check\"\n",
        )
        .unwrap();
        fs::write(project.join("check.gd"), CHECK).unwrap();
        let path = hop(&dir);
        godot(
            &path,
            Some(project.join("art")),
            false,
            &look(2.0),
            &PACKING,
            &Extras::default(),
        )
        .unwrap();
        assert_eq!(
            Bundle::open(&path).unwrap().doc.export.godot.as_deref(),
            Some("game/art")
        );
        let tres = project.join("art/hop.tres");
        let tscn = project.join("art/hop.tscn");
        let uids = |p: &Path| sprites::header_uid(&fs::read_to_string(p).unwrap()).unwrap();
        let (frames_uid, scene_uid) = (uids(&tres), uids(&tscn));
        godot_run(&binary, &project, &["--import"]);
        let result = |text: &str| -> Value {
            let line = text
                .lines()
                .find_map(|l| l.strip_prefix("RESULT "))
                .unwrap_or_else(|| panic!("no result from godot:\n{text}"));
            serde_json::from_str(line).unwrap()
        };
        let first = result(&godot_run(&binary, &project, &["--script", "res://check.gd"]));
        let anims = &first["animations"];
        let idle = &anims["idle"];
        assert_eq!(
            (idle["speed"].as_f64(), idle["loop"].as_bool()),
            (Some(12.0), Some(true))
        );
        let durations = |a: &Value| -> Vec<f64> {
            a["frames"]
                .as_array()
                .unwrap()
                .iter()
                .map(|f| f[0].as_f64().unwrap())
                .collect()
        };
        assert_eq!(durations(idle), [3.0, 1.0]);
        let hop_anim = &anims["hop"];
        assert_eq!(
            (hop_anim["speed"].as_f64(), hop_anim["loop"].as_bool()),
            (Some(24.0), Some(false))
        );
        assert_eq!(durations(hop_anim), [1.0, 2.0, 3.0]);
        assert!(anims.get("default").is_none());
        hop_anim["frames"].as_array().unwrap().iter().for_each(|f| {
            assert_eq!(
                (f[1].as_f64(), f[2].as_f64()),
                (Some(128.0), Some(96.0)),
                "full canvas size at 2x"
            );
        });
        let idle_first = &idle["frames"][0];
        let hop_last = &hop_anim["frames"][2];
        assert_eq!(
            idle_first.as_array().unwrap()[3..],
            hop_last.as_array().unwrap()[3..],
            "a link shares its picture"
        );
        assert_eq!(
            first["scene"],
            serde_json::json!(["AnimatedSprite2D", "idle", false, -40.0, -80.0])
        );
        let png = project.join("art/hop.png.import");
        let texture_uid = sprites::import_uid(&fs::read_to_string(&png).unwrap()).unwrap();
        let bundle = Bundle::open(&path).unwrap();
        let moved = marks(vec![
            Stroke::new(1, "ink", [0.1, 0.3, 0.9, 1.0], 12.0, 1).with_points(&[(40.0, 10.0, 1.0)], 0.0),
        ]);
        bundle.write_marks("l1", "c3", &moved).unwrap();
        godot(&path, None, false, &look(2.0), &PACKING, &Extras::default()).unwrap();
        assert_eq!((uids(&tres), uids(&tscn)), (frames_uid.clone(), scene_uid.clone()));
        let text = fs::read_to_string(&tres).unwrap();
        assert!(
            text.contains(&format!("uid=\"{texture_uid}\" path=\"hop.png\"")),
            "{text}"
        );
        godot_run(&binary, &project, &["--import"]);
        let second = result(&godot_run(
            &binary,
            &project,
            &["--script", "res://check.gd", "--", &frames_uid, &scene_uid],
        ));
        assert_eq!(
            second["uids"],
            serde_json::json!(["res://art/hop.tres", "res://art/hop.tscn"]),
            "the uids Godot knew still load the files"
        );
        assert_ne!(
            second["animations"]["hop"]["frames"][0],
            first["animations"]["hop"]["frames"][0]
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn sheets_and_frames_land_next_to_the_bundle_and_add_up_to_each_clip() {
        let dir = std::env::temp_dir().join(format!("draw-sprites-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = hop(&dir);
        sheet(&path, "all", &look(1.0), &PACKING, None).unwrap();
        let json: Value = serde_json::from_str(&fs::read_to_string(dir.join("hop-sheet.json")).unwrap()).unwrap();
        let frames = json["frames"].as_array().unwrap();
        let tags = json["meta"]["frameTags"].as_array().unwrap();
        let ms = |tag: &Value| -> u64 {
            let (a, b) = (
                tag["from"].as_u64().unwrap() as usize,
                tag["to"].as_u64().unwrap() as usize,
            );
            frames[a..=b].iter().map(|f| f["duration"].as_u64().unwrap()).sum()
        };
        assert_eq!(
            (tags[0]["name"].as_str(), ms(&tags[0])),
            (Some("idle"), 333),
            "4 frames at 12 fps"
        );
        assert_eq!(
            (tags[1]["name"].as_str(), ms(&tags[1])),
            (Some("hop"), 250),
            "6 frames at 24 fps"
        );
        assert_eq!(frames.len(), 5);
        let rects: BTreeSet<String> = frames.iter().map(|f| f["frame"].to_string()).collect();
        assert_eq!(rects.len(), 4, "the link back to c1 reuses its picture");
        assert!(
            frames
                .iter()
                .all(|f| f["trimmed"] == true && f["sourceSize"] == serde_json::json!({"w": 64, "h": 48}))
        );
        let png = fs::read(dir.join("hop-sheet.png")).unwrap();
        sheet(&path, "all", &look(1.0), &PACKING, None).unwrap();
        assert_eq!(
            fs::read(dir.join("hop-sheet.png")).unwrap(),
            png,
            "exports repeat exactly"
        );
        sheet(&path, "hop", &look(0.5), &PACKING, None).unwrap();
        assert!(dir.join("hop-hop@0.5x.png").exists() && dir.join("hop-hop@0.5x.json").exists());
        frames_out(&path, &dir);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_clip_moved_by_keys_exports_a_picture_per_pose() {
        let dir = std::env::temp_dir().join(format!("draw-sprites-keys-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = hop(&dir);
        let mut bundle = Bundle::open(&path).unwrap();
        let key = |frame: u32, value: f32| klad_core::keys::Key {
            frame,
            value,
            ease: klad_core::keys::Ease::Linear,
        };
        [key(0, 0.0), key(2, 12.0), key(3, 12.0)]
            .into_iter()
            .for_each(|k| klad_core::keys::set(&mut bundle.doc.layers[0].keys, "x", k).unwrap());
        bundle.save().unwrap();
        frames(&path, "idle", &look(1.0), None).unwrap();
        let read = |n: usize| fs::read(dir.join("hop-idle").join(format!("idle-{n:04}.png"))).unwrap();
        assert_ne!(read(0), read(1), "one drawing held, moved by a key");
        assert_ne!(read(1), read(2));
        sheet(&path, "idle", &look(1.0), &PACKING, None).unwrap();
        let json: Value = serde_json::from_str(&fs::read_to_string(dir.join("hop-idle.json")).unwrap()).unwrap();
        let rects: BTreeSet<String> = json["frames"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["frame"].to_string() + &f["spriteSourceSize"].to_string())
            .collect();
        assert_eq!(
            rects.len(),
            4,
            "four poses: three of the held drawing and the next drawing"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    fn frames_out(path: &Path, dir: &Path) {
        frames(path, "hop", &look(1.0), None).unwrap();
        let out = dir.join("hop-hop");
        let names: Vec<String> = fs::read_dir(&out)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        assert_eq!(names.len(), 6);
        assert_eq!((names[0].as_str(), names[5].as_str()), ("hop-0000.png", "hop-0005.png"));
        let read = |n: usize| fs::read(out.join(&names[n])).unwrap();
        assert_eq!(read(1), read(2), "a held frame is the same picture");
        assert_ne!(read(0), read(1));
        assert_eq!(u32::from_be_bytes(read(0)[16..20].try_into().unwrap()), 64);
    }

    #[test]
    fn relative_folders_walk_up_and_down() {
        let rel = |a: &str, b: &str| kept_folder(Path::new(a), Path::new(b));
        assert_eq!(rel("/work/art", "/work/game/sprites"), "../game/sprites");
        assert_eq!(rel("/work/art", "/work/art/godot"), "godot");
        assert_eq!(rel("/work/art", "/work/art"), ".");
        assert_eq!(rel("/a/b/c", "/x"), "../../../x");
    }

    const LIT: &str = r#"extends SceneTree

func _init():
	var out := {}
	var frames: SpriteFrames = load("res://art/hop.tres")
	var t: AtlasTexture = frames.get_frame_texture("hop", 0)
	var canvas: CanvasTexture = t.atlas
	out["atlas"] = t.atlas.get_class()
	out["maps"] = [canvas.normal_texture.get_width() == canvas.diffuse_texture.get_width(), canvas.specular_texture != null, snapped(canvas.specular_shininess, 0.001)]
	var sprite: AnimatedSprite2D = load("res://art/hop.tscn").instantiate()
	root.add_child(sprite)
	var glow: AnimatedSprite2D = sprite.get_node("glow")
	out["glow"] = [glow.material.light_mode, glow.sprite_frames.get_frame_count("hop"), glow.offset == sprite.offset]
	var shadows = sprite.get_node("shadows")
	var first: OccluderPolygon2D = shadows.shapes[0][0]
	out["shadows"] = [shadows.get_child(0) is LightOccluder2D, shadows.shapes.size(), first.polygon.size() > 2, first.cull_mode]
	var shader: Shader = sprite.material.shader
	out["shader"] = shader.get_shader_uniform_list().size()
	out["params"] = sprite.material.get_shader_parameter("depth_map") != null
	var lit = load("res://art/hop_lit.tscn").instantiate()
	var kinds := []
	for c in lit.get_children():
		kinds.append(c.get_class())
	out["lit"] = kinds
	var light: PointLight2D = lit.get_child(3)
	out["soft"] = [light.shadow_enabled, light.shadow_filter, light.shadow_filter_smooth]
	var backdrop: ColorRect = lit.get_child(1)
	out["backdrop"] = [backdrop.size.x, backdrop.position.x]
	root.add_child(lit)
	var pointer: PointLight2D = lit.get_child(3)
	var holder: Node2D = lit.get_child(2).get_node("shadows")
	var occluder: LightOccluder2D = holder.get_child(0)
	occluder.occluder = holder.shapes[0][0]
	var poly: PackedVector2Array = occluder.occluder.polygon
	var mid := Vector2.ZERO
	for p in poly:
		mid += p
	mid /= poly.size()
	var probe := mid
	for p in poly:
		var c := p.lerp(mid, 0.1)
		if Geometry2D.is_point_in_polygon(c, poly):
			probe = c
			break
	pointer.global_position = occluder.get_global_transform() * probe
	var hit: bool = pointer._inside()
	pointer.global_position = Vector2(-100000, -100000)
	out["inside"] = [pointer.get_node_or_null(pointer.shadows) != null, hit, pointer._inside()]
	lit.free()
	print("RESULT ", JSON.stringify(out))
	quit()
"#;

    #[test]
    fn godot_lights_the_sprite_through_canvas_textures_with_glow_shadows_and_shaders() {
        let dir = std::env::temp_dir().join(format!("draw-godot-lit-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let project = dir.join("game");
        fs::create_dir_all(project.join("art")).unwrap();
        let path = hop(&dir);
        let mut bundle = Bundle::open(&path).unwrap();
        bundle
            .apply(
                &klad_core::ops::parse_lines(
                    r##"{"op":"maps","surface":{"bevel":4},"lights":[{"at":[10,10],"height":40}]}
{"op":"add","layer":{"id":"lamp","kind":"vector","name":"lamp","emissive":true,"cels":[{"frame":0,"nodes":[{"id":"n1","type":"ellipse","cx":50,"cy":10,"rx":5,"ry":5,"fill":"#ffd040"}]}]}}
{"op":"set","id":"l1","shine":0.5}"##,
                )
                .unwrap(),
            )
            .unwrap();
        let extras = Extras {
            occluders: Some(1.5),
            shaders: Effect::list("all").unwrap(),
        };
        let shown = |shaders: bool| Extras {
            shaders: if shaders { extras.shaders.clone() } else { Vec::new() },
            ..extras.clone()
        };
        godot(
            &path,
            Some(project.join("art")),
            false,
            &look(1.0),
            &PACKING,
            &shown(true),
        )
        .unwrap();
        let art = project.join("art");
        [
            "hop_n.png",
            "hop_s.png",
            "hop_e.png",
            "hop_d.png",
            "hop_h.png",
            "hop_o.png",
            "hop-glow.tres",
            "hop_lit.tscn",
            "hop-maps.gdshader",
        ]
        .iter()
        .for_each(|f| assert!(art.join(f).exists(), "{f} is written"));
        let normal = fs::read(art.join("hop_n.png")).unwrap();
        assert!(
            !normal.windows(4).any(|w| w == b"sRGB"),
            "a normal map is data, with no colour profile"
        );
        assert!(fs::read(art.join("hop.png")).unwrap().windows(4).any(|w| w == b"sRGB"));
        let depth = fs::read(art.join("hop_d.png")).unwrap();
        assert_eq!((depth[24], depth[25]), (16, 4), "depth is 16-bit grey with alpha");
        let tres = fs::read_to_string(art.join("hop.tres")).unwrap();
        assert!(tres.contains("[sub_resource type=\"CanvasTexture\" id=\"CanvasTexture_1\"]"));
        assert!(tres.contains("atlas = SubResource(\"CanvasTexture_1\")"));
        let uids = |f: &str| sprites::header_uid(&fs::read_to_string(art.join(f)).unwrap()).unwrap();
        let before = (uids("hop-glow.tres"), uids("hop_lit.tscn"));
        godot(&path, None, false, &look(1.0), &PACKING, &shown(true)).unwrap();
        assert_eq!(
            (uids("hop-glow.tres"), uids("hop_lit.tscn")),
            before,
            "re-exports keep their uids"
        );
        let Some(binary) = godot_binary() else {
            eprintln!("no godot on PATH (or $GODOT): skipping the import check");
            fs::remove_dir_all(&dir).unwrap();
            return;
        };
        fs::write(
            project.join("project.godot"),
            "config_version=5\n\n[application]\n\nconfig/name=\"klad lit\"\n",
        )
        .unwrap();
        fs::write(project.join("lit.gd"), LIT).unwrap();
        godot_run(&binary, &project, &["--import"]);
        let text = godot_run(&binary, &project, &["--script", "res://lit.gd"]);
        assert!(!text.contains("SHADER ERROR"), "the shader compiles:\n{text}");
        let line = text
            .lines()
            .find_map(|l| l.strip_prefix("RESULT "))
            .unwrap_or_else(|| panic!("no result from godot:\n{text}"));
        let got: Value = serde_json::from_str(line).unwrap();
        assert_eq!(got["atlas"], "CanvasTexture", "{got}");
        assert_eq!(got["maps"], serde_json::json!([true, true, 0.372]));
        assert_eq!(
            got["glow"],
            serde_json::json!([1, 3, true]),
            "an unshaded glow child plays the same frames"
        );
        assert_eq!(got["shadows"][0], true);
        assert_eq!(got["shadows"][2], true);
        assert_eq!(
            got["shadows"][3], 2,
            "only the edges facing away cast, so the sprite does not shadow itself"
        );
        assert!(got["shader"].as_u64().unwrap() >= 10, "{got}");
        assert_eq!(got["params"], true);
        assert_eq!(
            got["lit"],
            serde_json::json!([
                "CanvasModulate",
                "ColorRect",
                "AnimatedSprite2D",
                "PointLight2D",
                "Camera2D"
            ])
        );
        let canvas_width = Bundle::open(&path).unwrap().doc.canvas.width as f64;
        assert_eq!(
            (got["backdrop"][0].as_f64(), got["backdrop"][1].as_f64()),
            (Some(3.0 * canvas_width), Some(-canvas_width)),
            "a backdrop three canvases wide behind the sprite catches the light and the shadows"
        );
        assert_eq!(
            got["inside"],
            serde_json::json!([true, true, false]),
            "the mouse light knows when it is inside the sprite, and drops the sprite's shadow there"
        );
        assert_eq!(
            (
                got["soft"][0].as_bool(),
                got["soft"][1].as_u64(),
                got["soft"][2].as_f64()
            ),
            (Some(true), Some(1), Some(1.0)),
            "the preview lights cast soft shadows that still start at the outline (PCF5, smoothed once)"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn sheets_and_frames_carry_their_maps_in_the_same_places() {
        let dir = std::env::temp_dir().join(format!("draw-sprites-maps-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = hop(&dir);
        let mut bundle = Bundle::open(&path).unwrap();
        bundle
            .apply(&klad_core::ops::parse_lines(r#"{"op":"maps","surface":{"bevel":3}}"#).unwrap())
            .unwrap();
        let mapped = Look {
            maps: Map::list("normal,depth,emission").unwrap(),
            ..look(1.0)
        };
        sheet(&path, "all", &mapped, &PACKING, None).unwrap();
        let read = |f: &str| {
            let bytes = fs::read(dir.join(f)).unwrap();
            let size = (
                u32::from_be_bytes(bytes[16..20].try_into().unwrap()),
                u32::from_be_bytes(bytes[20..24].try_into().unwrap()),
            );
            (bytes, size)
        };
        let (colour, size) = read("hop-sheet.png");
        let (normal, normal_size) = read("hop-sheet_n.png");
        assert_eq!(size, normal_size, "the map page is the colour page's size");
        let decode = |bytes: &[u8]| klad_render::assets::decode(bytes).unwrap();
        let (c, n) = (decode(&colour), decode(&normal));
        let covered = |p: &klad_render::assets::Rgba, i: usize| p.pixels[i * 4 + 3] > 0;
        let same = (0..(size.0 * size.1) as usize)
            .filter(|i| covered(&c, *i) == covered(&n, *i))
            .count();
        assert!(
            same as f32 > 0.99 * (size.0 * size.1) as f32,
            "every picture sits in the same place on both pages"
        );
        let json: Value = serde_json::from_str(&fs::read_to_string(dir.join("hop-sheet.json")).unwrap()).unwrap();
        assert_eq!(json["meta"]["maps"]["normal"], serde_json::json!(["hop-sheet_n.png"]));
        frames(&path, "hop", &mapped, None).unwrap();
        assert!(dir.join("hop-hop/hop-0000_n.png").exists() && dir.join("hop-hop/hop-0005_d.png").exists());
        let flat = Look {
            layer: Some("ball".into()),
            ..mapped
        };
        assert!(
            sheet(&path, "all", &flat, &PACKING, None).is_err(),
            "maps are for the whole picture"
        );
        fs::remove_dir_all(&dir).unwrap();
    }
}
