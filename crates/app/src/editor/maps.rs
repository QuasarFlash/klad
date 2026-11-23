use super::Editor;
use crate::color;
use crate::messages::Tone;
use crate::theme::Hint;
use anyhow::Result;
use egui::{Color32, Key, Mesh, Rect, Sense, Stroke, TextureHandle, TextureOptions, pos2, vec2};
use klad_core::doc::{Document, Kind, Layer};
use klad_core::maps::{Light, LightKind, Maps, Profile, Role, Surface, decoded, encoded, leaning, palette};
use klad_core::ops::Op;
use klad_render::Frame;
use klad_render::maps::{Lit, Map};
use klad_render::occluders::Polygon;
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::mpsc::Receiver;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Show {
    Map(Map),
    Lit,
}

impl Show {
    fn name(self) -> &'static str {
        match self {
            Show::Map(map) => map.name(),
            Show::Lit => "lit",
        }
    }
}

enum Ready {
    Map(egui::ColorImage),
    Lit(Arc<Lit>),
}

#[derive(Clone, Copy, PartialEq)]
struct Want {
    rev: u64,
    frame: u32,
    kind: Option<Map>,
}

type Traced = ((u64, u32), Vec<Polygon>);
type Tracing = ((u64, u32), Receiver<Result<Vec<Polygon>>>);

#[derive(Default)]
pub(super) struct MapsView {
    pub(super) show: Option<Show>,
    pub(super) outlines: bool,
    pub(super) tilt: bool,
    want: Option<Want>,
    job: Option<(Want, Receiver<Result<Ready>>)>,
    lit: Option<(Want, Arc<Lit>)>,
    texture: Option<(Want, Option<[f32; 2]>, usize, TextureHandle)>,
    traced: Option<Traced>,
    tracing: Option<Tracing>,
    auto_for: Option<(String, Map)>,
    parked: Option<Maps>,
    pub(super) making: bool,
}

fn own_map(role: Role) -> (Map, &'static str) {
    match role {
        Role::Height => (
            Map::Height,
            "the height map while a height layer is selected: white raises, black lowers",
        ),
        Role::Normal => (
            Map::Normal,
            "the normal map while a normal layer is selected: what it paints are directions",
        ),
        Role::Specular => (
            Map::Specular,
            "the specular map while a specular layer is selected: brighter is shinier",
        ),
        Role::Emission => (
            Map::Emission,
            "the emission map while an emission layer is selected: what glows whatever the light",
        ),
    }
}

const HEIGHTS: [(&str, f32); 5] = [
    ("lower", 0.0),
    ("lower a little", 0.25),
    ("flat", 0.5),
    ("raise a little", 0.75),
    ("raise", 1.0),
];

fn height_words(lift: f32, range: f32) -> String {
    let px = lift * range;
    match px {
        p if p.abs() < 0.05 => "flat: leaves the surface as it is".into(),
        p if p > 0.0 => format!("raises it {p:.1} px"),
        p => format!("lowers it {:.1} px", -p),
    }
}

fn ramp(ui: &mut egui::Ui, current: f32, size: egui::Vec2) -> (egui::Response, Option<f32>) {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Slider, true, "the height"));
    let painter = ui.painter_at(rect);
    let mut mesh = Mesh::default();
    [
        (rect.left_top(), Color32::BLACK),
        (rect.right_top(), Color32::WHITE),
        (rect.right_bottom(), Color32::WHITE),
        (rect.left_bottom(), Color32::BLACK),
    ]
    .into_iter()
    .for_each(|(pos, color)| {
        mesh.vertices.push(egui::epaint::Vertex {
            pos,
            uv: egui::epaint::WHITE_UV,
            color,
        })
    });
    mesh.indices.extend([0, 1, 2, 0, 2, 3]);
    painter.add(mesh);
    painter.rect_stroke(
        rect,
        2.0,
        Stroke::new(1.0, Color32::from_gray(30)),
        egui::StrokeKind::Inside,
    );
    let x = rect.left() + current.clamp(0.0, 1.0) * rect.width();
    painter.line_segment(
        [pos2(x, rect.top()), pos2(x, rect.bottom())],
        Stroke::new(3.0, Color32::from_rgb(230, 80, 60)),
    );
    let picked = (response.clicked() || response.dragged())
        .then(|| response.interact_pointer_pos())
        .flatten()
        .map(|p| ((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0));
    (response, picked)
}

fn sphere(ui: &mut egui::Ui, current: [f32; 3], size: f32) -> (egui::Response, Option<[f32; 3]>) {
    let (rect, response) = ui.allocate_exact_size(vec2(size, size), Sense::click_and_drag());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Slider, true, "the direction"));
    let (c, r) = (rect.center(), size * 0.5 - 3.0);
    let (rings, around) = (16usize, 48usize);
    let mut mesh = Mesh::default();
    let colour = |x: f32, y: f32| {
        let [cr, cg, cb] = encoded(leaning(x, y)).map(|v| (v * 255.0).round() as u8);
        Color32::from_rgb(cr, cg, cb)
    };
    mesh.vertices.push(egui::epaint::Vertex {
        pos: c,
        uv: egui::epaint::WHITE_UV,
        color: colour(0.0, 0.0),
    });
    (1..=rings).for_each(|k| {
        let rho = k as f32 / rings as f32;
        (0..around).for_each(|a| {
            let t = a as f32 / around as f32 * std::f32::consts::TAU;
            let (x, y) = (rho * t.cos(), rho * t.sin());
            mesh.vertices.push(egui::epaint::Vertex {
                pos: c + vec2(x, y) * r,
                uv: egui::epaint::WHITE_UV,
                color: colour(x, y),
            });
        });
    });
    let at = |k: usize, a: usize| (1 + (k - 1) * around + a % around) as u32;
    (0..around).for_each(|a| mesh.indices.extend([0, at(1, a), at(1, a + 1)]));
    (2..=rings).for_each(|k| {
        (0..around).for_each(|a| {
            mesh.indices.extend([
                at(k - 1, a),
                at(k, a),
                at(k, a + 1),
                at(k - 1, a),
                at(k, a + 1),
                at(k - 1, a + 1),
            ])
        })
    });
    let painter = ui.painter_at(rect);
    painter.add(mesh);
    painter.circle_stroke(c, r, Stroke::new(1.0, Color32::from_gray(30)));
    let mark = c + vec2(current[0], -current[1]) * r;
    painter.circle_stroke(mark, 5.0, Stroke::new(2.5, Color32::from_black_alpha(200)));
    painter.circle_stroke(mark, 5.0, Stroke::new(1.2, Color32::WHITE));
    let picked = (response.clicked() || response.dragged())
        .then(|| response.interact_pointer_pos())
        .flatten()
        .map(|p| {
            let d = (p - c) / r;
            leaning(d.x, d.y)
        });
    (response, picked)
}

fn lean_words(n: [f32; 3]) -> String {
    let angle = n[2].clamp(-1.0, 1.0).acos().to_degrees();
    if angle < 1.0 {
        return "facing out".into();
    }
    let side = |v: f32, plus: &str, minus: &str| match v {
        v if v > 0.15 => plus.to_string(),
        v if v < -0.15 => minus.to_string(),
        _ => String::new(),
    };
    let way: Vec<String> = [side(n[1], "up", "down"), side(n[0], "right", "left")]
        .into_iter()
        .filter(|w| !w.is_empty())
        .collect();
    format!("leaning {} by {angle:.0} degrees", way.join(" and "))
}

fn preview_scale(doc: &Document) -> f32 {
    match doc.pixel_art() {
        true => 1.0,
        false => (1536.0 / doc.canvas.width.max(doc.canvas.height) as f32).clamp(1.0 / 16.0, 1.0),
    }
}

fn image_of(frame: &Frame) -> egui::ColorImage {
    egui::ColorImage::from_rgba_unmultiplied([frame.width as usize, frame.height as usize], &frame.rgba)
}

fn pointer_light(doc: &Document, at: [f32; 2]) -> Light {
    let longest = doc.canvas.width.max(doc.canvas.height) as f32;
    Light {
        at,
        height: (longest * 0.15).round(),
        range: longest,
        ..Light::default()
    }
}

pub(super) const NEXT: [Option<Map>; 7] = [
    Some(Map::Normal),
    Some(Map::Height),
    Some(Map::Specular),
    Some(Map::Emission),
    Some(Map::Depth),
    Some(Map::Occlusion),
    None,
];

impl Editor {
    #[cfg(test)]
    pub(super) fn map_shown(&self) -> Option<&'static str> {
        let want = self.maps.want?;
        self.maps
            .texture
            .as_ref()
            .filter(|t| t.0.kind == want.kind)
            .map(|_| self.maps.show.map_or("lit", Show::name))
    }

    pub(super) fn next_map(&mut self) {
        let now = match self.maps.show {
            Some(Show::Map(map)) => NEXT.iter().position(|m| *m == Some(map)),
            _ => None,
        };
        let next = match now {
            Some(i) => NEXT[(i + 1) % NEXT.len()],
            None => NEXT[0],
        };
        self.maps.show = next.map(Show::Map);
        let shown = self.maps.show.map_or("the picture", Show::name);
        self.say(Tone::Info, format!("showing {shown}"));
    }

    pub(super) fn toggle_lit(&mut self) {
        self.maps.show = match self.maps.show {
            Some(Show::Lit) => None,
            _ => Some(Show::Lit),
        };
        if self.maps.show.is_some() && klad_render::maps::settings(&self.scene.bundle.doc).lights.is_empty() {
            self.say(
                Tone::Info,
                "no preview lights yet: add one in the light maps panel, or hold L to light it with the pointer",
            );
        }
    }

    fn start_maps_job(&mut self, want: Want, ctx: &egui::Context) {
        let doc = self.scene.bundle.doc.clone();
        let rasters = self.scene.rasters_now();
        let frame = want.frame;
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let scale = preview_scale(&doc);
            let done = match want.kind {
                Some(map) => klad_render::maps::render(&doc, frame, &rasters, scale)
                    .map(|set| Ready::Map(image_of(&set.frame(map)))),
                None => Lit::prepare(&doc, frame, &rasters, scale).map(|lit| Ready::Lit(Arc::new(lit))),
            };
            let _ = tx.send(done);
            ctx.request_repaint();
        });
        self.maps.job = Some((want, rx));
    }

    fn follow_map_layer(&mut self) {
        let on = self.scene.bundle.doc.maps.is_some();
        let active = self
            .scene
            .active_layer()
            .filter(|_| on)
            .and_then(|l| l.map.map(|r| (l.id.clone(), own_map(r))));
        match (self.maps.auto_for.clone(), active) {
            (None, Some((id, (map, words)))) if self.maps.show.is_none() => {
                self.maps.show = Some(Show::Map(map));
                self.maps.auto_for = Some((id, map));
                self.say(Tone::Info, format!("showing {words} (M for the next map)"));
            }
            (Some((was, map)), now) if now.as_ref().map(|(id, (m, _))| (id, *m)) != Some((&was, map)) => {
                if self.maps.show == Some(Show::Map(map)) {
                    self.maps.show = None;
                }
                self.maps.auto_for = None;
            }
            _ => {}
        }
    }

    fn sketch_live(&self, painter: &egui::Painter, view: crate::view::View, rect: Rect) {
        let Some(live) = &self.live else {
            return;
        };
        let points: Vec<egui::Pos2> = live
            .stroke
            .samples
            .iter()
            .map(|s| {
                let (x, y) = s.pos();
                self.screen(view, rect, vec2(x, y))
            })
            .collect();
        let (Some(first), Some(last)) = (points.first().copied(), points.last().copied()) else {
            return;
        };
        let zoom = (self.screen(view, rect, vec2(1.0, 0.0)) - self.screen(view, rect, vec2(0.0, 0.0))).length();
        let width = (live.stroke.size * zoom).max(1.0);
        let [r, g, b, _] = self.color.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
        let ink = Color32::from_rgba_unmultiplied(r, g, b, 150);
        painter.circle_filled(first, width * 0.5, ink);
        if points.len() > 1 {
            painter.add(egui::Shape::line(points, Stroke::new(width, ink)));
            painter.circle_filled(last, width * 0.5, ink);
        }
    }

    pub(super) fn map_overlay(&mut self, ui: &egui::Ui, view: crate::view::View, rect: Rect) {
        self.maps.making = false;
        self.follow_map_layer();
        let typing = ui.ctx().egui_wants_keyboard_input();
        let held = !typing && ui.input(|i| i.key_down(Key::L) && !i.modifiers.any());
        let show = match held {
            true => Some(Show::Lit),
            false => self.maps.show,
        };
        let Some(show) = show else {
            return;
        };
        let want = Want {
            rev: self.scene.bundle.doc.rev,
            frame: self.scene.frame,
            kind: match show {
                Show::Map(map) => Some(map),
                Show::Lit => None,
            },
        };
        self.maps.want = Some(want);
        if let Some((asked, rx)) = &self.maps.job
            && let Ok(done) = rx.try_recv()
        {
            let asked = *asked;
            self.maps.job = None;
            match done {
                Ok(Ready::Map(image)) => {
                    let texture = ui.ctx().load_texture("light map", image, TextureOptions::LINEAR);
                    self.maps.texture = Some((asked, None, 0, texture));
                }
                Ok(Ready::Lit(lit)) => self.maps.lit = Some((asked, lit)),
                Err(e) => {
                    self.maps.show = None;
                    self.say(Tone::Error, format!("the light maps could not be made: {e:#}"));
                    return;
                }
            }
        }
        let stale = match want.kind {
            Some(_) => self.maps.texture.as_ref().is_none_or(|t| t.0 != want),
            None => self.maps.lit.as_ref().is_none_or(|l| l.0 != want),
        };
        if stale && self.maps.job.is_none() {
            self.start_maps_job(want, ui.ctx());
        }
        let pointer = ui
            .input(|i| i.pointer.hover_pos())
            .filter(|p| rect.contains(*p))
            .map(|p| {
                let c = view.to_canvas(p - rect.min, rect.size());
                [c.x, c.y]
            });
        if want.kind.is_none() {
            let doc = &self.scene.bundle.doc;
            let mut lights = klad_render::maps::settings(doc).lights;
            let at = match held {
                true => pointer,
                false => None,
            };
            lights.extend(at.map(|p| pointer_light(doc, p)));
            let count = lights.len();
            let fresh = self
                .maps
                .texture
                .as_ref()
                .is_some_and(|t| t.0 == want && t.1 == at && t.2 == count);
            if let (Some((made, lit)), false) = (&self.maps.lit, fresh) {
                match lit.shade(&lights) {
                    Ok(frame) => {
                        let made = *made;
                        let texture = ui
                            .ctx()
                            .load_texture("lit preview", image_of(&frame), TextureOptions::LINEAR);
                        self.maps.texture = Some((made, at, count, texture));
                    }
                    Err(e) => self.say(Tone::Error, format!("the light preview failed: {e:#}")),
                }
            }
        }
        let shown = self.maps.texture.as_ref().filter(|t| match want.kind {
            Some(_) => t.0.kind == want.kind,
            None => t.0.kind.is_none(),
        });
        let painter = ui.painter().with_clip_rect(rect);
        let Some((_, _, _, texture)) = shown else {
            self.maps.making = true;
            ui.ctx().request_repaint();
            return;
        };
        let (w, h) = self.scene.size();
        let corners = [
            vec2(0.0, 0.0),
            vec2(w as f32, 0.0),
            vec2(w as f32, h as f32),
            vec2(0.0, h as f32),
        ]
        .map(|c| self.screen(view, rect, c));
        let mut mesh = Mesh::with_texture(texture.id());
        [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]
            .iter()
            .zip(corners)
            .for_each(|((u, v), p)| {
                mesh.vertices.push(egui::epaint::Vertex {
                    pos: p,
                    uv: pos2(*u, *v),
                    color: Color32::WHITE,
                })
            });
        mesh.indices.extend([0, 1, 2, 0, 2, 3]);
        painter.add(mesh);
        self.sketch_live(&painter, view, rect);
        if self.maps.job.is_some() || held {
            ui.ctx().request_repaint();
        }
    }

    pub(super) fn on_normal_layer(&self) -> bool {
        self.scene.active_layer().is_some_and(|l| l.map == Some(Role::Normal))
    }

    pub(super) fn on_height_layer(&self) -> bool {
        self.scene.active_layer().is_some_and(|l| l.map == Some(Role::Height))
    }

    pub(super) fn height_panel(&mut self, ui: &mut egui::Ui) -> Result<()> {
        let range = self
            .scene
            .active_layer()
            .map_or(12.0, |l| klad_render::maps::height_range(&self.scene.bundle.doc, l));
        let lift = klad_render::maps::lift(self.color);
        let grey = (self.color[0] - self.color[1])
            .abs()
            .max((self.color[1] - self.color[2]).abs())
            < 0.02;
        let mut chosen = None;
        ui.horizontal(|ui| {
            let (response, picked) = ramp(ui, (lift + 1.0) * 0.5, vec2(150.0, 36.0));
            response.hint("how high: click or drag; black lowers the surface, mid grey leaves it, white raises it");
            chosen = picked;
            ui.vertical(|ui| {
                color::swatch(ui, self.color, false, 44.0).hint("the current colour: only its lightness counts");
                ui.label(height_words(lift, range));
            });
        });
        ui.horizontal_wrapped(|ui| {
            HEIGHTS.iter().for_each(|(name, v)| {
                let close = grey && ((lift + 1.0) * 0.5 - v).abs() < 0.01;
                let swatch = color::swatch(ui, [*v, *v, *v, 1.0], close, crate::theme::CONTROL);
                swatch.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, *name));
                if swatch
                    .hint(format!("{name}: {}", height_words(2.0 * v - 1.0, range)))
                    .clicked()
                {
                    chosen = Some(*v);
                }
            });
        });
        if !grey {
            ui.label(
                egui::RichText::new("only the lightness of this colour counts: pick a grey to see what it does")
                    .color(crate::theme::REFUSED),
            );
        }
        ui.label(
            egui::RichText::new(format!(
                "a height layer raises the surface where it is white and lowers it where it is black, by how much it covers, up to {range:.0} px; it never shows in the picture, so the canvas shows the height map while it is selected"
            ))
            .color(crate::theme::WEAK),
        );
        if let Some(v) = chosen {
            self.pick_new_colour([v, v, v, 1.0]);
        }
        Ok(())
    }

    pub(super) fn tilted_brush(&self) -> bool {
        self.maps.tilt && self.on_normal_layer()
    }

    pub(super) fn normal_panel(&mut self, ui: &mut egui::Ui) -> Result<()> {
        let current = decoded([self.color[0], self.color[1], self.color[2]]);
        let mut chosen = None;
        ui.horizontal(|ui| {
            let (response, picked) = sphere(ui, current, 150.0);
            response.hint(
                "the direction the surface faces: click or drag where the light should catch it; the middle faces out",
            );
            chosen = picked;
            ui.vertical(|ui| {
                color::swatch(ui, self.color, false, 44.0).hint("the direction as a normal map colour");
                ui.label(lean_words(current));
                if ui
                    .button("flatten")
                    .hint("facing out (#8080ff): painting it takes the normals back towards flat")
                    .clicked()
                {
                    chosen = Some([0.0, 0.0, 1.0]);
                }
                ui.checkbox(&mut self.maps.tilt, "direction from pen tilt")
                    .hint("each dab paints the way the pen leans: lean it the way the surface faces");
            });
        });
        let doc = &self.scene.bundle.doc;
        let directions = match klad_render::maps::settings(doc).surface.directions_for(doc.pixel_art()) {
            0 => 9,
            d => d,
        };
        ui.label(match doc.pixel_art() {
            true => "normal palette",
            false => "directions",
        });
        ui.horizontal_wrapped(|ui| {
            palette(directions).into_iter().for_each(|(name, n)| {
                let [r, g, b] = encoded(n);
                let close = current.iter().zip(n).map(|(a, b)| a * b).sum::<f32>() > 0.999;
                let swatch = color::swatch(ui, [r, g, b, 1.0], close, crate::theme::CONTROL);
                swatch.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name.as_str()));
                if swatch.hint(format!("{name}: {}", lean_words(n))).clicked() {
                    chosen = Some(n);
                }
            });
        });
        ui.label(
            egui::RichText::new(
                "a normal layer paints directions: any brush, its colour the way the surface faces, mixed over the drawing's own normals by how much it covers",
            )
            .color(crate::theme::WEAK),
        );
        if let Some(n) = chosen {
            let [r, g, b] = encoded(n);
            self.pick_new_colour([r, g, b, 1.0]);
        }
        Ok(())
    }

    pub(super) fn light_section(&mut self, ui: &mut egui::Ui, layer: &Layer) -> Result<()> {
        if self.scene.bundle.doc.maps.is_none()
            || layer.map.is_some()
            || matches!(layer.kind(), Kind::Adjust | Kind::Bones)
        {
            return Ok(());
        }
        let id = layer.id.clone();
        ui.separator();
        ui.label(egui::RichText::new("Light").strong())
            .hint("how this layer takes the light maps' light: shine, glow and how near it is");
        let mut props: Vec<(String, Value)> = Vec::new();
        let paint = self.color;
        ui.horizontal_wrapped(|ui| {
            if let Some(v) = self.number(
                ui,
                &format!("{id}.shine"),
                "shine ",
                layer.shine.unwrap_or(0.0),
                0.0..=1.0,
                0.01,
                "how shiny it is under light, the specular map: 0 matte, 1 as shiny as it gets",
            ) {
                props.push(("shine".into(), if v <= 0.0 { Value::Null } else { json!(v) }));
            }
            let tint = layer.specular.as_deref().and_then(color::rgba_of).unwrap_or([1.0; 4]);
            if let Some(rgba) = color::Square::new(ui, format!("{id}.specular"), tint, "the colour of its shine")
                .current(paint)
                .pick(ui)
            {
                props.push(("specular".into(), json!(color::hex(rgba))));
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("glow");
            let mut glow = layer.emissive;
            if ui
                .checkbox(&mut glow, "emissive")
                .hint(
                    "the whole layer glows: drawn at full brightness whatever the light, in the emission map (windows, eyes, fire)",
                )
                .changed()
            {
                props.push(("emissive".into(), if glow { json!(true) } else { Value::Null }));
            }
            if !layer.emissive {
                let mut glows = layer.glows.clone();
                let mut changed = false;
                ui.label("or only where it is");
                layer.glows.iter().enumerate().for_each(|(i, hex)| {
                    let rgba = color::rgba_of(hex).unwrap_or([1.0; 4]);
                    match color::Square::new(ui, format!("{id}.glow{i}"), rgba, format!("glow colour {}", i + 1))
                        .current(paint)
                        .removable()
                        .show(ui)
                    {
                        Some(color::Pick::Chosen(rgba)) if color::hex([rgba[0], rgba[1], rgba[2], 1.0]) != *hex => {
                            let picked = color::hex([rgba[0], rgba[1], rgba[2], 1.0]);
                            glows = glows
                                .iter()
                                .filter(|g| **g != picked)
                                .map(|g| if g == hex { picked.clone() } else { g.clone() })
                                .collect();
                            changed = true;
                        }
                        Some(color::Pick::Remove) => {
                            glows.retain(|g| g != hex);
                            changed = true;
                        }
                        _ => {}
                    }
                });
                let current = color::hex([paint[0], paint[1], paint[2], 1.0]);
                if ui
                    .button("+ the current colour")
                    .hint("the parts of this layer in the current colour glow whatever the light, on every frame: pick the colour of the eyes or the windows from the drawing first")
                    .clicked()
                    && !glows.contains(&current)
                {
                    glows.push(current);
                    changed = true;
                }
                if changed {
                    props.push((
                        "glows".into(),
                        match glows.is_empty() {
                            true => Value::Null,
                            false => json!(glows),
                        },
                    ));
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            if let Some(v) = self.number(
                ui,
                &format!("{id}.z"),
                "z ",
                layer.z.unwrap_or(0.0),
                -2000.0..=2000.0,
                0.5,
                "px nearer than the rest of its plane, for the depth map (below 0: farther)",
            ) {
                props.push(("z".into(), if v == 0.0 { Value::Null } else { json!(v) }));
            }
        });
        self.set(&id, props)?;
        if !self.scene.bundle.doc.layers.iter().any(|l| l.id == id) {
            return Ok(());
        }
        let mut own = layer.surface.is_some();
        if ui
            .checkbox(&mut own, "a surface of its own")
            .hint("this layer rounds in from its own outline instead of being part of the drawing's surface")
            .changed()
        {
            let value = match own {
                true => serde_json::to_value(klad_render::maps::settings(&self.scene.bundle.doc).surface)?,
                false => Value::Null,
            };
            return self.set(&id, vec![("surface".into(), value)]);
        }
        match &layer.surface {
            Some(surface) => match self.surface_fields(ui, &format!("{id}.surface"), Some(&id), surface) {
                Some(next) => self.set(&id, vec![("surface".into(), serde_json::to_value(next)?)]),
                None => Ok(()),
            },
            None => Ok(()),
        }
    }

    pub(super) fn occluder_overlay(&mut self, ui: &egui::Ui, view: crate::view::View, rect: Rect) {
        if !self.maps.outlines {
            return;
        }
        let key = (self.scene.bundle.doc.rev, self.scene.frame);
        if let Some((asked, rx)) = &self.maps.tracing
            && let Ok(done) = rx.try_recv()
        {
            let asked = *asked;
            self.maps.tracing = None;
            match done {
                Ok(rings) => self.maps.traced = Some((asked, rings)),
                Err(e) => {
                    self.maps.outlines = false;
                    self.say(Tone::Error, format!("the occluders could not be traced: {e:#}"));
                    return;
                }
            }
        }
        let stale = self.maps.traced.as_ref().is_none_or(|(k, _)| *k != key);
        if stale && self.maps.tracing.is_none() {
            let doc = self.scene.bundle.doc.clone();
            let rasters = self.scene.rasters_now();
            let (tx, rx) = std::sync::mpsc::channel();
            let ctx = ui.ctx().clone();
            std::thread::spawn(move || {
                let options = klad_render::export::Options {
                    scale: 1.0,
                    background: false,
                    references: false,
                    detail: 1.0,
                };
                let traced =
                    klad_render::export::render(&doc, key.1, &rasters, &klad_render::export::Target::Frame, options)
                        .and_then(|image| klad_render::read_rgba8(&image))
                        .map(|picture| {
                            let alpha: Vec<f32> = picture.rgba.chunks(4).map(|p| p[3] as f32 / 255.0).collect();
                            klad_render::occluders::polygons(
                                &alpha,
                                picture.width as usize,
                                picture.height as usize,
                                klad_render::occluders::TOLERANCE,
                            )
                        });
                let _ = tx.send(traced);
                ctx.request_repaint();
            });
            self.maps.tracing = Some((key, rx));
        }
        if self.maps.tracing.is_some() {
            ui.ctx().request_repaint();
        }
        let Some((_, rings)) = &self.maps.traced else {
            return;
        };
        let painter = ui.painter().with_clip_rect(rect);
        rings.iter().for_each(|ring| {
            let mut points: Vec<egui::Pos2> = ring.iter().map(|p| self.screen(view, rect, vec2(p[0], p[1]))).collect();
            if let Some(first) = points.first().copied() {
                points.push(first);
            }
            painter.add(egui::Shape::line(
                points.clone(),
                Stroke::new(3.0, Color32::from_black_alpha(160)),
            ));
            painter.add(egui::Shape::line(
                points,
                Stroke::new(1.5, Color32::from_rgb(255, 215, 50)),
            ));
        });
    }

    #[cfg(test)]
    pub(super) fn occluders_traced(&self) -> Option<usize> {
        self.maps.traced.as_ref().map(|(_, rings)| rings.len())
    }

    fn findings_list(&mut self, ui: &mut egui::Ui) -> Result<()> {
        let Some(found) = self.checks.maps.clone() else {
            return Ok(());
        };
        ui.separator();
        ui.label(egui::RichText::new("Checked").strong());
        if found.is_empty() {
            ui.label("the light maps look right on every frame");
            return Ok(());
        }
        let mut go = None;
        found.iter().for_each(|f| {
            let [x, y, w, h] = f.area.map(|v| v.round());
            if ui
                .selectable_label(
                    f.frame == self.scene.frame,
                    format!("frame {}  {}  at {x},{y} {w}x{h}", f.frame, f.kind),
                )
                .hint(&f.note)
                .clicked()
            {
                go = Some((f.frame, f.area));
            }
        });
        if let Some((frame, area)) = go {
            self.go_to(frame)?;
            self.frame_area(area);
        }
        Ok(())
    }

    fn frame_area(&mut self, area: [f32; 4]) {
        if let Some(view) = self.view.as_mut() {
            view.center = vec2(area[0] + area[2] / 2.0, area[1] + area[3] / 2.0);
        }
    }

    fn set_maps(&mut self, maps: Option<Maps>) -> Result<()> {
        self.apply(vec![Op::Maps(maps)])
    }

    fn fit_bevel(&mut self, owner: Option<&str>) -> Option<f32> {
        let frame = self.scene.frame;
        let rasters = self.scene.rasters_now();
        match klad_render::maps::fit_bevel(&self.scene.bundle.doc, frame, &rasters, owner) {
            Ok(bevel) => {
                self.say(
                    Tone::Info,
                    format!("bevel fitted to {bevel} px: the whole silhouette rounds over, as deep as the shape"),
                );
                Some(bevel)
            }
            Err(e) => {
                self.say(Tone::Error, format!("the bevel could not be fitted: {e:#}"));
                None
            }
        }
    }

    fn surface_fields(
        &mut self,
        ui: &mut egui::Ui,
        key: &str,
        owner: Option<&str>,
        surface: &Surface,
    ) -> Option<Surface> {
        let mut next = surface.clone();
        ui.horizontal_wrapped(|ui| {
            Profile::ALL.iter().for_each(|p| {
                if ui
                    .selectable_label(surface.profile == *p, p.name())
                    .hint(match p {
                        Profile::Round => "the rim rounds over like a cushion's edge, steep at the outline",
                        Profile::Chamfer => "a straight slope from the outline to the top",
                        Profile::Pillow => "soft at the outline, steepest halfway in",
                        Profile::Flat => "no rim: only paint with body and height layers shape it",
                    })
                    .clicked()
                {
                    next.profile = *p;
                }
            });
        });
        ui.horizontal_wrapped(|ui| {
            [
                (
                    "bevel ",
                    surface.bevel,
                    0.0..=512.0,
                    0.5,
                    "px the rim rounds over, in from the outline",
                ),
                (
                    "strength ",
                    surface.strength,
                    0.0..=10.0,
                    0.02,
                    "how steep the slopes read in the normal map",
                ),
                (
                    "body ",
                    surface.body,
                    0.0..=10.0,
                    0.02,
                    "how much paint with body raises it; 0 for none",
                ),
                (
                    "range ",
                    surface.range,
                    0.0..=512.0,
                    0.5,
                    "px a white height layer raises it (black lowers)",
                ),
            ]
            .into_iter()
            .enumerate()
            .for_each(|(i, (label, value, range, speed, hint))| {
                if let Some(v) = self.number(ui, &format!("{key}.{i}"), label, value, range, speed, hint) {
                    match i {
                        0 => next.bevel = v,
                        1 => next.strength = v,
                        2 => next.body = v,
                        _ => next.range = v,
                    }
                }
                if i == 0
                    && ui
                        .button("fit")
                        .hint("the bevel as deep as the shape, so the whole silhouette rounds over like a ball; flat-bottomed things turn into domes")
                        .clicked()
                    && let Some(bevel) = self.fit_bevel(owner)
                {
                    next.bevel = bevel;
                }
            });
        });
        (next != *surface).then_some(next)
    }

    fn height_range_field(&mut self, ui: &mut egui::Ui, layer: &Layer) -> Result<()> {
        let doc = &self.scene.bundle.doc;
        let owner = klad_render::maps::surface_owner(doc, layer);
        let range = klad_render::maps::height_range(doc, layer);
        let whose = owner
            .as_deref()
            .and_then(|id| doc.layer(id))
            .map_or("the drawing's surface".to_string(), |l| format!("{}'s surface", l.name));
        let mut next = None;
        ui.horizontal_wrapped(|ui| {
            next = self.number(
                ui,
                &format!("{}.range", layer.id),
                "white raises ",
                range,
                0.0..=512.0,
                0.5,
                &format!("px pure white raises it, and black lowers it as much: the range of {whose}"),
            );
            ui.label("px");
        });
        let Some(v) = next else {
            return Ok(());
        };
        match owner {
            Some(id) => {
                let mut surface = self
                    .scene
                    .bundle
                    .doc
                    .layer(&id)
                    .and_then(|l| l.surface.clone())
                    .unwrap_or_default();
                surface.range = v;
                self.set(&id, vec![("surface".into(), serde_json::to_value(surface)?)])
            }
            None => {
                let mut maps = klad_render::maps::settings(&self.scene.bundle.doc);
                maps.surface.range = v;
                self.set_maps(Some(maps))
            }
        }
    }

    fn maps_on(&mut self) -> Vec<Op> {
        if self.scene.bundle.doc.maps.is_some() {
            return Vec::new();
        }
        let back = self.maps.parked.take().unwrap_or_default();
        self.say(Tone::Info, "turned the light maps on: a map layer shapes them");
        vec![Op::Maps(Some(back))]
    }

    pub(super) fn new_map_layer(&mut self) -> Result<()> {
        let first = self.maps_on();
        let frame = self.scene.frame;
        self.add_layer_after(
            first,
            json!({"kind": "paint", "name": "height", "map": "height", "cels": [{"frame": frame}]}),
        )?;
        self.pick_new_colour([1.0; 4]);
        Ok(())
    }

    pub(super) fn make_map_layer(&mut self) -> Result<()> {
        let Some(id) = self.scene.active_id() else {
            return Ok(());
        };
        let props = ["surface", "depth", "shine", "specular", "emissive", "glows", "z"]
            .into_iter()
            .map(|key| (key.to_string(), Value::Null))
            .chain(std::iter::once(("map".to_string(), json!("height"))))
            .collect();
        let ops = self
            .maps_on()
            .into_iter()
            .chain(std::iter::once(Op::Set { id, props }))
            .collect();
        self.apply(ops)?;
        self.pick_new_colour([1.0; 4]);
        Ok(())
    }

    pub(super) fn make_colour_layer(&mut self) -> Result<()> {
        let Some(id) = self.scene.active_id() else {
            return Ok(());
        };
        self.set(&id, vec![("map".into(), Value::Null)])
    }

    pub(super) fn map_section(&mut self, ui: &mut egui::Ui, layer: &Layer) -> Result<()> {
        let Some(role) = layer.map else {
            return Ok(());
        };
        let id = layer.id.clone();
        let mut chosen = role;
        ui.horizontal_wrapped(|ui| {
            ui.label("draws");
            Role::ALL.iter().for_each(|r| {
                if ui
                    .selectable_label(chosen == *r, r.name())
                    .hint(match r {
                        Role::Height => "height: white raises the surface, black lowers it",
                        Role::Normal => "directions: its colours are the way the surface faces (#8080ff faces out)",
                        Role::Specular => "shine: brighter is shinier under the preview lights",
                        Role::Emission => "glow: what it paints glows whatever the light",
                    })
                    .clicked()
                {
                    chosen = *r;
                }
            });
        });
        if chosen != role {
            if chosen == Role::Height {
                self.pick_new_colour([1.0; 4]);
            }
            return self.set(&id, vec![("map".into(), json!(chosen.name()))]);
        }
        if role == Role::Height {
            self.height_range_field(ui, layer)?;
        }
        ui.label(
            egui::RichText::new(match role {
                Role::Height => "a map layer: it shapes the light maps and never shows in the picture",
                Role::Normal => {
                    "a map layer: it paints the normal map over the drawing's own and never shows in the picture"
                }
                Role::Specular => "a map layer: it paints the specular map and never shows in the picture",
                Role::Emission => "a map layer: it paints the emission map and never shows in the picture",
            })
            .color(crate::theme::WEAK),
        );
        self.command_button(
            ui,
            crate::commands::Cmd::MakeColourLayer,
            Some("make it a colour layer"),
        );
        Ok(())
    }

    pub(super) fn maps_pane(&mut self, ui: &mut egui::Ui) -> Result<()> {
        ui.horizontal_wrapped(|ui| {
            ui.label("show");
            let mut show = self.maps.show;
            if ui
                .selectable_label(show.is_none(), "picture")
                .hint("the picture as drawn, with no map over it")
                .clicked()
            {
                show = None;
            }
            Map::ALL.iter().for_each(|m| {
                if ui
                    .selectable_label(show == Some(Show::Map(*m)), m.name())
                    .hint(match m {
                        Map::Normal => "which way each pixel faces, as Godot reads it (green up)",
                        Map::Height => "how high the surface rises, white highest",
                        Map::Specular => "how shiny it is, and in what colour",
                        Map::Emission => "what glows whatever the light",
                        Map::Depth => "how near each pixel is: the plane's depth, z and height, near is white",
                        Map::Occlusion => "creases and the foot of raised forms, darker",
                    })
                    .clicked()
                {
                    show = Some(Show::Map(*m));
                }
            });
            if ui
                .selectable_label(show == Some(Show::Lit), "lit")
                .hint("the picture lit by the preview lights; hold L to light it with the pointer")
                .clicked()
            {
                show = Some(Show::Lit);
            }
            self.maps.show = show;
        });
        let maps = self.scene.bundle.doc.maps.clone();
        let mut on = maps.is_some();
        if ui
            .checkbox(&mut on, "light maps")
            .hint("normal, height, specular and emission maps for a game's 2D lights, made from what is drawn; turned off and on again, they come back with their surface and lights")
            .changed()
        {
            let next = match on {
                true => Some(self.maps.parked.take().unwrap_or_default()),
                false => {
                    self.maps.parked = maps;
                    None
                }
            };
            return self.set_maps(next);
        }
        let current = maps.clone().unwrap_or_default();
        ui.label(egui::RichText::new("The drawing's surface").strong());
        if let Some(next) = self.surface_fields(ui, "maps.surface", None, &current.surface) {
            return self.set_maps(Some(Maps {
                surface: next,
                ..current
            }));
        }
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            self.command_check(ui, crate::commands::Cmd::CheckMaps, "check the maps");
            self.command_check(ui, crate::commands::Cmd::ShowOccluders, "occluders");
        });
        self.findings_list(ui)?;
        ui.separator();
        ui.label(egui::RichText::new("Preview lights").strong());
        let mut next = current.clone();
        let paint = self.color;
        ui.horizontal_wrapped(|ui| {
            let ambient = color::rgba_of(&current.ambient).unwrap_or([0.2, 0.2, 0.25, 1.0]);
            if let Some(rgba) = color::Square::new(
                ui,
                "maps.ambient",
                ambient,
                "the ambient colour, where no light reaches (Godot's CanvasModulate)",
            )
            .current(paint)
            .pick(ui)
            {
                next.ambient = color::hex(rgba);
            }
            ui.label("ambient");
            if let Some(v) = self.number(
                ui,
                "maps.shininess",
                "shininess ",
                current.shininess,
                1.0..=512.0,
                0.5,
                "how tight the highlights on the specular map are",
            ) {
                next.shininess = v;
            }
        });
        let (w, h) = self.scene.size();
        let mut gone = None;
        current.lights.iter().enumerate().for_each(|(i, light)| {
            ui.horizontal_wrapped(|ui| {
                let rgba = color::rgba_of(&light.color).unwrap_or([1.0; 4]);
                if let Some(rgba) =
                    color::Square::new(ui, ("maps.light", i), rgba, format!("the colour of light {}", i + 1))
                        .current(paint)
                        .pick(ui)
                {
                    next.lights[i].color = color::hex(rgba);
                }
                let point = light.kind == LightKind::Point;
                ui.label(match point {
                    true => "point",
                    false => "from",
                });
                let fields: Vec<(&str, f32, std::ops::RangeInclusive<f32>, f64, &str)> = match point {
                    true => vec![
                        (
                            "x ",
                            light.at[0],
                            -(w as f32)..=2.0 * w as f32,
                            1.0,
                            "where it is across the canvas",
                        ),
                        (
                            "y ",
                            light.at[1],
                            -(h as f32)..=2.0 * h as f32,
                            1.0,
                            "where it is down the canvas",
                        ),
                        ("height ", light.height, 0.0..=4096.0, 1.0, "px in front of the canvas"),
                        ("energy ", light.energy, 0.0..=16.0, 0.02, "how bright"),
                        ("range ", light.range, 1.0..=16384.0, 2.0, "px to where it fades out"),
                    ],
                    false => vec![
                        (
                            "x ",
                            light.at[0],
                            -1.0..=1.0,
                            0.01,
                            "the direction it comes from, across",
                        ),
                        ("y ", light.at[1], -1.0..=1.0, 0.01, "the direction it comes from, down"),
                        (
                            "height ",
                            light.height,
                            0.0..=1.0,
                            0.01,
                            "0 along the picture, 1 straight from the front",
                        ),
                        ("energy ", light.energy, 0.0..=16.0, 0.02, "how bright"),
                    ],
                };
                fields
                    .into_iter()
                    .enumerate()
                    .for_each(|(k, (label, value, range, speed, hint))| {
                        if let Some(v) = self.number(ui, &format!("light.{i}.{k}"), label, value, range, speed, hint) {
                            let l = &mut next.lights[i];
                            match k {
                                0 => l.at[0] = v,
                                1 => l.at[1] = v,
                                2 => l.height = v,
                                3 => l.energy = v,
                                _ => l.range = v,
                            }
                        }
                    });
                if ui
                    .button(crate::theme::icon::TRASH)
                    .hint(format!("remove light {}", i + 1))
                    .clicked()
                {
                    gone = Some(i);
                }
            });
        });
        if let Some(i) = gone {
            next.lights.remove(i);
        }
        ui.horizontal_wrapped(|ui| {
            if ui
                .button("add a point light")
                .hint("a light above the top left of the drawing; move it with x and y")
                .clicked()
            {
                next.lights.push(pointer_light(
                    &self.scene.bundle.doc,
                    [w as f32 * 0.25, h as f32 * 0.25],
                ));
            }
            if ui
                .button("add a directional light")
                .hint("light from one direction everywhere, like the sun")
                .clicked()
            {
                next.lights.push(Light::directional([-1.0, -1.0], 0.5));
            }
        });
        if next != current {
            if next.validate().is_err() {
                return Ok(());
            }
            if maps.is_none() && next.lights.is_empty() && next == Maps::default() {
                return Ok(());
            }
            return self.set_maps(Some(next));
        }
        Ok(())
    }
}
