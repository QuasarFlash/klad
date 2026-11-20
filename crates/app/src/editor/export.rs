use super::Editor;
use super::clips::resolve;
use crate::jobs::{Spec, Tag};
use crate::picker;
use crate::places::tilde;
use crate::theme::{self, Hint};
use anyhow::{Result, bail};
use egui::RichText;
use klad_render::maps::Map;
use klad_render::shaders::Effect;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum What {
    #[default]
    Image,
    Frames,
    Sheet,
    Gif,
    Aseprite,
    Godot,
    Svg,
    Pdf,
    Video,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Picture {
    #[default]
    Png,
    Jpeg,
    Webp,
    Tiff,
    Psd,
}

impl Picture {
    pub(super) const ALL: [Picture; 5] = [Picture::Png, Picture::Jpeg, Picture::Webp, Picture::Tiff, Picture::Psd];

    pub(super) fn name(self) -> &'static str {
        match self {
            Picture::Png => "PNG",
            Picture::Jpeg => "JPEG",
            Picture::Webp => "WebP",
            Picture::Tiff => "TIFF",
            Picture::Psd => "PSD",
        }
    }

    fn arg(self) -> &'static str {
        match self {
            Picture::Png => "png",
            Picture::Jpeg => "jpeg",
            Picture::Webp => "webp",
            Picture::Tiff => "tiff",
            Picture::Psd => "psd",
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Picture::Png => "png",
            Picture::Jpeg => "jpg",
            Picture::Webp => "webp",
            Picture::Tiff => "tif",
            Picture::Psd => "psd",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Picture::Png => "exact pixels and transparency; the usual choice",
            Picture::Jpeg => {
                "small files for photos and the web; no transparency, a white or the canvas colour shows there"
            }
            Picture::Webp => "small files with transparency, for the web; lossless keeps every pixel",
            Picture::Tiff => "for print and photo apps, 8 or 16 bits, with transparency",
            Picture::Psd => "for Photoshop, Krita and Affinity: a layer for each layer, with its name, look and groups",
        }
    }

    fn squeezes(self, lossless: bool) -> bool {
        self == Picture::Jpeg || (self == Picture::Webp && !lossless)
    }
}

pub(super) const PAPERS: [(&str, &str); 7] = [
    ("", "the drawing's size"),
    ("a4", "A4"),
    ("a3", "A3"),
    ("a5", "A5"),
    ("letter", "Letter"),
    ("legal", "Legal"),
    ("tabloid", "Tabloid"),
];

impl What {
    pub(super) fn title(self) -> &'static str {
        match self {
            What::Image => "Export image",
            What::Frames => "Export frames",
            What::Sheet => "Export sprite sheet",
            What::Gif => "Export GIF",
            What::Aseprite => "Export Aseprite file",
            What::Godot => "Export for Godot",
            What::Svg => "Export SVG",
            What::Pdf => "Export PDF",
            What::Video => "Export video",
        }
    }

    fn about(self) -> &'static str {
        match self {
            What::Image => "The frame on screen as one PNG.",
            What::Frames => "One PNG per frame, numbered, in a folder.",
            What::Sheet => {
                "Every distinct picture once on sheet pages, with a JSON file that says where each frame is."
            }
            What::Gif => "An animated GIF: one for each clip, or one of the frames you name.",
            What::Aseprite => {
                "An .aseprite file of the pixel layers and groups, with every frame's drawing, the palette and the clips as tags."
            }
            What::Godot => "Files a Godot project uses as they are, written into a folder of that project.",
            What::Svg => {
                "The frame on screen as an SVG: vector layers as shapes an illustration app can edit, each layer a group."
            }
            What::Pdf => {
                "A PDF to print or send: vector layers stay shapes, sharp at any size; paint and pixel layers go in as pictures. A page for the frame on screen, or one per frame or clip."
            }
            What::Video => {
                "An MP4 video of a clip or the timeline at its speed (AV1, so it plays in browsers, VLC and on newer Macs and phones). Transparency becomes white or the canvas colour."
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Godot {
    #[default]
    Sprite,
    Scene,
    Rig,
    Tiles,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Written {
    pub first: PathBuf,
    pub more: usize,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Form {
    pub what: What,
    pub layer: Option<String>,
    pub scale: f32,
    pub background: bool,
    pub deep: bool,
    pub dither: bool,
    pub variant: String,
    pub range: String,
    pub padding: u32,
    pub max: u32,
    pub folder: String,
    pub name: String,
    pub maps: Vec<Map>,
    pub occluders: bool,
    pub shaders: Vec<Effect>,
    pub godot: Godot,
    pub last: Option<Written>,
    pub typed: Vec<(What, String, String)>,
    pub pictures: bool,
    pub outlines: bool,
    pub only: Vec<String>,
    pub picture: Picture,
    pub quality: f32,
    pub lossless: bool,
    pub webp: bool,
    pub paper: String,
    pub landscape: bool,
    pub dpi: f32,
    pub pages: String,
}

impl Form {
    pub(super) fn extension(&self) -> Option<&'static str> {
        match self.what {
            What::Image => Some(self.picture.extension()),
            What::Sheet => Some("png"),
            What::Gif => Some(if self.webp { "webp" } else { "gif" }),
            What::Aseprite => Some("aseprite"),
            What::Svg => Some("svg"),
            What::Pdf => Some("pdf"),
            What::Video => Some("mp4"),
            What::Frames | What::Godot => None,
        }
    }
}

impl Default for Form {
    fn default() -> Self {
        Self {
            what: What::Image,
            layer: None,
            scale: 1.0,
            background: false,
            deep: false,
            dither: false,
            variant: String::new(),
            range: "all".into(),
            padding: 2,
            max: 4096,
            folder: String::new(),
            name: String::new(),
            maps: Vec::new(),
            occluders: false,
            shaders: Vec::new(),
            godot: Godot::Sprite,
            last: None,
            typed: Vec::new(),
            pictures: true,
            outlines: false,
            only: Vec::new(),
            picture: Picture::Png,
            quality: 90.0,
            lossless: false,
            webp: false,
            paper: String::new(),
            landscape: false,
            dpi: 72.0,
            pages: String::new(),
        }
    }
}

impl Form {
    fn switch_to(&mut self, what: What) {
        if what == self.what {
            return;
        }
        let was = (
            self.what,
            std::mem::take(&mut self.name),
            std::mem::take(&mut self.range),
        );
        self.typed.retain(|(kind, ..)| *kind != was.0);
        self.typed.push(was);
        let (name, range) = self.typed.iter().find(|(kind, ..)| *kind == what).map_or_else(
            || (String::new(), Form::default().range),
            |(_, n, r)| (n.clone(), r.clone()),
        );
        self.what = what;
        self.name = name;
        self.range = range;
    }
}

fn listed<T>(items: &[T], name: impl Fn(&T) -> &'static str) -> String {
    items.iter().map(name).collect::<Vec<_>>().join(",")
}

pub(super) fn godot_extras(form: &Form) -> Vec<String> {
    let mut out = Vec::new();
    if form.occluders {
        out.push("--occluders".to_string());
    }
    if !form.shaders.is_empty() {
        out.push("--shaders".to_string());
        out.push(listed(&form.shaders, |e| e.name()));
    }
    out
}

fn toggled<T: PartialEq + Copy + Ord>(list: &mut Vec<T>, item: T, on: bool) {
    list.retain(|x| *x != item);
    if on {
        list.push(item);
        list.sort();
    }
}

pub(super) const EVERY: &str = "*";

pub(super) fn destination(home: &Path, form: &Form) -> Option<PathBuf> {
    let (folder, name) = (form.folder.trim(), form.name.trim());
    let base = match folder.is_empty() {
        true => home.to_path_buf(),
        false => resolve(home, folder),
    };
    match (name.is_empty(), form.extension()) {
        (true, _) if folder.is_empty() => None,
        (true, _) => Some(base),
        (false, None) => Some(base.join(name)),
        (false, Some(extension)) => {
            let file = PathBuf::from(name);
            Some(base.join(match file.extension() {
                Some(_) => file,
                None => file.with_extension(extension),
            }))
        }
    }
}

fn folder_of(form: &Form, target: &Path) -> PathBuf {
    match (form.extension(), target.extension()) {
        (Some(_), Some(_)) => target.parent().map_or_else(|| target.to_path_buf(), Path::to_path_buf),
        _ => target.to_path_buf(),
    }
}

pub(super) fn args(bundle: &Path, home: &Path, frame: u32, form: &Form) -> Vec<String> {
    let mut args = vec!["export".to_string(), bundle.to_string_lossy().to_string()];
    let mut push = |a: &str, b: Option<String>| {
        args.push(a.to_string());
        args.extend(b);
    };
    match form.what {
        What::Godot => push("--frame", Some(frame.to_string())),
        What::Image => {
            push("--frame", Some(frame.to_string()));
            if form.picture != Picture::Png {
                push("--format", Some(form.picture.arg().into()));
            }
            if form.picture == Picture::Webp && form.lossless {
                push("--lossless", None);
            }
            if form.picture.squeezes(form.lossless) && form.quality != 90.0 {
                push("--quality", Some(form.quality.round().to_string()));
            }
            if !form.only.is_empty() && form.picture != Picture::Psd {
                push("--only", Some(form.only.join(",")));
            }
        }
        What::Frames => push("--frames", Some(form.range.trim().to_string())),
        What::Sheet => push("--sheet", Some(form.range.trim().to_string())),
        What::Gif if form.webp => {
            push("--format", Some("webp".into()));
            push("--animated", Some(form.range.trim().to_string()));
            if form.lossless {
                push("--lossless", None);
            } else if form.quality != 90.0 {
                push("--quality", Some(form.quality.round().to_string()));
            }
        }
        What::Gif => push("--gif", Some(form.range.trim().to_string())),
        What::Pdf => {
            push("--format", Some("pdf".into()));
            match form.pages.trim() {
                "" => push("--frame", Some(frame.to_string())),
                pages => push("--pages", Some(pages.to_string())),
            }
            match (form.paper.as_str(), form.landscape) {
                ("", _) if form.dpi != 72.0 => push("--dpi", Some(form.dpi.round().to_string())),
                ("", _) => {}
                (paper, true) => push("--paper", Some(format!("{paper},landscape"))),
                (paper, false) => push("--paper", Some(paper.to_string())),
            }
        }
        What::Video => {
            push("--format", Some("mp4".into()));
            push("--animated", Some(form.range.trim().to_string()));
            if form.quality != 80.0 {
                push("--quality", Some(form.quality.round().to_string()));
            }
        }
        What::Aseprite => push("--aseprite", None),
        What::Svg => {
            push("--svg", None);
            push("--frame", Some(frame.to_string()));
            if !form.pictures {
                push("--vectors-only", None);
            }
            if form.outlines {
                push("--outline-text", None);
            }
            if !form.only.is_empty() {
                push("--only", Some(form.only.join(",")));
            }
        }
    }
    if let Some(layer) = form.layer.as_ref().filter(|_| {
        !matches!(form.what, What::Frames | What::Aseprite | What::Pdf)
            && form.only.is_empty()
            && !(form.what == What::Image && form.picture == Picture::Psd)
    }) {
        push("--layer", Some(layer.clone()));
    }
    let layered = form.what == What::Image && form.picture == Picture::Psd;
    if form.scale != 1.0 && !matches!(form.what, What::Aseprite | What::Svg | What::Pdf) && !layered {
        push("--scale", Some(form.scale.to_string()));
    }
    if form.deep
        && (form.what == What::Sheet
            || (form.what == What::Image && matches!(form.picture, Picture::Png | Picture::Tiff)))
    {
        push("--depth", Some("16".into()));
    }
    if form.background && matches!(form.what, What::Image | What::Gif | What::Pdf | What::Video) {
        push("--background", None);
    }
    if form.dither && form.what == What::Gif && !form.webp {
        push("--dither", None);
    }
    match form.variant.as_str() {
        "" => {}
        EVERY => push("--variants", None),
        name => push("--variant", Some(name.to_string())),
    }
    if form.what == What::Sheet {
        push("--padding", Some(form.padding.to_string()));
        push("--max", Some(form.max.to_string()));
    }
    let png = form.what != What::Image || form.picture == Picture::Png;
    if !form.maps.is_empty()
        && form.layer.is_none()
        && png
        && form.only.is_empty()
        && matches!(form.what, What::Image | What::Frames | What::Sheet)
    {
        push("--maps", Some(listed(&form.maps, |m| m.name())));
    }
    if let Some(target) = destination(home, form) {
        push("-o", Some(target.to_string_lossy().to_string()));
    }
    args
}

pub(super) fn found(stdout: &str, stderr: &str) -> Option<Written> {
    let paths: Vec<&str> = stdout.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let first = paths.first()?;
    Some(Written {
        first: PathBuf::from(first),
        more: paths.len() - 1,
        notes: stderr
            .lines()
            .map(str::trim)
            .filter(|l| l.starts_with("note: "))
            .map(str::to_string)
            .collect(),
    })
}

pub(super) fn said(done: &Written) -> String {
    let more = match done.more {
        0 => String::new(),
        1 => " and one more file beside it".to_string(),
        n => format!(" and {n} more files beside it"),
    };
    let notes = match done.notes.is_empty() {
        true => String::new(),
        false => format!(" ({})", done.notes.join("; ")),
    };
    format!("exported to {}{more}{notes}", tilde(&done.first))
}

fn written(out: &str, err: &str) -> String {
    found(out, err).map_or_else(String::new, |done| said(&done))
}

pub(super) fn godot_written(out: &str, err: &str) -> String {
    let files = written(out, err);
    let names = err.lines().map(str::trim).find(|l| l.starts_with("animations:"));
    [Some(files.as_str()).filter(|f| !f.is_empty()), names]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("; ")
}

impl Editor {
    pub(super) fn open_export(&mut self, what: What) {
        let again = self.windows.export && self.export.what == what;
        self.export.switch_to(what);
        self.export.only = match what {
            What::Svg => self.shapes(),
            What::Image => self.selection_refs(),
            _ => Vec::new(),
        };
        if what == What::Video && self.export.quality == 90.0 {
            self.export.quality = 80.0;
        }
        self.windows.export = !again;
    }

    fn selection_refs(&self) -> Vec<String> {
        let shapes = self.shapes();
        if !shapes.is_empty() {
            return shapes;
        }
        self.picked.as_ref().map_or_else(Vec::new, |p| {
            p.ids
                .iter()
                .map(|id| format!("{}:s{id}", p.cel))
                .chain(
                    p.more
                        .iter()
                        .flat_map(|part| part.ids.iter().map(move |id| format!("{}:s{id}", part.cel))),
                )
                .collect()
        })
    }

    pub(super) fn exporting(&self, what: What) -> bool {
        self.windows.export && self.export.what == what
    }

    pub(super) fn exported(&mut self, stdout: &str, output: &str) {
        self.export.last = found(stdout, output);
    }

    pub(super) fn export(&mut self) -> Result<()> {
        let form = self.export.clone();
        if !matches!(form.what, What::Image | What::Aseprite | What::Svg | What::Pdf) && form.range.trim().is_empty() {
            bail!("say which frames: all, a clip's name or a range like 0-11");
        }
        let home = self.bundle_home();
        if let Some(target) = destination(&home, &form) {
            let folder = folder_of(&form, &target);
            std::fs::create_dir_all(&folder).map_err(|e| {
                anyhow::anyhow!(
                    "the folder {} could not be made: {}",
                    tilde(&folder),
                    crate::report::folder_words(&e)
                )
            })?;
        }
        let args = args(&self.scene.bundle.root, &home, self.scene.frame, &form);
        let name = match form.what {
            What::Image | What::Godot => format!("export frame {}", self.scene.frame),
            What::Frames => format!("export frames {}", form.range.trim()),
            What::Sheet => format!("export the sheet of {}", form.range.trim()),
            What::Gif if form.webp => format!("export an animated WebP of {}", form.range.trim()),
            What::Gif => format!("export a GIF of {}", form.range.trim()),
            What::Pdf => match form.pages.trim() {
                "" => format!("export a PDF of frame {}", self.scene.frame),
                pages => format!("export a PDF of {pages}"),
            },
            What::Video => format!("export a video of {}", form.range.trim()),
            What::Aseprite => "export an Aseprite file".to_string(),
            What::Svg => format!("export an SVG of frame {}", self.scene.frame),
        };
        self.start_job(Spec {
            tag: Tag::Export,
            name,
            exe: PathBuf::new(),
            args,
            env: Vec::new(),
            reruns: 3,
            summary: written,
            failure: |_, err| super::tweens::failure(err),
        });
        Ok(())
    }

    fn export_godot_page(&mut self, ui: &mut egui::Ui) {
        let doc = &self.scene.bundle.doc;
        let (lit, rigged, tiled) = (
            doc.maps.is_some(),
            doc.all_layers().iter().any(|l| !l.bones.is_empty()),
            doc.canvas.tile.is_some(),
        );
        let home = self.bundle_home();
        let mut run = false;
        let heading = |ui: &mut egui::Ui, text: &str| {
            ui.add_space(6.0);
            ui.label(RichText::new(text).color(theme::WEAK));
        };
        ui.horizontal(|ui| {
            let label = ui.label(RichText::new("Godot folder").color(theme::WEAK));
            ui.add(
                egui::TextEdit::singleline(&mut self.timeline.godot.1)
                    .desired_width(260.0)
                    .hint_text("a folder of the Godot project, e.g. ../game/art"),
            )
            .labelled_by(label.id)
            .hint("where the files go; a folder that is not a full path is taken from the folder holding the drawing, and the drawing remembers it");
            if picker::NATIVE
                && ui
                    .button("Choose…")
                    .hint("pick the folder in a file panel")
                    .clicked()
                && let Some(folder) = picker::choose_folder(Some(&home), "Export here")
            {
                self.timeline.godot.1 = folder.display().to_string();
            }
        });
        heading(ui, "what to write");
        {
            let form = &mut self.export;
            ui.radio_value(&mut form.godot, Godot::Sprite, "an animated sprite")
                .hint(
                    "the sprite sheet, SpriteFrames with an animation per clip, and a scene with an AnimatedSprite2D",
                );
            ui.radio_value(&mut form.godot, Godot::Scene, "a parallax scene")
                .hint("each top-level layer a texture with its fog, on a Parallax2D that scrolls by its depth");
            if ui
                .add_enabled(rigged, egui::RadioButton::new(form.godot == Godot::Rig, "a rig"))
                .hint("the rest drawing on Polygon2Ds weighted to a Skeleton2D, and the poses of every clip")
                .on_disabled_hover_text("a rig needs bones: draw them with the bone tool (J)")
                .clicked()
            {
                form.godot = Godot::Rig;
            }
            if ui
                .add_enabled(tiled, egui::RadioButton::new(form.godot == Godot::Tiles, "a tile set"))
                .hint("a Godot TileSet with a tile for every cell that is not empty")
                .on_disabled_hover_text("a tile set needs the drawing cut into tiles: View > Tileset: cut into tiles…")
                .clicked()
            {
                form.godot = Godot::Tiles;
            }
        }
        if self.export.godot != Godot::Tiles {
            heading(ui, "light");
            let form = &mut self.export;
            ui.checkbox(&mut form.occluders, "cast shadows")
                .hint("an occluder traced from the silhouette of every picture, so Godot's lights cast its shadow");
            Effect::ALL.iter().for_each(|e| {
                let mut on = form.shaders.contains(e);
                if ui
                    .add_enabled(lit, egui::Checkbox::new(&mut on, format!("{} shader", e.name())))
                    .hint(match e {
                        Effect::Fog => "a sample shader on the sprite: fog towards a colour by the depth map",
                        Effect::Rim => "a sample shader on the sprite: a rim light from the height map",
                        Effect::Occlusion => "a sample shader on the sprite: darker creases from the occlusion map",
                    })
                    .on_disabled_hover_text("the shaders read the light maps: turn them on in Window > Light maps")
                    .changed()
                {
                    toggled(&mut form.shaders, *e, on);
                }
            });
        }
        ui.add_space(6.0);
        let typed = self.godot_folder();
        let place = match typed.is_empty() {
            true => "choose the Godot folder first".to_string(),
            false => format!("goes into {}", tilde(&resolve(&home, &typed))),
        };
        ui.label(RichText::new(place).color(theme::WEAK));
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            run = ui
                .add_enabled(!typed.is_empty(), egui::Button::new("Export"))
                .hint("write the files, as klad export does")
                .on_disabled_hover_text("choose the Godot folder first")
                .clicked();
        });
        if run {
            match self.export.godot {
                Godot::Sprite => self.export_godot(),
                Godot::Scene => self.export_godot_as("--godot-scene"),
                Godot::Rig => self.export_godot_as("--godot-rig"),
                Godot::Tiles => self.export_godot_as("--tileset"),
            }
        }
    }

    pub(super) fn export_window(&mut self, ui: &mut egui::Ui) -> Result<()> {
        ui.set_min_width(460.0);
        ui.label(RichText::new(self.export.what.about()).color(theme::WEAK));
        ui.add_space(8.0);
        if self.export.what == What::Godot {
            self.export_godot_page(ui);
            self.export_result(ui);
            return Ok(());
        }
        let mut run = false;
        let layers: Vec<(String, String)> = self
            .scene
            .bundle
            .doc
            .all_layers()
            .into_iter()
            .map(|l| (l.id.clone(), l.name.clone()))
            .collect();
        let clips: Vec<String> = self
            .scene
            .bundle
            .doc
            .timeline
            .clips
            .iter()
            .map(|c| c.name.clone())
            .collect();
        let home = self.bundle_home();
        let doc = &self.scene.bundle.doc;
        let (pixel, most) = (doc.pixel_art(), klad_render::export::pixel_most(doc));
        let (width, height) = (doc.canvas.width, doc.canvas.height);
        let lit = doc.maps.is_some();
        let variants: Vec<String> = doc.variant_names().into_iter().map(String::from).collect();
        let form = &mut self.export;
        if form.variant != EVERY && !form.variant.is_empty() && !variants.contains(&form.variant) {
            form.variant.clear();
        }
        let what = form.what;
        let label = |ui: &mut egui::Ui, text: &str| ui.label(RichText::new(text).color(theme::WEAK));
        egui::Grid::new("export options")
            .num_columns(2)
            .spacing(egui::vec2(14.0, 10.0))
            .show(ui, |ui| {
                if what == What::Image {
                    label(ui, "format");
                    ui.horizontal_wrapped(|ui| {
                        Picture::ALL.iter().for_each(|p| {
                            if ui.selectable_label(form.picture == *p, p.name()).hint(p.hint()).clicked() {
                                form.picture = *p;
                            }
                        });
                    });
                    ui.end_row();
                }
                if what == What::Gif {
                    label(ui, "format");
                    ui.horizontal(|ui| {
                        if ui.selectable_label(!form.webp, "GIF").hint("plays everywhere; 255 colours").clicked() {
                            form.webp = false;
                        }
                        if ui
                            .selectable_label(form.webp, "WebP")
                            .hint("an animated WebP: full colour and soft edges, smaller, for the web")
                            .clicked()
                        {
                            form.webp = true;
                        }
                    });
                    ui.end_row();
                }
                if what == What::Pdf {
                    label(ui, "pages");
                    ui.horizontal_wrapped(|ui| {
                        [("", "this frame"), ("all", "every frame")]
                            .into_iter()
                            .chain((!clips.is_empty()).then_some(("clips", "a page per clip")))
                            .for_each(|(value, text)| {
                                if ui.selectable_label(form.pages == value, text).clicked() {
                                    form.pages = value.to_string();
                                }
                            });
                        clips.iter().for_each(|c| {
                            if ui.selectable_label(form.pages == *c, c).hint("a page for each frame of this clip").clicked() {
                                form.pages = c.clone();
                            }
                        });
                    });
                    ui.end_row();
                    label(ui, "paper");
                    ui.horizontal(|ui| {
                        let shown = PAPERS.iter().find(|(v, _)| *v == form.paper).map_or("the drawing's size", |(_, t)| t);
                        egui::ComboBox::from_id_salt("export paper")
                            .selected_text(shown)
                            .show_ui(ui, |ui| {
                                PAPERS.iter().for_each(|(value, text)| {
                                    ui.selectable_value(&mut form.paper, value.to_string(), *text);
                                });
                            })
                            .response
                            .hint("the page: the drawing's own size, or a paper with the picture fitted on it");
                        if !form.paper.is_empty() {
                            ui.checkbox(&mut form.landscape, "landscape");
                        }
                    });
                    ui.end_row();
                    if form.paper.is_empty() {
                        label(ui, "resolution");
                        ui.horizontal(|ui| {
                            ui.add(egui::Slider::new(&mut form.dpi, 36.0..=600.0).logarithmic(true).text("pixels per inch"))
                                .hint("how many of the drawing's pixels make an inch on the page; 72 makes a pixel a point");
                            ui.label(
                                RichText::new(format!(
                                    "{:.0} x {:.0} mm",
                                    width as f32 / form.dpi * 25.4,
                                    height as f32 / form.dpi * 25.4
                                ))
                                .color(theme::WEAK),
                            );
                        });
                        ui.end_row();
                    }
                }
                if !matches!(what, What::Image | What::Aseprite | What::Svg | What::Pdf) {
                    let frames = label(ui, "frames");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut form.range)
                                .desired_width(110.0)
                                .hint_text("all, a clip or 0-11"),
                        )
                        .labelled_by(frames.id)
                        .hint("which frames: all, a clip's name or a range");
                        clips.iter().for_each(|c| {
                            if ui.button(c).hint("export this clip's frames").clicked() {
                                form.range = c.clone();
                            }
                        });
                    });
                    ui.end_row();
                }
                let layered = what == What::Image && form.picture == Picture::Psd;
                if !matches!(what, What::Frames | What::Aseprite | What::Pdf) && !(what == What::Image && !form.only.is_empty()) && !layered {
                    label(ui, "what");
                    let shown = form
                        .layer
                        .as_ref()
                        .and_then(|id| layers.iter().find(|(l, _)| l == id))
                        .map_or("the whole image".to_string(), |(_, n)| n.clone());
                    egui::ComboBox::from_id_salt("export layer")
                        .selected_text(shown)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut form.layer, None, "the whole image");
                            layers.iter().for_each(|(id, name)| {
                                ui.selectable_value(&mut form.layer, Some(id.clone()), name);
                            });
                        })
                        .response
                        .hint("export the whole image or one layer or group");
                    ui.end_row();
                }
                if what == What::Svg {
                    label(ui, "options");
                    ui.vertical(|ui| {
                        ui.checkbox(&mut form.pictures, "paint and pixel layers as pictures")
                            .hint("off: only the vector layers go into the SVG");
                        ui.checkbox(&mut form.outlines, "text as outlines")
                            .hint("the letters as paths, so the SVG looks the same where the font is missing; off keeps text editable");
                        if !form.only.is_empty() {
                            let mut alone = true;
                            if ui
                                .checkbox(&mut alone, format!("only the {} selected shapes", form.only.len()))
                                .hint("off: the whole image or the layer chosen above")
                                .changed()
                            {
                                form.only.clear();
                            }
                        }
                    });
                    ui.end_row();
                }
                if what == What::Image && !form.only.is_empty() && !layered {
                    label(ui, "what");
                    let mut alone = true;
                    if ui
                        .checkbox(&mut alone, format!("only the selection ({} {})", form.only.len(), if form.only.len() == 1 { "thing" } else { "things" }))
                        .hint("the selected strokes or shapes alone, cut to them; off: the whole image")
                        .changed()
                    {
                        form.only.clear();
                    }
                    ui.end_row();
                }
                if !matches!(what, What::Aseprite | What::Svg | What::Pdf) && !layered {
                    label(ui, "size");
                    ui.horizontal(|ui| {
                        match pixel {
                            true => {
                                form.scale = form.scale.round().clamp(1.0, most);
                                ui.add(
                                    egui::Slider::new(&mut form.scale, 1.0..=most.min(32.0))
                                        .step_by(1.0)
                                        .fixed_decimals(0)
                                        .text("times real size"),
                                )
                                .hint("pixel art is enlarged by whole numbers only, so every pixel stays a sharp square");
                            }
                            false => {
                                ui.add(
                                    egui::Slider::new(&mut form.scale, 0.125..=4.0)
                                        .logarithmic(true)
                                        .text("scale"),
                                )
                                .hint("the size of the export against the canvas");
                            }
                        }
                        ui.label(
                            RichText::new(format!(
                                "{} x {} px",
                                (width as f32 * form.scale).round() as u32,
                                (height as f32 * form.scale).round() as u32
                            ))
                            .color(theme::WEAK),
                        );
                    });
                    ui.end_row();
                }
                if what == What::Sheet {
                    label(ui, "sheet");
                    ui.vertical(|ui| {
                        ui.add(egui::Slider::new(&mut form.padding, 0..=16).text("padding px"))
                            .hint("space between the cells of the sheet");
                        ui.add(egui::Slider::new(&mut form.max, 512..=8192).text("page size px"))
                            .hint("the largest sheet page; more frames go on more pages");
                    });
                    ui.end_row();
                }
                if matches!(what, What::Image | What::Sheet | What::Gif | What::Pdf | What::Video) {
                    label(ui, "options");
                    ui.vertical(|ui| {
                        let deep = what == What::Sheet || (what == What::Image && matches!(form.picture, Picture::Png | Picture::Tiff));
                        if deep {
                            ui.checkbox(&mut form.deep, "16 bits per channel")
                                .hint("16 bits per channel; 8-bit pictures are dithered so gradients do not band");
                        }
                        let webp = (what == What::Image && form.picture == Picture::Webp) || (what == What::Gif && form.webp);
                        if webp {
                            ui.checkbox(&mut form.lossless, "lossless")
                                .hint("every pixel exactly; off makes a smaller file that may soften fine detail");
                        }
                        let squeezed = (what == What::Image && form.picture.squeezes(form.lossless))
                            || (what == What::Gif && form.webp && !form.lossless)
                            || what == What::Video;
                        if squeezed {
                            ui.add(egui::Slider::new(&mut form.quality, 1.0..=100.0).fixed_decimals(0).text("quality"))
                                .hint("higher keeps more detail and makes a bigger file");
                        }
                        if matches!(what, What::Image | What::Gif | What::Pdf | What::Video) {
                            let off = match (what, form.picture) {
                                (What::Video, _) | (What::Image, Picture::Jpeg) => "off: white where the picture is transparent",
                                _ => "off: a transparent picture",
                            };
                            ui.checkbox(&mut form.background, "keep the background colour").hint(off);
                        }
                        if what == What::Gif && !pixel && !form.webp {
                            ui.checkbox(&mut form.dither, "dither")
                                .hint("a GIF holds 255 colours: mix them in a fine pattern so soft gradients do not band; off keeps flat areas of one colour");
                        }
                    });
                    ui.end_row();
                }
                if variants.len() > 1 {
                    label(ui, "palette variant");
                    let chosen = match form.variant.as_str() {
                        "" => "as shown".to_string(),
                        EVERY => "every variant".to_string(),
                        name => name.to_string(),
                    };
                    egui::ComboBox::from_id_salt("export variant")
                        .selected_text(chosen)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut form.variant, String::new(), "as shown");
                            ui.selectable_value(&mut form.variant, EVERY.to_string(), "every variant");
                            variants.iter().for_each(|name| {
                                ui.selectable_value(&mut form.variant, name.clone(), name);
                            });
                        })
                        .response
                        .hint("the colours to export in: what the drawing shows now, one variant, or a file for each (the variant's name is added to the file name)");
                    ui.end_row();
                }
                if lit && matches!(what, What::Image | What::Frames | What::Sheet) {
                    label(ui, "light maps");
                    ui.horizontal_wrapped(|ui| {
                        Map::ALL.iter().for_each(|m| {
                            let mut on = form.maps.contains(m);
                            if ui
                                .add_enabled(form.layer.is_none(), egui::Checkbox::new(&mut on, m.name()))
                                .hint(format!(
                                    "the {} map beside the picture, the same size, trim and layout, its file name ending in {}",
                                    m.name(),
                                    m.suffix()
                                ))
                                .on_disabled_hover_text("light maps are made for the whole image: export the whole image")
                                .changed()
                            {
                                toggled(&mut form.maps, *m, on);
                            }
                        });
                    });
                    ui.end_row();
                }
                let folder = label(ui, "folder");
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut form.folder)
                            .desired_width(260.0)
                            .hint_text(tilde(&home)),
                    )
                    .labelled_by(folder.id)
                    .hint("the folder to write into; empty writes next to the drawing");
                    if picker::NATIVE
                        && ui
                            .button("Choose…")
                            .hint("pick the folder in a file panel")
                            .clicked()
                    {
                        let start = match form.folder.trim().is_empty() {
                            true => home.clone(),
                            false => resolve(&home, &form.folder),
                        };
                        if let Some(folder) = picker::choose_folder(Some(&start), "Export here") {
                            form.folder = folder.display().to_string();
                        }
                    }
                });
                ui.end_row();
                let name = label(ui, "save as");
                let extension = form.extension();
                ui.add(
                    egui::TextEdit::singleline(&mut form.name)
                        .desired_width(260.0)
                        .hint_text(match extension {
                            Some(_) => "named after the drawing",
                            None => "a folder named after the drawing",
                        }),
                )
                .labelled_by(name.id)
                .hint(match extension {
                    Some(_) => "the file's name; empty names it after the drawing",
                    None => "the name of the folder the frames go into; empty names it after the drawing",
                });
                ui.end_row();
            });
        ui.add_space(6.0);
        let goes = match (destination(&home, form), form.name.trim().is_empty()) {
            (None, _) => format!("goes next to the drawing, in {}", tilde(&home)),
            (Some(target), true) => format!("goes into {}, named after the drawing", tilde(&target)),
            (Some(target), false) => format!("goes to {}", tilde(&target)),
        };
        ui.label(RichText::new(goes).color(theme::WEAK));
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            run = ui.button("Export").hint("write it, as klad export does").clicked();
        });
        if run {
            self.export()?;
        }
        self.export_result(ui);
        Ok(())
    }

    fn export_result(&mut self, ui: &mut egui::Ui) {
        let Some(done) = self.export.last.clone() else {
            return;
        };
        ui.add_space(4.0);
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            ui.label(said(&done));
            let folder = match done.first.is_dir() {
                true => Some(done.first.clone()),
                false => done.first.parent().map(Path::to_path_buf),
            };
            if let Some(folder) = folder
                && ui
                    .button("Show the folder")
                    .hint("open the folder it went to")
                    .clicked()
                && let Err(e) = crate::places::open_outside(&folder.to_string_lossy())
            {
                self.say(
                    crate::messages::Tone::Refused,
                    format!("could not open {}: {e}", tilde(&folder)),
                );
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_builds_the_cli_export() {
        let (bundle, home) = (Path::new("/w/art.klad"), Path::new("/w"));
        let image = Form {
            background: true,
            scale: 2.0,
            folder: "out".into(),
            name: "hero".into(),
            ..Form::default()
        };
        assert_eq!(
            args(bundle, home, 3, &image).join(" "),
            format!(
                "export /w/art.klad --frame 3 --scale 2 --background -o {}",
                home.join("out/hero.png").display()
            )
        );
        let jpeg = Form {
            picture: Picture::Jpeg,
            quality: 75.0,
            deep: true,
            name: "hero".into(),
            ..Form::default()
        };
        assert_eq!(
            args(bundle, home, 0, &jpeg).join(" "),
            format!(
                "export /w/art.klad --frame 0 --format jpeg --quality 75 -o {}",
                home.join("hero.jpg").display()
            ),
            "a JPEG has no 16 bits and takes its own extension"
        );
        let alone = Form {
            picture: Picture::Webp,
            lossless: true,
            only: vec!["c1:s4".into(), "c1:s9".into()],
            layer: Some("l2".into()),
            ..Form::default()
        };
        assert_eq!(
            args(bundle, home, 2, &alone).join(" "),
            "export /w/art.klad --frame 2 --format webp --lossless --only c1:s4,c1:s9"
        );
        let tiff = Form {
            picture: Picture::Tiff,
            deep: true,
            ..Form::default()
        };
        assert_eq!(
            args(bundle, home, 0, &tiff).join(" "),
            "export /w/art.klad --frame 0 --format tiff --depth 16"
        );
        let pdf = Form {
            what: What::Pdf,
            pages: "walk".into(),
            paper: "a4".into(),
            landscape: true,
            scale: 2.0,
            layer: Some("l2".into()),
            ..Form::default()
        };
        assert_eq!(
            args(bundle, home, 0, &pdf).join(" "),
            "export /w/art.klad --format pdf --pages walk --paper a4,landscape"
        );
        let print = Form {
            what: What::Pdf,
            dpi: 300.0,
            background: true,
            ..Form::default()
        };
        assert_eq!(
            args(bundle, home, 5, &print).join(" "),
            "export /w/art.klad --format pdf --frame 5 --dpi 300 --background"
        );
        let psd = Form {
            picture: Picture::Psd,
            scale: 2.0,
            layer: Some("l2".into()),
            only: vec!["n4".into()],
            background: true,
            ..Form::default()
        };
        assert_eq!(
            args(bundle, home, 1, &psd).join(" "),
            "export /w/art.klad --frame 1 --format psd --background",
            "a PSD is the whole drawing, layer by layer, at its size"
        );
        let video = Form {
            what: What::Video,
            range: "walk".into(),
            quality: 80.0,
            scale: 2.0,
            name: "walk".into(),
            ..Form::default()
        };
        assert_eq!(
            args(bundle, home, 0, &video).join(" "),
            format!(
                "export /w/art.klad --format mp4 --animated walk --scale 2 -o {}",
                home.join("walk.mp4").display()
            )
        );
        let moving = Form {
            what: What::Gif,
            webp: true,
            range: "all".into(),
            dither: true,
            ..Form::default()
        };
        assert_eq!(
            args(bundle, home, 0, &moving).join(" "),
            "export /w/art.klad --format webp --animated all"
        );
        let sheet = Form {
            what: What::Sheet,
            range: "walk".into(),
            layer: Some("l2".into()),
            deep: true,
            ..Form::default()
        };
        assert_eq!(
            args(bundle, home, 0, &sheet).join(" "),
            "export /w/art.klad --sheet walk --layer l2 --depth 16 --padding 2 --max 4096"
        );
        let frames = Form {
            what: What::Frames,
            range: "0-11".into(),
            layer: Some("l2".into()),
            scale: 0.5,
            ..Form::default()
        };
        assert_eq!(
            args(bundle, home, 0, &frames).join(" "),
            "export /w/art.klad --frames 0-11 --scale 0.5"
        );
        let gif = Form {
            what: What::Gif,
            range: "hop".into(),
            scale: 8.0,
            deep: true,
            background: true,
            dither: true,
            ..Form::default()
        };
        assert_eq!(
            args(bundle, home, 0, &gif).join(" "),
            "export /w/art.klad --gif hop --scale 8 --background --dither"
        );
        let file = Form {
            what: What::Aseprite,
            scale: 4.0,
            layer: Some("l2".into()),
            range: String::new(),
            variant: "blue".into(),
            ..Form::default()
        };
        assert_eq!(
            args(bundle, home, 0, &file).join(" "),
            "export /w/art.klad --aseprite --variant blue"
        );
        assert_eq!(
            written(
                "/w/art.aseprite\n",
                "16x12, 1 layers, 2 frames\nnote: left out, since an Aseprite file holds only pixel layers: notes\n"
            ),
            "exported to /w/art.aseprite (note: left out, since an Aseprite file holds only pixel layers: notes)"
        );
        let every = Form {
            what: What::Sheet,
            variant: EVERY.into(),
            ..Form::default()
        };
        assert_eq!(
            args(bundle, home, 0, &every).join(" "),
            "export /w/art.klad --sheet all --variants --padding 2 --max 4096"
        );
        let blue = Form {
            variant: "blue".into(),
            ..Form::default()
        };
        assert_eq!(
            args(bundle, home, 2, &blue).join(" "),
            "export /w/art.klad --frame 2 --variant blue"
        );
        let image = Form {
            dither: true,
            ..Form::default()
        };
        assert!(!args(bundle, home, 0, &image).contains(&"--dither".to_string()));
        assert_eq!(
            written(
                "/w/art-hop@8x.gif\n",
                "128x96  4 frames (2 pictures), 500 ms, loops  0.5 KB\n"
            ),
            "exported to /w/art-hop@8x.gif"
        );
        assert_eq!(
            written("/w/art-sheet.png\n/w/art-sheet-2.png\n/w/art-sheet.json\n", ""),
            "exported to /w/art-sheet.png and 2 more files beside it"
        );
        let named = |what, folder: &str, name: &str| Form {
            what,
            folder: folder.into(),
            name: name.into(),
            ..Form::default()
        };
        assert_eq!(destination(home, &named(What::Image, "", "")), None);
        assert_eq!(
            destination(home, &named(What::Image, "", "hero")),
            Some(home.join("hero.png")),
            "a name alone goes next to the drawing, with the kind's ending"
        );
        assert_eq!(
            destination(home, &named(What::Gif, "/out", "hop.gif")),
            Some(PathBuf::from("/out/hop.gif"))
        );
        assert_eq!(
            destination(home, &named(What::Image, "shots", "")),
            Some(home.join("shots")),
            "a folder alone takes the export under its usual name"
        );
        assert_eq!(
            destination(home, &named(What::Frames, "shots", "walk")),
            Some(home.join("shots/walk")),
            "frames go into a folder of that name"
        );
        let lit = Form {
            what: What::Sheet,
            maps: vec![Map::Normal, Map::Depth],
            ..Form::default()
        };
        assert_eq!(
            args(bundle, home, 0, &lit).join(" "),
            "export /w/art.klad --sheet all --padding 2 --max 4096 --maps normal,depth"
        );
        let one = Form {
            layer: Some("l2".into()),
            ..lit.clone()
        };
        assert!(
            !args(bundle, home, 0, &one).contains(&"--maps".to_string()),
            "maps go with the whole image"
        );
        let godot = Form {
            occluders: true,
            shaders: vec![Effect::Fog, Effect::Occlusion],
            ..Form::default()
        };
        assert_eq!(godot_extras(&godot).join(" "), "--occluders --shaders fog,occlusion");
        assert!(godot_extras(&Form::default()).is_empty());
    }

    #[test]
    fn each_kind_of_export_keeps_its_own_name_and_frames_and_shares_the_folder() {
        let mut form = Form {
            folder: "game".into(),
            name: "shadow".into(),
            ..Form::default()
        };
        form.switch_to(What::Sheet);
        assert_eq!((form.name.as_str(), form.range.as_str()), ("", "all"));
        form.range = "walk".into();
        form.switch_to(What::Frames);
        assert_eq!((form.name.as_str(), form.range.as_str()), ("", "all"));
        form.switch_to(What::Sheet);
        assert_eq!((form.name.as_str(), form.range.as_str()), ("", "walk"));
        form.switch_to(What::Image);
        assert_eq!((form.name.as_str(), form.folder.as_str()), ("shadow", "game"));
        form.switch_to(What::Image);
        assert_eq!(form.name, "shadow");
    }
}
