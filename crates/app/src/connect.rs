use crate::settings::Settings;
use crate::theme::{self, Hint};
use egui::RichText;
use klad_mcp::{App, Entry, Places, Status};
use std::path::{Path, PathBuf};

pub(crate) struct Connect {
    pub places: Option<Places>,
    pub program: PathBuf,
    pub said: Option<String>,
}

pub(crate) const ASK: &str = "Then ask it in your own words, for example: \u{201c}Paint a red apple in a new klad drawing and open it in klad.\u{201d}";

impl Connect {
    pub fn here() -> Self {
        let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("klad"));
        let sibling = exe.with_file_name(if cfg!(windows) { "klad.exe" } else { "klad" });
        Self {
            places: if cfg!(test) { None } else { Places::here() },
            program: if sibling.is_file() { sibling } else { exe },
            said: None,
        }
    }

    fn act(&mut self, app: App, places: &Places, connect: bool) {
        let entry = Entry::serving(&self.program);
        self.said = Some(match connect {
            true => match app.connect(places, &entry) {
                Ok(_) => format!("klad is in {}. Now {}.", app.name(), app.next_step()),
                Err(e) => e,
            },
            false => match app.disconnect(places) {
                Ok(_) => format!("klad is out of {}. {}.", app.name(), capital(app.next_step())),
                Err(e) => e,
            },
        });
    }

    pub fn panel(&mut self, ui: &mut egui::Ui, settings: &mut Settings) -> bool {
        ui.label("Claude Desktop, Codex and other agent apps can paint and edit your drawings with klad, while you keep drawing in it. Connect one, then quit and reopen it.");
        ui.label(RichText::new(ASK).color(theme::WEAK));
        ui.add_space(6.0);
        match self.places.clone() {
            Some(places) => App::ALL.into_iter().for_each(|app| {
                ui.horizontal(|ui| {
                    let status = app.status(&places);
                    let (word, enabled, hint) = match &status {
                        Status::Missing => (
                            format!("connect {}", app.name()),
                            false,
                            format!("{} is not on this computer", app.name()),
                        ),
                        Status::Found => (
                            format!("connect {}", app.name()),
                            true,
                            format!(
                                "puts klad in {}'s settings; the rest of them stay as they are",
                                app.name()
                            ),
                        ),
                        Status::Moved(_) => (
                            format!("connect {} again", app.name()),
                            true,
                            "klad was moved since it was connected: this points the app at klad where it is now".into(),
                        ),
                        Status::Connected(_) => (
                            format!("disconnect {}", app.name()),
                            true,
                            format!("takes klad out of {}'s settings", app.name()),
                        ),
                    };
                    let clicked = ui.add_enabled(enabled, egui::Button::new(word)).hint(hint).clicked();
                    let state = match &status {
                        Status::Missing => "not on this computer",
                        Status::Found => "found",
                        Status::Moved(_) => "set to a klad that was moved",
                        Status::Connected(_) => "connected",
                    };
                    ui.label(RichText::new(state).color(theme::WEAK));
                    if clicked {
                        self.act(app, &places, !matches!(status, Status::Connected(_)));
                    }
                });
            }),
            None => {
                ui.label(
                    RichText::new("this computer has no home folder, where agent apps keep their settings")
                        .color(theme::WEAK),
                );
            }
        }
        if let Some(said) = &self.said {
            ui.add_space(4.0);
            ui.label(RichText::new(said).strong());
        }
        ui.add_space(8.0);
        ui.separator();
        ui.label(RichText::new("Folders agents may work in").strong());
        let added: Vec<PathBuf> = settings.agent_folders.iter().map(PathBuf::from).collect();
        let mut gone = None;
        settings.agent_roots().into_iter().for_each(|folder| {
            ui.horizontal(|ui| {
                ui.label(crate::places::tilde(&folder));
                if added.contains(&folder)
                    && ui
                        .small_button("remove")
                        .hint("agents can no longer work in this folder")
                        .clicked()
                {
                    gone = Some(folder.clone());
                }
            });
        });
        let mut changed = false;
        if let Some(folder) = gone {
            settings.agent_folders.retain(|f| Path::new(f) != folder);
            changed = true;
        }
        if ui
            .button("add a folder")
            .hint("agents may then make and change drawings in it")
            .clicked()
            && let Some(folder) = crate::picker::choose_folder(None, "Agents may work in")
        {
            let folder = folder.display().to_string();
            if !settings.agent_folders.contains(&folder) {
                settings.agent_folders.push(folder);
                changed = true;
            }
        }
        ui.label(
            RichText::new("The drawings folder and the folders of your recent drawings are always in.")
                .color(theme::WEAK)
                .small(),
        );
        changed
    }
}

fn capital(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}
