use super::{Editor, Tool};
use crate::messages::Tone;
use crate::theme::{self, Hint, icon};
use anyhow::{Result, anyhow};
use egui::RichText;
use klad_brush::BrushSet;
use klad_core::brushes::BrushDef;
use klad_render::library::{Library, Saved};

pub(super) struct Yours {
    pub lib: Option<Library>,
    pub saved: Vec<Saved>,
    pub set: BrushSet,
    pub name: String,
    pub group: String,
    pub editing: Option<String>,
}

impl Yours {
    pub fn at(lib: Option<Library>) -> Self {
        let mut yours = Self {
            lib,
            saved: Vec::new(),
            set: BrushSet::presets(),
            name: String::new(),
            group: String::new(),
            editing: None,
        };
        yours.reload();
        yours
    }

    pub fn reload(&mut self) {
        self.saved = self.lib.as_ref().map(Library::list).unwrap_or_default();
        let mut set = BrushSet::presets();
        self.saved.iter().for_each(|s| set.insert(&key(&s.id), s.def.clone()));
        self.set = set;
    }

    fn lib(&self) -> Result<&Library> {
        self.lib
            .as_ref()
            .ok_or_else(|| anyhow!("there is no folder for your brushes on this computer"))
    }
}

pub(super) fn key(id: &str) -> String {
    format!("yours/{id}")
}

enum Act {
    Take(String),
    Rename(String),
    Group(String),
    Shift(String, i32),
    Forget(String),
    Export,
    Import,
}

impl Editor {
    pub(super) fn save_to_yours(&mut self, id: &str) -> Result<()> {
        let def = self
            .brushes
            .def(id)
            .or_else(|| self.embedded.get(id))
            .cloned()
            .ok_or_else(|| anyhow!("no brush {id} here"))?;
        self.yours.lib()?.save(&self.scene.bundle, id, &def)?;
        self.yours.reload();
        self.say(
            Tone::Done,
            format!("{id} is in your brushes now: every drawing shows it"),
        );
        Ok(())
    }

    fn take_yours(&mut self, id: &str) -> Result<()> {
        let (_, fresh) = self.yours.lib()?.take(&self.scene.bundle, id)?;
        self.embedded = self.scene.bundle.brushes()?;
        self.brushes.refresh(&self.scene.bundle)?;
        self.tool = Tool::Brush;
        self.previous_brush = id.to_string();
        self.pick(id);
        if !fresh {
            self.say(
                Tone::Info,
                format!("this drawing has a brush {id} of its own and keeps it"),
            );
        }
        Ok(())
    }

    fn act_yours(&mut self, act: Act) -> Result<()> {
        let lib = self.yours.lib()?.clone();
        match act {
            Act::Take(id) => return self.take_yours(&id),
            Act::Rename(id) => lib.rename(&id, &self.yours.name)?,
            Act::Group(id) => lib.group(&id, &self.yours.group)?,
            Act::Shift(id, by) => lib.shift(&id, by)?,
            Act::Forget(id) => {
                lib.forget(&id)?;
                self.say(
                    Tone::Done,
                    format!("{id} is out of your brushes; drawings that use it keep their copy"),
                );
            }
            Act::Export => {
                if let Some(folder) = crate::picker::choose_folder(None, "Share your brushes in") {
                    let file = folder.join("klad-brushes.json");
                    let n = lib.export(&file)?;
                    self.say(
                        Tone::Done,
                        format!("{n} brushes written to {}", crate::places::tilde(&file)),
                    );
                }
            }
            Act::Import => {
                if let Some(file) = crate::picker::choose_file(None, "Add brushes from") {
                    let added = lib.import(&file)?;
                    self.say(Tone::Done, format!("{} brushes added to your brushes", added.len()));
                }
            }
        }
        self.yours.reload();
        Ok(())
    }

    pub(super) fn yours_tiles(&mut self, ui: &mut egui::Ui) {
        if self.yours.lib.is_none() {
            return;
        }
        let ctx = ui.ctx().clone();
        let saved = self.yours.saved.clone();
        let mut act = None;
        ui.horizontal(|ui| {
            ui.label(RichText::new("your brushes").color(theme::WEAK).small());
            let square = egui::vec2(theme::TARGET, theme::TARGET);
            if ui
                .add(egui::Button::new(icon::EXPORT).min_size(square))
                .hint("write your brushes to one file to share")
                .clicked()
            {
                act = Some(Act::Export);
            }
            if ui
                .add(egui::Button::new(icon::DOWNLOAD).min_size(square))
                .hint("add the brushes of a shared file to yours")
                .clicked()
            {
                act = Some(Act::Import);
            }
        });
        if saved.is_empty() {
            ui.label(
                RichText::new("right click a brush of this drawing to keep it for every drawing")
                    .color(theme::WEAK)
                    .small(),
            );
        }
        saved.iter().enumerate().for_each(|(i, s)| {
            if !s.group.is_empty() && (i == 0 || saved[i - 1].group != s.group) {
                ui.label(RichText::new(&s.group).color(theme::WEAK).small());
            }
            let response = self.brush_tile(ui, &ctx, &key(&s.id), &s.id, &s.def, true);
            if response.clicked() {
                act = Some(Act::Take(s.id.clone()));
            }
            response.context_menu(|ui| {
                if let Some(a) = self.yours_menu(ui, s, i, saved.len()) {
                    act = Some(a);
                    ui.close();
                }
            });
        });
        if let Some(a) = act {
            self.check(|e| e.act_yours(a));
        }
    }

    fn yours_menu(&mut self, ui: &mut egui::Ui, s: &Saved, at: usize, of: usize) -> Option<Act> {
        if self.yours.editing.as_deref() != Some(s.id.as_str()) {
            self.yours.editing = Some(s.id.clone());
            self.yours.name = s.def.name.clone();
            self.yours.group = s.group.clone();
        }
        let mut act = None;
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.yours.name).desired_width(120.0))
                .hint("the brush's name");
            if ui.button("rename").clicked() {
                act = Some(Act::Rename(s.id.clone()));
            }
        });
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.yours.group).desired_width(120.0))
                .hint("brushes in one group are listed together; empty for none");
            if ui.button("group").clicked() {
                act = Some(Act::Group(s.id.clone()));
            }
        });
        if ui.add_enabled(at > 0, egui::Button::new("move up")).clicked() {
            act = Some(Act::Shift(s.id.clone(), -1));
        }
        if ui.add_enabled(at + 1 < of, egui::Button::new("move down")).clicked() {
            act = Some(Act::Shift(s.id.clone(), 1));
        }
        if ui
            .button(format!("{} forget", icon::TRASH))
            .hint("out of your brushes; drawings that use it keep their copy")
            .clicked()
        {
            act = Some(Act::Forget(s.id.clone()));
        }
        if act.is_some() {
            self.yours.editing = None;
        }
        act
    }

    pub(super) fn own_brush_menu(&mut self, ui: &mut egui::Ui, id: &str) {
        if ui
            .button(format!("{} save to your brushes", icon::STAR))
            .hint("keep it for every drawing")
            .clicked()
        {
            let id = id.to_string();
            self.check(|e| e.save_to_yours(&id));
            ui.close();
        }
    }

    pub(super) fn brush_tile(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        sample: &str,
        id: &str,
        def: &BrushDef,
        yours: bool,
    ) -> egui::Response {
        let label = super::options::label_of(id, def);
        let selected = self.brush == id;
        let texture = match id {
            "eraser" => None,
            _ => match yours {
                true => self.samples.texture(ctx, &self.yours.set, sample, def),
                false => self.samples.texture(ctx, &self.brushes, sample, def),
            },
        };
        let text = RichText::new(match id {
            "eraser" => format!("{} {label}", icon::ERASER),
            _ => label.clone(),
        });
        let size = egui::vec2(super::options::SAMPLE.0 as f32, super::options::SAMPLE.1 as f32);
        let button = match &texture {
            Some(t) => egui::Button::image_and_text(egui::Image::new(t).fit_to_exact_size(size), text),
            None => egui::Button::new(text),
        };
        ui.add(
            button
                .selected(selected)
                .min_size(egui::vec2(ui.available_width(), theme::TARGET)),
        )
        .hint(format!("{id}: {}", def.name))
    }
}
