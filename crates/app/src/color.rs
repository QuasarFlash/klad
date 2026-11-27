use crate::theme::{self, Hint};
use egui::containers::{Popup, PopupCloseBehavior};
use egui::{Color32, Mesh, Pos2, Rect, Sense, Shape, Stroke, Vec2, pos2, vec2};
use klad_core::color::parse_hex;
use std::hash::Hash;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hsva {
    pub h: f32,
    pub s: f32,
    pub v: f32,
    pub a: f32,
}

pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> [f32; 3] {
    let h = h.rem_euclid(1.0) * 6.0;
    let sector = h.floor();
    let f = h - sector;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    match sector as u8 {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}

impl Hsva {
    pub fn from_rgba([r, g, b, a]: [f32; 4], keep_hue: f32) -> Self {
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let d = max - min;
        let h = if d <= 1e-6 {
            keep_hue
        } else if max == r {
            ((g - b) / d).rem_euclid(6.0) / 6.0
        } else if max == g {
            ((b - r) / d + 2.0) / 6.0
        } else {
            ((r - g) / d + 4.0) / 6.0
        };
        let s = if max <= 1e-6 { 0.0 } else { d / max };
        Self { h, s, v: max, a }
    }

    pub fn rgba(&self) -> [f32; 4] {
        let [r, g, b] = hsv_to_rgb(self.h, self.s, self.v);
        [r, g, b, self.a]
    }
}

pub fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [f32; 3] {
    let v = l + s * l.min(1.0 - l);
    let sv = match v <= 1e-6 {
        true => 0.0,
        false => 2.0 * (1.0 - l / v),
    };
    hsv_to_rgb(h, sv, v)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Model {
    #[default]
    Hsv,
    Hsl,
    Rgb,
    Lab,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Channel {
    pub name: &'static str,
    pub lo: f32,
    pub hi: f32,
    pub about: &'static str,
}

const fn channel(name: &'static str, lo: f32, hi: f32, about: &'static str) -> Channel {
    Channel { name, lo, hi, about }
}

impl Model {
    pub const ALL: [Model; 4] = [Model::Hsv, Model::Hsl, Model::Rgb, Model::Lab];

    pub fn name(self) -> &'static str {
        match self {
            Model::Hsv => "HSV",
            Model::Hsl => "HSL",
            Model::Rgb => "RGB",
            Model::Lab => "Lab",
        }
    }

    pub fn about(self) -> &'static str {
        match self {
            Model::Hsv => "hue, saturation and value: the wheel's own numbers",
            Model::Hsl => "hue, saturation and lightness: lightness runs from black through the pure colour to white",
            Model::Rgb => "red, green and blue, 0 to 255, as in a hex colour",
            Model::Lab => {
                "lightness as the eye sees it, with green to red (a) and blue to yellow (b): change L alone for a shadow or a light of the same colour"
            }
        }
    }

    pub fn channels(self) -> [Channel; 3] {
        match self {
            Model::Hsv => [
                channel("H", 0.0, 360.0, "hue, in degrees around the wheel"),
                channel("S", 0.0, 100.0, "saturation: grey to the pure colour"),
                channel("V", 0.0, 100.0, "value: black to the full colour"),
            ],
            Model::Hsl => [
                channel("H", 0.0, 360.0, "hue, in degrees around the wheel"),
                channel("S", 0.0, 100.0, "saturation: grey to the pure colour"),
                channel("L", 0.0, 100.0, "lightness: black, the pure colour at 50, white"),
            ],
            Model::Rgb => [
                channel("R", 0.0, 255.0, "red"),
                channel("G", 0.0, 255.0, "green"),
                channel("B", 0.0, 255.0, "blue"),
            ],
            Model::Lab => [
                channel("L", 0.0, 100.0, "lightness as the eye sees it"),
                channel("a", -128.0, 127.0, "green to red"),
                channel("b", -128.0, 127.0, "blue to yellow"),
            ],
        }
    }

    pub fn values(self, rgba: [f32; 4], keep: [f32; 3]) -> [f32; 3] {
        let [r, g, b, _] = rgba;
        let (max, min) = (r.max(g).max(b), r.min(g).min(b));
        let d = max - min;
        let hue = Hsva::from_rgba(rgba, keep[0] / 360.0).h * 360.0;
        match self {
            Model::Hsv => [
                hue,
                match max <= 1e-6 {
                    true => keep[1],
                    false => d / max * 100.0,
                },
                max * 100.0,
            ],
            Model::Hsl => {
                let l = (max + min) / 2.0;
                let room = 1.0 - (2.0 * l - 1.0).abs();
                [
                    hue,
                    match room <= 1e-6 {
                        true => keep[1],
                        false => (d / room).min(1.0) * 100.0,
                    },
                    l * 100.0,
                ]
            }
            Model::Rgb => [r * 255.0, g * 255.0, b * 255.0],
            Model::Lab => klad_core::color::to_lab(rgba),
        }
    }

    pub fn rgb(self, [x, y, z]: [f32; 3]) -> [f32; 3] {
        match self {
            Model::Hsv => hsv_to_rgb(x / 360.0, y / 100.0, z / 100.0),
            Model::Hsl => hsl_to_rgb(x / 360.0, y / 100.0, z / 100.0),
            Model::Rgb => [x / 255.0, y / 255.0, z / 255.0],
            Model::Lab => {
                let [r, g, b, _] = klad_core::color::from_lab([x, y, z], 1.0);
                [r, g, b]
            }
        }
    }
}

pub fn channel_slider(
    ui: &mut egui::Ui,
    channel: &Channel,
    value: &mut f32,
    width: f32,
    ramp: impl Fn(f32) -> [f32; 3],
) -> egui::Response {
    let (rect, mut response) = ui.allocate_exact_size(vec2(width, theme::CONTROL), Sense::click_and_drag());
    let shown = value.round() as f64;
    response.widget_info(|| egui::WidgetInfo::slider(true, shown, channel.name));
    let span = channel.hi - channel.lo;
    if let Some(pos) = response.interact_pointer_pos() {
        let t = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
        let wanted = channel.lo + t * span;
        if wanted != *value {
            *value = wanted;
            response.mark_changed();
        }
    }
    let painter = ui.painter_at(rect.expand(2.0));
    let track = rect.shrink2(vec2(0.0, 6.0));
    let steps = 32;
    let mut mesh = Mesh::default();
    (0..=steps).for_each(|i| {
        let t = i as f32 / steps as f32;
        let [r, g, b] = ramp(channel.lo + t * span);
        let c = color32([r, g, b, 1.0]);
        let x = track.left() + track.width() * t;
        mesh.colored_vertex(pos2(x, track.top()), c);
        mesh.colored_vertex(pos2(x, track.bottom()), c);
        if i > 0 {
            let k = (i as u32) * 2;
            mesh.add_triangle(k - 2, k - 1, k);
            mesh.add_triangle(k - 1, k + 1, k);
        }
    });
    painter.add(Shape::mesh(mesh));
    painter.rect_stroke(
        track,
        2.0,
        Stroke::new(1.0, Color32::from_black_alpha(160)),
        egui::StrokeKind::Outside,
    );
    let at = track.left() + track.width() * ((*value - channel.lo) / span).clamp(0.0, 1.0);
    let handle = Rect::from_center_size(pos2(at, rect.center().y), vec2(6.0, rect.height() - 6.0));
    painter.rect_stroke(
        handle,
        2.0,
        Stroke::new(2.5, Color32::from_black_alpha(200)),
        egui::StrokeKind::Outside,
    );
    painter.rect_stroke(handle, 2.0, Stroke::new(1.5, Color32::WHITE), egui::StrokeKind::Inside);
    response.hint(channel.about)
}

pub fn color32([r, g, b, a]: [f32; 4]) -> Color32 {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgba_unmultiplied(byte(r), byte(g), byte(b), byte(a))
}

pub fn hex(rgba: [f32; 4]) -> String {
    let [r, g, b, a] = rgba.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
    if a == 255 {
        format!("#{r:02x}{g:02x}{b:02x}")
    } else {
        format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
    }
}

pub fn from_srgb(rgb: [u8; 3]) -> [f32; 4] {
    let [r, g, b] = rgb.map(|v| v as f32 / 255.0);
    [r, g, b, 1.0]
}

pub fn to_srgb([r, g, b, _]: [f32; 4]) -> [u8; 3] {
    [r, g, b].map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
}

pub fn rgba_of(text: &str) -> Option<[f32; 4]> {
    let trimmed = text.trim();
    let with_hash = if trimmed.starts_with('#') {
        trimmed.to_string()
    } else {
        format!("#{trimmed}")
    };
    parse_hex(&with_hash).ok()
}

#[derive(Clone, Copy, PartialEq)]
enum Part {
    Ring,
    Square,
}

fn square(center: Pos2, inner: f32) -> Rect {
    Rect::from_center_size(center, Vec2::splat(inner * std::f32::consts::SQRT_2 - 8.0))
}

pub fn picker(ui: &mut egui::Ui, color: &mut Hsva, size: f32) -> egui::Response {
    let (rect, mut response) = ui.allocate_exact_size(Vec2::splat(size), Sense::click_and_drag());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Slider, true, "colour wheel"));
    let center = rect.center();
    let outer = size / 2.0 - 1.0;
    let inner = outer - (size * 0.09).max(10.0);
    let sv = square(center, inner);
    let id = response.id.with("part");
    let mut changed = false;
    if let Some(pos) = response.interact_pointer_pos() {
        if response.drag_started() || response.clicked() || ui.data(|d| d.get_temp::<u8>(id)).is_none() {
            let d = (pos - center).length();
            let part = if d >= inner - 2.0 && d <= outer + 4.0 {
                Some(Part::Ring)
            } else if sv.expand(4.0).contains(pos) {
                Some(Part::Square)
            } else {
                None
            };
            ui.data_mut(|data| match part {
                Some(Part::Ring) => data.insert_temp(id, 1u8),
                Some(Part::Square) => data.insert_temp(id, 2u8),
                None => data.insert_temp(id, 0u8),
            });
        }
        match ui.data(|d| d.get_temp::<u8>(id)) {
            Some(1) => {
                let d = pos - center;
                color.h = (d.y.atan2(d.x) / std::f32::consts::TAU).rem_euclid(1.0);
                changed = true;
            }
            Some(2) => {
                color.s = ((pos.x - sv.left()) / sv.width()).clamp(0.0, 1.0);
                color.v = 1.0 - ((pos.y - sv.top()) / sv.height()).clamp(0.0, 1.0);
                changed = true;
            }
            _ => {}
        }
    }
    if response.drag_stopped() || (!response.dragged() && !response.is_pointer_button_down_on()) {
        ui.data_mut(|d| d.remove::<u8>(id));
    }
    let painter = ui.painter_at(rect.expand(2.0));
    let mut ring = Mesh::default();
    let segments = 96;
    (0..=segments).for_each(|i| {
        let t = i as f32 / segments as f32;
        let angle = t * std::f32::consts::TAU;
        let dir = vec2(angle.cos(), angle.sin());
        let [r, g, b] = hsv_to_rgb(t, 1.0, 1.0);
        let c = color32([r, g, b, 1.0]);
        ring.colored_vertex(center + dir * inner, c);
        ring.colored_vertex(center + dir * outer, c);
        if i > 0 {
            let k = (i as u32) * 2;
            ring.add_triangle(k - 2, k - 1, k);
            ring.add_triangle(k - 1, k + 1, k);
        }
    });
    painter.add(Shape::mesh(ring));
    let mut field = Mesh::default();
    let cells = 16;
    (0..=cells).for_each(|j| {
        (0..=cells).for_each(|i| {
            let (s, v) = (i as f32 / cells as f32, 1.0 - j as f32 / cells as f32);
            let [r, g, b] = hsv_to_rgb(color.h, s, v);
            field.colored_vertex(
                pos2(sv.left() + sv.width() * s, sv.top() + sv.height() * (1.0 - v)),
                color32([r, g, b, 1.0]),
            );
            if i > 0 && j > 0 {
                let at = |x: u32, y: u32| y * (cells + 1) + x;
                field.add_triangle(at(i - 1, j - 1), at(i, j - 1), at(i, j));
                field.add_triangle(at(i - 1, j - 1), at(i, j), at(i - 1, j));
            }
        });
    });
    painter.add(Shape::mesh(field));
    let dark = Stroke::new(2.5, Color32::from_black_alpha(200));
    let light = Stroke::new(1.5, Color32::WHITE);
    let hue_at = center
        + vec2(
            (color.h * std::f32::consts::TAU).cos(),
            (color.h * std::f32::consts::TAU).sin(),
        ) * (inner + outer)
            / 2.0;
    painter.circle_stroke(hue_at, (outer - inner) / 2.0, dark);
    painter.circle_stroke(hue_at, (outer - inner) / 2.0, light);
    let sv_at = pos2(
        sv.left() + sv.width() * color.s,
        sv.top() + sv.height() * (1.0 - color.v),
    );
    painter.circle_stroke(sv_at, 5.0, dark);
    painter.circle_stroke(sv_at, 5.0, light);
    if changed {
        response.mark_changed();
    }
    response.hint("the ring sets the hue; the square inside sets how strong and how bright")
}

pub fn swatch(ui: &mut egui::Ui, rgba: [f32; 4], selected: bool, size: f32) -> egui::Response {
    swatch_with(ui, rgba, selected, size, Sense::click())
}

pub fn swatch_with(ui: &mut egui::Ui, rgba: [f32; 4], selected: bool, size: f32, sense: Sense) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), sense);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, hex(rgba)));
    paint_swatch(ui, rect, rgba, selected);
    response
}

fn paint_swatch(ui: &egui::Ui, rect: Rect, rgba: [f32; 4], selected: bool) {
    let painter = ui.painter_at(rect);
    let half = rect.width() / 2.0;
    painter.rect_filled(rect, 2.0, Color32::from_gray(200));
    painter.rect_filled(
        Rect::from_min_size(rect.min, Vec2::splat(half)),
        0.0,
        Color32::from_gray(140),
    );
    painter.rect_filled(
        Rect::from_min_size(rect.min + Vec2::splat(half), Vec2::splat(half)),
        0.0,
        Color32::from_gray(140),
    );
    painter.rect_filled(rect, 2.0, color32(rgba));
    let outline = if selected {
        Stroke::new(2.0, Color32::WHITE)
    } else {
        Stroke::new(1.0, Color32::from_black_alpha(160))
    };
    painter.rect_stroke(rect, 2.0, outline, egui::StrokeKind::Inside);
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pick {
    Dragging([f32; 4]),
    Chosen([f32; 4]),
    Remove,
}

impl Pick {
    pub fn chosen(self) -> Option<[f32; 4]> {
        match self {
            Pick::Chosen(rgba) => Some(rgba),
            _ => None,
        }
    }
}

pub struct Square {
    id: egui::Id,
    rgba: [f32; 4],
    what: String,
    current: Option<[f32; 4]>,
    alpha: bool,
    removable: Option<(String, String)>,
    upward: bool,
    palette: Vec<Shelf>,
}

struct Shelf {
    title: String,
    colours: Vec<([f32; 4], String)>,
}

impl Square {
    pub fn new(ui: &egui::Ui, salt: impl Hash + std::fmt::Debug, rgba: [f32; 4], what: impl Into<String>) -> Self {
        Self {
            id: ui.make_persistent_id(salt),
            rgba,
            what: what.into(),
            current: None,
            alpha: false,
            removable: None,
            upward: false,
            palette: Vec::new(),
        }
    }

    pub fn colours(mut self, title: &str, colours: Vec<([f32; 4], String)>) -> Self {
        if !colours.is_empty() {
            self.palette.push(Shelf {
                title: title.to_string(),
                colours,
            });
        }
        self
    }

    pub fn upward(mut self, up: bool) -> Self {
        self.upward = up;
        self
    }

    pub fn current(mut self, rgba: [f32; 4]) -> Self {
        self.current = Some(rgba);
        self
    }

    pub fn alpha(mut self) -> Self {
        self.alpha = true;
        self
    }

    pub fn removable(self) -> Self {
        self.removable_as("remove", "take this colour out")
    }

    pub fn removable_as(mut self, label: &str, hint: &str) -> Self {
        self.removable = Some((label.to_string(), hint.to_string()));
        self
    }

    pub fn pick(self, ui: &mut egui::Ui) -> Option<[f32; 4]> {
        self.show(ui).and_then(Pick::chosen)
    }

    fn live(&self, ui: &egui::Ui) -> (Hsva, bool) {
        let live_id = self.id.with("live");
        let open = Popup::is_id_open(ui.ctx(), self.id);
        let stored = ui.data(|d| d.get_temp::<(Hsva, [f32; 4])>(live_id));
        let live = match stored {
            Some((hsva, base)) if open && hex(base) == hex(self.rgba) => hsva,
            _ => Hsva::from_rgba(self.rgba, stored.map_or(0.0, |(hsva, _)| hsva.h)),
        };
        if !open && stored.is_some() {
            ui.data_mut(|d| d.remove::<(Hsva, [f32; 4])>(live_id));
        }
        (live, open)
    }

    pub fn show(self, ui: &mut egui::Ui) -> Option<Pick> {
        let (live, open) = self.live(ui);
        let shown = if open { live.rgba() } else { self.rgba };
        let response = swatch(ui, shown, open, theme::CONTROL);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &self.what));
        let response = response.hint(format!("{}, {}; click to pick a colour", self.what, hex(self.rgba)));
        self.popup(ui, &response, live)
    }

    pub fn under(self, ui: &mut egui::Ui, button: &egui::Response) -> Option<Pick> {
        let (live, _) = self.live(ui);
        self.popup(ui, button, live)
    }

    fn popup(self, ui: &mut egui::Ui, response: &egui::Response, mut live: Hsva) -> Option<Pick> {
        let live_id = self.id.with("live");
        let text_id = self.id.with("hex");
        let mut out = None;
        let popup = Popup::from_toggle_button_response(response)
            .id(self.id)
            .close_behavior(PopupCloseBehavior::CloseOnClickOutside);
        let popup = match self.upward {
            true => popup.align(egui::RectAlign::TOP_START),
            false => popup,
        };
        popup.show(|ui| {
            ui.label(egui::RichText::new(&self.what).color(theme::WEAK));
            let wheel = picker(ui, &mut live, 150.0);
            if wheel.drag_stopped() || (wheel.clicked() && wheel.changed()) {
                out = Some(Pick::Chosen(live.rgba()));
            } else if wheel.changed() {
                out = Some(Pick::Dragging(live.rgba()));
            }
            if self.alpha {
                let slider = ui
                    .add(egui::Slider::new(&mut live.a, 0.0..=1.0).text("opacity"))
                    .hint("how see-through this colour is");
                if slider.drag_stopped() || (slider.changed() && !slider.dragged()) {
                    out = Some(Pick::Chosen(live.rgba()));
                } else if slider.changed() {
                    out = Some(Pick::Dragging(live.rgba()));
                }
            }
            ui.horizontal(|ui| {
                let label = ui.label("hex");
                let typed = ui.data(|d| d.get_temp::<String>(text_id));
                let editing = typed.is_some();
                let mut text = typed.unwrap_or_else(|| hex(live.rgba()));
                let field = ui
                    .add(egui::TextEdit::singleline(&mut text).desired_width(90.0))
                    .labelled_by(label.id)
                    .hint("type a colour as #rrggbb; Return keeps it");
                match field.has_focus() {
                    true => ui.data_mut(|d| {
                        d.insert_temp(text_id, text.clone());
                    }),
                    false => ui.data_mut(|d| d.remove::<String>(text_id)),
                }
                if field.lost_focus()
                    && editing
                    && let Some(rgba) = rgba_of(&text)
                {
                    let rgba = if self.alpha {
                        rgba
                    } else {
                        [rgba[0], rgba[1], rgba[2], live.a]
                    };
                    live = Hsva::from_rgba(rgba, live.h);
                    out = Some(Pick::Chosen(rgba));
                    ui.memory_mut(|m| m.surrender_focus(field.id));
                }
            });
            self.palette.iter().for_each(|Shelf { title, colours }| {
                ui.label(egui::RichText::new(title).color(theme::WEAK));
                ui.scope(|ui| {
                    ui.set_max_width(8.0 * (theme::CONTROL + 3.0));
                    ui.spacing_mut().item_spacing = vec2(3.0, 3.0);
                    ui.horizontal_wrapped(|ui| {
                        colours.iter().for_each(|(rgba, name)| {
                            let same = hex(*rgba) == hex(live.rgba());
                            if swatch(ui, *rgba, same, theme::CONTROL).hint(name.clone()).clicked() {
                                let rgba = if self.alpha {
                                    *rgba
                                } else {
                                    [rgba[0], rgba[1], rgba[2], live.a]
                                };
                                live = Hsva::from_rgba(rgba, live.h);
                                out = Some(Pick::Chosen(rgba));
                            }
                        });
                    });
                });
            });
            ui.horizontal(|ui| {
                if let Some(current) = self.current {
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(theme::CONTROL), Sense::hover());
                    paint_swatch(ui, rect, current, false);
                    if ui
                        .button("use the current colour")
                        .hint("use the colour you paint with")
                        .clicked()
                    {
                        let rgba = if self.alpha {
                            current
                        } else {
                            [current[0], current[1], current[2], live.a]
                        };
                        out = Some(Pick::Chosen(rgba));
                        Popup::close_id(ui.ctx(), self.id);
                    }
                }
                if let Some((label, hint)) = &self.removable
                    && ui.button(label.as_str()).hint(hint.as_str()).clicked()
                {
                    out = Some(Pick::Remove);
                    Popup::close_id(ui.ctx(), self.id);
                }
            });
        });
        let base = match out {
            Some(Pick::Dragging(rgba) | Pick::Chosen(rgba)) => rgba,
            _ => self.rgba,
        };
        if Popup::is_id_open(ui.ctx(), self.id) {
            let keep = match out {
                Some(Pick::Chosen(rgba)) => Hsva::from_rgba(rgba, live.h),
                _ => live,
            };
            ui.data_mut(|d| d.insert_temp(live_id, (keep, base)));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsv_round_trips_and_keeps_the_hue_of_greys() {
        [
            [1.0, 0.0, 0.0, 1.0],
            [0.2, 0.6, 0.4, 0.5],
            [0.95, 0.9, 0.1, 1.0],
            [0.1, 0.2, 0.9, 1.0],
        ]
        .iter()
        .for_each(|rgba| {
            let back = Hsva::from_rgba(*rgba, 0.0).rgba();
            assert!(
                back.iter().zip(rgba).all(|(a, b)| (a - b).abs() < 1e-5),
                "{rgba:?} {back:?}"
            );
        });
        assert_eq!(Hsva::from_rgba([0.5, 0.5, 0.5, 1.0], 0.3).h, 0.3);
    }

    #[test]
    fn every_colour_model_round_trips_and_keeps_what_a_grey_cannot_say() {
        let colours = [
            [1.0, 0.0, 0.0, 1.0],
            [0.2, 0.6, 0.4, 1.0],
            [0.95, 0.9, 0.1, 1.0],
            [0.1, 0.2, 0.9, 1.0],
            [0.03, 0.03, 0.03, 1.0],
        ];
        Model::ALL.into_iter().for_each(|model| {
            colours.iter().for_each(|rgba| {
                let values = model.values(*rgba, [0.0; 3]);
                let back = model.rgb(values);
                assert!(
                    back.iter().zip(rgba).all(|(a, b)| (a - b).abs() < 2e-3),
                    "{} {rgba:?} {values:?} {back:?}",
                    model.name()
                );
                assert!(
                    model
                        .channels()
                        .iter()
                        .zip(values)
                        .all(|(c, v)| v >= c.lo - 0.01 && v <= c.hi + 0.01),
                    "{} {values:?} stays inside its sliders",
                    model.name()
                );
            });
        });
        assert_eq!(Model::Hsl.values([1.0, 0.0, 0.0, 1.0], [0.0; 3]), [0.0, 100.0, 50.0]);
        assert_eq!(Model::Rgb.values([1.0, 0.5, 0.0, 1.0], [0.0; 3]), [255.0, 127.5, 0.0]);
        assert_eq!(
            Model::Hsl.values([0.0, 0.0, 0.0, 1.0], [210.0, 40.0, 0.0]),
            [210.0, 40.0, 0.0],
            "black keeps the hue and saturation the sliders had"
        );
        assert_eq!(
            Model::Hsv.values([0.5, 0.5, 0.5, 1.0], [120.0, 80.0, 0.0])[0],
            120.0,
            "a grey keeps the hue"
        );
        assert_eq!(
            hex({
                let [r, g, b] = Model::Hsl.rgb([210.0, 50.0, 25.0]);
                [r, g, b, 1.0]
            }),
            "#204060"
        );
        assert_eq!(
            serde_json::to_string(&Model::Lab).unwrap(),
            "\"lab\"",
            "the settings file keeps the model by its name"
        );
        assert_eq!(hex([1.0, 0.5, 0.0, 1.0]), "#ff8000");
        assert_eq!(hex([0.0, 0.0, 0.0, 0.5]), "#00000080");
        assert_eq!(rgba_of("ff8000"), Some([1.0, 128.0 / 255.0, 0.0, 1.0]));
        assert!(rgba_of("nope").is_none());
    }
}
