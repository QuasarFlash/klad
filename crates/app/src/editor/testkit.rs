use super::Editor;
use crate::home::Home;
use crate::messages::Tone;
use crate::settings::Settings;
use egui::accesskit::Role;
use egui::{Key, Modifiers, Pos2, Vec2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub(super) fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    std::fs::read_dir(from).unwrap().for_each(|e| {
        let e = e.unwrap();
        let target = to.join(e.file_name());
        match e.file_type().unwrap().is_dir() {
            true if e.file_name() == ".cache" => {}
            true => copy_dir(&e.path(), &target),
            false => {
                std::fs::copy(e.path(), target).unwrap();
            }
        }
    });
}

pub(super) fn released(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/formats/0.2.0")
        .join(name)
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("klad-journey-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::canonicalize(&dir).unwrap()
}

const PATIENCE: Duration = Duration::from_secs(30);

pub(super) const FNV: u64 = 0xcbf2_9ce4_8422_2325;

pub(super) fn mix(hash: u64, value: u64) -> u64 {
    (hash ^ value).wrapping_mul(0x0100_0000_01b3)
}

pub(super) fn picture(ed: &Editor) -> u64 {
    let outbox = crate::stats::lock(&ed.outbox);
    ed.scene
        .all_tiles()
        .iter()
        .filter_map(|i| outbox.tile(*i))
        .fold(FNV, |hash, tile| {
            tile.iter().fold(hash, |hash, v| mix(hash, u64::from(v.to_bits())))
        })
}

pub(super) fn written(node: &egui_kittest::kittest::AccessKitNode<'_>) -> Option<String> {
    match node.role() {
        Role::Label => node.value(),
        _ => node.label(),
    }
    .filter(|text| !text.is_empty())
}

fn tone(tone: Tone) -> &'static str {
    match tone {
        Tone::Info => "info",
        Tone::Done => "done",
        Tone::Refused => "refused",
        Tone::Error => "error",
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Rows {
    Panel,
    Timeline,
}

pub(super) struct Journey {
    pub(super) h: Harness<'static, Home>,
    pub(super) folder: PathBuf,
    out: Option<PathBuf>,
    step: usize,
    heard: u64,
    lines: Vec<String>,
    misses: Vec<String>,
    pictures: bool,
}

impl Journey {
    pub(super) fn begin(name: &str, size: Vec2, open: Option<&str>) -> Self {
        Self::begin_scaled(name, size, open, 1.0)
    }

    pub(super) fn begin_scaled(name: &str, size: Vec2, open: Option<&str>, scale: f32) -> Self {
        Self::start(name, size, open, scale, true)
    }

    pub(super) fn first_launch(name: &str, size: Vec2) -> Self {
        Self::start(name, size, None, 1.0, false)
    }

    fn start(name: &str, size: Vec2, open: Option<&str>, scale: f32, welcomed: bool) -> Self {
        let folder = scratch(name);
        let out = std::env::var_os("KLAD_JOURNEY_OUT")
            .filter(|dir| !dir.is_empty())
            .map(|dir| PathBuf::from(dir).join(name));
        if let Some(out) = &out {
            let _ = std::fs::remove_dir_all(out);
            std::fs::create_dir_all(out).unwrap();
        }
        let gpu = out.as_ref().map(|_| {
            egui_kittest::wgpu::create_render_state(
                egui_kittest::wgpu::default_wgpu_setup(),
                egui_wgpu::RendererOptions::PREDICTABLE,
            )
        });
        let settings = Settings {
            drawings: Some(folder.display().to_string()),
            check_updates: false,
            ui_scale: scale,
            welcomed,
            ..Settings::default()
        };
        let mut home = Home::with(settings, None, gpu.clone());
        let opened = open.map(|sample| {
            let copy = folder.join(sample);
            copy_dir(&released(sample), &copy);
            copy
        });
        home.begin(&egui::Context::default(), opened.as_deref());
        let builder = Harness::builder().with_size(size).with_step_dt(1.0 / 30.0);
        let builder = match gpu {
            Some(gpu) => builder.renderer(egui_kittest::wgpu::WgpuTestRenderer::from_render_state(gpu)),
            None => builder,
        };
        let h = builder.build_ui_state(|ui, home: &mut Home| home.frame(ui), home);
        let mut journey = Self {
            h,
            folder,
            out,
            step: 0,
            heard: 0,
            lines: vec![format!("# {name}"), String::new()],
            misses: Vec::new(),
            pictures: true,
        };
        journey.h.run_steps(4);
        journey
    }

    pub(super) fn pictures(&mut self, on: bool) {
        self.pictures = on;
    }

    pub(super) fn out(&self) -> Option<&Path> {
        self.out.as_deref()
    }

    pub(super) fn window(&self) -> egui::Rect {
        self.h.ctx.content_rect()
    }

    pub(super) fn wheel_at(&mut self, at: Pos2, down: f32) {
        self.h.event(egui::Event::PointerMoved(at));
        self.h.run_steps(1);
        self.h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, -down),
            modifiers: Modifiers::NONE,
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
        });
        self.h.run_steps(2);
    }

    pub(super) fn ed(&self) -> Option<&Editor> {
        self.h.state().editor.as_deref()
    }

    pub(super) fn ed_mut(&mut self) -> Option<&mut Editor> {
        self.h.state_mut().editor.as_deref_mut()
    }

    fn pass(&mut self) {
        self.h.run_steps(1);
        if self.out.is_some() {
            let _ = self.h.render();
        }
    }

    fn ready(&mut self) {
        let started = Instant::now();
        while self.ed().is_some_and(|ed| !ed.canvas_current()) && started.elapsed() < Duration::from_secs(3) {
            self.pass();
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    pub(super) fn settle(&mut self) {
        let started = Instant::now();
        let waiting = |ed: &Editor| {
            !ed.canvas_current()
                || !ed.jobs.is_empty()
                || ed
                    .preview
                    .as_ref()
                    .is_some_and(|p| p.posed.is_none() || p.job.is_some())
        };
        self.pass();
        while self.ed().is_some_and(waiting) && started.elapsed() < Duration::from_secs(60) {
            self.pass();
            std::thread::sleep(Duration::from_millis(5));
        }
        if let Some(ed) = self.ed_mut() {
            ed.redraw_canvas();
        }
        (0..6).for_each(|_| self.pass());
    }

    fn said(&mut self) -> Vec<String> {
        let heard = self.heard;
        let fresh: Vec<(u64, String)> = self
            .ed()
            .map(|ed| {
                ed.messages
                    .log()
                    .filter(|m| m.id > heard)
                    .map(|m| (m.id, format!("[{}] {}", tone(m.tone), m.text.replace('\n', " / "))))
                    .collect()
            })
            .unwrap_or_default();
        self.heard = fresh.iter().map(|(id, _)| *id).max().unwrap_or(heard);
        fresh.into_iter().map(|(_, text)| text).collect()
    }

    fn state(&self) -> String {
        let home = self.h.state();
        match (&home.editor, &home.start) {
            (Some(ed), _) => {
                let layer = ed.scene.active_layer().map_or_else(
                    || "no layer".to_string(),
                    |l| {
                        format!(
                            "{} ({}{})",
                            l.name,
                            l.kind_name(),
                            if l.pixel() { ", pixels" } else { "" }
                        )
                    },
                );
                let view = ed.view.map_or_else(String::new, |v| {
                    format!(
                        "zoom {:.2}, turned {:.0}{}",
                        v.zoom,
                        v.angle.to_degrees(),
                        if v.flip { ", flipped" } else { "" }
                    )
                });
                let picked = ed.picked.as_ref().map_or(0, |p| p.ids.len());
                let shape = ed.selection.clone().unwrap_or_else(|| "none".into());
                let doc = &ed.scene.bundle.doc;
                format!(
                    "editor on {}: tool {:?}, layer {layer}, frame {} of {}, {} marks here, {} in all, {} layers, strokes picked {picked}, shape picked {shape}, {view}, size {:.1}, colour #{}",
                    ed.scene.bundle.root.file_name().unwrap_or_default().to_string_lossy(),
                    ed.tool,
                    ed.scene.frame,
                    doc.timeline.frames,
                    ed.scene.active_marks().len(),
                    ed.scene.total_marks(),
                    doc.layers.len(),
                    ed.size,
                    ed.color[..3]
                        .iter()
                        .map(|c| format!("{:02x}", (c.clamp(0.0, 1.0) * 255.0).round() as u8))
                        .collect::<String>(),
                )
            }
            (None, Some(start)) => format!(
                "start window{}",
                start
                    .error
                    .as_ref()
                    .map_or_else(String::new, |e| format!(", error: {e}"))
            ),
            (None, None) => "nothing open".to_string(),
        }
    }

    pub(super) fn screen(&self) -> Vec<String> {
        let mut seen = BTreeSet::new();
        self.h
            .query_all(egui_kittest::kittest::By::new().predicate(|n| written(n).is_some()))
            .filter_map(|n| {
                let node = n.accesskit_node();
                let label = written(&node)?;
                let text = match node.role() {
                    Role::Label => label,
                    role => format!("{role:?} \"{label}\""),
                };
                seen.insert(text.clone()).then_some(text)
            })
            .take(400)
            .collect()
    }

    pub(super) fn picture(&mut self) -> u64 {
        self.settle();
        self.ed().map_or(0, |ed| {
            ed.scene
                .compose(&ed.scene.all_tiles(), None)
                .map(|tiles| {
                    tiles.iter().fold(FNV, |hash, (_, tile)| {
                        tile.iter().fold(hash, |hash, v| mix(hash, u64::from(v.to_bits())))
                    })
                })
                .unwrap_or(0)
        })
    }

    pub(super) fn wait(&mut self, what: &str, until: impl Fn(&Editor) -> bool) -> bool {
        let started = Instant::now();
        let there = |j: &Self| j.ed().is_some_and(&until);
        while !there(self) && started.elapsed() < PATIENCE {
            self.pass();
            std::thread::sleep(Duration::from_millis(10));
        }
        let ok = there(self);
        self.note(match ok {
            true => format!("{what}: there after {:.1} s", started.elapsed().as_secs_f32()),
            false => format!("{what}: not there after {} s", PATIENCE.as_secs()),
        });
        ok
    }

    pub(super) fn agent(&mut self, args: &[&str]) -> (bool, String) {
        let klad = crate::start::draw_binary().expect("build the klad binary first (just test does)");
        let home = format!("{}/", self.folder.display());
        let said = args.iter().map(|a| a.replace(&home, "")).collect::<Vec<_>>().join(" ");
        let out = std::process::Command::new(klad)
            .args(args)
            .output()
            .expect("the klad command runs");
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
        .replace(&home, "");
        self.note(format!(
            "the agent ran: klad {said}{}",
            if out.status.success() { "" } else { " (it failed)" }
        ));
        text.lines()
            .filter(|line| !line.trim().is_empty())
            .take(8)
            .for_each(|line| self.lines.push(format!("  - it read: {line}")));
        (out.status.success(), text)
    }

    pub(super) fn copied(&self) -> Vec<String> {
        self.h
            .output()
            .platform_output
            .commands
            .iter()
            .filter_map(|c| match c {
                egui::OutputCommand::CopyText(text) => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn hint_of(hints: &[(egui::Rect, String)], r: egui::Rect) -> Option<usize> {
        let same = |a: egui::Rect| (a.min - r.min).abs().max_elem() < 0.5 && (a.max - r.max).abs().max_elem() < 0.5;
        hints.iter().position(|(rect, _)| same(*rect)).or_else(|| {
            hints
                .iter()
                .enumerate()
                .filter(|(_, (rect, _))| rect.contains(r.center()) && rect.height() < 60.0)
                .min_by(|a, b| a.1.0.area().total_cmp(&b.1.0.area()))
                .map(|(i, _)| i)
        })
    }

    fn pane_of(&self, r: egui::Rect) -> Option<(f32, f32)> {
        let canvas = self.ed()?.canvas_rect;
        let at = r.center();
        let hints = crate::theme::hints(&self.h.ctx);
        let me = Self::hint_of(&hints, r)?;
        let title =
            |text: &str| (text.starts_with("fold ") && text.ends_with(" to its title")) || text.starts_with("open ");
        let top = |i: usize| hints[i].0.center().y - crate::layout::TITLE / 2.0;
        let titles: Vec<usize> = hints
            .iter()
            .enumerate()
            .filter(|(_, (rect, text))| rect.center().x > canvas.right() && title(text))
            .map(|(i, _)| i)
            .collect();
        let play = hints.iter().position(|(_, text)| text.starts_with("play or stop"));
        let docked = at.x > canvas.right()
            && titles.first().is_some_and(|first| {
                me >= *first
                    && match play {
                        Some(play) if play > *first => me < play,
                        _ => true,
                    }
            });
        let pane = titles.iter().rposition(|t| *t <= me).filter(|_| docked)?;
        let body = top(titles[pane]) + crate::layout::TITLE;
        let floor = titles
            .get(pane + 1)
            .map_or(canvas.bottom(), |next| top(*next).min(canvas.bottom()));
        Some((body, floor))
    }

    fn raised(&self, at: Pos2) -> bool {
        self.h
            .ctx
            .layer_id_at(at)
            .is_some_and(|layer| layer.order != egui::Order::Background)
    }

    fn visible(&self, r: egui::Rect) -> bool {
        let window = self.h.ctx.content_rect();
        let Some(canvas) = self.ed().map(|ed| ed.canvas_rect) else {
            return window.contains(r.center());
        };
        let at = r.center();
        if !window.contains(at) {
            return false;
        }
        if self.raised(at) {
            return true;
        }
        if canvas.contains(at) {
            return false;
        }
        let hints = crate::theme::hints(&self.h.ctx);
        if Self::hint_of(&hints, r).is_none() {
            return true;
        }
        match self.pane_of(r) {
            Some((body, floor)) => {
                at.y < canvas.bottom()
                    && ((at.y < body && at.y >= body - crate::layout::TITLE)
                        || (r.top() >= body - 1.0 && r.bottom() <= floor + 1.0))
            }
            None => {
                at.y < canvas.bottom()
                    || r.bottom() <= window.bottom() - crate::layout::STATUS + 1.0
                    || self
                        .ed()
                        .is_some_and(|ed| r.top() >= canvas.bottom() + ed.timeline_shown - 1.0)
            }
        }
    }

    pub(super) fn scroll_to(&mut self, label: &str) -> bool {
        let label = &crate::commands::local_keys(label);
        let mut turns = 0;
        while turns < 24 && !self.in_sight(label) {
            let Some((r, body, floor)) = self
                .h
                .query_all_by_label(label)
                .map(|n| n.rect())
                .find_map(|r| self.pane_of(r).map(|(body, floor)| (r, body, floor)))
            else {
                break;
            };
            self.wheel_pane(r, body, floor);
            turns += 1;
        }
        let seen = self.in_sight(label);
        self.note(match (seen, turns) {
            (true, 0) => format!("\"{label}\" is in sight"),
            (true, n) => format!("scrolled its panel {n} times to bring \"{label}\" in sight"),
            (false, _) => format!("looked for \"{label}\": it could not be brought in sight"),
        });
        seen
    }

    fn wheel_pane(&mut self, r: egui::Rect, body: f32, floor: f32) {
        let over = egui::pos2(r.center().x, (body + floor) / 2.0);
        let down = r.bottom() > floor;
        self.h.event(egui::Event::PointerMoved(over));
        self.h.run_steps(1);
        self.h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, if down { -40.0 } else { 40.0 }),
            modifiers: Modifiers::NONE,
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
        });
        self.h.run_steps(3);
    }

    pub(super) fn scroll_to_hinted(&mut self, hint: &str) -> bool {
        let hint = &crate::commands::local_keys(hint);
        let mut turns = 0;
        while turns < 24 && !self.hint_in_sight(hint) {
            let Some((r, body, floor)) = crate::theme::hints(&self.h.ctx)
                .into_iter()
                .filter(|(_, text)| text.starts_with(hint))
                .find_map(|(r, _)| self.pane_of(r).map(|(body, floor)| (r, body, floor)))
            else {
                break;
            };
            self.wheel_pane(r, body, floor);
            turns += 1;
        }
        let seen = self.hint_in_sight(hint);
        self.note(match (seen, turns) {
            (true, 0) => format!("the control \"{hint}\" is in sight"),
            (true, n) => format!("scrolled its panel {n} times to bring the control \"{hint}\" in sight"),
            (false, _) => format!("looked for the control \"{hint}\": it could not be brought in sight"),
        });
        seen
    }

    fn hinted(&self, hint: &str) -> Option<(egui::Rect, String, bool)> {
        let hint = &crate::commands::local_keys(hint);
        let found: Vec<(egui::Rect, String)> = crate::theme::hints(&self.h.ctx)
            .into_iter()
            .filter(|(_, text)| text.starts_with(hint))
            .collect();
        found
            .iter()
            .find(|(rect, _)| self.visible(*rect))
            .map(|(rect, text)| (*rect, text.clone(), true))
            .or(found.first().map(|(rect, text)| (*rect, text.clone(), false)))
    }

    pub(super) fn press_hinted(&mut self, hint: &str) -> bool {
        let hint = &crate::commands::local_keys(hint);
        let found = self.hinted(hint);
        self.note(match &found {
            Some((_, _, false)) => format!("looked for the button \"{hint}\": it is out of sight"),
            Some((_, text, true)) => match text.split_once("not now: ") {
                Some((_, why)) => format!("clicked the button \"{hint}\", which is greyed out: {}", why.trim()),
                None => format!("clicked the button \"{hint}\""),
            },
            None => format!("looked for the button \"{hint}\": not on screen"),
        });
        let Some((rect, _, true)) = found else {
            return false;
        };
        self.h.hover_at(rect.center());
        self.h.run_steps(1);
        let at = self
            .hinted(hint)
            .filter(|(_, _, seen)| *seen)
            .map_or(rect.center(), |(now, _, _)| now.center());
        self.h.hover_at(at);
        self.press(at, egui::PointerButton::Primary, true, Modifiers::NONE);
        self.press(at, egui::PointerButton::Primary, false, Modifiers::NONE);
        self.h.run_steps(3);
        true
    }

    fn row(&self, layer: &str, rows: Rows) -> Option<egui::Rect> {
        let canvas = self.ed()?.canvas_rect;
        self.h
            .query_all_by_label(layer)
            .filter(|n| n.accesskit_node().role() == Role::Button)
            .map(|n| n.rect())
            .find(|r| {
                self.visible(*r)
                    && match rows {
                        Rows::Panel => r.center().x > canvas.right() && r.center().y < canvas.bottom(),
                        Rows::Timeline => r.center().y > canvas.bottom(),
                    }
            })
    }

    pub(super) fn rows_in_sight(&self, layers: &[String], rows: Rows) -> Vec<String> {
        layers
            .iter()
            .filter(|layer| self.row(layer, rows).is_some())
            .cloned()
            .collect()
    }

    pub(super) fn drag_row(&mut self, layer: &str, onto: &str, part: f32, rows: Rows) -> bool {
        self.drag_row_from(layer, onto, part, rows, false)
    }

    pub(super) fn drag_row_from(&mut self, layer: &str, onto: &str, part: f32, rows: Rows, by_name: bool) -> bool {
        let place = match rows {
            Rows::Panel => "the Layers panel",
            Rows::Timeline => "the timeline",
        };
        let (Some(from), Some(to)) = (self.row(layer, rows), self.row(onto, rows)) else {
            self.note(format!(
                "looked for the rows of {layer} and {onto} in {place}: not both in sight"
            ));
            return false;
        };
        let name = self
            .h
            .query_all_by_label(layer)
            .filter(|n| n.accesskit_node().role() != Role::Button)
            .map(|n| n.rect())
            .find(|r| from.contains(r.center()));
        let a = match (by_name, name) {
            (true, Some(name)) => name.center(),
            _ => egui::pos2(name.map_or(from.left(), |n| n.right()) + 30.0, from.center().y),
        };
        self.note(format!(
            "dragged the row of {layer} in {place}, held {}, onto {onto}, {}",
            if by_name { "by its name" } else { "right of its name" },
            match part {
                p if p < 0.3 => "near its top edge",
                p if p > 0.7 => "near its bottom edge",
                _ => "in its middle",
            }
        ));
        let b = egui::pos2(a.x, to.top() + to.height() * part);
        self.h.hover_at(a);
        self.h.run_steps(1);
        self.press(a, egui::PointerButton::Primary, true, Modifiers::NONE);
        (1..=8).for_each(|i| {
            self.h.hover_at(a + (b - a) * (i as f32 / 8.0));
            self.h.run_steps(1);
        });
        self.press(b, egui::PointerButton::Primary, false, Modifiers::NONE);
        self.h.run_steps(3);
        true
    }

    pub(super) fn spot_of(&self, hint: &str) -> Option<egui::Rect> {
        self.hinted(hint).filter(|(_, _, seen)| *seen).map(|(rect, _, _)| rect)
    }

    pub(super) fn hint_in_sight(&self, hint: &str) -> bool {
        let hint = &crate::commands::local_keys(hint);
        self.hinted(hint).is_some_and(|(_, _, seen)| seen)
    }

    pub(super) fn drag_screen(&mut self, from: Pos2, to: Pos2, what: &str) {
        self.note(format!("dragged {what}"));
        self.h.hover_at(from);
        self.h.run_steps(1);
        self.press(from, egui::PointerButton::Primary, true, Modifiers::NONE);
        (1..=8).for_each(|i| {
            self.h.hover_at(from + (to - from) * (i as f32 / 8.0));
            self.h.run_steps(1);
        });
        self.press(to, egui::PointerButton::Primary, false, Modifiers::NONE);
        self.h.run_steps(3);
    }

    pub(super) fn act(&mut self, what: &str) {
        self.step += 1;
        self.lines.push(String::new());
        self.lines.push(format!("## {}. {what}", self.step));
    }

    pub(super) fn note(&mut self, text: impl AsRef<str>) {
        self.lines.push(format!("- {}", text.as_ref()));
    }

    pub(super) fn look(&mut self, name: &str) {
        self.settle();
        self.said()
            .into_iter()
            .for_each(|m| self.lines.push(format!("- said {m}")));
        let state = self.state();
        self.lines.push(format!("- now: {state}"));
        if let Some(out) = self.out.clone().filter(|_| self.pictures) {
            let screen = self.screen().join(" | ");
            self.lines.push(format!("- on screen: {screen}"));
            let file = format!("{:02}-{name}.png", self.step);
            match self.h.render() {
                Ok(image) => {
                    image.save(out.join(&file)).unwrap();
                    self.lines.push(format!("- picture: {file}"));
                }
                Err(e) => self.lines.push(format!("- no picture: {e}")),
            }
        }
    }

    pub(super) fn expect(&mut self, key: &str, ok: bool, what: &str) {
        self.lines
            .push(format!("- {} {what} `{key}`", if ok { "ok:" } else { "MISS:" }));
        if !ok {
            self.misses.push(key.to_string());
        }
    }

    pub(super) fn key(&mut self, key: Key) {
        self.note(format!("pressed {}", key.name()));
        self.h.key_press(key);
        self.h.run_steps(3);
    }

    pub(super) fn hold(&mut self, key: Key, down: bool) {
        self.note(format!(
            "{} {}",
            if down { "held down" } else { "let go of" },
            key.name()
        ));
        self.h.event(egui::Event::Key {
            key,
            physical_key: None,
            pressed: down,
            repeat: false,
            modifiers: Modifiers::NONE,
        });
        self.h.run_steps(2);
    }

    pub(super) fn chord(&mut self, modifiers: Modifiers, key: Key) {
        self.note(format!(
            "pressed {}{}{}{}",
            if modifiers.command { "cmd " } else { "" },
            if modifiers.shift { "shift " } else { "" },
            if modifiers.alt { "alt " } else { "" },
            key.name()
        ));
        self.h.key_press_modifiers(modifiers, key);
        self.h.run_steps(3);
    }

    pub(super) fn shown(&self, label: &str) -> bool {
        let label = &crate::commands::local_keys(label);
        self.h.query_all_by_label_contains(label).next().is_some()
    }

    fn choose(&self, label: &str, keep: impl Fn(Role) -> bool) -> Result<usize, bool> {
        let hints = crate::theme::hints(&self.h.ctx);
        let hinted = |r: egui::Rect| {
            hints
                .iter()
                .any(|(h, _)| (h.min - r.min).abs().max_elem() < 0.5 && (h.max - r.max).abs().max_elem() < 0.5)
        };
        let nodes: Vec<(usize, egui::Rect, bool)> = self
            .h
            .query_all_by_label(label)
            .enumerate()
            .filter(|(_, n)| keep(n.accesskit_node().role()))
            .map(|(i, n)| (i, n.rect(), hinted(n.rect())))
            .collect();
        let on_top = nodes
            .iter()
            .find(|(_, r, _)| self.h.ctx.content_rect().contains(r.center()) && self.raised(r.center()));
        match on_top.or_else(|| nodes.iter().find(|(_, r, hinted)| *hinted && self.visible(*r))) {
            Some((i, _, _)) => Ok(*i),
            None if nodes.iter().any(|(_, _, hinted)| *hinted) => Err(true),
            None => nodes.first().map(|(i, _, _)| *i).ok_or(false),
        }
    }

    fn missing(label: &str, hidden: bool, to: &str) -> String {
        match hidden {
            true => format!("looked for \"{label}\" to {to}: it is scrolled out of sight"),
            false => format!("looked for \"{label}\" to {to}: not on screen"),
        }
    }

    fn poke(&mut self, at: Pos2) {
        self.h.hover_at(at);
        self.h.run_steps(1);
        self.press(at, egui::PointerButton::Primary, true, Modifiers::NONE);
        self.press(at, egui::PointerButton::Primary, false, Modifiers::NONE);
    }

    pub(super) fn click(&mut self, label: &str) -> bool {
        let label = &crate::commands::local_keys(label);
        let chosen = self.choose(label, |_| true);
        let at = chosen
            .ok()
            .and_then(|i| self.h.query_all_by_label(label).nth(i))
            .map(|n| n.rect().center());
        if let Some(at) = at {
            self.poke(at);
        }
        self.note(match chosen {
            Ok(_) => format!("clicked \"{label}\""),
            Err(hidden) => Self::missing(label, hidden, "click"),
        });
        self.h.run_steps(3);
        chosen.is_ok()
    }

    pub(super) fn pick_in_list(&mut self, list: &str, item: &str) -> bool {
        let Some(top) = self.h.query_all_by_label(list).next().map(|n| n.rect()) else {
            self.note(format!("looked for the list \"{list}\": not on screen"));
            return false;
        };
        self.click(list);
        self.h.run_steps(12);
        let below = egui::pos2(top.center().x, top.bottom() + 40.0);
        let reach = |j: &Self| {
            j.h.query_all_by_label(item).map(|n| n.rect()).find(|r| {
                r.center().y > top.bottom()
                    && (r.center().x - top.center().x).abs() < 200.0
                    && [-10.0, 0.0, 10.0]
                        .iter()
                        .all(|dy| j.raised(r.center() + vec2(0.0, *dy)))
            })
        };
        let mut scrolled = 0;
        while reach(self).is_none() && scrolled < 40 {
            let x = top.center().x;
            let floor = (1..120)
                .map(|i| top.bottom() + 6.0 + i as f32 * 8.0)
                .take_while(|y| self.raised(egui::pos2(x, *y)))
                .last()
                .unwrap_or(top.bottom() + 40.0);
            let above = self
                .h
                .query_all_by_label(item)
                .map(|n| n.rect())
                .any(|r| (r.center().x - x).abs() < 200.0 && r.center().y < (top.bottom() + floor) / 2.0);
            self.h.event(egui::Event::PointerMoved(below));
            self.h.run_steps(1);
            self.h.event(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: vec2(0.0, if above { 30.0 } else { -30.0 }),
                modifiers: Modifiers::NONE,
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Wheel,
            });
            self.h.run_steps(10);
            scrolled += 1;
        }
        let found = reach(self);
        self.note(match (found, scrolled) {
            (Some(_), 0) => format!("chose \"{item}\" in the list"),
            (Some(_), n) => format!("scrolled the list {n} times to reach \"{item}\" and chose it"),
            (None, _) => format!("looked for \"{item}\" in the list: not reached"),
        });
        match found {
            Some(r) => {
                self.poke(r.center());
                self.h.run_steps(3);
                true
            }
            None => {
                self.h.key_press(Key::Escape);
                self.h.run_steps(2);
                false
            }
        }
    }

    pub(super) fn click_containing(&mut self, label: &str) -> bool {
        let label = &crate::commands::local_keys(label);
        let at = self
            .h
            .query_all_by_label_contains(label)
            .filter(|n| n.accesskit_node().role() != Role::Window)
            .map(|n| n.rect())
            .find(|r| self.visible(*r))
            .map(|r| r.center());
        if let Some(at) = at {
            self.poke(at);
        }
        let found = at.is_some();
        self.note(match found {
            true => format!("clicked the control saying \"{label}\""),
            false => format!("looked for a control saying \"{label}\": not on screen"),
        });
        self.h.run_steps(3);
        found
    }

    pub(super) fn menu(&mut self, menu: &str, sub: &str, item: &str) -> bool {
        let item = &crate::commands::local_keys(item);
        let bar = self.h.query_all_by_label(menu).next().map(|n| n.rect().center());
        if let Some(at) = bar {
            self.poke(at);
        }
        self.h.run_steps(2);
        let middle = self
            .h
            .query_all_by_label_contains(sub)
            .find(|n| n.accesskit_node().role() != Role::Window)
            .map(|n| n.rect().center());
        if let Some(at) = middle {
            self.h.hover_at(at);
            self.h.run_steps(3);
        }
        let at = self
            .h
            .query_all_by_label(item)
            .find(|n| !n.accesskit_node().is_disabled())
            .map(|n| n.rect().center());
        if let Some(at) = at {
            self.poke(at);
        }
        let found = at.is_some();
        self.note(match found {
            true => format!("chose {menu} > {sub} > {item}"),
            false => format!("looked for {menu} > {sub} > {item}: not there, or greyed out"),
        });
        self.h.run_steps(3);
        if !found {
            self.h.key_press(Key::Escape);
            self.h.run_steps(2);
        }
        found
    }

    pub(super) fn double_click(&mut self, label: &str) -> bool {
        let label = &crate::commands::local_keys(label);
        let chosen = self.choose(label, |_| true);
        let at = chosen
            .ok()
            .and_then(|i| self.h.query_all_by_label(label).nth(i))
            .map(|n| n.rect().center());
        self.note(match chosen {
            Ok(_) => format!("double clicked \"{label}\""),
            Err(hidden) => Self::missing(label, hidden, "double click"),
        });
        if let Some(at) = at {
            self.h.run_steps(15);
            self.h.hover_at(at);
            self.h.run_steps(1);
            (0..2).for_each(|_| {
                self.press(at, egui::PointerButton::Primary, true, Modifiers::NONE);
                self.press(at, egui::PointerButton::Primary, false, Modifiers::NONE);
            });
            self.h.run_steps(3);
        }
        at.is_some()
    }

    pub(super) fn type_text(&mut self, text: &str) {
        self.note(format!("typed \"{text}\""));
        self.h.event(egui::Event::Text(text.to_string()));
        self.h.run_steps(2);
    }

    pub(super) fn type_into(&mut self, label: &str, text: &str) -> bool {
        let chosen = self.choose(label, |role| matches!(role, Role::TextInput | Role::MultilineTextInput));
        if let Ok(i) = chosen
            && let Some(n) = self.h.query_all_by_label(label).nth(i)
        {
            n.focus();
            n.type_text(text);
        }
        self.note(match chosen {
            Ok(_) => format!("typed \"{text}\" into \"{label}\""),
            Err(hidden) => Self::missing(label, hidden, "type in"),
        });
        self.h.run_steps(3);
        chosen.is_ok()
    }

    pub(super) fn retype(&mut self, label: &str, text: &str) -> bool {
        let chosen = self.choose(label, |role| matches!(role, Role::TextInput | Role::MultilineTextInput));
        let at = chosen
            .ok()
            .and_then(|i| self.h.query_all_by_label(label).nth(i))
            .map(|n| n.rect().center());
        if let Some(at) = at {
            self.poke(at);
        }
        let found = chosen.is_ok();
        self.h.run_steps(2);
        if found {
            self.h.key_press_modifiers(Modifiers::COMMAND, Key::A);
            self.h.run_steps(1);
            self.h.event(egui::Event::Text(text.to_string()));
            self.h.run_steps(1);
            self.h.key_press(Key::Enter);
        }
        self.note(match found {
            true => format!("clicked into \"{label}\", selected all, typed \"{text}\" and pressed Return"),
            false => format!("looked for a field \"{label}\" to type in: not on screen"),
        });
        self.h.run_steps(3);
        found
    }

    pub(super) fn click_leftmost(&mut self, label: &str) -> bool {
        let at = self
            .h
            .query_all_by_label(label)
            .map(|n| n.rect())
            .min_by(|a, b| a.left().total_cmp(&b.left()))
            .map(|r| r.center());
        if let Some(at) = at {
            self.poke(at);
        }
        let found = at.is_some();
        self.note(match found {
            true => format!("clicked \"{label}\", the one furthest left"),
            false => format!("looked for \"{label}\" to click: not on screen"),
        });
        self.h.run_steps(3);
        found
    }

    pub(super) fn retype_leftmost(&mut self, label: &str, text: &str) -> bool {
        let found = self
            .h
            .query_all_by_label(label)
            .filter(|n| matches!(n.accesskit_node().role(), Role::TextInput | Role::MultilineTextInput))
            .map(|n| n.rect())
            .min_by(|a, b| a.left().total_cmp(&b.left()))
            .map(|r| r.center());
        if let Some(at) = found {
            self.poke(at);
        }
        let found = found.is_some();
        self.h.run_steps(2);
        if found {
            self.h.key_press_modifiers(Modifiers::COMMAND, Key::A);
            self.h.run_steps(1);
            self.h.event(egui::Event::Text(text.to_string()));
            self.h.run_steps(1);
            self.h.key_press(Key::Enter);
        }
        self.note(match found {
            true => format!(
                "clicked into the \"{label}\" field furthest left, selected all, typed \"{text}\" and pressed Return"
            ),
            false => format!("looked for a field \"{label}\" to type in: not on screen"),
        });
        self.h.run_steps(3);
        found
    }

    pub(super) fn in_sight_containing(&self, label: &str) -> bool {
        self.h
            .query_all_by_label_contains(label)
            .map(|n| n.rect())
            .any(|r| self.visible(r))
    }

    pub(super) fn in_sight(&self, label: &str) -> bool {
        self.h
            .query_all_by_label(label)
            .map(|n| n.rect())
            .any(|r| self.visible(r))
    }

    pub(super) fn click_inside(&mut self, label: &str, across: f32, down: f32) -> bool {
        let label = &crate::commands::local_keys(label);
        let rect = self.h.query_all_by_label(label).next().map(|n| n.rect());
        self.note(match rect {
            Some(_) => format!(
                "clicked \"{label}\" at {:.0}% across, {:.0}% down",
                across * 100.0,
                down * 100.0
            ),
            None => format!("looked for \"{label}\" to click: not on screen"),
        });
        if let Some(rect) = rect {
            let at = rect.min + vec2(rect.width() * across, rect.height() * down);
            self.h.hover_at(at);
            self.h.run_steps(1);
            self.press(at, egui::PointerButton::Primary, true, Modifiers::NONE);
            self.press(at, egui::PointerButton::Primary, false, Modifiers::NONE);
            self.h.run_steps(3);
        }
        rect.is_some()
    }

    pub(super) fn at(&self, x: f32, y: f32) -> Pos2 {
        let ed = self.ed().expect("a drawing is open");
        let rect = ed.canvas_rect;
        rect.min
            + ed.view
                .expect("the canvas has a view")
                .to_screen(vec2(x, y), rect.size())
    }

    fn press(&mut self, at: Pos2, button: egui::PointerButton, pressed: bool, modifiers: Modifiers) {
        self.h.event(egui::Event::PointerButton {
            pos: at,
            button,
            pressed,
            modifiers,
        });
        self.h.run_steps(1);
    }

    pub(super) fn drag_with(&mut self, points: &[(f32, f32)], modifiers: Modifiers) {
        let screen: Vec<Pos2> = points.iter().map(|(x, y)| self.at(*x, *y)).collect();
        self.note(format!(
            "dragged on the canvas from {:.0},{:.0} to {:.0},{:.0}{}",
            points[0].0,
            points[0].1,
            points[points.len() - 1].0,
            points[points.len() - 1].1,
            if modifiers.is_none() {
                ""
            } else {
                " with a modifier held"
            }
        ));
        self.ready();
        self.h.event(egui::Event::ModifiersChanged(modifiers));
        self.h.hover_at(screen[0]);
        self.h.run_steps(1);
        self.press(screen[0], egui::PointerButton::Primary, true, modifiers);
        screen.windows(2).for_each(|pair| {
            (1..=6).for_each(|i| {
                self.h.hover_at(pair[0] + (pair[1] - pair[0]) * (i as f32 / 6.0));
                self.h.run_steps(1);
            });
        });
        self.press(screen[screen.len() - 1], egui::PointerButton::Primary, false, modifiers);
        self.h.event(egui::Event::ModifiersChanged(Modifiers::NONE));
        self.h.run_steps(3);
    }

    pub(super) fn pen_down(&mut self, x: f32, y: f32) {
        let at = self.at(x, y);
        self.note(format!("pressed on the canvas at {x:.0},{y:.0} and held"));
        self.h.hover_at(at);
        self.h.run_steps(1);
        self.press(at, egui::PointerButton::Primary, true, Modifiers::NONE);
    }

    pub(super) fn pen_move(&mut self, x: f32, y: f32) {
        let at = self.at(x, y);
        self.note(format!("moved to {x:.0},{y:.0} with the button held"));
        self.h.hover_at(at);
        self.h.run_steps(2);
    }

    pub(super) fn pen_up(&mut self, x: f32, y: f32) {
        let at = self.at(x, y);
        self.note(format!("let go at {x:.0},{y:.0}"));
        self.press(at, egui::PointerButton::Primary, false, Modifiers::NONE);
        self.h.run_steps(3);
    }

    pub(super) fn pen_stroke(&mut self, points: &[(f32, f32, f32)]) {
        let screen: Vec<(Pos2, f32)> = points
            .iter()
            .map(|(x, y, pressure)| (self.at(*x, *y), *pressure))
            .collect();
        let (light, hard) = points
            .iter()
            .fold((f32::MAX, 0.0f32), |(lo, hi), p| (lo.min(p.2), hi.max(p.2)));
        self.note(format!(
            "drew with a pen from {:.0},{:.0} to {:.0},{:.0} through {} points, pressing {light:.1} to {hard:.1}",
            points[0].0,
            points[0].1,
            points[points.len() - 1].0,
            points[points.len() - 1].1,
            points.len()
        ));
        self.ready();
        let touch = |phase, pos, force| egui::Event::Touch {
            device_id: egui::TouchDeviceId(7),
            id: egui::TouchId(1),
            phase,
            pos,
            force: Some(force),
        };
        let (first, last) = (screen[0], screen[screen.len() - 1]);
        self.h.hover_at(first.0);
        self.h.run_steps(1);
        self.h.event(touch(egui::TouchPhase::Start, first.0, first.1));
        self.h.run_steps(1);
        screen.windows(2).for_each(|pair| {
            (1..=4).for_each(|i| {
                let t = i as f32 / 4.0;
                let at = pair[0].0 + (pair[1].0 - pair[0].0) * t;
                self.h.event(touch(
                    egui::TouchPhase::Move,
                    at,
                    pair[0].1 + (pair[1].1 - pair[0].1) * t,
                ));
                self.h.run_steps(1);
                std::thread::sleep(Duration::from_millis(4));
            });
        });
        self.h.event(touch(egui::TouchPhase::End, last.0, 0.0));
        self.h.run_steps(3);
    }

    pub(super) fn drawing(&self) -> String {
        self.ed()
            .map_or_else(String::new, |ed| ed.scene.bundle.root.display().to_string())
    }

    pub(super) fn drag(&mut self, points: &[(f32, f32)]) {
        self.drag_with(points, Modifiers::NONE);
    }

    pub(super) fn tap_with(&mut self, x: f32, y: f32, button: egui::PointerButton, modifiers: Modifiers) {
        self.ready();
        let at = self.at(x, y);
        self.note(format!(
            "{} the canvas at {x:.0},{y:.0}",
            match button {
                egui::PointerButton::Secondary => "right clicked",
                _ => "clicked",
            }
        ));
        self.h.event(egui::Event::ModifiersChanged(modifiers));
        self.h.hover_at(at);
        self.h.run_steps(1);
        self.press(at, button, true, modifiers);
        self.press(at, button, false, modifiers);
        self.h.event(egui::Event::ModifiersChanged(Modifiers::NONE));
        self.h.run_steps(3);
    }

    pub(super) fn hover_with(&mut self, x: f32, y: f32, modifiers: Modifiers) {
        let at = self.at(x, y);
        self.note(format!(
            "moved the pointer to {x:.0},{y:.0}{}",
            if modifiers.is_none() {
                ""
            } else {
                " with a modifier held"
            }
        ));
        self.h.event(egui::Event::ModifiersChanged(modifiers));
        self.h.hover_at(at);
        self.h.run_steps(2);
    }

    pub(super) fn let_go(&mut self) {
        self.h.event(egui::Event::ModifiersChanged(Modifiers::NONE));
        self.h.run_steps(2);
    }

    pub(super) fn tap(&mut self, x: f32, y: f32) {
        self.tap_with(x, y, egui::PointerButton::Primary, Modifiers::NONE);
    }

    fn cell(&self, layer: &str, frame: u32) -> Option<Pos2> {
        let ed = self.ed()?;
        let row = ed
            .layer_rows(super::layers::Where::Timeline)
            .iter()
            .position(|r| r.name == layer)?;
        ed.timeline.geom.map(|g| g.cell_centre(frame, row))
    }

    pub(super) fn drag_frames(&mut self, layer: &str, from: u32, to: u32) -> bool {
        self.drag_frames_with(layer, from, to, Modifiers::NONE)
    }

    pub(super) fn drag_frames_with(&mut self, layer: &str, from: u32, to: u32, modifiers: Modifiers) -> bool {
        self.drag_frames_off(layer, from, to, egui::Vec2::ZERO, modifiers)
    }

    pub(super) fn click_clip(&mut self, name: &str) -> bool {
        let spot = self.ed().and_then(|ed| {
            let clips = &ed.scene.bundle.doc.timeline.clips;
            let at = clips.iter().position(|c| c.name == name)?;
            let lane = *super::clips::lanes(clips).get(at)?;
            let g = ed.timeline.geom?;
            let (first, last) = (g.cell_centre(clips[at].from, 0), g.cell_centre(clips[at].to, 0));
            let rows_top = first.y - crate::theme::ROW / 2.0 - 18.0;
            Some(egui::pos2(
                (first.x + last.x) / 2.0,
                rows_top - super::clips::band_height(clips) + (lane as f32 + 0.5) * super::clips::LANE,
            ))
        });
        match spot {
            Some(at) => {
                self.note(format!("clicked the bar of the clip {name} above the frames"));
                self.poke(at);
                true
            }
            None => {
                self.note(format!("looked for the bar of the clip {name}: there is none"));
                false
            }
        }
    }

    pub(super) fn drag_frames_off(
        &mut self,
        layer: &str,
        from: u32,
        to: u32,
        off: egui::Vec2,
        modifiers: Modifiers,
    ) -> bool {
        let cells = (self.cell(layer, from), self.cell(layer, to));
        let (Some(a), Some(b)) = (cells.0.map(|p| p + off), cells.1.map(|p| p + off)) else {
            self.note(format!(
                "looked for frames {from} to {to} of {layer} in the timeline: not in sight"
            ));
            return false;
        };
        let held = if modifiers.shift { " with shift held" } else { "" };
        let place = match off == egui::Vec2::ZERO {
            true => String::new(),
            false => format!(
                ", pressing {} px right and {} px down of the middle of the frame",
                off.x, off.y
            ),
        };
        self.note(match from == to {
            true => format!("clicked frame {from} of {layer} in the timeline{held}{place}"),
            false => format!("dragged over frames {from} to {to} of {layer} in the timeline{held}{place}"),
        });
        self.h.event(egui::Event::ModifiersChanged(modifiers));
        self.h.hover_at(a);
        self.h.run_steps(1);
        self.press(a, egui::PointerButton::Primary, true, modifiers);
        (1..=6).for_each(|i| {
            self.h.hover_at(a + (b - a) * (i as f32 / 6.0));
            self.h.run_steps(1);
        });
        self.press(b, egui::PointerButton::Primary, false, modifiers);
        self.h.event(egui::Event::ModifiersChanged(Modifiers::NONE));
        self.h.run_steps(3);
        true
    }

    pub(super) fn twist(&mut self, x: f32, y: f32, degrees: f32, modifiers: Modifiers) {
        let at = self.at(x, y);
        self.note(format!(
            "twisted two fingers {degrees:.0} degrees over the canvas{}",
            if modifiers.is_none() { "" } else { " with shift held" }
        ));
        self.h.event(egui::Event::ModifiersChanged(modifiers));
        self.h.hover_at(at);
        self.h.event(egui::Event::Rotate(degrees.to_radians()));
        self.h.run_steps(2);
        self.h.event(egui::Event::ModifiersChanged(Modifiers::NONE));
        self.h.run_steps(2);
    }

    pub(super) fn heard(&self) -> String {
        self.ed()
            .and_then(|ed| ed.messages.latest().map(|m| m.text.clone()))
            .unwrap_or_default()
    }

    pub(super) fn wheel(&mut self, x: f32, y: f32, delta: Vec2, modifiers: Modifiers) {
        let at = self.at(x, y);
        self.note(format!(
            "scrolled by {:.0},{:.0} over the canvas{}",
            delta.x,
            delta.y,
            if modifiers.command { " with cmd held" } else { "" }
        ));
        self.h.event(egui::Event::ModifiersChanged(modifiers));
        self.h.event(egui::Event::PointerMoved(at));
        self.h.run_steps(1);
        self.h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta,
            modifiers,
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
        });
        self.h.run_steps(2);
        self.h.event(egui::Event::ModifiersChanged(Modifiers::NONE));
        self.h.run_steps(4);
    }

    pub(super) fn pinch(&mut self, x: f32, y: f32, factor: f32) {
        let at = self.at(x, y);
        self.note(format!("pinched by {factor:.2} over the canvas"));
        self.h.event(egui::Event::PointerMoved(at));
        self.h.run_steps(1);
        self.h.event(egui::Event::Zoom(factor));
        self.h.run_steps(4);
    }

    pub(super) fn finish(mut self, known: &[&str]) {
        let found: BTreeSet<String> = self.misses.iter().cloned().collect();
        let known: BTreeSet<String> = known.iter().map(|k| k.to_string()).collect();
        let new: Vec<&String> = found.difference(&known).collect();
        let gone: Vec<&String> = known.difference(&found).collect();
        self.lines.push(String::new());
        self.lines.push(format!(
            "misses: {}",
            found.iter().cloned().collect::<Vec<_>>().join(", ")
        ));
        if let Some(out) = &self.out {
            std::fs::write(out.join("steps.md"), self.lines.join("\n") + "\n").unwrap();
        }
        let _ = std::fs::remove_dir_all(&self.folder);
        assert!(
            new.is_empty() && gone.is_empty(),
            "the journey met problems it did not list: {new:?}; listed problems it no longer meets (take them off its list): {gone:?}"
        );
    }
}
