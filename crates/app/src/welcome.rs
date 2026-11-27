use crate::connect::Connect;
use crate::theme::{self, Hint, icon, leading};
use crate::widgets::PenScroll;
use crate::widgets::{self, Place};
use anyhow::{Result, anyhow};
use egui::{
    Align, Color32, Layout, Pos2, Rect, RichText, Sense, Shape, Stroke, TextureHandle, TextureOptions, UiBuilder, Vec2,
    emath::Rot2, pos2, vec2,
};
use std::collections::BTreeMap;

const OUT: f64 = 0.12;
const IN: f64 = 0.26;
const STAGGER: f64 = 0.05;
const RISE: f32 = 12.0;
const FRAME: Vec2 = vec2(1040.0, 540.0);
const MARGIN: f32 = 40.0;
const COLUMN: f32 = 400.0;
const GUTTER: f32 = 72.0;
const BRAND: f32 = 72.0;
const BRAND_GAP: f32 = 10.0;
const BRAND_ICON: f32 = 28.0;
const TO_PROGRESS: f32 = 40.0;
const BAR: Vec2 = vec2(32.0, 4.0);
const BAR_GAP: f32 = 6.0;
const TO_HEADING: f32 = 20.0;
const TO_BODY: f32 = 16.0;
const READING: f32 = 380.0;
const ACTION_GAP: f32 = 12.0;
const STEP_GAP: f32 = 16.0;
const TILE: f32 = 40.0;
pub(crate) const SHOWN: f64 = 0.2;
const SIDE: usize = 1024;
const REACH: f32 = 120.0;
const SOFT: f32 = 0.08;
const LOGO: f32 = 306.0;
const EMBER: (f64, f64) = (0.3, 0.95);
const INK: (f64, f64) = (0.85, 1.35);
const AGENT: f64 = INK.1 + 0.1;
const YOU: f64 = AGENT + 0.6;
const HANDLES: f64 = YOU + 0.6;
const POP: f64 = 0.6;
const HANDLES_IN: f64 = 0.5;
const BOB: f64 = 2.8;
const BOB_AFTER: f64 = 0.7;
const SWAY: f32 = 4.0;
const SETTLED: f64 = HANDLES + HANDLES_IN;
const PILL: f32 = 22.0;
const PILL_PAD: f32 = 10.0;
const PILL_TEXT: f32 = 12.0;
const ARROW: [Vec2; 4] = [vec2(0.0, 0.0), vec2(0.0, 15.0), vec2(4.0, 11.2), vec2(10.0, 11.2)];
const ARROW_EDGE: f32 = 1.2;
const AGENT_AT: Vec2 = vec2(14.0, 12.0);
const YOU_AT: Vec2 = vec2(18.0, -8.0);
const YOU_NUDGE: Vec2 = vec2(4.0, 3.0);
const END_NUDGE: Vec2 = vec2(2.0, 2.0);
const SPINE: f32 = 1.5;
const SPINE_UNDER: f32 = 3.5;
const ANCHOR: f32 = 9.0;
const ANCHOR_EDGE: f32 = 2.0;
const KNOB: f32 = 4.0;
const KNOB_EDGE: f32 = 1.2;
const POP_KEYS: [(f32, f32, f32); 4] = [(0.0, 0.0, -20.0), (0.55, 1.2, 7.0), (0.78, 0.93, -3.0), (1.0, 1.0, 0.0)];
const PICTURE: &str =
    "The klad logo painted stroke by stroke. One stroke is selected and shows handles for reshaping it by hand.";

pub(crate) const STAGE: Color32 = theme::BG;
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
    layers: [Layer; 2],
    bounds: Rect,
    arm: [Pos2; 4],
    end: Pos2,
}

#[derive(Clone, Copy)]
struct Columns {
    frame: Rect,
    left: Rect,
    right: Rect,
}

impl Columns {
    fn of(stage: Rect) -> Self {
        let size = (stage.size() - Vec2::splat(2.0 * MARGIN)).min(FRAME).max(Vec2::ZERO);
        let frame = Rect::from_center_size(stage.center(), size);
        let squeeze = (frame.width() / FRAME.x).min(1.0);
        let left = Rect::from_min_size(frame.min, vec2(COLUMN * squeeze, frame.height()));
        let right = Rect::from_min_max(pos2(left.right() + GUTTER * squeeze, frame.top()), frame.max);
        Self { frame, left, right }
    }

    fn progress(&self) -> Rect {
        Rect::from_min_size(
            pos2(self.left.left(), self.frame.top() + BRAND + TO_PROGRESS),
            vec2(3.0 * BAR.x + 2.0 * BAR_GAP, BAR.y),
        )
    }

    fn actions(&self) -> Rect {
        Rect::from_min_max(
            pos2(self.left.left(), self.frame.bottom() - theme::CONTROL_LARGE),
            pos2(self.left.right(), self.frame.bottom()),
        )
    }
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
                let dy = py as f32 + 0.5 - y;
                (x0..x1).for_each(|px| {
                    let i = py * SIDE + px;
                    let dx = px as f32 + 0.5 - x;
                    if time[i].is_infinite() && dx * dx + dy * dy <= REACH * REACH {
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
        let mut out = egui::ColorImage::filled([SIDE, SIDE], Color32::TRANSPARENT);
        self.painted.iter().for_each(|&i| {
            let cover = ((progress - self.time[i]) / SOFT).clamp(0.0, 1.0);
            let px = &self.rgba[i * 4..i * 4 + 4];
            let alpha = (f32::from(px[3]) * cover).round() as u8;
            out.pixels[i] = Color32::from_rgba_unmultiplied(px[0], px[1], px[2], alpha);
        });
        out
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

    fn alpha(&self, at: Pos2) -> u8 {
        let (x, y) = (at.x.round(), at.y.round());
        match (0.0..SIDE as f32).contains(&x) && (0.0..SIDE as f32).contains(&y) {
            true => self.rgba[(y as usize * SIDE + x as usize) * 4 + 3],
            false => 0,
        }
    }

    fn bounds(&self) -> Rect {
        self.painted.iter().fold(Rect::NOTHING, |b, &i| {
            b.union(Rect::from_min_size(
                pos2((i % SIDE) as f32, (i / SIDE) as f32),
                Vec2::splat(1.0),
            ))
        })
    }
}

fn bezier(curve: &[Pos2; 4], t: f32) -> Pos2 {
    let u = 1.0 - t;
    let p = curve[0].to_vec2() * u * u * u
        + curve[1].to_vec2() * 3.0 * u * u * t
        + curve[2].to_vec2() * 3.0 * u * t * t
        + curve[3].to_vec2() * t * t * t;
    p.to_pos2()
}

fn slope(curve: &[Pos2; 4], t: f32) -> Vec2 {
    let u = 1.0 - t;
    (curve[1] - curve[0]) * 3.0 * u * u + (curve[2] - curve[1]) * 6.0 * u * t + (curve[3] - curve[2]) * 3.0 * t * t
}

impl Hello {
    fn load() -> Result<Self> {
        let paths: BTreeMap<String, Vec<[f32; 2]>> = serde_json::from_str(include_str!("../assets/hello/paths.json"))?;
        let path = |name: &str| paths.get(name).cloned().unwrap_or_default();
        let arm: [Pos2; 4] = path("arm")
            .iter()
            .map(|[x, y]| pos2(*x, *y))
            .collect::<Vec<_>>()
            .try_into()
            .map_err(|_| anyhow!("the arm of the K is one curve of four points"))?;
        let ember = Layer::new(decode(include_bytes!("../assets/hello/ember.png"))?, &path("ember"));
        let ink = Layer::new(decode(include_bytes!("../assets/hello/ink.png"))?, &path("ink"));
        let stem = path("ink");
        let end = match stem.as_slice() {
            [.., [ax, ay], [bx, by]] => {
                let (tip, along) = (pos2(*bx, *by), (vec2(bx - ax, by - ay)).normalized());
                (0..SIDE)
                    .map(|k| tip + along * k as f32)
                    .find(|at| ink.alpha(*at) < 16)
                    .unwrap_or(tip)
            }
            _ => pos2(0.0, 0.0),
        };
        let bounds = ember.bounds().union(ink.bounds());
        Ok(Self {
            layers: [ember, ink],
            bounds,
            arm,
            end,
        })
    }
}

pub(crate) fn ease(t: f64) -> f32 {
    let t = t.clamp(0.0, 1.0) as f32;
    1.0 - (1.0 - t).powi(3)
}

fn pop(t: f32) -> (f32, f32, f32) {
    let alpha = (t / POP_KEYS[1].0).clamp(0.0, 1.0);
    let (scale, turn) = match POP_KEYS.windows(2).find(|w| t < w[1].0) {
        Some(w) => {
            let u = ((t - w[0].0) / (w[1].0 - w[0].0)).clamp(0.0, 1.0);
            let e = 1.0 - (1.0 - u) * (1.0 - u);
            (w[0].1 + (w[1].1 - w[0].1) * e, w[0].2 + (w[1].2 - w[0].2) * e)
        }
        None => (1.0, 0.0),
    };
    (scale.max(0.0), turn.to_radians(), alpha)
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

struct Pill<'a> {
    words: &'a str,
    fill: Color32,
    ink: Color32,
    edge: Color32,
    at: Vec2,
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
            heights: [FRAME.y, 2.0 * FRAME.y / 3.0, FRAME.y / 2.0],
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

    fn bobbing(&self, ui: &egui::Ui) -> bool {
        self.step == Step::Hello && !self.still && ui.input(|i| i.viewport().focused != Some(false))
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) -> Option<Outcome> {
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        let columns = Columns::of(ui.max_rect());
        self.chrome(ui, columns);
        if let Some((from, at, shown)) = self.leaving {
            let gone = ((now - at) / OUT) as f32;
            match self.still || gone >= 1.0 {
                true => self.leaving = None,
                false => {
                    ui.scope_builder(UiBuilder::new().max_rect(ui.max_rect()), |ui| {
                        ui.disable();
                        ui.multiply_opacity(1.0 - gone);
                        let was = (
                            std::mem::replace(&mut self.step, from),
                            std::mem::replace(&mut self.entered, shown),
                        );
                        self.screen(ui, columns, now);
                        (self.step, self.entered) = was;
                    });
                    ctx.request_repaint();
                    return None;
                }
            }
        }
        self.entered.get_or_insert(now);
        let act = self.screen(ui, columns, now).or_else(|| self.keys(ui));
        if self.animating(now) || self.bobbing(ui) {
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

    fn chrome(&self, ui: &mut egui::Ui, columns: Columns) {
        let ctx = ui.ctx().clone();
        let branded =
            ctx.animate_bool_with_time(egui::Id::unique("welcome brand"), self.step != Step::Hello, IN as f32);
        if branded > 0.0 {
            let slot = Rect::from_min_size(columns.frame.min, vec2(columns.left.width(), BRAND));
            ui.scope_builder(
                UiBuilder::new()
                    .max_rect(slot)
                    .layout(Layout::left_to_right(Align::Min)),
                |ui| {
                    ui.multiply_opacity(branded);
                    ui.spacing_mut().item_spacing.x = BRAND_GAP;
                    if let Some(texture) = widgets::app_icon(&ctx) {
                        ui.add(egui::Image::new(&texture).fit_to_exact_size(Vec2::splat(BRAND_ICON)));
                    }
                    ui.allocate_ui_with_layout(
                        vec2(columns.left.width(), BRAND_ICON),
                        Layout::left_to_right(Align::Center),
                        |ui| {
                            ui.label(
                                RichText::new("klad")
                                    .font(theme::wordmark(theme::BRAND))
                                    .color(theme::TEXT),
                            );
                        },
                    );
                },
            );
        }
        let bars = columns.progress();
        let index = self.step.index();
        Step::ALL.iter().enumerate().for_each(|(k, step)| {
            let on = ctx.animate_bool_with_time(egui::Id::unique(("welcome bar", step.index())), k <= index, 0.2);
            let bar = Rect::from_min_size(bars.min + vec2(k as f32 * (BAR.x + BAR_GAP), 0.0), BAR);
            ui.painter()
                .rect_filled(bar, BAR.y / 2.0, theme::BORDER_STRONG.lerp_to_gamma(theme::TEXT, on));
        });
    }

    fn screen(&mut self, ui: &mut egui::Ui, columns: Columns, now: f64) -> Option<Act> {
        let rise = match self.still {
            true => 0.0,
            false => (1.0 - ease(self.since(now) / IN)) * RISE,
        };
        let top = columns.progress().bottom() + TO_HEADING;
        let words = Rect::from_min_max(pos2(columns.left.left(), top + rise), columns.left.max);
        let (title, line) = match self.step {
            Step::Hello => (
                RichText::new("klad")
                    .font(theme::wordmark(theme::HERO))
                    .line_height(Some(theme::HERO))
                    .extra_letter_spacing(-0.01 * theme::HERO),
                RichText::new(LINE).size(theme::INTRO).line_height(Some(leading::INTRO)),
            ),
            Step::Agent => (Self::title("Connect your agent app"), Self::lead(AGENT_LINE)),
            Step::Steps => (Self::title("Take the first steps"), Self::lead(STEPS_LINE)),
        };
        ui.scope_builder(
            UiBuilder::new().max_rect(words).layout(Layout::top_down(Align::Min)),
            |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                self.appear(ui, now, 0.0, |ui| {
                    ui.label(title.color(theme::TEXT));
                });
                ui.add_space(TO_BODY);
                self.appear(ui, now, STAGGER, |ui| {
                    ui.set_max_width(READING.min(columns.left.width()));
                    ui.add(egui::Label::new(line.color(theme::WEAK)).wrap());
                });
            },
        );
        let right = columns.right.translate(vec2(0.0, rise));
        let shown = self.fade(now, STAGGER * 2.0);
        ui.scope_builder(UiBuilder::new().max_rect(right), |ui| {
            ui.multiply_opacity(shown);
            match self.step {
                Step::Hello => self.hello(ui, right, now),
                Step::Agent => self.side(ui, right, |welcome, ui, width| welcome.connect.chooser(ui, width)),
                Step::Steps => self.side(ui, right, |_, ui, width| steps(ui, width)),
            }
        });
        self.actions(ui, columns.actions(), now)
    }

    fn title(words: &str) -> RichText {
        RichText::new(words)
            .font(theme::semibold(theme::TITLE))
            .line_height(Some(leading::TITLE))
            .extra_letter_spacing(-0.02 * theme::TITLE)
    }

    fn lead(words: &str) -> RichText {
        RichText::new(words).size(theme::LEAD).line_height(Some(leading::LEAD))
    }

    fn appear(&self, ui: &mut egui::Ui, now: f64, delay: f64, add: impl FnOnce(&mut egui::Ui)) {
        let shown = self.fade(now, delay);
        ui.scope(|ui| {
            ui.multiply_opacity(shown);
            add(ui);
        });
    }

    fn side(&mut self, ui: &mut egui::Ui, room: Rect, add: impl FnOnce(&mut Self, &mut egui::Ui, f32)) {
        let index = self.step.index();
        let tall = self.heights[index].min(room.height());
        let top = room.center().y - tall / 2.0;
        let place = Rect::from_min_max(pos2(room.left(), top), room.max);
        let mut height = None;
        ui.scope_builder(
            UiBuilder::new().max_rect(place).layout(Layout::top_down(Align::Min)),
            |ui| {
                ui.spacing_mut().scroll.fade = egui::style::ScrollFadeStyle {
                    strength: 1.0,
                    size: 2.0 * theme::space::XL,
                };
                let shown = egui::ScrollArea::vertical()
                    .id_salt(("welcome side", index))
                    .auto_shrink([false, true])
                    .max_height(room.bottom() - top)
                    .pen_scroll(ui.ctx())
                    .show(ui, |ui| add(self, ui, room.width()));
                height = Some(shown.content_size.y);
            },
        );
        if let Some(height) = height {
            self.heights[index] = height;
        }
    }

    fn hello(&mut self, ui: &mut egui::Ui, panel: Rect, now: f64) {
        let response = ui.interact(panel, egui::Id::unique("welcome picture"), Sense::hover());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, PICTURE));
        let painter = ui.painter_at(panel.expand(theme::HAIRLINE));
        painter.rect(
            panel,
            theme::radius::PANEL,
            theme::PLATE,
            Stroke::new(theme::HAIRLINE, theme::BORDER),
            egui::StrokeKind::Inside,
        );
        if self.hello.is_none() {
            match Hello::load() {
                Ok(hello) => self.hello = Some(hello),
                Err(_) => return,
            }
        }
        let since = self.since(now);
        let still = self.still;
        let bob = self.bobbing(ui);
        let Some(hello) = self.hello.as_mut() else {
            return;
        };
        let ctx = ui.ctx().clone();
        let scale = LOGO / hello.bounds.height();
        let origin = panel.center() - hello.bounds.center().to_vec2() * scale;
        let at = |p: Pos2| origin + p.to_vec2() * scale;
        let picture = Rect::from_min_size(origin, Vec2::splat(SIDE as f32 * scale));
        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        let full = |window: (f64, f64)| match still {
            true => 1.0 + SOFT,
            false => ((since - window.0) / (window.1 - window.0)) as f32 * (1.0 + SOFT),
        };
        hello
            .layers
            .iter_mut()
            .zip([full(EMBER), full(INK)])
            .zip(["welcome ember", "welcome ink"])
            .for_each(|((layer, progress), name)| {
                let id = layer.texture(&ctx, name, progress);
                painter.image(id, picture, uv, Color32::WHITE);
            });
        let after = |start: f64, span: f64| match still {
            true => 1.0,
            false => ((since - start) / span).clamp(0.0, 1.0) as f32,
        };
        let arm = hello.arm.map(&at);
        let mid = bezier(&arm, 0.5);
        let reach = slope(&arm, 0.5) / 6.0;
        let handles = ease(f64::from(after(HANDLES, HANDLES_IN)));
        if handles > 0.0 {
            edit_handles(&painter, &arm, mid, reach, handles);
        }
        let sway = |start: f64| match bob && !still && since > start {
            true => -SWAY * (1.0 - (std::f64::consts::TAU * (since - start) / BOB).cos() as f32) / 2.0,
            false => 0.0,
        };
        let agent = Pill {
            words: "your agent",
            fill: theme::BG,
            ink: theme::TEXT,
            edge: theme::PLATE,
            at: AGENT_AT,
        };
        let you = Pill {
            words: "you",
            fill: theme::ACCENT_FILL,
            ink: theme::ON_ACCENT,
            edge: theme::ON_ACCENT,
            at: YOU_AT,
        };
        let agent_tip = at(hello.end) + END_NUDGE + vec2(0.0, sway(AGENT + BOB_AFTER));
        let you_tip = mid - reach + YOU_NUDGE + vec2(0.0, sway(AGENT + BOB_AFTER + BOB / 2.0));
        [(agent, agent_tip, AGENT), (you, you_tip, YOU)]
            .into_iter()
            .for_each(|(pill, tip, start)| {
                let t = after(start, POP);
                if t > 0.0 {
                    paint_pill(&painter, &pill, tip, pop(t));
                }
            });
    }

    fn actions(&self, ui: &mut egui::Ui, row: Rect, now: f64) -> Option<Act> {
        let shown = self.fade(now, STAGGER * 3.0);
        let mut act = None;
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(row)
                .layout(Layout::left_to_right(Align::Center)),
            |ui| {
                ui.multiply_opacity(shown);
                ui.spacing_mut().item_spacing.x = ACTION_GAP;
                let mut button = |ui: &mut egui::Ui, made: egui::Button<'_>, hint: &str, to: Act| {
                    if ui.add(made).hint(hint).clicked() {
                        act = Some(to);
                    }
                };
                match self.step {
                    Step::Hello => button(
                        ui,
                        widgets::large_primary_button("Get started"),
                        "then connect your agent app and take the first steps, or skip them",
                        Act::To(Step::Agent),
                    ),
                    Step::Agent => {
                        match self.connect.any_connected() {
                            true => button(
                                ui,
                                widgets::large_primary_button("Continue"),
                                "on to the first steps",
                                Act::To(Step::Steps),
                            ),
                            false => button(
                                ui,
                                widgets::large_ghost_button("Skip"),
                                "on to the first steps; File > Connect an agent app\u{2026} does it later",
                                Act::To(Step::Steps),
                            ),
                        }
                        button(
                            ui,
                            widgets::large_ghost_button("Back"),
                            "the screen before",
                            Act::To(Step::Hello),
                        );
                    }
                    Step::Steps => {
                        button(
                            ui,
                            widgets::large_primary_button("Start first steps"),
                            "opens a drawing made for the first steps, with the steps beside it",
                            Act::End(Outcome::FirstSteps),
                        );
                        button(
                            ui,
                            widgets::large_ghost_button("Skip"),
                            "to your drawings; Help > First steps is there whenever you want it",
                            Act::End(Outcome::Done),
                        );
                        button(
                            ui,
                            widgets::large_ghost_button("Back"),
                            "the screen before",
                            Act::To(Step::Agent),
                        );
                    }
                }
            },
        );
        act
    }
}

fn steps(ui: &mut egui::Ui, width: f32) {
    widgets::list_panel(ui, width, |ui| {
        CHIPS.iter().enumerate().for_each(|(k, (glyph, words))| {
            widgets::list_row(ui, Place::of(k, CHIPS.len()), Color32::TRANSPARENT, STEP_GAP, |ui| {
                let (tile, _) = ui.allocate_exact_size(Vec2::splat(TILE), Sense::hover());
                ui.painter().rect_filled(tile, theme::radius::CARD, theme::CONTROL_FILL);
                ui.painter().text(
                    tile.center(),
                    egui::Align2::CENTER_CENTER,
                    *glyph,
                    egui::FontId::proportional(theme::ICON_LARGE),
                    theme::ACCENT,
                );
                ui.label(
                    RichText::new(*words)
                        .font(theme::medium(theme::LEAD))
                        .line_height(Some(leading::LIST))
                        .color(theme::TEXT),
                );
            });
        });
    });
}

fn edit_handles(painter: &egui::Painter, arm: &[Pos2; 4], mid: Pos2, reach: Vec2, shown: f32) {
    let blue = theme::ACCENT_FILL.gamma_multiply(shown);
    let white = Color32::WHITE.gamma_multiply(shown);
    [(SPINE_UNDER, white), (SPINE, blue)]
        .into_iter()
        .for_each(|(width, colour)| {
            painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
                *arm,
                false,
                Color32::TRANSPARENT,
                Stroke::new(width, colour),
            ));
        });
    let ends = [mid - reach, mid + reach];
    painter.line_segment(ends, Stroke::new(SPINE, blue));
    ends.into_iter().for_each(|end| {
        painter.circle(end, KNOB, blue, Stroke::new(KNOB_EDGE, white));
    });
    [arm[3], mid, arm[0]].into_iter().for_each(|anchor| {
        painter.rect(
            Rect::from_center_size(anchor, Vec2::splat(ANCHOR)),
            0.0,
            white,
            Stroke::new(ANCHOR_EDGE, blue),
            egui::StrokeKind::Middle,
        );
    });
}

fn paint_pill(painter: &egui::Painter, pill: &Pill<'_>, tip: Pos2, (scale, turn, alpha): (f32, f32, f32)) {
    if scale <= 0.01 {
        return;
    }
    let spin = Rot2::from_angle(turn);
    let place = |offset: Vec2| tip + spin * (offset * scale);
    let fill = pill.fill.gamma_multiply(alpha);
    let edge = pill.edge.gamma_multiply(alpha);
    let arrow = ARROW.map(place);
    [[arrow[0], arrow[1], arrow[2]], [arrow[0], arrow[2], arrow[3]]]
        .into_iter()
        .for_each(|triangle| {
            painter.add(Shape::convex_polygon(triangle.to_vec(), fill, Stroke::NONE));
        });
    painter.add(Shape::closed_line(
        arrow.to_vec(),
        Stroke::new(ARROW_EDGE * scale, edge),
    ));
    let size = (PILL_TEXT * scale * 2.0).round() / 2.0;
    if size < 1.0 {
        return;
    }
    let galley = painter.layout_no_wrap(
        pill.words.to_string(),
        theme::semibold(size),
        pill.ink.gamma_multiply(alpha),
    );
    let wide = galley.size().x / scale + 2.0 * PILL_PAD;
    let body = Rect::from_min_size(tip + pill.at * scale, vec2(wide, PILL) * scale);
    painter.add(egui::epaint::RectShape::filled(body, PILL * scale / 2.0, fill).with_angle_and_pivot(turn, tip));
    let corner = body.center() - galley.size() / 2.0;
    let text_at = tip + spin * (corner - tip);
    painter.add(egui::epaint::TextShape::new(text_at, galley, pill.ink).with_angle(turn));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_k_is_painted_along_its_strokes_from_klads_own_pictures() {
        let hello = Hello::load().unwrap();
        let [ember, ink] = &hello.layers;
        assert!(ember.painted.len() > 40_000 && ink.painted.len() > 40_000);
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
            x > 600 && y < 400,
            "the ember stroke starts at the arm's tip, top right: {x} {y}"
        );
        let (_, y) = start(ink);
        assert!(y < 300, "the stem starts at its top: {y}");
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

    #[test]
    fn the_handles_sit_on_the_upper_arm_and_your_agent_waits_where_the_stem_ends() {
        let hello = Hello::load().unwrap();
        let [ember, ink] = &hello.layers;
        let mid = bezier(&hello.arm, 0.5);
        assert!(
            [hello.arm[0], mid, hello.arm[3]].iter().all(|p| ember.alpha(*p) > 200),
            "the three anchors lie on the painted arm"
        );
        let reach = slope(&hello.arm, 0.5) / 6.0;
        assert!(
            reach.x < 0.0 && reach.y > 0.0,
            "the arm runs from its tip down to the stem: {reach:?}"
        );
        assert!(ink.alpha(hello.end) < 16 && ink.alpha(hello.end - vec2(0.0, 8.0)) > 0);
        assert!(
            hello.end.y > hello.bounds.bottom() - 40.0,
            "the stem ends at the foot of the K: {:?} {:?}",
            hello.end,
            hello.bounds
        );
    }

    #[test]
    fn a_pill_pops_past_its_size_and_settles() {
        let (start, _, hidden) = pop(0.0);
        assert!(start == 0.0 && hidden == 0.0);
        let peak = (1..100).map(|k| pop(k as f32 / 100.0).0).fold(0.0_f32, f32::max);
        assert!((peak - 1.2).abs() < 0.02, "{peak}");
        assert_eq!(pop(1.0), (1.0, 0.0, 1.0));
        assert_eq!(pop(2.0), (1.0, 0.0, 1.0));
    }
}
