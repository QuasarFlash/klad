use crate::settings::Settings;
use crate::theme::{self, Hint, icon, leading};
use crate::widgets::{self, Place};
use egui::{Align, Color32, Layout, Rect, RichText, Sense, Stroke, TextWrapMode, UiBuilder, WidgetText, vec2};
use klad_mcp::{App, Entry, Places, Status};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub(crate) struct Connect {
    pub places: Option<Places>,
    pub program: PathBuf,
    pub said: Option<Said>,
    copied: bool,
    steps_height: f32,
    reveal: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Said {
    Connected(App),
    Disconnected(App),
    Failed(String),
}

pub(crate) const EXAMPLE: &str = "Paint a red apple in a new klad drawing and open it in klad.";

const INTRO: &str = "Your agent app can draw in klad: Claude Desktop, Codex and others. It paints stroke by stroke while you watch, and you can fix any stroke by hand.";

pub(crate) const WIDTH: f32 = 568.0;
const OPENING: f32 = 0.22;
const LOOK_AGAIN: std::time::Duration = std::time::Duration::from_secs(1);
const DOT: f32 = 8.0;
const RING: f32 = 1.5;
const GAP: f32 = 14.0;
const BETWEEN: f32 = 2.0;

type Found = Vec<(App, Status)>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Row {
    Open,
    Moved,
    Restart,
    Connected,
}

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
            steps_height: 120.0,
            reveal: false,
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
        self.reveal = true;
        let said = match connect {
            true => match app.connect(places, &entry) {
                Ok(_) => Said::Connected(app),
                Err(e) => Said::Failed(e),
            },
            false => match app.disconnect(places) {
                Ok(_) => Said::Disconnected(app),
                Err(e) => Said::Failed(e),
            },
        };
        self.said = Some(said);
    }

    pub fn any_connected(&self) -> bool {
        self.statuses().iter().any(|(_, s)| matches!(s, Status::Connected(_)))
    }

    pub fn panel(&mut self, ui: &mut egui::Ui, settings: &mut Settings) -> bool {
        let width = ui.available_width().min(WIDTH);
        ui.set_max_width(width);
        ui.add(egui::Label::new(INTRO).wrap());
        ui.add_space(theme::space::SM);
        self.chooser(ui, width);
        ui.add_space(theme::space::SM);
        folders(ui, settings)
    }

    pub fn chooser(&mut self, ui: &mut egui::Ui, width: f32) {
        let Some(places) = self.places.clone() else {
            ui.label(
                RichText::new("This computer has no home folder, where agent apps keep their settings.")
                    .color(theme::WEAK),
            );
            return;
        };
        let (missing, shown): (Found, Found) = App::ALL
            .into_iter()
            .map(|app| (app, app.status(&places)))
            .partition(|(_, s)| matches!(s, Status::Missing));
        let waiting: Vec<App> = shown
            .iter()
            .filter(|(app, status)| matches!(status, Status::Connected(_)) && app.waiting(&places))
            .map(|(app, _)| *app)
            .collect();
        if !waiting.is_empty() {
            ui.ctx().request_repaint_after(LOOK_AGAIN);
        }
        let open =
            ui.ctx()
                .animate_bool_with_time(egui::Id::unique("connect next steps"), self.said.is_some(), OPENING);
        let said = self.said.clone().filter(|_| open > 0.0);
        let count = shown.len().max(1) + usize::from(said.is_some());
        let mut picked = None;
        widgets::list_panel(ui, width, |ui| {
            if shown.is_empty() {
                widgets::list_section(ui, Place::of(0, count), |ui| {
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
                });
            }
            shown.iter().enumerate().for_each(|(k, (app, status))| {
                if let Some(connect) = row(ui, *app, status, waiting.contains(app), Place::of(k, count)) {
                    picked = Some((*app, connect));
                }
            });
            if let Some(said) = &said {
                self.next_steps(ui, said, open, Place::of(count - 1, count));
            }
        });
        if let Some((app, connect)) = picked {
            self.act(app, &places, connect);
        }
        if !missing.is_empty() {
            ui.add_space(theme::space::SM);
            ui.label(
                RichText::new(format!(
                    "Also works with {}.",
                    names(&missing.iter().map(|(app, _)| *app).collect::<Vec<_>>())
                ))
                .color(theme::WEAK)
                .small(),
            );
        }
    }

    fn next_steps(&mut self, ui: &mut egui::Ui, said: &Said, open: f32, place: Place) {
        let eased = 1.0 - (1.0 - open).powi(3);
        let width = ui.available_width();
        let (room, _) = ui.allocate_exact_size(vec2(width, self.steps_height * eased), Sense::hover());
        let mut inner = ui.new_child(
            UiBuilder::new()
                .max_rect(Rect::from_min_size(room.min, vec2(width, f32::INFINITY)))
                .layout(Layout::top_down(Align::Min)),
        );
        inner.set_clip_rect(room.intersect(ui.clip_rect()));
        inner.multiply_opacity(open);
        widgets::list_section(&mut inner, place, |ui| self.next_words(ui, said));
        self.steps_height = inner.min_rect().height();
        if self.reveal && ui.stack().contained_in(egui::UiKind::ScrollArea) {
            ui.scroll_to_rect(Rect::from_min_size(room.min, vec2(width, self.steps_height)), None);
        }
        self.reveal &= open < 1.0;
        if open < 1.0 {
            ui.ctx().request_repaint();
        }
    }

    fn next_words(&mut self, ui: &mut egui::Ui, said: &Said) {
        let line = |text: String, colour: Color32| {
            egui::Label::new(RichText::new(text).line_height(Some(leading::BODY)).color(colour)).wrap()
        };
        match said {
            Said::Connected(app) => {
                ui.add(line(
                    format!("Then ask {} for a drawing in your own words, for example:", app.name()),
                    theme::TEXT,
                ));
                ui.add_space(theme::space::SM);
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(format!("\u{201c}{EXAMPLE}\u{201d}"))
                            .italics()
                            .color(theme::TEXT),
                    );
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
                ui.add_space(theme::space::SM);
                ui.add(line(
                    format!("The first time, {} asks whether it may use klad: allow it.", app.name()),
                    theme::WEAK,
                ));
            }
            Said::Disconnected(app) => {
                ui.add(line(
                    format!("klad is out of {}. {}.", app.name(), capital(app.next_step())),
                    theme::TEXT,
                ));
            }
            Said::Failed(e) => {
                ui.add(line(e.clone(), theme::ERROR));
            }
        }
    }
}

fn row(ui: &mut egui::Ui, app: App, status: &Status, waiting: bool, place: Place) -> Option<bool> {
    let state = match status {
        Status::Connected(_) if waiting => Row::Restart,
        Status::Connected(_) => Row::Connected,
        Status::Moved(_) => Row::Moved,
        Status::Found | Status::Missing => Row::Open,
    };
    let (words, colour, tint) = match state {
        Row::Open => (capital(here_word()), theme::WEAK, Color32::TRANSPARENT),
        Row::Moved => (
            "klad moved since you connected it".to_string(),
            theme::REFUSED,
            Color32::TRANSPARENT,
        ),
        Row::Restart => (
            format!("{} Connected. {}.", icon::RESTART, capital(app.next_step())),
            theme::REFUSED,
            theme::REFUSED_SUBTLE,
        ),
        Row::Connected => (format!("{} Connected", icon::CHECK), theme::DONE, theme::DONE_SUBTLE),
    };
    let (button, hint, connect) = match state {
        Row::Open => (
            "Connect",
            format!("adds klad to {} and leaves the rest of its settings alone", app.name()),
            true,
        ),
        Row::Moved => (
            "Connect again",
            format!("points {} at klad where it is now", app.name()),
            true,
        ),
        Row::Restart | Row::Connected => ("Disconnect", format!("takes klad out of {}", app.name()), false),
    };
    let lit = ui.ctx().animate_bool_with_time(
        egui::Id::unique(("connected", app.name())),
        tint != Color32::TRANSPARENT,
        0.24,
    );
    let fill = match tint == Color32::TRANSPARENT {
        true => Color32::TRANSPARENT,
        false => tint.gamma_multiply(lit),
    };
    widgets::list_row(ui, place, fill, GAP, |ui| {
        let text = ui.available_width() - DOT - widgets::ROW_BUTTON - 2.0 * GAP;
        let (spot, _) = ui.allocate_exact_size(vec2(DOT, DOT), Sense::hover());
        match state {
            Row::Open => {
                ui.painter()
                    .circle_stroke(spot.center(), (DOT - RING) / 2.0, Stroke::new(RING, theme::DISABLED));
            }
            _ => {
                ui.painter().circle_filled(spot.center(), DOT / 2.0, colour);
            }
        }
        let lay = |rich: RichText, wrap: TextWrapMode| -> Arc<egui::Galley> {
            WidgetText::from(rich).into_galley(ui, Some(wrap), text, egui::TextStyle::Body)
        };
        let name = lay(
            RichText::new(app.name())
                .font(theme::medium(theme::HEADING))
                .line_height(Some(leading::HEADING))
                .color(theme::TEXT),
            TextWrapMode::Truncate,
        );
        let state_line = lay(
            RichText::new(words).line_height(Some(leading::BODY)).color(colour),
            TextWrapMode::Wrap,
        );
        let tall = name.size().y + BETWEEN + state_line.size().y;
        ui.allocate_ui_with_layout(vec2(text, tall), Layout::top_down(Align::Min), |ui| {
            ui.set_min_size(vec2(text, tall));
            ui.spacing_mut().item_spacing.y = BETWEEN;
            ui.add(egui::Label::new(name));
            ui.add(egui::Label::new(state_line));
        });
        ui.add(widgets::row_button(button, connect))
            .hint(hint)
            .clicked()
            .then_some(connect)
    })
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
            ui.add_space(theme::space::XS);
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
