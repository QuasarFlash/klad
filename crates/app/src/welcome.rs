use crate::connect::{Connect, main_button};
use crate::theme::{self, Hint, icon};
use anyhow::{Result, anyhow};
use egui::{Align, Color32, Layout, Rect, RichText, Sense, TextureHandle, TextureOptions, UiBuilder, pos2, vec2};
use std::collections::BTreeMap;

const OUT: f64 = 0.12;
const IN: f64 = 0.26;
const STAGGER: f64 = 0.05;
const RISE: f32 = 12.0;
const COLUMN: f32 = 470.0;
const BAR: f32 = 112.0;
const CENTRE: f32 = 0.42;
pub(crate) const SHOWN: f64 = 0.2;
const TILE: f32 = 168.0;
const SIDE: usize = 512;
const REACH: f32 = 60.0;
const SOFT: f32 = 0.08;
const PLATE: f64 = 0.35;
const EMBER: (f64, f64) = (0.3, 0.95);
const INK: (f64, f64) = (0.85, 1.35);
const WORDS: f64 = 1.3;
const BUTTON: f64 = 0.15;
const DOT: f32 = 6.0;
const PILL: f32 = 18.0;
const SETTLED: f64 = 1.8;

pub(crate) const STAGE: Color32 = Color32::from_gray(27);
pub(crate) const LINE: &str = "Paint by hand or let your agent paint.\nEvery stroke stays yours to change.";
pub(crate) const AGENT_LINE: &str =
    "Your agent app paints in klad while you watch, and you can fix any stroke by hand.";
pub(crate) const STEPS_LINE: &str = "A few minutes in a drawing made for it: paint and erase with your pen or mouse, fill a shape, reshape a stroke you painted, and let your agent change it.";
const CHIPS: [(&str, &str); 4] = [
    (icon::BRUSH, "Paint and erase"),
    (icon::FILL, "Fill a shape"),
    (icon::POINTS, "Reshape a stroke"),
    (icon::SPARKLE, "Your agent changes it"),
];
const CHIP: f32 = 90.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    Hello,
    Agent,
    Steps,
}

impl Step {
    const ALL: [Step; 3] = [Step::Hello, Step::Agent, Step::Steps];

    fn index(self) -> usize {
        Self::ALL.iter().position(|s| *s == self).unwrap_or(0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    Done,
    FirstSteps,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Act {
    To(Step),
    End(Outcome),
}

pub(crate) struct Welcome {
    pub step: Step,
    entered: Option<f64>,
    leaving: Option<(Step, f64, Option<f64>)>,
    pub connect: Connect,
    hello: Option<Hello>,
    still: bool,
    heights: [f32; 3],
}

struct Layer {
    rgba: Vec<u8>,
    time: Vec<f32>,
    painted: Vec<usize>,
    texture: Option<TextureHandle>,
    shown: Option<f32>,
}

struct Hello {
    plate: egui::ColorImage,
    plate_texture: Option<TextureHandle>,
    layers: [Layer; 2],
}

fn decode(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info()?;
    let mut buffer = vec![
        0;
        reader
            .output_buffer_size()
            .ok_or_else(|| anyhow!("a picture too large"))?
    ];
    let info = reader.next_frame(&mut buffer)?;
    match (info.color_type, info.width as usize, info.height as usize) {
        (png::ColorType::Rgba, SIDE, SIDE) => Ok(buffer[..info.buffer_size()].to_vec()),
        (kind, w, h) => Err(anyhow!(
            "the welcome's pictures are {SIDE} px rgba, not {w}x{h} {kind:?}"
        )),
    }
}

pub(crate) fn reveal_order(rgba: &[u8], path: &[[f32; 2]]) -> Vec<f32> {
    let lengths: Vec<f32> = path
        .windows(2)
        .scan(0.0, |total, w| {
            *total += (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]);
            Some(*total)
        })
        .collect();
    let whole = lengths.last().copied().unwrap_or(0.0).max(f32::EPSILON);
    let mut time = vec![f32::INFINITY; SIDE * SIDE];
    std::iter::once(0.0)
        .chain(lengths)
        .zip(path)
        .for_each(|(along, [x, y])| {
            let at = along / whole;
            let (x0, x1) = (
                (x - REACH).floor().max(0.0) as usize,
                ((x + REACH).ceil() as usize).min(SIDE),
            );
            let (y0, y1) = (
                (y - REACH).floor().max(0.0) as usize,
                ((y + REACH).ceil() as usize).min(SIDE),
            );
            (y0..y1).for_each(|py| {
                (x0..x1).for_each(|px| {
                    let near = (px as f32 + 0.5 - x).hypot(py as f32 + 0.5 - y) <= REACH;
                    let i = py * SIDE + px;
                    if near && time[i].is_infinite() {
                        time[i] = at;
                    }
                });
            });
        });
    time.iter()
        .zip(rgba.as_chunks::<4>().0)
        .map(|(t, px)| match (px[3], t.is_finite()) {
            (0, _) => f32::INFINITY,
            (_, true) => *t,
            (_, false) => 1.0,
        })
        .collect()
}

impl Layer {
    fn new(rgba: Vec<u8>, path: &[[f32; 2]]) -> Self {
        let time = reveal_order(&rgba, path);
        let painted = (0..SIDE * SIDE).filter(|i| time[*i].is_finite()).collect();
        Self {
            rgba,
            time,
            painted,
            texture: None,
            shown: None,
        }
    }

    fn image(&self, progress: f32) -> egui::ColorImage {
        let mut out = vec![0u8; SIDE * SIDE * 4];
        self.painted.iter().for_each(|&i| {
            let cover = ((progress - self.time[i]) / SOFT).clamp(0.0, 1.0);
            let px = &self.rgba[i * 4..i * 4 + 4];
            out[i * 4..i * 4 + 3].copy_from_slice(&px[..3]);
            out[i * 4 + 3] = (f32::from(px[3]) * cover).round() as u8;
        });
        egui::ColorImage::from_rgba_unmultiplied([SIDE, SIDE], &out)
    }

    fn texture(&mut self, ctx: &egui::Context, name: &str, progress: f32) -> egui::TextureId {
        let progress = progress.clamp(0.0, 1.0 + SOFT);
        if self.shown != Some(progress) {
            let image = self.image(progress);
            match self.texture.as_mut() {
                Some(t) => t.set(image, TextureOptions::LINEAR),
                None => self.texture = Some(ctx.load_texture(name, image, TextureOptions::LINEAR)),
            }
            self.shown = Some(progress);
        }
        self.texture
            .as_ref()
            .map_or(egui::TextureId::default(), TextureHandle::id)
    }
}

impl Hello {
    fn load() -> Result<Self> {
        let paths: BTreeMap<String, Vec<[f32; 2]>> = serde_json::from_str(include_str!("../assets/hello/paths.json"))?;
        let half = |name: &str| -> Vec<[f32; 2]> {
            paths
                .get(name)
                .map(|p| p.iter().map(|[x, y]| [x * 0.5, y * 0.5]).collect())
                .unwrap_or_default()
        };
        let plate = decode(include_bytes!("../assets/hello/plate.png"))?;
        Ok(Self {
            plate: egui::ColorImage::from_rgba_unmultiplied([SIDE, SIDE], &plate),
            plate_texture: None,
            layers: [
                Layer::new(decode(include_bytes!("../assets/hello/ember.png"))?, &half("ember")),
                Layer::new(decode(include_bytes!("../assets/hello/ink.png"))?, &half("ink")),
            ],
        })
    }
}

pub(crate) fn ease(t: f64) -> f32 {
    let t = t.clamp(0.0, 1.0) as f32;
    1.0 - (1.0 - t).powi(3)
}

#[cfg(target_os = "macos")]
fn reduce_motion() -> bool {
    objc2_app_kit::NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
}

#[cfg(windows)]
fn reduce_motion() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SPI_GETCLIENTAREAANIMATION, SystemParametersInfoW};
    let mut animate: windows_sys::core::BOOL = 1;
    let read = unsafe { SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, (&raw mut animate).cast(), 0) } != 0;
    read && animate == 0
}

#[cfg(not(any(target_os = "macos", windows)))]
fn reduce_motion() -> bool {
    false
}

impl Welcome {
    pub fn new(connect: Connect) -> Self {
        Self {
            step: Step::Hello,
            entered: None,
            leaving: None,
            connect,
            hello: None,
            still: !cfg!(test) && reduce_motion(),
            heights: [400.0, 420.0, 240.0],
        }
    }

    fn go(&mut self, to: Step, now: f64) {
        self.leaving = Some((self.step, now, self.entered));
        self.step = to;
        self.entered = None;
    }

    fn since(&self, now: f64) -> f64 {
        self.entered.map_or(0.0, |at| now - at)
    }

    fn fade(&self, now: f64, delay: f64) -> f32 {
        match self.still {
            true => ease(self.since(now) / IN),
            false => ease((self.since(now) - delay) / IN),
        }
    }

    pub fn animating(&self, now: f64) -> bool {
        self.leaving.is_some() || self.entered.is_none() || self.since(now) < SETTLED
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) -> Option<Outcome> {
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        let stage = ui.max_rect();
        if let Some((from, at, shown)) = self.leaving {
            let gone = ((now - at) / OUT) as f32;
            match self.still || gone >= 1.0 {
                true => self.leaving = None,
                false => {
                    ui.scope_builder(UiBuilder::new().max_rect(stage), |ui| {
                        ui.disable();
                        ui.multiply_opacity(1.0 - gone);
                        let was = (
                            std::mem::replace(&mut self.step, from),
                            std::mem::replace(&mut self.entered, shown),
                        );
                        self.screen(ui, stage, now);
                        (self.step, self.entered) = was;
                    });
                    ctx.request_repaint();
                    return None;
                }
            }
        }
        self.entered.get_or_insert(now);
        let act = self.screen(ui, stage, now).or_else(|| self.keys(ui));
        if self.animating(now) {
            ctx.request_repaint();
        }
        match act {
            Some(Act::To(step)) => {
                self.go(step, now);
                ctx.request_repaint();
                None
            }
            Some(Act::End(outcome)) => Some(outcome),
            None => None,
        }
    }

    fn keys(&self, ui: &egui::Ui) -> Option<Act> {
        let (enter, escape) = ui.input(|i| (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape)));
        match (escape, enter, self.step) {
            (true, _, _) => Some(Act::End(Outcome::Done)),
            (_, true, Step::Hello) => Some(Act::To(Step::Agent)),
            (_, true, Step::Agent) => Some(Act::To(Step::Steps)),
            (_, true, Step::Steps) => Some(Act::End(Outcome::FirstSteps)),
            _ => None,
        }
    }

    fn screen(&mut self, ui: &mut egui::Ui, stage: Rect, now: f64) -> Option<Act> {
        let rise = match self.still {
            true => 0.0,
            false => (1.0 - ease(self.since(now) / IN)) * RISE,
        };
        let index = self.step.index();
        let room = stage.height() - BAR;
        let centred = stage.top() + ((room - self.heights[index]) * CENTRE).max(24.0);
        let top = match self.still || self.since(now) < IN {
            true => centred,
            false => ui
                .ctx()
                .animate_value_with_time(egui::Id::unique(("welcome top", index)), centred, 0.25),
        };
        let column = Rect::from_min_max(
            pos2(stage.center().x - COLUMN / 2.0, top + rise),
            pos2(stage.center().x + COLUMN / 2.0, stage.bottom() - BAR),
        );
        let bar = Rect::from_center_size(
            pos2(stage.center().x, stage.bottom() - BAR + 36.0),
            vec2(COLUMN.min(stage.width() - 32.0), 40.0),
        );
        let dots = Rect::from_center_size(pos2(stage.center().x, stage.bottom() - 26.0), vec2(COLUMN, DOT));
        let mut act = None;
        let mut height = None;
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(column)
                .layout(Layout::top_down(Align::Center)),
            |ui| {
                let shown = egui::ScrollArea::vertical()
                    .id_salt("welcome")
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.with_layout(Layout::top_down(Align::Center), |ui| {
                            act = match self.step {
                                Step::Hello => self.hello(ui, now),
                                Step::Agent => self.agent(ui, now),
                                Step::Steps => self.steps(ui, now),
                            };
                        });
                    });
                height = Some(shown.content_size.y);
            },
        );
        if let Some(height) = height {
            self.heights[index] = height;
        }
        self.dots(ui, dots);
        act.or_else(|| self.bar(ui, bar, now))
    }

    fn appear(&self, ui: &mut egui::Ui, now: f64, delay: f64, add: impl FnOnce(&mut egui::Ui)) {
        let shown = self.fade(now, delay);
        ui.scope(|ui| {
            ui.multiply_opacity(shown);
            add(ui);
        });
    }

    fn hello(&mut self, ui: &mut egui::Ui, now: f64) -> Option<Act> {
        ui.add_space(12.0);
        let (tile, _) = ui.allocate_exact_size(vec2(TILE, TILE), Sense::hover());
        self.paint_tile(ui, tile, now);
        ui.add_space(22.0);
        self.appear(ui, now, WORDS, |ui| {
            ui.label(
                RichText::new("klad")
                    .font(theme::wordmark(theme::WORDMARK))
                    .color(theme::TEXT),
            );
        });
        ui.add_space(8.0);
        self.appear(ui, now, WORDS + STAGGER * 2.0, |ui| {
            ui.set_max_width(400.0);
            ui.add(egui::Label::new(RichText::new(LINE).size(theme::HEADING).color(theme::WEAK)).wrap());
        });
        ui.add_space(28.0);
        let mut act = None;
        self.appear(ui, now, BUTTON, |ui| {
            if ui
                .add(main_button(ui, "Get started", vec2(180.0, 36.0)))
                .hint("then connect your agent app and take the first steps, or skip them")
                .clicked()
            {
                act = Some(Act::To(Step::Agent));
            }
        });
        act
    }

    fn paint_tile(&mut self, ui: &mut egui::Ui, tile: Rect, now: f64) {
        if self.hello.is_none() {
            match Hello::load() {
                Ok(hello) => self.hello = Some(hello),
                Err(_) => return,
            }
        }
        let Some(hello) = self.hello.as_mut() else {
            return;
        };
        let ctx = ui.ctx().clone();
        let since = self.entered.map_or(0.0, |at| now - at);
        let full = |window: (f64, f64)| match self.still {
            true => 1.0 + SOFT,
            false => ((since - window.0) / (window.1 - window.0)) as f32 * (1.0 + SOFT),
        };
        let plate_in = match self.still {
            true => 1.0,
            false => ease(since / PLATE),
        };
        let rect = Rect::from_center_size(tile.center(), tile.size() * (0.94 + 0.06 * plate_in));
        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        let opacity = ui.opacity();
        let tint = Color32::WHITE.gamma_multiply(plate_in * opacity);
        let plate = hello
            .plate_texture
            .get_or_insert_with(|| ctx.load_texture("welcome plate", hello.plate.clone(), TextureOptions::LINEAR))
            .id();
        ui.painter().image(plate, rect, uv, tint);
        let progress = [full(EMBER), full(INK)];
        hello
            .layers
            .iter_mut()
            .zip(progress)
            .zip(["welcome ember", "welcome ink"])
            .for_each(|((layer, progress), name)| {
                let id = layer.texture(&ctx, name, progress);
                ui.painter().image(id, rect, uv, Color32::WHITE.gamma_multiply(opacity));
            });
    }

    fn title(&self, ui: &mut egui::Ui, now: f64, title: &str, line: &str) {
        self.appear(ui, now, 0.0, |ui| {
            ui.label(
                RichText::new(title)
                    .font(theme::semibold(theme::DISPLAY))
                    .color(theme::TEXT),
            );
        });
        ui.add_space(6.0);
        self.appear(ui, now, STAGGER, |ui| {
            ui.set_max_width(440.0);
            ui.add(egui::Label::new(RichText::new(line).size(theme::HEADING).color(theme::WEAK)).wrap());
        });
        ui.add_space(18.0);
    }

    fn agent(&mut self, ui: &mut egui::Ui, now: f64) -> Option<Act> {
        self.title(ui, now, &format!("{} Connect your agent app", icon::LINK), AGENT_LINE);
        let shown = self.fade(now, STAGGER * 2.0);
        ui.scope(|ui| {
            ui.multiply_opacity(shown);
            ui.with_layout(Layout::top_down(Align::Min), |ui| {
                ui.set_max_width(470.0);
                self.connect.chooser(ui, 470.0);
            });
        });
        None
    }

    fn steps(&mut self, ui: &mut egui::Ui, now: f64) -> Option<Act> {
        self.title(ui, now, &format!("{} Take the first steps", icon::BRUSH), STEPS_LINE);
        ui.horizontal(|ui| {
            let count = CHIPS.len() as f32;
            let width = count * (CHIP + 20.0) + (count - 1.0) * ui.spacing().item_spacing.x;
            ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
            CHIPS.iter().enumerate().for_each(|(k, (glyph, words))| {
                self.appear(ui, now, STAGGER * (2.0 + 1.6 * k as f64), |ui| {
                    egui::Frame::new()
                        .fill(ui.visuals().faint_bg_color)
                        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
                        .corner_radius(8)
                        .inner_margin(egui::Margin::symmetric(10, 12))
                        .show(ui, |ui| {
                            ui.set_width(CHIP);
                            ui.set_min_height(78.0);
                            ui.vertical_centered(|ui| {
                                ui.label(RichText::new(*glyph).size(theme::ICON_LARGE).color(theme::ACCENT));
                                ui.add_space(4.0);
                                ui.add(egui::Label::new(RichText::new(*words).font(theme::medium(theme::BODY))).wrap());
                            });
                        });
                });
            });
        });
        None
    }

    fn bar(&self, ui: &mut egui::Ui, bar: Rect, now: f64) -> Option<Act> {
        if self.step == Step::Hello {
            return None;
        }
        let mut act = None;
        let shown = self.fade(now, STAGGER * 3.0);
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(bar)
                .layout(Layout::left_to_right(Align::Center)),
            |ui| {
                ui.multiply_opacity(shown);
                let back = match self.step {
                    Step::Steps => Step::Agent,
                    _ => Step::Hello,
                };
                if ui
                    .add(egui::Button::new("Back").min_size(vec2(80.0, 32.0)))
                    .hint("the screen before")
                    .clicked()
                {
                    act = Some(Act::To(back));
                }
            },
        );
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(bar)
                .layout(Layout::right_to_left(Align::Center)),
            |ui| {
                ui.multiply_opacity(shown);
                match self.step {
                    Step::Agent => {
                        let connected = self.connect.any_connected();
                        let (words, hint) = match connected {
                            true => ("Continue", "on to the first steps"),
                            false => (
                                "Skip",
                                "on to the first steps; File > Connect an agent app\u{2026} does it later",
                            ),
                        };
                        let button = match connected {
                            true => main_button(ui, words, vec2(120.0, 32.0)),
                            false => egui::Button::new(words).min_size(vec2(96.0, 32.0)),
                        };
                        if ui.add(button).hint(hint).clicked() {
                            act = Some(Act::To(Step::Steps));
                        }
                    }
                    Step::Steps => {
                        if ui
                            .add(main_button(ui, "Start first steps", vec2(170.0, 32.0)))
                            .hint("opens a drawing made for the first steps, with the steps beside it")
                            .clicked()
                        {
                            act = Some(Act::End(Outcome::FirstSteps));
                        }
                        if ui
                            .add(egui::Button::new("Skip").min_size(vec2(80.0, 32.0)))
                            .hint("to your drawings; Help > First steps is there whenever you want it")
                            .clicked()
                        {
                            act = Some(Act::End(Outcome::Done));
                        }
                    }
                    Step::Hello => {}
                }
            },
        );
        act
    }

    fn dots(&self, ui: &egui::Ui, row: Rect) {
        let ctx = ui.ctx();
        let widths: Vec<f32> = Step::ALL
            .iter()
            .map(|step| {
                let on = ctx.animate_bool_with_time(
                    egui::Id::unique(("welcome dot", step.index())),
                    *step == self.step,
                    0.2,
                );
                DOT + (PILL - DOT) * on
            })
            .collect();
        let gap = 8.0;
        let total: f32 = widths.iter().sum::<f32>() + gap * (widths.len() - 1) as f32;
        let painter = ui.painter();
        widths
            .iter()
            .zip(Step::ALL)
            .scan(row.center().x - total / 2.0, |x, (w, step)| {
                let rect = Rect::from_min_size(pos2(*x, row.center().y - DOT / 2.0), vec2(*w, DOT));
                *x += w + gap;
                Some((rect, step))
            })
            .for_each(|(rect, step)| {
                let colour = match step == self.step {
                    true => theme::TEXT,
                    false => Color32::from_gray(84),
                };
                painter.rect_filled(rect, DOT / 2.0, colour.gamma_multiply(ui.opacity()));
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_k_is_painted_along_its_strokes_from_klads_own_pictures() {
        let hello = Hello::load().unwrap();
        let [ember, ink] = &hello.layers;
        assert!(ember.painted.len() > 10_000 && ink.painted.len() > 10_000);
        let start = |layer: &Layer| {
            layer
                .painted
                .iter()
                .copied()
                .min_by(|a, b| layer.time[*a].total_cmp(&layer.time[*b]))
                .map(|i| (i % SIDE, i / SIDE))
                .unwrap()
        };
        let (x, y) = start(ember);
        assert!(
            x > 300 && y < 200,
            "the ember stroke starts at the arm's tip, top right: {x} {y}"
        );
        let (_, y) = start(ink);
        assert!(y < 150, "the stem starts at its top: {y}");
        assert!(
            ember.painted.iter().all(|i| ember.time[*i] <= 1.0),
            "every painted pixel shows by the end"
        );
        let half = ember.image(0.5);
        let shown = |image: &egui::ColorImage| image.pixels.iter().filter(|p| p.a() > 0).count();
        let all = ember.image(1.0 + SOFT);
        assert!(shown(&half) > 0 && shown(&half) < shown(&all));
        assert_eq!(shown(&all), ember.painted.len());
        assert_eq!(shown(&ember.image(-SOFT)), 0);
    }
}
