use super::layers::Where;
use super::ui_tests::{editor, harness, pixel_sample, sample};
use super::{Editor, Tool};
use crate::commands::Cmd;
use crate::composite::Scene;
use egui::{Key, Modifiers, Pos2, vec2};
use egui_kittest::Harness;
use klad_core::bundle::Bundle;
use klad_core::doc::Document;
use klad_core::history::{self, Stack};
use klad_core::rng::Rng;
use klad_core::stroke::Mark;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const WINDOW: egui::Vec2 = vec2(1440.0, 900.0);
const SLOW: Duration = Duration::from_secs(4);
const OUTSIDE: [Cmd; 2] = [Cmd::InstallCommand, Cmd::Quit];

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mods {
    None,
    Cmd,
    Shift,
    CmdShift,
    Alt,
}

impl Mods {
    fn of(self) -> Modifiers {
        match self {
            Mods::None => Modifiers::NONE,
            Mods::Cmd => Modifiers::COMMAND,
            Mods::Shift => Modifiers::SHIFT,
            Mods::CmdShift => Modifiers::COMMAND | Modifiers::SHIFT,
            Mods::Alt => Modifiers::ALT,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Act {
    Press(Mods, Key),
    Tap(f32, f32),
    Drag([(f32, f32); 3]),
    DragThenEscape((f32, f32), (f32, f32)),
    DragThenAway((f32, f32), (f32, f32)),
    Run(Cmd),
    Layer(usize),
    Frame(u32),
    Type(&'static str),
    Agent(u8),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Paper {
    Painting,
    Sprite,
}

#[derive(Clone, Debug, PartialEq)]
struct Broken {
    rule: &'static str,
    what: String,
}

const TOOLS: [Key; 11] = [
    Key::B,
    Key::E,
    Key::G,
    Key::S,
    Key::V,
    Key::A,
    Key::P,
    Key::U,
    Key::T,
    Key::J,
    Key::N,
];

const KEYS: [(Mods, Key); 36] = [
    (Mods::None, Key::Escape),
    (Mods::None, Key::Escape),
    (Mods::None, Key::Delete),
    (Mods::None, Key::Backspace),
    (Mods::None, Key::Enter),
    (Mods::None, Key::Tab),
    (Mods::None, Key::ArrowLeft),
    (Mods::None, Key::ArrowRight),
    (Mods::None, Key::ArrowUp),
    (Mods::None, Key::ArrowDown),
    (Mods::None, Key::X),
    (Mods::None, Key::K),
    (Mods::Shift, Key::K),
    (Mods::None, Key::I),
    (Mods::Shift, Key::I),
    (Mods::None, Key::Equals),
    (Mods::None, Key::Minus),
    (Mods::None, Key::O),
    (Mods::None, Key::H),
    (Mods::None, Key::M),
    (Mods::None, Key::Period),
    (Mods::None, Key::Comma),
    (Mods::None, Key::Home),
    (Mods::None, Key::End),
    (Mods::None, Key::OpenBracket),
    (Mods::None, Key::CloseBracket),
    (Mods::Cmd, Key::Z),
    (Mods::CmdShift, Key::Z),
    (Mods::Cmd, Key::A),
    (Mods::Cmd, Key::C),
    (Mods::Cmd, Key::V),
    (Mods::Cmd, Key::X),
    (Mods::Cmd, Key::D),
    (Mods::Cmd, Key::T),
    (Mods::Alt, Key::Comma),
    (Mods::Alt, Key::Period),
];

const WORDS: [&str; 4] = ["a", "klad", "12", " "];

fn point(rng: &mut Rng, (w, h): (f32, f32)) -> (f32, f32) {
    let spread = |rng: &mut Rng, size: f32| (rng.range(-0.08, 1.08) * size).round();
    (spread(rng, w), spread(rng, h))
}

fn act(rng: &mut Rng, size: (f32, f32), commands: &[Cmd]) -> Act {
    match rng.below(100) {
        0..=11 => Act::Press(Mods::None, TOOLS[rng.below(TOOLS.len())]),
        12..=24 => {
            let (m, k) = KEYS[rng.below(KEYS.len())];
            Act::Press(m, k)
        }
        25..=36 => {
            let (x, y) = point(rng, size);
            Act::Tap(x, y)
        }
        37..=58 => Act::Drag([point(rng, size), point(rng, size), point(rng, size)]),
        59..=62 => Act::DragThenEscape(point(rng, size), point(rng, size)),
        63..=65 => Act::DragThenAway(point(rng, size), point(rng, size)),
        66..=85 => Act::Run(commands[rng.below(commands.len())]),
        86..=90 => Act::Layer(rng.below(12)),
        91..=94 => Act::Frame(rng.below(24) as u32),
        95..=97 => Act::Agent(rng.below(5) as u8),
        _ => Act::Type(WORDS[rng.below(WORDS.len())]),
    }
}

fn script(seed: u64, paper: Paper, length: usize) -> Vec<Act> {
    let mut rng = Rng::new(seed);
    let size = match paper {
        Paper::Painting => (512.0, 384.0),
        Paper::Sprite => (32.0, 24.0),
    };
    let commands: Vec<Cmd> = Cmd::ALL.into_iter().filter(|c| !OUTSIDE.contains(c)).collect();
    (0..length).map(|_| act(&mut rng, size, &commands)).collect()
}

fn at(h: &Harness<'static, Editor>, (x, y): (f32, f32)) -> Option<Pos2> {
    let ed = h.state();
    let rect = ed.canvas_rect;
    ed.view
        .map(|view| rect.min + view.to_screen(vec2(x, y), rect.size()))
        .filter(|p| p.x.is_finite() && p.y.is_finite())
}

fn button(h: &mut Harness<'static, Editor>, pos: Pos2, pressed: bool) {
    h.event(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(1);
}

fn patience() -> Duration {
    let seconds = std::env::var("KLAD_MONKEY_WAIT")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(20);
    Duration::from_secs(seconds)
}

fn settle(h: &mut Harness<'static, Editor>) {
    let started = Instant::now();
    let limit = patience();
    let waiting = |ed: &Editor| {
        (!ed.canvas_current() && ed.timeline.player.is_none())
            || !ed.jobs.is_empty()
            || ed.scene.bundle.changed_on_disk()
    };
    h.run_steps(2);
    while waiting(h.state()) && started.elapsed() < limit {
        h.run_steps(1);
        std::thread::sleep(Duration::from_millis(3));
    }
}

fn agent(h: &Harness<'static, Editor>, kind: u8) -> Option<String> {
    let ed = h.state();
    let doc = &ed.scene.bundle.doc;
    let paint = doc
        .all_layers()
        .into_iter()
        .find(|l| l.kind() == klad_core::doc::Kind::Paint && !l.pixel() && !l.cels().is_empty())
        .map(|l| (l.id.clone(), l.cels()[0].frame));
    match kind {
        0 => Some(r#"{"op":"add","layer":{"kind":"paint","name":"from the agent","cels":[{"frame":0}]}}"#.to_string()),
        1 => (doc.layers.len() > 1)
            .then(|| doc.layers.last().map(|l| format!(r#"{{"op":"rm","id":"{}"}}"#, l.id)))
            .flatten(),
        2 => paint.map(|(id, frame)| {
            format!(r#"{{"op":"stroke","layer":"{id}","frame":{frame},"path":"M60 60 L200 150","size":6}}"#)
        }),
        3 => (doc.layers.len() > 1)
            .then(|| ed.scene.active_id().map(|id| format!(r#"{{"op":"rm","id":"{id}"}}"#)))
            .flatten(),
        _ => doc
            .layers
            .first()
            .map(|l| format!(r#"{{"op":"set","id":"{}","name":"renamed by the agent"}}"#, l.id)),
    }
}

fn busy_with(h: &Harness<'static, Editor>) -> String {
    let ed = h.state();
    let jobs: Vec<String> = ed
        .jobs
        .running(Instant::now())
        .into_iter()
        .map(|(_, name, _)| name)
        .collect();
    format!(
        "tool {:?}, layer {:?}, frame {} (the canvas shows {:?}), waiting for a rebuild: {}, playing: {}, jobs: {jobs:?}, it said: {}",
        ed.tool,
        ed.scene.active_layer().map(|l| l.name.clone()),
        ed.scene.frame,
        ed.timeline.canvas_frame,
        ed.scene.stale,
        ed.timeline.player.is_some(),
        ed.messages.latest().map_or("", |m| m.text.as_str())
    )
}

fn perform(h: &mut Harness<'static, Editor>, act: Act) {
    match act {
        Act::Press(mods, key) => h.key_press_modifiers(mods.of(), key),
        Act::Tap(x, y) => {
            if let Some(p) = at(h, (x, y)) {
                h.hover_at(p);
                h.run_steps(1);
                button(h, p, true);
                button(h, p, false);
            }
        }
        Act::Drag(points) => {
            let screen: Vec<Pos2> = points.iter().filter_map(|p| at(h, *p)).collect();
            if let [a, b, c] = screen[..] {
                h.hover_at(a);
                h.run_steps(1);
                button(h, a, true);
                [a.lerp(b, 0.5), b, b.lerp(c, 0.5), c].into_iter().for_each(|p| {
                    h.hover_at(p);
                    h.run_steps(1);
                });
                button(h, c, false);
            }
        }
        Act::DragThenEscape(from, to) => {
            if let (Some(a), Some(b)) = (at(h, from), at(h, to)) {
                h.hover_at(a);
                h.run_steps(1);
                button(h, a, true);
                h.hover_at(a.lerp(b, 0.5));
                h.run_steps(1);
                h.key_press(Key::Escape);
                h.run_steps(1);
                h.hover_at(b);
                h.run_steps(1);
                button(h, b, false);
            }
        }
        Act::DragThenAway(from, to) => {
            if let (Some(a), Some(b)) = (at(h, from), at(h, to)) {
                h.hover_at(a);
                h.run_steps(1);
                button(h, a, true);
                h.hover_at(a.lerp(b, 0.5));
                h.run_steps(1);
                h.event(egui::Event::WindowFocused(false));
                h.event(egui::Event::PointerGone);
                h.run_steps(2);
                h.event(egui::Event::WindowFocused(true));
                h.run_steps(1);
                button(h, b, false);
            }
        }
        Act::Run(cmd) => {
            let _ = h.state_mut().invoke(cmd);
        }
        Act::Layer(n) => {
            let rows = h.state().layer_rows(Where::Panel);
            if let Some(row) = rows.get(n % rows.len().max(1)) {
                let _ = h.state_mut().activate(row.path.clone());
            }
        }
        Act::Frame(f) => {
            let frames = h.state().scene.bundle.doc.timeline.frames.max(1);
            let _ = h.state_mut().go_to(f % frames);
        }
        Act::Type(text) => h.event(egui::Event::Text(text.to_string())),
        Act::Agent(kind) => {
            let path = h.state().scene.bundle.root.clone();
            if let Some(line) = agent(h, kind) {
                let done = klad_core::ops::parse_lines(&line)
                    .ok()
                    .zip(Bundle::open(&path).ok())
                    .map(|(ops, mut bundle)| bundle.apply(&ops).is_ok());
                if done == Some(true) {
                    let started = Instant::now();
                    while !h.state().scene.bundle.changed_on_disk() && started.elapsed() < Duration::from_millis(300) {
                        h.run_steps(1);
                        std::thread::sleep(Duration::from_millis(5));
                    }
                }
            }
        }
    }
    settle(h);
}

fn stack(path: &Path, which: Stack) -> usize {
    Bundle::open(path)
        .ok()
        .and_then(|b| history::entries(&b, which).ok())
        .map_or(0, |e| e.len())
}

fn marks_on_disk(path: &Path) -> Option<(Document, BTreeMap<String, Vec<Mark>>)> {
    let bundle = Bundle::open(path).ok()?;
    let marks = bundle
        .doc
        .all_layers()
        .iter()
        .flat_map(|l| l.cels().iter().map(move |c| (l.id.clone(), c.id.clone())))
        .filter_map(|(layer, cel)| bundle.read_marks(&layer, &cel).ok().map(|m| (cel, m)))
        .collect();
    Some((bundle.doc.clone(), marks))
}

fn calm(h: &mut Harness<'static, Editor>) {
    h.key_press(Key::Escape);
    h.run_steps(2);
    h.key_press(Key::Escape);
    h.run_steps(2);
    let ed = h.state_mut();
    ed.windows = super::Windows::default();
    ed.palette = None;
    if ed.timeline.player.is_some() {
        let _ = ed.invoke(Cmd::Play);
    }
    let _ = ed.invoke(Cmd::Brush);
    if ed.scene.onion_on {
        let _ = ed.invoke(Cmd::Onion);
    }
    if !ed.scene.tint_mask {
        let _ = ed.invoke(Cmd::TintMask);
    }
    ed.maps.show = None;
    settle(h);
    h.state_mut().redraw_canvas();
    settle(h);
}

fn same_on_disk(h: &Harness<'static, Editor>, path: &Path) -> Result<(), Broken> {
    let ed = h.state();
    let Some((doc, marks)) = marks_on_disk(path) else {
        return Err(Broken {
            rule: "the drawing on disk opens",
            what: "the drawing in its folder no longer opens".to_string(),
        });
    };
    if doc != ed.scene.bundle.doc {
        return Err(Broken {
            rule: "disk and editor hold the same drawing",
            what: "the drawing in the editor differs from the one in its folder".to_string(),
        });
    }
    let frame = ed.scene.frame;
    let odd: Vec<String> = doc
        .all_layers()
        .iter()
        .filter_map(|l| l.content_at(frame).map(|c| (l.name.clone(), c.id.clone())))
        .filter(|(_, cel)| marks.get(cel).is_some_and(|m| m.as_slice() != ed.scene.marks_of(cel)))
        .map(|(layer, cel)| {
            format!(
                "{layer}: {} marks in the editor, {} in its folder",
                ed.scene.marks_of(&cel).len(),
                marks.get(&cel).map_or(0, |m| m.len())
            )
        })
        .collect();
    match odd.is_empty() {
        true => Ok(()),
        false => Err(Broken {
            rule: "disk and editor hold the same marks",
            what: odd.join("; "),
        }),
    }
}

fn hash(tiles: &[(usize, Vec<half::f16>)]) -> u64 {
    tiles.iter().fold(super::testkit::FNV, |hash, (i, tile)| {
        tile.iter().fold(super::testkit::mix(hash, *i as u64), |hash, v| {
            super::testkit::mix(hash, u64::from(v.to_bits()))
        })
    })
}

fn far(a: &[(usize, Vec<half::f16>)], b: &[(usize, Vec<half::f16>)]) -> usize {
    a.iter()
        .zip(b)
        .map(|((_, x), (_, y))| {
            x.iter()
                .zip(y)
                .filter(|(p, q)| (p.to_f32() - q.to_f32()).abs() > 0.02)
                .count()
        })
        .sum()
}

fn same_as_reopened(h: &Harness<'static, Editor>, path: &Path) -> Result<(), Broken> {
    let ed = h.state();
    let frame = ed.scene.frame;
    let live = ed.scene.compose(&ed.scene.all_tiles(), None).map_err(|e| Broken {
        rule: "the canvas can be drawn",
        what: format!("{e:#}"),
    })?;
    let mut fresh = Scene::open_at(path, frame).map_err(|e| Broken {
        rule: "the drawing on disk opens",
        what: format!("{e:#}"),
    })?;
    fresh.set_active(ed.scene.active.clone()).map_err(|e| Broken {
        rule: "the drawing on disk opens",
        what: format!("{e:#}"),
    })?;
    let again = fresh.compose(&fresh.all_tiles(), None).map_err(|e| Broken {
        rule: "the canvas can be drawn",
        what: format!("{e:#}"),
    })?;
    match hash(&live) == hash(&again) || far(&live, &again) == 0 {
        true => Ok(()),
        false => Err(Broken {
            rule: "the canvas shows what a fresh open shows",
            what: format!(
                "{} values of the picture on frame {frame} differ from the same drawing opened again",
                far(&live, &again)
            ),
        }),
    }
}

type Slowest = (Duration, Option<Act>);

fn tidy(path: &Path) {
    let mut quiet = 0;
    let mut turns = 0;
    while quiet < 3 && turns < 40 {
        match path.exists() {
            true => {
                let _ = std::fs::remove_dir_all(path);
                quiet = 0;
            }
            false => quiet += 1,
        }
        std::thread::sleep(Duration::from_millis(40));
        turns += 1;
    }
}

fn way_out(h: &mut Harness<'static, Editor>, path: &Path) -> Result<(), Broken> {
    (0..3).for_each(|_| {
        h.key_press(Key::Escape);
        h.run_steps(2);
    });
    settle(h);
    if h.state().timeline.player.is_some() {
        h.key_press(Key::Enter);
        h.run_steps(2);
        settle(h);
    }
    same_on_disk(h, path)
}

fn stroke_lands(h: &mut Harness<'static, Editor>) -> Result<(), Broken> {
    h.key_press(Key::B);
    h.run_steps(2);
    settle(h);
    if h.state().tool != Tool::Brush {
        return Err(Broken {
            rule: "three Escapes and B give the brush",
            what: busy_with(h),
        });
    }
    let shown: Vec<u64> = h.state().messages.toasts(Instant::now()).iter().map(|m| m.id).collect();
    let ed = h.state_mut();
    shown.into_iter().for_each(|id| ed.messages.dismiss(id));
    ed.windows = super::Windows::default();
    ed.first.open = false;
    ed.palette = None;
    let _ = ed.invoke(Cmd::NewPaintLayer);
    settle(h);
    let before = h.state().scene.total_marks();
    let (w, hgt) = h.state().scene.size();
    let line = [
        (w as f32 * 0.3, hgt as f32 * 0.5),
        (w as f32 * 0.5, hgt as f32 * 0.6),
        (w as f32 * 0.7, hgt as f32 * 0.7),
    ];
    let mut tries = 0;
    while h.state().scene.total_marks() <= before && tries < 3 {
        perform(h, Act::Drag(line));
        tries += 1;
    }
    match h.state().scene.total_marks() > before {
        true => Ok(()),
        false => Err(Broken {
            rule: "after three Escapes, B and a new paint layer a stroke lands",
            what: busy_with(h),
        }),
    }
}

#[derive(Clone, Copy, Default)]
struct How {
    spoil: Option<usize>,
    look_first: bool,
}

fn run(paper: Paper, acts: &[Act], name: &str, how: How) -> Result<Slowest, Broken> {
    let path: PathBuf = match paper {
        Paper::Painting => sample(name),
        Paper::Sprite => pixel_sample(name),
    };
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| walk(&path, acts, how)));
    tidy(&path);
    match outcome {
        Ok(result) => result,
        Err(panic) => Err(Broken {
            rule: "no panic",
            what: panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "a panic without words".to_string()),
        }),
    }
}

fn walk(path: &Path, acts: &[Act], how: How) -> Result<Slowest, Broken> {
    let mut h = harness(editor(path), WINDOW);
    settle(&mut h);
    let start = marks_on_disk(path).ok_or(Broken {
        rule: "the drawing on disk opens",
        what: "the sample does not open".to_string(),
    })?;
    let base = stack(path, Stack::Undo);
    let mut slowest: Slowest = (Duration::ZERO, None);
    let mut past_the_start = false;
    acts.iter().enumerate().try_for_each(|(n, act)| {
        let began = Instant::now();
        perform(&mut h, *act);
        if began.elapsed() > slowest.0 {
            slowest = (began.elapsed(), Some(*act));
        }
        if began.elapsed() > SLOW && std::env::var_os("KLAD_MONKEY_SLOW").is_some() {
            return Err(Broken {
                rule: "no act keeps the editor busy for long",
                what: format!("{act:?} took {:.1?}: {}", began.elapsed(), busy_with(&h)),
            });
        }
        past_the_start |= stack(path, Stack::Undo) < base;
        if how.spoil == Some(n)
            && let Some(layer) = h.state_mut().scene.bundle.doc.layers.first_mut()
        {
            layer.opacity = 0.37;
        }
        Ok(())
    })?;
    if how.look_first {
        same_as_reopened(&h, path)?;
    }
    way_out(&mut h, path)?;
    calm(&mut h);
    same_on_disk(&h, path)?;
    same_as_reopened(&h, path)?;
    let end = marks_on_disk(path).ok_or(Broken {
        rule: "the drawing on disk opens",
        what: "the drawing no longer opens after the acts".to_string(),
    })?;
    let floor = match past_the_start {
        true => stack(path, Stack::Undo).saturating_sub(6),
        false => base,
    };
    let redo_floor = stack(path, Stack::Redo);
    let mut turns = 0;
    while stack(path, Stack::Undo) > floor && turns < acts.len() * 6 + 40 {
        let before = stack(path, Stack::Undo);
        let _ = h.state_mut().invoke(Cmd::Undo);
        settle(&mut h);
        turns += 1;
        if stack(path, Stack::Undo) >= before {
            return Err(Broken {
                rule: "undo takes a step back",
                what: format!(
                    "undo left {before} steps to take back and said: {}",
                    h.state().messages.latest().map_or("", |m| m.text.as_str())
                ),
            });
        }
    }
    let back = marks_on_disk(path);
    if !past_the_start && back.as_ref() != Some(&start) {
        let what = match &back {
            Some((doc, _)) if *doc != start.0 => "the layers, frames or settings differ from the start",
            Some(_) => "the marks differ from the start",
            None => "the drawing no longer opens",
        };
        return Err(Broken {
            rule: "undoing everything gives the start",
            what: what.to_string(),
        });
    }
    same_as_reopened(&h, path)?;
    let mut turns = 0;
    while stack(path, Stack::Redo) > redo_floor && turns < acts.len() * 6 + 40 {
        let before = stack(path, Stack::Redo);
        let _ = h.state_mut().invoke(Cmd::Redo);
        settle(&mut h);
        turns += 1;
        if stack(path, Stack::Redo) >= before {
            return Err(Broken {
                rule: "redo takes a step forward",
                what: format!(
                    "redo left {before} steps and said: {}",
                    h.state().messages.latest().map_or("", |m| m.text.as_str())
                ),
            });
        }
    }
    let forth = marks_on_disk(path);
    if forth.as_ref() != Some(&end) {
        let what = match &forth {
            Some((doc, _)) if *doc != end.0 => "the layers, frames or settings differ from where the acts ended",
            Some(_) => "the marks differ from where the acts ended",
            None => "the drawing no longer opens",
        };
        return Err(Broken {
            rule: "redoing everything gives the end",
            what: what.to_string(),
        });
    }
    same_as_reopened(&h, path)?;
    calm(&mut h);
    stroke_lands(&mut h)?;
    Ok(slowest)
}

fn shrink(paper: Paper, acts: &[Act], name: &str, rule: &'static str) -> Vec<Act> {
    let fails = |acts: &[Act]| run(paper, acts, name, How::default()).is_err_and(|b| b.rule == rule);
    let mut kept = acts.to_vec();
    let mut chunk = kept.len().div_ceil(2);
    let mut tries = 0;
    while chunk >= 1 && tries < 60 {
        let mut at = 0;
        let mut cut = false;
        while at < kept.len() && tries < 60 {
            let mut shorter = kept.clone();
            shorter.drain(at..(at + chunk).min(kept.len()));
            tries += 1;
            match !shorter.is_empty() && fails(&shorter) {
                true => {
                    kept = shorter;
                    cut = true;
                }
                false => at += chunk,
            }
        }
        chunk = match (cut, chunk) {
            (_, 1) => 0,
            (true, n) => n,
            (false, n) => n / 2,
        };
    }
    kept
}

fn written(acts: &[Act]) -> String {
    acts.iter()
        .map(|a| match a {
            Act::Press(m, k) => format!("    Act::Press(Mods::{m:?}, Key::{k:?}),"),
            Act::Run(c) => format!("    Act::Run(Cmd::{c:?}),"),
            other => format!("    Act::{other:?},"),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn seeds() -> (u64, u64, usize) {
    let number = |name: &str, or: u64| {
        std::env::var(name)
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(or)
    };
    (
        number("KLAD_MONKEY_FROM", 1),
        number("KLAD_MONKEY_SEEDS", 40),
        number("KLAD_MONKEY_ACTS", 40) as usize,
    )
}

fn many(paper: Paper, from: u64, count: u64, length: usize) -> Vec<(u64, Result<Slowest, Broken>)> {
    let next = std::sync::atomic::AtomicU64::new(from);
    let found = std::sync::Mutex::new(Vec::new());
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get()).min(8);
    std::thread::scope(|scope| {
        (0..workers).for_each(|_| {
            scope.spawn(|| {
                loop {
                    let seed = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if seed >= from + count {
                        break;
                    }
                    let acts = script(seed, paper, length);
                    let result = run(paper, &acts, &format!("monkey-{paper:?}-{seed}"), How::default());
                    found.lock().unwrap().push((seed, result));
                }
            });
        });
    });
    let mut found = found.into_inner().unwrap();
    found.sort_by_key(|(seed, _)| *seed);
    found
}

fn report(paper: Paper, length: usize, found: &[(u64, Result<Slowest, Broken>)]) -> String {
    let mut lines = vec![format!("## {paper:?}: {} seeds of {length} acts", found.len())];
    let slow: Vec<String> = found
        .iter()
        .filter_map(|(seed, r)| {
            r.as_ref()
                .ok()
                .filter(|(d, _)| *d > SLOW)
                .map(|(d, act)| format!("seed {seed}: {d:.1?} for {act:?}"))
        })
        .collect();
    lines.push(format!(
        "{} passed, {} broke a rule, {} had an act slower than {SLOW:?} ({})",
        found.iter().filter(|(_, r)| r.is_ok()).count(),
        found.iter().filter(|(_, r)| r.is_err()).count(),
        slow.len(),
        slow.join(", ")
    ));
    let mut by_rule: BTreeMap<&str, Vec<(u64, &str)>> = BTreeMap::new();
    found
        .iter()
        .filter_map(|(seed, r)| r.as_ref().err().map(|b| (*seed, b)))
        .for_each(|(seed, b)| by_rule.entry(b.rule).or_default().push((seed, b.what.as_str())));
    by_rule.iter().for_each(|(rule, seeds)| {
        lines.push(format!("### {rule}: {} seeds", seeds.len()));
        seeds.iter().for_each(|(seed, what)| {
            let line: String = what.split_whitespace().collect::<Vec<_>>().join(" ");
            lines.push(format!("- seed {seed}: {}", line.chars().take(400).collect::<String>()));
        });
    });
    lines.join("\n")
}

#[test]
#[ignore = "random use by seed; run with `just monkey`"]
fn random_use_keeps_the_rules() {
    let (from, count, length) = seeds();
    let out = std::env::var_os("KLAD_MONKEY_OUT")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from);
    let text: Vec<String> = [Paper::Painting, Paper::Sprite]
        .into_iter()
        .map(|paper| {
            let found = many(paper, from, count, length);
            let mut text = report(paper, length, &found);
            let first: BTreeMap<&str, u64> = found
                .iter()
                .rev()
                .filter_map(|(seed, r)| r.as_ref().err().map(|b| (b.rule, *seed)))
                .collect();
            first.iter().for_each(|(rule, seed)| {
                let acts = script(*seed, paper, length);
                let small = shrink(paper, &acts, &format!("monkey-shrink-{paper:?}-{seed}"), rule);
                text += &format!(
                    "\n### the shortest run of seed {seed} that still breaks \"{rule}\" ({} acts)\n```\n{}\n```",
                    small.len(),
                    written(&small)
                );
            });
            text
        })
        .collect();
    let all = text.join("\n\n");
    println!("{all}");
    if let Some(out) = out {
        let stamp = std::env::var("KLAD_STAMP").map_or(String::new(), |s| format!("_Last updated: {s}_\n\n"));
        let head = format!(
            "{stamp}# Random use, seeds {from} to {}\n\nMade by `just monkey <folder>`. A seed is a run of {length} acts; the rules it is held to are in `crates/app/src/editor/monkey.rs`.\n\n",
            from + count - 1
        );
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(out.join("monkey.md"), head + &all + "\n").unwrap();
    }
}

struct Kept {
    name: &'static str,
    paper: Paper,
    acts: &'static [Act],
    rule: &'static str,
    look_first: bool,
}

const KEPT: [Kept; 0] = [];

#[test]
fn the_kept_runs_still_break_their_rule() {
    let changed: Vec<String> = KEPT
        .iter()
        .enumerate()
        .filter_map(|(n, kept)| {
            let how = How {
                spoil: None,
                look_first: kept.look_first,
            };
            match run(kept.paper, kept.acts, &format!("monkey-kept-{n}"), how) {
                Err(b) if b.rule == kept.rule => None,
                Err(b) => Some(format!(
                    "{}: it breaks \"{}\" now ({}), not \"{}\"",
                    kept.name, b.rule, b.what, kept.rule
                )),
                Ok(_) => Some(format!("{}: it keeps the rules now; take it off the list", kept.name)),
            }
        })
        .collect();
    assert!(changed.is_empty(), "{changed:#?}");
}

#[test]
fn the_rules_catch_a_change_that_never_reached_the_disk() {
    let acts = script(7, Paper::Painting, 6);
    let found = run(
        Paper::Painting,
        &acts,
        "monkey-spoiled",
        How {
            spoil: Some(3),
            look_first: false,
        },
    );
    assert_eq!(
        found.as_ref().err().map(|b| b.rule),
        Some("disk and editor hold the same drawing"),
        "{found:?}"
    );
}

#[test]
fn a_few_seeds_keep_the_rules() {
    let broken: Vec<String> = [Paper::Painting, Paper::Sprite]
        .into_iter()
        .flat_map(|paper| many(paper, 1, 4, 30).into_iter().map(move |(seed, r)| (paper, seed, r)))
        .filter_map(|(paper, seed, r)| {
            r.err()
                .map(|b| format!("{paper:?} seed {seed}: {}: {}", b.rule, b.what))
        })
        .collect();
    assert!(broken.is_empty(), "{broken:#?}");
}

#[test]
fn selecting_all_frames_after_the_last_layer_is_gone_keeps_the_rules() {
    let acts = [Act::Run(Cmd::DeleteLayer), Act::Run(Cmd::SelectAllFrames)];
    let found = run(Paper::Sprite, &acts, "monkey-no-layer-frames", How::default());
    assert!(found.is_ok(), "sprite seed 149 of the monkey: {found:?}");
}

#[test]
fn what_the_agent_does_on_disk_reaches_the_editor() {
    let path = sample("monkey-agent");
    let mut h = harness(editor(&path), WINDOW);
    settle(&mut h);
    let layers = |h: &Harness<'static, Editor>| h.state().scene.bundle.doc.all_layers().len();
    let marks = |h: &Harness<'static, Editor>| h.state().scene.total_marks();
    let (before, drawn) = (layers(&h), marks(&h));
    perform(&mut h, Act::Agent(0));
    assert_eq!(layers(&h), before + 1, "a layer the agent adds shows in the editor");
    perform(&mut h, Act::Agent(2));
    assert_eq!(marks(&h), drawn + 1, "a stroke the agent draws shows in the editor");
    perform(&mut h, Act::Agent(3));
    assert_eq!(
        layers(&h),
        before,
        "the layer in hand, removed by the agent, is gone in the editor"
    );
    assert!(
        h.state().scene.active_layer().is_some(),
        "and the editor has another layer in hand"
    );
    drop(h);
    tidy(&path);
}
