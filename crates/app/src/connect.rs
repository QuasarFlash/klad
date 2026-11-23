use crate::settings::Settings;
use crate::theme::{self, Hint, icon};
use crate::widgets;
use egui::{RichText, vec2};
use klad_mcp::{App, Entry, Places, Status};
use std::path::{Path, PathBuf};

pub(crate) struct Connect {
    pub places: Option<Places>,
    pub program: PathBuf,
    pub said: Option<Said>,
    copied: bool,
    steps_height: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Said {
    Connected(App),
    Disconnected(App),
    Failed(String),
}

pub(crate) const EXAMPLE: &str = "Paint a red apple in a new klad drawing and open it in klad.";

const INTRO: &str = "Your agent app can draw in klad: Claude Desktop, Codex and others. It paints stroke by stroke while you watch, and you can fix any stroke by hand.";

const CARD: f32 = 470.0;
const OPENING: f32 = 0.22;

pub(crate) fn here_word() -> &'static str {
    match cfg!(target_os = "macos") {
        true => "on this Mac",
        false => "on this computer",
    }
}

pub(crate) fn names(apps: &[App]) -> String {
    let words: Vec<&str> = apps.iter().map(|a| a.name()).collect();
    match words.as_slice() {
        [] => String::new(),
        [one] => (*one).to_string(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

impl Connect {
    pub fn here() -> Self {
        let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("klad"));
        let sibling = exe.with_file_name(if cfg!(windows) { "klad.exe" } else { "klad" });
        Self {
            places: if cfg!(test) { None } else { Places::here() },
            program: if sibling.is_file() { sibling } else { exe },
            said: None,
            copied: false,
            steps_height: 150.0,
        }
    }

    pub fn statuses(&self) -> Vec<(App, Status)> {
        self.places.as_ref().map_or_else(Vec::new, |places| {
            App::ALL.into_iter().map(|app| (app, app.status(places))).collect()
        })
    }

    fn act(&mut self, app: App, places: &Places, connect: bool) {
        let entry = Entry::serving(&self.program);
        self.copied = false;
        self.said = Some(match connect {
            true => match app.connect(places, &entry) {
                Ok(_) => Said::Connected(app),
                Err(e) => Said::Failed(e),
            },
            false => match app.disconnect(places) {
                Ok(_) => Said::Disconnected(app),
                Err(e) => Said::Failed(e),
            },
        });
    }

    pub fn any_connected(&self) -> bool {
        self.statuses().iter().any(|(_, s)| matches!(s, Status::Connected(_)))
    }

    pub fn panel(&mut self, ui: &mut egui::Ui, settings: &mut Settings) -> bool {
        let width = ui.available_width().min(CARD);
        ui.set_max_width(width);
        ui.add(egui::Label::new(INTRO).wrap());
        ui.add_space(8.0);
        self.chooser(ui, width);
        ui.add_space(8.0);
        folders(ui, settings)
    }

    pub fn chooser(&mut self, ui: &mut egui::Ui, width: f32) {
        match self.places.clone() {
            Some(places) => self.apps(ui, &places, width),
            None => {
                ui.label(
                    RichText::new("This computer has no home folder, where agent apps keep their settings.")
                        .color(theme::WEAK),
                );
            }
        }
        let open =
            ui.ctx()
                .animate_bool_with_time(egui::Id::unique("connect next steps"), self.said.is_some(), OPENING);
        let Some(said) = self.said.clone().filter(|_| open > 0.0) else {
            return;
        };
        let eased = 1.0 - (1.0 - open).powi(3);
        let (room, _) = ui.allocate_exact_size(vec2(width, self.steps_height * eased), egui::Sense::hover());
        let mut inner = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(egui::Rect::from_min_size(room.min, vec2(width, f32::INFINITY)))
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        inner.set_clip_rect(room.intersect(ui.clip_rect()));
        inner.multiply_opacity(open);
        inner.add_space(6.0);
        self.next_steps(&mut inner, &said, width);
        self.steps_height = inner.min_rect().height();
        if open < 1.0 {
            ui.ctx().request_repaint();
        }
    }

    fn apps(&mut self, ui: &mut egui::Ui, places: &Places, width: f32) {
        let statuses: Vec<(App, Status)> = App::ALL.into_iter().map(|app| (app, app.status(places))).collect();
        let missing: Vec<App> = statuses
            .iter()
            .filter(|(_, s)| matches!(s, Status::Missing))
            .map(|(a, _)| *a)
            .collect();
        let shown: Vec<(App, Status)> = statuses
            .into_iter()
            .filter(|(_, s)| !matches!(s, Status::Missing))
            .collect();
        if shown.is_empty() {
            ui.add(
                egui::Label::new(format!(
                    "No agent app {} yet. klad works with {}: install one, then come back here.",
                    here_word(),
                    names(&App::ALL)
                ))
                .wrap(),
            );
            ui.hyperlink_to("Working with an agent", klad_core::contact::AGENT_GUIDE)
                .on_hover_text("klad's guide on klad.studio, in your browser");
            return;
        }
        shown.into_iter().for_each(|(app, status)| {
            if let Some(connect) = card(ui, app, &status, width) {
                self.act(app, places, connect);
            }
            ui.add_space(4.0);
        });
        if !missing.is_empty() {
            ui.label(
                RichText::new(format!("Also works with {}.", names(&missing)))
                    .color(theme::WEAK)
                    .small(),
            );
        }
    }

    fn next_steps(&mut self, ui: &mut egui::Ui, said: &Said, width: f32) {
        match said {
            Said::Connected(app) => {
                framed(ui, width, |ui| {
                    ui.label(
                        RichText::new(format!(
                            "{} klad is connected to {}. Two more steps:",
                            icon::CHECK,
                            app.name()
                        ))
                        .color(theme::DONE)
                        .strong(),
                    );
                    ui.add_space(4.0);
                    numbered(ui, "1", &format!("{}.", capital(app.next_step())));
                    numbered(ui, "2", "Ask it for a drawing in your own words, for example:");
                    ui.horizontal(|ui| {
                        ui.add_space(18.0);
                        ui.label(RichText::new(format!("\u{201c}{EXAMPLE}\u{201d}")).italics());
                        let word = match self.copied {
                            true => format!("{} Copied", icon::CHECK),
                            false => format!("{} Copy", icon::COPY),
                        };
                        if ui
                            .small_button(word)
                            .hint("copies the example to paste into a new chat")
                            .clicked()
                        {
                            ui.ctx().copy_text(EXAMPLE.into());
                            self.copied = true;
                        }
                    });
                    ui.add_space(4.0);
                    ui.add(
                        egui::Label::new(
                            RichText::new(format!(
                                "The first time, {} asks whether it may use klad: allow it.",
                                app.name()
                            ))
                            .color(theme::WEAK),
                        )
                        .wrap(),
                    );
                });
            }
            Said::Disconnected(app) => {
                ui.add(
                    egui::Label::new(format!("klad is out of {}. {}.", app.name(), capital(app.next_step()))).wrap(),
                );
            }
            Said::Failed(e) => {
                ui.add(egui::Label::new(RichText::new(e).color(theme::ERROR)).wrap());
            }
        }
    }
}

fn framed(ui: &mut egui::Ui, width: f32, add: impl FnOnce(&mut egui::Ui)) {
    let frame = widgets::card_frame();
    let inset = frame.total_margin().sum().x;
    frame.show(ui, |ui| {
        ui.set_width(width - inset);
        add(ui);
    });
}

fn card(ui: &mut egui::Ui, app: App, status: &Status, width: f32) -> Option<bool> {
    let (state, colour, button, hint, connect) = match status {
        Status::Connected(_) => (
            format!("{} Connected", icon::CHECK),
            theme::DONE,
            "Disconnect",
            format!("takes klad out of {}", app.name()),
            false,
        ),
        Status::Moved(_) => (
            "klad moved since you connected it".to_string(),
            theme::REFUSED,
            "Connect again",
            format!("points {} at klad where it is now", app.name()),
            true,
        ),
        Status::Found | Status::Missing => (
            capital(here_word()),
            theme::WEAK,
            "Connect",
            format!("adds klad to {} and leaves the rest of its settings alone", app.name()),
            true,
        ),
    };
    let mut clicked = false;
    let on = ui.ctx().animate_bool_with_time(
        egui::Id::unique(("connected", app.name())),
        matches!(status, Status::Connected(_)),
        0.24,
    );
    let colour = match status {
        Status::Connected(_) => theme::WEAK.lerp_to_gamma(colour, on),
        _ => colour,
    };
    framed(ui, width, |ui| {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new(app.name()).font(theme::medium(theme::BODY)));
                ui.label(RichText::new(state).color(colour));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let made = match connect {
                    true => widgets::primary_button(button),
                    false => widgets::secondary_button(button),
                };
                clicked = ui.add(made.min_size(vec2(104.0, theme::CONTROL))).hint(hint).clicked();
            });
        });
    });
    clicked.then_some(connect)
}

fn numbered(ui: &mut egui::Ui, number: &str, text: &str) {
    ui.horizontal_top(|ui| {
        ui.label(RichText::new(format!("{number}.")).strong());
        ui.add(egui::Label::new(text).wrap());
    });
}

fn folders(ui: &mut egui::Ui, settings: &mut Settings) -> bool {
    let mut changed = false;
    egui::CollapsingHeader::new("Where your agent may work")
        .id_salt("agent folders")
        .show(ui, |ui| {
            ui.add(
                egui::Label::new(
                    RichText::new("Your agent makes and changes drawings only in these folders.").color(theme::WEAK),
                )
                .wrap(),
            );
            ui.add_space(4.0);
            let drawings = settings.drawings_folder();
            let added: Vec<PathBuf> = settings.agent_folders.iter().map(PathBuf::from).collect();
            let recent: Vec<PathBuf> = settings
                .agent_roots()
                .into_iter()
                .filter(|f| *f != drawings && !added.contains(f))
                .collect();
            ui.label(format!(
                "{} Your drawings folder, {}",
                icon::FOLDER_OPEN,
                crate::places::tilde(&drawings)
            ));
            let mut gone = None;
            added.iter().for_each(|folder| {
                ui.horizontal(|ui| {
                    ui.label(format!("{} {}", icon::FOLDER_OPEN, crate::places::tilde(folder)));
                    if ui
                        .small_button("Remove")
                        .hint("your agent can no longer work in this folder")
                        .clicked()
                    {
                        gone = Some(folder.clone());
                    }
                });
            });
            if !recent.is_empty() {
                let list: Vec<String> = recent.iter().map(|f| crate::places::tilde(f)).collect();
                ui.label(format!(
                    "{} The folders of drawings you opened lately ({})",
                    icon::FOLDER_OPEN,
                    recent.len()
                ))
                .on_hover_text(list.join("\n"));
            }
            if let Some(folder) = gone {
                settings.agent_folders.retain(|f| Path::new(f) != folder);
                changed = true;
            }
            ui.add_space(2.0);
            if ui
                .button("Add a folder\u{2026}")
                .hint("your agent may then make and change drawings in it")
                .clicked()
                && let Some(folder) = crate::picker::choose_folder(None, "Your agent may work in")
            {
                let folder = folder.display().to_string();
                if !settings.agent_folders.contains(&folder) {
                    settings.agent_folders.push(folder);
                    changed = true;
                }
            }
        });
    changed
}

pub(crate) fn capital(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}
