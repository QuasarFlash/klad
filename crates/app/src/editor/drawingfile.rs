use super::{Editor, Want};
use crate::messages::Tone;
use crate::theme::{self, Hint};
use anyhow::{Result, bail};
use egui::RichText;
use klad_core::drawings;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Ask {
    SaveCopy,
    Rename,
}

#[derive(Clone, Debug)]
pub(super) struct Naming {
    pub ask: Ask,
    pub name: String,
}

impl Editor {
    fn stem(&self) -> String {
        self.scene
            .bundle
            .root
            .file_stem()
            .map_or_else(String::new, |s| s.to_string_lossy().to_string())
    }

    fn beside(&self, name: &str) -> PathBuf {
        let root = &self.scene.bundle.root;
        root.parent().map_or_else(|| PathBuf::from(name), |p| p.join(name))
    }

    pub(super) fn ask_name(&mut self, ask: Ask) {
        let name = match ask {
            Ask::SaveCopy => drawings::copy_name(&self.scene.bundle.root)
                .file_stem()
                .map_or_else(String::new, |s| s.to_string_lossy().to_string()),
            Ask::Rename => self.stem(),
        };
        self.naming = Some(Naming { ask, name });
    }

    fn settled_for_files(&mut self) -> Result<()> {
        if self.live.is_some() || self.drag.is_some() {
            bail!("finish the stroke or the move first");
        }
        self.follow_the_folder()
    }

    pub(super) fn save_copy(&mut self, name: &str, go_on: bool) -> Result<PathBuf> {
        self.settled_for_files()?;
        let made = drawings::copy(&self.scene.bundle.root, &self.beside(name.trim()))?;
        let shown = made
            .file_stem()
            .map_or_else(String::new, |s| s.to_string_lossy().to_string());
        match go_on {
            true => self.wants = Some(Want::Path(made.clone())),
            false => {
                self.settings.opened(&made);
                self.settings.opened(&self.scene.bundle.root.clone());
                self.settings_dirty = true;
                self.say(
                    Tone::Done,
                    format!("saved a copy, {shown}, next to this drawing; you go on here"),
                );
            }
        }
        Ok(made)
    }

    pub(super) fn rename_drawing(&mut self, name: &str) -> Result<PathBuf> {
        self.settled_for_files()?;
        let from = self.scene.bundle.root.clone();
        if name.trim() == self.stem() {
            return Ok(from);
        }
        let to = drawings::rename(&from, &self.beside(name.trim()))?;
        self.follow(to.clone());
        Ok(to)
    }

    pub(super) fn duplicate_drawing(&mut self) -> Result<PathBuf> {
        self.settled_for_files()?;
        let made = drawings::copy(&self.scene.bundle.root, &drawings::copy_name(&self.scene.bundle.root))?;
        self.settings.opened(&made);
        self.settings.opened(&self.scene.bundle.root.clone());
        self.settings_dirty = true;
        let shown = made
            .file_stem()
            .map_or_else(String::new, |s| s.to_string_lossy().to_string());
        self.say(
            Tone::Done,
            format!("made {shown} next to this drawing; it is in File > Open Recent"),
        );
        Ok(made)
    }

    pub(super) fn revert_drawing(&mut self) -> Result<()> {
        self.settled_for_files()?;
        match drawings::revert(&mut self.scene.bundle, "editor")? {
            true => {
                self.reload()?;
                self.say(
                    Tone::Done,
                    "back to how the drawing was when you opened it (cmd Z takes it back)",
                );
            }
            false => self.say(Tone::Info, "nothing changed since you opened the drawing"),
        }
        Ok(())
    }

    pub(super) fn naming_window(&mut self, ctx: &egui::Context) {
        let Some(mut naming) = self.naming.clone() else {
            return;
        };
        let mut open = true;
        let mut act: Option<bool> = None;
        let title = match naming.ask {
            Ask::SaveCopy => "Save a copy",
            Ask::Rename => "Rename the drawing",
        };
        egui::Window::new(title)
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 80.0))
            .show(ctx, |ui| {
                let label = ui.label(RichText::new("name").color(theme::WEAK));
                let field = ui
                    .add(egui::TextEdit::singleline(&mut naming.name).desired_width(280.0))
                    .labelled_by(label.id);
                if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    act = Some(naming.ask == Ask::SaveCopy);
                }
                ui.label(
                    RichText::new(match naming.ask {
                        Ask::SaveCopy => {
                            "the copy goes next to this drawing, with everything in it but its undo history"
                        }
                        Ask::Rename => "the drawing's folder takes the new name; klad goes on in it",
                    })
                    .color(theme::WEAK),
                );
                ui.horizontal(|ui| match naming.ask {
                    Ask::SaveCopy => {
                        if ui
                            .button("Save and go on in the copy")
                            .hint("open the copy instead of this drawing")
                            .clicked()
                        {
                            act = Some(true);
                        }
                        if ui
                            .button("Save, stay here")
                            .hint("keep working in this drawing")
                            .clicked()
                        {
                            act = Some(false);
                        }
                    }
                    Ask::Rename => {
                        if ui.button("Rename").clicked() {
                            act = Some(true);
                        }
                    }
                });
            });
        let done = match act {
            Some(go_on) => {
                let name = naming.name.clone();
                let result = match naming.ask {
                    Ask::SaveCopy => self.save_copy(&name, go_on).map(|_| ()),
                    Ask::Rename => self.rename_drawing(&name).map(|_| ()),
                };
                match result {
                    Ok(()) => true,
                    Err(e) => {
                        self.error(format!("{e:#}"));
                        false
                    }
                }
            }
            None => false,
        };
        self.naming = match open && !done {
            true => Some(naming),
            false => None,
        };
    }
}
