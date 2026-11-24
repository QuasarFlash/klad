use crate::picker;
use crate::settings::Settings;
use crate::theme::{self, icon};
use crate::widgets;
use anyhow::{Result, anyhow, bail};
use egui::{RichText, TextureHandle, vec2};
use klad_core::bundle::{Blank, Bundle};
use klad_core::samples::{self, Sample};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, SystemTime};

pub const PRESETS: [(&str, u32, u32); 5] = [
    ("square", 2048, 2048),
    ("screen", 1920, 1080),
    ("4K screen", 3840, 2160),
    ("sprite", 1024, 1024),
    ("large", 4096, 4096),
];

pub const PIXEL_PRESETS: [(&str, u32, u32); 5] = [
    ("icon", 16, 16),
    ("small sprite", 32, 32),
    ("sprite", 64, 64),
    ("large sprite", 128, 128),
    ("screen", 320, 180),
];

const THUMB: f32 = 168.0;
const PICTURE: f32 = THUMB * 0.75;
const TILE: f32 = THUMB + 2.0 * theme::space::SM;
const OPEN_WIDTH: f32 = 270.0;
const LABELS: f32 = 128.0;
const CHOOSE: f32 = 84.0;
const CHECK: f32 = 8.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    New,
    Open,
    Recent,
}

enum Card {
    Open(PathBuf),
    Forget,
    Duplicate(PathBuf),
    Trash(PathBuf),
    Idle,
}

struct Tile<'a> {
    key: &'a str,
    bundle: &'a Path,
    png: PathBuf,
    name: &'a str,
    caption: &'a str,
    lines: usize,
    there: bool,
}

enum Thumb {
    Waiting(Receiver<Option<egui::ColorImage>>),
    Ready(TextureHandle),
    Missing,
}

pub struct Start {
    pub name: String,
    pub folder: String,
    pub preset: Option<usize>,
    pub width: u32,
    pub height: u32,
    pub paper: bool,
    pub colour: [u8; 3],
    pub fps: u32,
    pub sketch: bool,
    pub pixel: bool,
    pub pixel_preset: Option<usize>,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub path: String,
    pub error: Option<String>,
    pub repair: Option<PathBuf>,
    pub repaired: Option<String>,
    pub section: Section,
    pub changed: bool,
    pub open: Option<PathBuf>,
    pub trash_to: Option<PathBuf>,
    pub samples: Vec<Sample>,
    pub tour: bool,
    pub connecting: bool,
    focused: bool,
    thumbs: BTreeMap<String, Thumb>,
    pub connect: crate::connect::Connect,
    cards_height: f32,
    checker: Option<TextureHandle>,
}

fn repaired_words(done: &klad_core::bundle::Repaired) -> String {
    let names: Vec<String> = done
        .renamed
        .iter()
        .map(|r| format!("{} \"{}\" ({} is now {})", r.what, r.name, r.from, r.to))
        .collect();
    format!("repaired ids used twice: {}", names.join(", "))
}

impl Start {
    pub fn new(settings: &Settings, section: Section) -> Self {
        let blank = Blank::default();
        let folder = settings.drawings_folder();
        Self {
            name: free_name(&folder),
            folder: folder.display().to_string(),
            preset: Some(0),
            width: blank.width,
            height: blank.height,
            paper: blank.background.is_some(),
            colour: [244, 239, 230],
            fps: blank.fps,
            sketch: true,
            pixel: false,
            pixel_preset: Some(2),
            pixel_width: 64,
            pixel_height: 64,
            path: String::new(),
            error: None,
            repair: None,
            repaired: None,
            section,
            changed: false,
            open: None,
            trash_to: None,
            samples: samples::shipped(),
            tour: false,
            connecting: false,
            connect: crate::connect::Connect::here(),
            focused: false,
            thumbs: BTreeMap::new(),
            cards_height: 0.0,
            checker: None,
        }
    }

    pub fn blank(&self) -> Blank {
        let (width, height) = self
            .preset
            .map_or((self.width, self.height), |i| (PRESETS[i].1, PRESETS[i].2));
        let [r, g, b] = self.colour;
        if self.pixel {
            let (width, height) = self.pixel_preset.map_or((self.pixel_width, self.pixel_height), |i| {
                (PIXEL_PRESETS[i].1, PIXEL_PRESETS[i].2)
            });
            return Blank {
                width,
                height,
                background: self.paper.then(|| format!("#{r:02x}{g:02x}{b:02x}")),
                fps: self.fps,
                pixel: true,
                palette: true,
                ..Blank::default()
            };
        }
        Blank {
            width,
            height,
            background: self.paper.then(|| format!("#{r:02x}{g:02x}{b:02x}")),
            fps: self.fps,
            sketch: self.sketch,
            ..Blank::default()
        }
    }

    pub fn target(&self) -> PathBuf {
        let name = self.name.trim();
        let file = match name.ends_with(".klad") {
            true => name.to_string(),
            false => format!("{name}.klad"),
        };
        expand(&self.folder).join(file)
    }

    pub fn create(&self, settings: &mut Settings) -> Result<PathBuf> {
        let name = self.name.trim();
        if name.is_empty() {
            bail!("give the drawing a name");
        }
        if name.contains('/') {
            bail!("a name cannot hold a /: pick the folder in the field below it");
        }
        let path = self.target();
        if path.exists() {
            bail!("{} already exists: pick another name", path.display());
        }
        std::fs::create_dir_all(expand(&self.folder))?;
        Bundle::blank(&path, &self.blank(), "editor")?;
        if expand(&self.folder) != settings.drawings_folder() {
            settings.drawings = Some(expand(&self.folder).display().to_string());
        }
        Ok(path)
    }

    fn samples_section(&mut self, ui: &mut egui::Ui, settings: &mut Settings) -> Option<PathBuf> {
        if self.samples.is_empty() {
            return None;
        }
        widgets::section_header(ui, icon::IMAGE, "Samples");
        ui.label(widgets::secondary_text(
            "Open one to look around or draw on it: it opens as a copy in your drawings folder.",
        ));
        ui.add_space(theme::space::SM);
        let mut chosen = None;
        let shown = self.samples.clone();
        row(ui, "samples", |ui| {
            shown.iter().for_each(|sample| {
                ui.vertical(|ui| {
                    ui.set_width(TILE);
                    let key = sample.path.display().to_string();
                    let picked = self
                        .picture(
                            ui,
                            Tile {
                                key: &key,
                                bundle: &sample.path,
                                png: sample_thumb(&sample.path),
                                name: &sample.name,
                                caption: &sample.about,
                                lines: 2,
                                there: true,
                            },
                        )
                        .on_hover_text(&sample.about)
                        .clicked();
                    if picked {
                        match samples::copy(sample, &settings.drawings_folder()) {
                            Ok(path) => {
                                self.changed = true;
                                chosen = Some(path);
                            }
                            Err(e) => self.error = Some(format!("{e:#}")),
                        }
                    }
                });
            });
        });
        chosen
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, settings: &mut Settings) -> Option<PathBuf> {
        let mut chosen = None;
        egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| {
            chosen = self.top_cards(ui, settings);
            if let Some(error) = self.error.clone() {
                ui.add_space(theme::space::SM);
                ui.colored_label(theme::ERROR, error);
                if let Some(path) = self.repair.clone()
                    && ui.button("Repair and open").clicked()
                {
                    match klad_core::bundle::Bundle::repair(&path) {
                        Ok(done) => {
                            self.repaired = Some(repaired_words(&done));
                            self.error = None;
                            self.repair = None;
                            chosen = Some(path);
                        }
                        Err(e) => self.error = Some(format!("{e:#}")),
                    }
                }
            }
            ui.add_space(theme::space::XL);
            chosen = chosen.take().or_else(|| self.recent_section(ui, settings));
            ui.add_space(theme::space::XL);
            chosen = chosen.take().or_else(|| self.samples_section(ui, settings));
        });
        self.connect_window(ui, settings);
        self.focused = true;
        chosen
    }

    fn top_cards(&mut self, ui: &mut egui::Ui, settings: &mut Settings) -> Option<PathBuf> {
        let mut chosen = None;
        let gap = theme::space::LG;
        let inset = widgets::card_frame().total_margin().sum().x;
        let new = (ui.available_width() - gap - 2.0 * inset - OPEN_WIDTH).max(LABELS * 3.0);
        let tall = self.cards_height;
        let mut heights = [0.0_f32; 2];
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            widgets::card(ui, |ui| {
                ui.vertical(|ui| {
                    ui.set_width(new);
                    let top = ui.cursor().top();
                    chosen = self.new_section(ui, settings, new);
                    heights[0] = ui.cursor().top() - top;
                    ui.add_space((tall - heights[0]).max(0.0));
                });
            });
            widgets::card(ui, |ui| {
                ui.vertical(|ui| {
                    ui.set_width(OPEN_WIDTH);
                    let top = ui.cursor().top();
                    chosen = chosen.take().or_else(|| self.open_section(ui, settings));
                    heights[1] = ui.cursor().top() - top;
                    ui.add_space((tall - heights[1]).max(0.0));
                });
            });
        });
        let want = heights[0].max(heights[1]);
        if (want - self.cards_height).abs() > 0.5 {
            self.cards_height = want;
            ui.ctx().request_repaint();
        }
        chosen
    }

    fn connect_window(&mut self, ui: &egui::Ui, settings: &mut Settings) {
        if !self.connecting {
            return;
        }
        let mut open = true;
        egui::Window::new("Connect an agent app")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ui.ctx(), |ui| {
                ui.set_width(crate::connect::WIDTH);
                if self.connect.panel(ui, settings) {
                    self.changed = true;
                }
            });
        self.connecting = open;
    }

    fn new_section(&mut self, ui: &mut egui::Ui, settings: &mut Settings, width: f32) -> Option<PathBuf> {
        let field = width - LABELS - theme::space::MD;
        widgets::heading_row(ui, icon::FILE_PLUS, "New drawing", |ui| {
            if let Some(pixel) = widgets::segmented(
                ui,
                self.pixel,
                &[
                    (false, "Painting", "brushes, shapes and text at any size"),
                    (
                        true,
                        "Pixel art",
                        "a small drawing of whole pixels, bound to a palette, shown enlarged",
                    ),
                ],
            ) {
                self.pixel = pixel;
                if pixel {
                    self.paper = false;
                }
            }
        });
        ui.add_space(theme::space::MD);
        let mut entered = false;
        egui::Grid::new("start new")
            .num_columns(2)
            .spacing(vec2(theme::space::MD, theme::space::SM))
            .min_col_width(LABELS)
            .show(ui, |ui| {
                let label = widgets::form_label(ui, "Name");
                let name = ui.add(widgets::field(&mut self.name, field)).labelled_by(label.id);
                if !self.focused && self.section == Section::New {
                    name.request_focus();
                    theme::select_all(ui.ctx(), name.id, &self.name);
                }
                entered = name.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                ui.end_row();
                let label = widgets::form_label(ui, "Folder");
                ui.horizontal(|ui| {
                    let room = match picker::NATIVE {
                        true => field - CHOOSE - ui.spacing().item_spacing.x,
                        false => field,
                    };
                    widgets::path_field(ui, &mut self.folder, room).labelled_by(label.id);
                    if picker::NATIVE
                        && ui
                            .add(widgets::secondary_button("Choose").min_size(vec2(CHOOSE, theme::CONTROL)))
                            .clicked()
                        && let Some(dir) = picker::choose_folder(Some(&expand(&self.folder)), "Choose")
                    {
                        self.folder = dir.display().to_string();
                    }
                });
                ui.end_row();
                let label = widgets::form_label(ui, "Size");
                let shown = match self.pixel {
                    true => self.pixel_preset.map_or("custom".to_string(), pixel_preset_name),
                    false => self.preset.map_or("custom".to_string(), preset_name),
                };
                egui::ComboBox::from_id_salt("start size")
                    .selected_text(shown)
                    .width(field)
                    .show_ui(ui, |ui| match self.pixel {
                        true => {
                            (0..PIXEL_PRESETS.len()).for_each(|i| {
                                ui.selectable_value(&mut self.pixel_preset, Some(i), pixel_preset_name(i));
                            });
                            ui.selectable_value(&mut self.pixel_preset, None, "custom");
                        }
                        false => {
                            (0..PRESETS.len()).for_each(|i| {
                                ui.selectable_value(&mut self.preset, Some(i), preset_name(i));
                            });
                            ui.selectable_value(&mut self.preset, None, "custom");
                        }
                    })
                    .response
                    .labelled_by(label.id);
                ui.end_row();
                if self.pixel && self.pixel_preset.is_none() {
                    widgets::form_label(ui, "Width and height");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::DragValue::new(&mut self.pixel_width)
                                .range(1..=4096)
                                .suffix(" px"),
                        );
                        ui.label("by");
                        ui.add(
                            egui::DragValue::new(&mut self.pixel_height)
                                .range(1..=4096)
                                .suffix(" px"),
                        );
                    });
                    ui.end_row();
                }
                if !self.pixel && self.preset.is_none() {
                    widgets::form_label(ui, "Width and height");
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut self.width).range(1..=16384).suffix(" px"));
                        ui.label("by");
                        ui.add(egui::DragValue::new(&mut self.height).range(1..=16384).suffix(" px"));
                    });
                    ui.end_row();
                }
                widgets::form_label(ui, "Background");
                ui.horizontal(|ui| {
                    if let Some(paper) = widgets::segmented(
                        ui,
                        self.paper,
                        &[
                            (
                                false,
                                "Transparent",
                                "the drawing starts see-through, with no paper under it",
                            ),
                            (true, "Colour", "a paper colour under everything you paint"),
                        ],
                    ) {
                        self.paper = paper;
                    }
                    if self.paper
                        && let Some(rgba) = crate::color::Square::new(
                            ui,
                            "start.paper",
                            crate::color::from_srgb(self.colour),
                            "the paper colour",
                        )
                        .pick(ui)
                    {
                        self.colour = crate::color::to_srgb(rgba);
                    }
                });
                ui.end_row();
                let label = widgets::form_label(ui, "Frames per second");
                ui.add(egui::DragValue::new(&mut self.fps).range(1..=120))
                    .labelled_by(label.id);
                ui.end_row();
            });
        if !self.pixel {
            ui.add_space(theme::space::XS);
            ui.checkbox(&mut self.sketch, "A sketch layer for blocking in shapes");
        }
        ui.add_space(theme::space::MD);
        if !(ui
            .add(widgets::primary_button("Create").min_size(vec2(LABELS, theme::CONTROL)))
            .clicked()
            || entered)
        {
            return None;
        }
        match self.create(settings) {
            Ok(path) => {
                self.changed = true;
                Some(path)
            }
            Err(e) => {
                self.error = Some(format!("{e:#}"));
                None
            }
        }
    }

    fn open_section(&mut self, ui: &mut egui::Ui, settings: &Settings) -> Option<PathBuf> {
        widgets::heading_row(ui, icon::FOLDER_OPEN, "Open a drawing", |_| {});
        ui.add_space(theme::space::MD);
        let mut picked = None;
        let whole = vec2(OPEN_WIDTH, theme::CONTROL);
        if picker::NATIVE
            && ui
                .add(widgets::secondary_button(picker::CHOOSE_DRAWING).min_size(whole))
                .clicked()
        {
            picked = picker::choose_drawing(Some(&settings.drawings_folder()), "Open");
        }
        if picker::NATIVE
            && ui
                .add(widgets::secondary_button("Choose an Aseprite file...").min_size(whole))
                .on_hover_text(
                    "an .aseprite or .ase file, or a PNG or JPEG picture, opens as a new klad drawing next to it",
                )
                .clicked()
        {
            picked = picker::choose_file(Some(&settings.drawings_folder()), "Open");
        }
        if picker::NATIVE {
            ui.add_space(theme::space::SM);
        }
        let label = widgets::form_label(ui, "Or paste its path");
        ui.horizontal(|ui| {
            let room = OPEN_WIDTH - CHOOSE - ui.spacing().item_spacing.x;
            let field = ui.add(widgets::field(&mut self.path, room)).labelled_by(label.id);
            if !self.focused && self.section == Section::Open {
                field.request_focus();
            }
            let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui
                .add(widgets::secondary_button("Open").min_size(vec2(CHOOSE, theme::CONTROL)))
                .clicked()
                || entered
            {
                picked = Some(expand(self.path.trim()));
            }
        });
        ui.add_space(theme::space::XS);
        ui.add(
            egui::Label::new(widgets::caption(
                "The path of an Aseprite file (.aseprite, .ase) or a picture (.png, .jpg) works too.",
            ))
            .wrap(),
        );
        let path = picked?;
        if foreign(&path) {
            return Some(path);
        }
        match openable(&path) {
            Ok(()) => Some(path),
            Err(e) => {
                self.error = Some(format!("{e:#}"));
                None
            }
        }
    }

    fn recent_section(&mut self, ui: &mut egui::Ui, settings: &mut Settings) -> Option<PathBuf> {
        widgets::section_header(ui, icon::HISTORY, "Recent");
        ui.add_space(6.0);
        if settings.documents.is_empty() {
            ui.label(RichText::new("Drawings you open show up here.").color(theme::WEAK));
            return None;
        }
        let mut chosen = None;
        let documents = settings.documents.clone();
        row(ui, "recent", |ui| {
            documents.iter().for_each(|doc| {
                ui.vertical(|ui| {
                    ui.set_width(TILE);
                    match self.card(ui, doc) {
                        Card::Open(path) => chosen = Some(path),
                        Card::Forget => {
                            settings.forget(doc);
                            self.changed = true;
                        }
                        Card::Duplicate(path) => {
                            match klad_core::drawings::copy(&path, &klad_core::drawings::copy_name(&path)) {
                                Ok(made) => {
                                    settings.opened(&made);
                                    self.changed = true;
                                }
                                Err(e) => self.error = Some(format!("{e:#}")),
                            }
                        }
                        Card::Trash(path) => match self.open.as_ref().is_some_and(|o| same_drawing(o, &path)) {
                            true => {
                                self.error =
                                    Some("that drawing is open: close it first, then move it to the Trash".into())
                            }
                            false => match klad_core::drawings::trash_to(
                                &path,
                                self.trash_to
                                    .clone()
                                    .or_else(|| std::env::var_os("KLAD_TRASH").map(PathBuf::from)),
                            ) {
                                Ok(_) => {
                                    settings.forget(doc);
                                    self.changed = true;
                                }
                                Err(e) => self.error = Some(format!("{e:#}")),
                            },
                        },
                        Card::Idle => {}
                    }
                });
            });
        });
        chosen
    }

    fn picture(&mut self, ui: &mut egui::Ui, tile: Tile<'_>) -> egui::Response {
        let Tile {
            key,
            bundle,
            png,
            name,
            caption,
            lines,
            there,
        } = tile;
        let space = theme::space::SM;
        let title = egui::WidgetText::from(RichText::new(name).font(theme::medium(theme::BODY))).into_galley(
            ui,
            Some(egui::TextWrapMode::Truncate),
            THUMB,
            egui::TextStyle::Body,
        );
        let mut job =
            egui::text::LayoutJob::simple(caption.to_string(), theme::regular(theme::LABEL), theme::WEAK, THUMB);
        job.wrap.max_rows = lines;
        job.wrap.overflow_character = Some('\u{2026}');
        let line = ui
            .painter()
            .layout_no_wrap("Ag".into(), theme::regular(theme::LABEL), theme::WEAK)
            .size()
            .y;
        let height =
            space + PICTURE + space + title.size().y + theme::space::XS + line * lines as f32 + theme::space::MD;
        let (rect, response) = ui.allocate_exact_size(vec2(TILE, height), egui::Sense::click());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, there, name));
        let lit = ui
            .ctx()
            .animate_bool_with_time(response.id.with("lit"), there && response.hovered(), theme::EASE);
        let fill = theme::PANEL.lerp_to_gamma(theme::CONTROL_FILL, lit);
        let edge = theme::BORDER.lerp_to_gamma(theme::PRESSED, lit);
        ui.painter().rect(
            rect,
            theme::radius::CARD,
            fill,
            egui::Stroke::new(theme::HAIRLINE, edge),
            egui::StrokeKind::Inside,
        );
        let picture = egui::Rect::from_min_size(rect.min + egui::Vec2::splat(space), vec2(THUMB, PICTURE));
        ui.painter().rect_filled(picture, theme::radius::CONTROL, theme::BG);
        if let Some(texture) = there.then(|| self.thumb(ui.ctx(), key, bundle, png)).flatten() {
            let at = egui::Rect::from_center_size(picture.center(), fit_in(texture.size_vec2(), picture.size()));
            let checks = self.checker(ui.ctx());
            let tiles = egui::Rect::from_min_size(egui::Pos2::ZERO, at.size() / (2.0 * CHECK));
            ui.painter().add(
                egui::epaint::RectShape::filled(at, theme::radius::CONTROL, egui::Color32::WHITE)
                    .with_texture(checks, tiles),
            );
            egui::Image::new(&texture)
                .corner_radius(theme::radius::CONTROL)
                .paint_at(ui, at);
        }
        let colour = match there {
            true => theme::TEXT,
            false => theme::WEAK,
        };
        let below = egui::Rect::from_min_size(
            egui::pos2(picture.left(), picture.bottom() + space),
            vec2(THUMB, title.size().y),
        );
        ui.painter().galley(below.min, title.clone(), colour);
        let words = egui::Rect::from_min_size(
            below.left_bottom() + vec2(0.0, theme::space::XS),
            vec2(THUMB, line * lines as f32),
        );
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(words)
                .layout(egui::Layout::top_down(egui::Align::Min)),
            |ui| ui.add(egui::Label::new(egui::WidgetText::LayoutJob(job.into())).selectable(false)),
        );
        response
    }

    fn checker(&mut self, ctx: &egui::Context) -> egui::TextureId {
        self.checker
            .get_or_insert_with(|| {
                let [l, d] = [theme::CHECK_LIGHT, theme::CHECK_DARK].map(|c| [c.r(), c.g(), c.b(), 255]);
                let image = egui::ColorImage::from_rgba_unmultiplied([2, 2], &[l, d, d, l].concat());
                ctx.load_texture("start checks", image, egui::TextureOptions::NEAREST_REPEAT)
            })
            .id()
    }

    fn card(&mut self, ui: &mut egui::Ui, doc: &str) -> Card {
        let path = PathBuf::from(doc);
        let there = path.join("doc.json").is_file();
        let name = path
            .file_stem()
            .map_or_else(|| doc.to_string(), |s| s.to_string_lossy().to_string());
        let folder = path
            .parent()
            .and_then(Path::file_name)
            .map_or_else(String::new, |s| s.to_string_lossy().to_string());
        let caption = format!("in {folder}");
        let response = self.picture(
            ui,
            Tile {
                key: doc,
                bundle: &path,
                png: path.join(".cache").join("thumb.png"),
                name: &name,
                caption: &caption,
                lines: 1,
                there,
            },
        );
        let mut asked = None;
        if there {
            response.context_menu(|ui| {
                if ui.button(format!("{} Duplicate", icon::DUPLICATE)).clicked() {
                    asked = Some(Card::Duplicate(path.clone()));
                    ui.close();
                }
                if ui
                    .button(format!("{} Move to the Trash", icon::TRASH))
                    .on_hover_text(
                        "the drawing goes to the Trash (the recycle bin on Windows), where you can get it back",
                    )
                    .clicked()
                {
                    asked = Some(Card::Trash(path.clone()));
                    ui.close();
                }
            });
        }
        if let Some(card) = asked {
            return card;
        }
        let clicked = there && response.on_hover_text(doc).clicked();
        if clicked {
            return Card::Open(path);
        }
        if !there {
            ui.label(RichText::new("moved or deleted").color(theme::REFUSED));
            if ui.add(widgets::secondary_button(format!("Remove {name}"))).clicked() {
                return Card::Forget;
            }
        }
        Card::Idle
    }

    fn thumb(&mut self, ctx: &egui::Context, doc: &str, bundle: &Path, png: PathBuf) -> Option<TextureHandle> {
        let state = self.thumbs.entry(doc.to_string()).or_insert_with(|| {
            let (tx, rx) = channel();
            let path = bundle.to_path_buf();
            let repaint = ctx.clone();
            std::thread::spawn(move || {
                let image = thumbnail(&path, &png);
                let _ = tx.send(image);
                repaint.request_repaint();
            });
            Thumb::Waiting(rx)
        });
        if let Thumb::Waiting(rx) = state {
            *state = match rx.try_recv() {
                Ok(Some(image)) => {
                    Thumb::Ready(ctx.load_texture(format!("thumb {doc}"), image, egui::TextureOptions::LINEAR))
                }
                Ok(None) | Err(TryRecvError::Disconnected) => Thumb::Missing,
                Err(TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(200));
                    return None;
                }
            };
        }
        match state {
            Thumb::Ready(texture) => Some(texture.clone()),
            _ => None,
        }
    }
}

fn row(ui: &mut egui::Ui, salt: &str, cards: impl FnOnce(&mut egui::Ui)) {
    ui.spacing_mut().scroll.fade = egui::style::ScrollFadeStyle {
        strength: 1.0,
        size: 2.0 * theme::space::XL,
    };
    egui::ScrollArea::horizontal()
        .id_salt(salt)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing = vec2(theme::space::MD, theme::space::SM);
                cards(ui);
            });
            ui.add_space(theme::space::MD);
        });
}

fn preset_name(i: usize) -> String {
    let (name, w, h) = PRESETS[i];
    format!("{w} x {h}  {name}")
}

fn pixel_preset_name(i: usize) -> String {
    let (name, w, h) = PIXEL_PRESETS[i];
    format!("{w} x {h}  {name}")
}

fn fit_in(size: egui::Vec2, room: egui::Vec2) -> egui::Vec2 {
    size * (room.x / size.x.max(1.0)).min(room.y / size.y.max(1.0))
}

pub fn expand(text: &str) -> PathBuf {
    match (text.strip_prefix("~/"), crate::places::home()) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(text),
    }
}

pub fn same_drawing(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

pub fn foreign(path: &Path) -> bool {
    path.is_file()
        && (is_picture(path)
            || path.extension().is_some_and(|e| e.eq_ignore_ascii_case("svg"))
            || path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                e.eq_ignore_ascii_case("aseprite") || e.eq_ignore_ascii_case("ase") || e.eq_ignore_ascii_case("psd")
            }))
}

pub fn brought_in(path: &Path) -> Result<(PathBuf, Option<String>)> {
    if !foreign(path) {
        return Ok((path.to_path_buf(), None));
    }
    let name = |p: &Path| p.file_name().map_or(String::new(), |n| n.to_string_lossy().to_string());
    let target = path.with_extension("klad");
    if target.join("doc.json").is_file() {
        let told = format!(
            "opened {}, the drawing made from {} before; rename or remove it to bring the file in again",
            name(&target),
            name(path)
        );
        return Ok((target, Some(told)));
    }
    let Some(exe) = draw_binary() else {
        bail!(
            "the klad command line tool is missing next to the editor, so an Aseprite file or a picture cannot be opened: install klad again (it ships both)"
        );
    };
    let out = std::process::Command::new(exe).arg("import").arg(path).output()?;
    let said = String::from_utf8_lossy(&out.stderr).to_string();
    if !out.status.success() {
        bail!("{}", said.trim().trim_start_matches("error: "));
    }
    let made = String::from_utf8_lossy(&out.stdout)
        .lines()
        .last()
        .map(|line| PathBuf::from(line.trim()))
        .filter(|p| p.join("doc.json").is_file())
        .ok_or_else(|| anyhow::anyhow!("klad import did not say which drawing it made"))?;
    let notes: Vec<&str> = said.lines().filter_map(|line| line.strip_prefix("note: ")).collect();
    let told = match notes.is_empty() {
        true => format!("made {} from {}", name(&made), name(path)),
        false => format!("made {} from {}; {}", name(&made), name(path), notes.join("; ")),
    };
    Ok((made, Some(told)))
}

const PICTURES: [&str; 6] = ["png", "jpg", "jpeg", "gif", "webp", "bmp"];

pub fn is_picture(path: &Path) -> bool {
    path.extension()
        .and_then(|x| x.to_str())
        .is_some_and(|x| PICTURES.iter().any(|p| x.eq_ignore_ascii_case(p)))
}

pub fn picture_words(name: &str) -> String {
    format!(
        "{name} is a picture, not a klad drawing: make a drawing, then bring the picture in with File > Import picture… or drop it on the window"
    )
}

pub fn openable(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty() {
        bail!("paste the path of a .klad drawing");
    }
    if !path.exists() {
        bail!("{} does not exist", path.display());
    }
    if !path.is_dir() {
        if is_picture(path) {
            bail!("{}", picture_words(&path.display().to_string()));
        }
        bail!("{} is not a klad drawing", path.display());
    }
    if !path.join("doc.json").is_file() {
        bail!("{} is not a drawing: it has no doc.json", path.display());
    }
    Ok(())
}

pub fn free_name(folder: &Path) -> String {
    (1..)
        .map(|i| match i {
            1 => "untitled".to_string(),
            n => format!("untitled {n}"),
        })
        .find(|name| !folder.join(format!("{name}.klad")).exists())
        .unwrap_or_default()
}

pub(crate) fn draw_binary() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    if exe.file_stem().is_some_and(|s| s == "klad") {
        return Some(exe);
    }
    let name = format!("klad{}", std::env::consts::EXE_SUFFIX);
    let reach = if cfg!(test) { 5 } else { 2 };
    exe.ancestors()
        .skip(1)
        .take(reach)
        .map(|dir| dir.join(&name))
        .find(|p| p.is_file())
}

#[cfg(test)]
pub(crate) fn test_binary() -> PathBuf {
    draw_binary().unwrap_or_else(|| {
        let exe = std::env::current_exe();
        let above = exe.as_ref().ok().and_then(|e| e.parent()).and_then(Path::parent);
        let held = above.and_then(|d| std::fs::read_dir(d).ok()).map_or_else(
            || "nothing readable".to_string(),
            |entries| {
                let mut names: Vec<String> = entries
                    .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().to_string()))
                    .filter(|n| n.starts_with("klad"))
                    .collect();
                names.sort();
                names.join(", ")
            },
        );
        panic!(
            "build the klad binary first (just test does); this test runs as {exe:?}, which exists: {}; {above:?} holds [{held}]",
            exe.as_ref().is_ok_and(|e| e.exists())
        )
    })
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn sample_thumb(bundle: &Path) -> PathBuf {
    let stem = bundle
        .file_stem()
        .map_or_else(String::new, |s| s.to_string_lossy().to_string());
    bundle.with_file_name(format!("{stem}.thumb.png"))
}

fn thumbnail(bundle: &Path, png: &Path) -> Option<egui::ColorImage> {
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());
    let png = png.to_path_buf();
    let kept = !png.starts_with(bundle) && png.is_file();
    let fresh = kept || matches!((modified(&png), modified(&bundle.join("doc.json"))), (Some(t), Some(d)) if t >= d);
    if !fresh {
        let _turn = ONE_AT_A_TIME.lock().ok()?;
        let doc: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(bundle.join("doc.json")).ok()?).ok()?;
        let side = ["width", "height"]
            .iter()
            .filter_map(|k| doc["canvas"][k].as_f64())
            .fold(1.0, f64::max);
        let done = std::process::Command::new(draw_binary()?)
            .env("RAYON_NUM_THREADS", "2")
            .arg("render")
            .arg(bundle)
            .arg("--scale")
            .arg(format!("{:.4}", (2.0 * THUMB as f64 / side).min(1.0)))
            .arg("-o")
            .arg(&png)
            .output()
            .ok()?;
        if !done.status.success() {
            return None;
        }
    }
    decode(&std::fs::read(&png).ok()?).ok()
}

fn decode(bytes: &[u8]) -> Result<egui::ColorImage> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info()?;
    let mut buffer = vec![0; reader.output_buffer_size().ok_or_else(|| anyhow!("a png too large"))?];
    let info = reader.next_frame(&mut buffer)?;
    let pixels = &buffer[..info.buffer_size()];
    let size = [info.width as usize, info.height as usize];
    Ok(match info.color_type {
        png::ColorType::Rgba => egui::ColorImage::from_rgba_unmultiplied(size, pixels),
        png::ColorType::Rgb => egui::ColorImage::from_rgb(size, pixels),
        other => bail!("thumbnails are rgb or rgba pngs, not {other:?}"),
    })
}

pub fn first_steps(settings: &Settings) -> Result<PathBuf> {
    let folder = settings.drawings_folder();
    std::fs::create_dir_all(&folder)?;
    let path = samples::free_path(&folder, "first steps");
    let blank = Blank {
        width: 1280,
        height: 800,
        background: Some("#f4efe6".into()),
        sketch: false,
        ..Blank::default()
    };
    Bundle::blank(&path, &blank, "editor")?;
    Ok(path)
}

pub fn title(path: Option<&Path>) -> String {
    match path {
        Some(path) => format!(
            "{}  klad",
            path.file_stem()
                .map_or_else(|| path.display().to_string(), |s| s.to_string_lossy().to_string())
        ),
        None => "klad".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_thumbnail_is_rendered_once_and_again_when_the_drawing_changes() {
        test_binary();
        let dir = std::env::temp_dir().join(format!("draw-thumb-{}.klad", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let blank = Blank {
            width: 400,
            height: 200,
            background: Some("#336699".into()),
            ..Blank::default()
        };
        Bundle::blank(&dir, &blank, "cli").unwrap();
        let image = thumbnail(&dir, &dir.join(".cache").join("thumb.png")).unwrap();
        assert_eq!(image.size, [336, 168]);
        let middle = image.pixels[84 * 336 + 168];
        assert!(
            middle.r().abs_diff(0x33) <= 2 && middle.b().abs_diff(0x99) <= 2,
            "{middle:?}"
        );
        let png = dir.join(".cache").join("thumb.png");
        let first = modified(&png).unwrap();
        thumbnail(&dir, &dir.join(".cache").join("thumb.png")).unwrap();
        assert_eq!(modified(&png), Some(first));
        std::thread::sleep(Duration::from_millis(20));
        Bundle::open(&dir).unwrap().save().unwrap();
        thumbnail(&dir, &dir.join(".cache").join("thumb.png")).unwrap();
        assert!(modified(&png).unwrap() > first);
        assert_eq!(free_name(dir.parent().unwrap()), "untitled");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn names_paths_and_sizes_come_out_as_typed() {
        let settings = Settings {
            drawings: Some("/art".into()),
            ..Settings::default()
        };
        let mut start = Start::new(&settings, Section::New);
        start.name = "  cave  ".into();
        assert_eq!(start.target(), PathBuf::from("/art/cave.klad"));
        start.name = "cave.klad".into();
        assert_eq!(start.target(), PathBuf::from("/art/cave.klad"));
        assert_eq!(
            (start.blank().width, start.blank().height, start.blank().background),
            (2048, 2048, None)
        );
        start.preset = None;
        (start.width, start.height, start.paper) = (640, 360, true);
        assert_eq!(
            (start.blank().width, start.blank().height, start.blank().background),
            (640, 360, Some("#f4efe6".to_string()))
        );
        start.pixel = true;
        let pixel = start.blank();
        assert_eq!(
            (pixel.width, pixel.height, pixel.pixel, pixel.sketch),
            (64, 64, true, false),
            "pixel art starts as a 64 pixel sprite with pixel layers and no sketch layer"
        );
        start.pixel_preset = None;
        (start.pixel_width, start.pixel_height) = (48, 24);
        assert_eq!((start.blank().width, start.blank().height), (48, 24));
        start.pixel = false;
        assert_eq!((start.blank().width, start.blank().pixel), (640, false));
        start.name = "a/b".into();
        assert!(format!("{:#}", start.create(&mut Settings::default()).unwrap_err()).contains("cannot hold a /"));
        assert!(format!("{:#}", openable(Path::new("/nowhere")).unwrap_err()).contains("does not exist"));
        if let Some(home) = std::env::var_os("HOME") {
            assert_eq!(expand("~/art"), PathBuf::from(home).join("art"));
        }
    }
}
