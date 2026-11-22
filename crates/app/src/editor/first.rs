use super::Editor;
use crate::commands::Cmd;
use crate::messages::Tone;
use crate::theme::{self, Hint, icon};
use egui::RichText;
use std::path::Path;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct FirstSteps {
    pub open: bool,
    pub painted: usize,
    pub erased: bool,
    pub filled: bool,
    pub reshaped: bool,
    pub handed: bool,
}

pub(super) fn prompt(path: &Path) -> String {
    format!(
        "In my klad drawing {path}, change the stroke I have selected: make it deep blue and twice as thick. \
         I have it open in klad, so I see the change as it lands.",
        path = path.display()
    )
}

const STEPS: [(&str, &str); 6] = [
    (
        "Paint a closed shape with the brush (B)",
        "A stone, a leaf, a cloud. A pen paints with its pressure; a mouse works too.",
    ),
    (
        "Erase part of its line with the eraser (E)",
        "Drag across the line, the way you painted it.",
    ),
    (
        "Paint a second closed shape",
        "Back to the brush with B, and paint it next to the first.",
    ),
    (
        "Fill the second shape (G)",
        "Pick the fill tool and click inside the shape.",
    ),
    (
        "Reshape the first shape with edit points (A)",
        "Click its line and drag one of its points. It is still a brush stroke, and the part you erased bends with it.",
    ),
    (
        "Let your agent change it",
        "With that stroke still picked, copy the request and paste it into a new chat with your agent app. The change shows up here as it lands.",
    ),
];

impl FirstSteps {
    fn ticks(&self) -> [bool; 6] {
        [
            self.painted >= 1,
            self.erased,
            self.painted >= 2,
            self.filled,
            self.reshaped,
            self.handed,
        ]
    }
}

fn step(ui: &mut egui::Ui, done: bool, current: bool, number: usize, (title, detail): (&str, &str)) {
    ui.horizontal_top(|ui| {
        let number = number.to_string();
        let (mark, colour) = match (done, current) {
            (true, _) => (icon::CHECK, theme::DONE),
            (false, true) => (number.as_str(), theme::ACCENT),
            (false, false) => (number.as_str(), theme::WEAK),
        };
        ui.label(RichText::new(mark).strong().color(colour));
        ui.vertical(|ui| {
            let words = RichText::new(title).color(match done {
                true => theme::WEAK,
                false => theme::TEXT,
            });
            ui.add(egui::Label::new(if current { words.strong() } else { words }).wrap());
            if current {
                ui.add(egui::Label::new(RichText::new(detail).color(theme::WEAK)).wrap());
            }
        });
    });
}

impl Editor {
    pub(crate) fn start_first_steps(&mut self) {
        self.first = FirstSteps {
            open: true,
            ..FirstSteps::default()
        };
    }

    pub(super) fn first_steps_window(&mut self, ctx: &egui::Context) {
        if !self.first.open {
            return;
        }
        let mut open = true;
        let layout = &self.settings.layout;
        let dock = if layout.shows_dock() { layout.dock_width } else { 0.0 };
        let corner = egui::pos2(ctx.content_rect().right() - dock - 376.0, self.canvas_rect.top() + 12.0);
        Self::window(
            ctx,
            "first steps",
            (format!("{} First steps", icon::INFO), super::panels::Spot::At(corner)),
            &mut open,
            |ui| {
                ui.set_width(320.0);
                let ticks = self.first.ticks();
                let current = ticks.iter().position(|done| !done);
                STEPS.iter().zip(ticks).enumerate().for_each(|(k, (words, done))| {
                    step(ui, done, current == Some(k), k + 1, *words);
                });
                ui.horizontal(|ui| {
                    self.command_button(ui, Cmd::ConnectAgent, Some("connect an agent app"));
                    if ui
                        .button(format!("{} copy a request", icon::COPY))
                        .hint("a first request for your agent, with this drawing's path in it")
                        .clicked()
                    {
                        ui.ctx().copy_text(prompt(&self.scene.bundle.root));
                        self.first.handed = true;
                        self.say(Tone::Done, "copied a request: paste it into a new chat with your agent");
                    }
                });
                if current == Some(STEPS.len() - 1) {
                    ui.add(
                        egui::Label::new(
                            RichText::new(
                                "Your agent works in a terminal? Install the klad command from the File menu, and ask it to run klad skill first.",
                            )
                            .color(theme::WEAK)
                            .small(),
                        )
                        .wrap(),
                    );
                }
                if current.is_none() {
                    ui.add_space(4.0);
                    ui.add(
                        egui::Label::new(
                            RichText::new(
                                "Done. File > Close shows the samples to look at; Help > Keyboard shortcuts lists the keys.",
                            )
                            .color(theme::DONE),
                        )
                        .wrap(),
                    );
                }
            },
        );
        if !open {
            self.first.open = false;
        }
    }
}
