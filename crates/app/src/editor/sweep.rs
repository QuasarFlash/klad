use super::Editor;
use super::testkit::{FNV, copy_dir, mix, picture, written};
use super::ui_tests::{canvas_at, editor, harness};
use crate::commands::Cmd;
use crate::layout;
use crate::messages::Tone;
use egui::accesskit::Role;
use egui::{Key, Pos2, Vec2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{By, NodeT, Queryable};
use klad_core::bundle::{Blank, Bundle};
use klad_core::doc::{Loc, MASK};
use klad_core::history::Stack;
use klad_core::ops::parse_lines;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

const WINDOW: Vec2 = vec2(1440.0, 900.0);
const SHOWN: usize = 400;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Stage {
    Painting,
    Lit,
    Sprite,
    Shapes,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Gesture {
    Click,
    Drag,
    Loop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Place {
    Own,
    Other,
    Empty,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Finish {
    Nothing,
    Path,
    Typed,
    Pinned,
}

struct Hand {
    name: &'static str,
    key: &'static str,
    cmd: Cmd,
    gestures: &'static [Gesture],
    finish: Finish,
}

const fn hand(name: &'static str, key: &'static str, cmd: Cmd, gestures: &'static [Gesture], finish: Finish) -> Hand {
    Hand {
        name,
        key,
        cmd,
        gestures,
        finish,
    }
}

struct Row {
    name: &'static str,
    about: &'static str,
    stage: Stage,
    layer: &'static str,
    mask: bool,
    frame: u32,
    own: Option<(f32, f32)>,
}

const PRESS: &[Gesture] = &[Gesture::Click, Gesture::Drag];
const AROUND: &[Gesture] = &[Gesture::Click, Gesture::Drag, Gesture::Loop];

const HANDS: [Hand; 11] = [
    hand("brush", "B", Cmd::Brush, PRESS, Finish::Nothing),
    hand("eraser", "E", Cmd::Eraser, PRESS, Finish::Nothing),
    hand("fill", "G", Cmd::Fill, AROUND, Finish::Nothing),
    hand("select and move", "V", Cmd::Shapes, AROUND, Finish::Nothing),
    hand("select strokes", "S", Cmd::Strokes, AROUND, Finish::Nothing),
    hand("edit points", "A", Cmd::Points, PRESS, Finish::Nothing),
    hand("pen", "P", Cmd::Pen, PRESS, Finish::Path),
    hand("rectangles and ellipses", "U", Cmd::ShapeTool, PRESS, Finish::Nothing),
    hand("text", "T", Cmd::TextTool, PRESS, Finish::Typed),
    hand("note", "N", Cmd::Note, PRESS, Finish::Pinned),
    hand("bones", "J", Cmd::BoneTool, PRESS, Finish::Nothing),
];

const ROWS: [Row; 16] = [
    Row {
        name: "paint",
        about: "a paint layer with a stroke",
        stage: Stage::Painting,
        layer: "paint",
        mask: false,
        frame: 0,
        own: Some((34.0, 30.0)),
    },
    Row {
        name: "pixel",
        about: "a pixel layer of a sprite with a block of pixels",
        stage: Stage::Sprite,
        layer: "pixels",
        mask: false,
        frame: 0,
        own: Some((14.5, 12.5)),
    },
    Row {
        name: "vector",
        about: "a vector layer with a rectangle",
        stage: Stage::Painting,
        layer: "vector",
        mask: false,
        frame: 0,
        own: Some((94.0, 30.0)),
    },
    Row {
        name: "group",
        about: "a group holding a paint layer with a stroke",
        stage: Stage::Painting,
        layer: "set",
        mask: false,
        frame: 0,
        own: Some((154.0, 30.0)),
    },
    Row {
        name: "mask",
        about: "the mask of a paint layer, picked to paint on",
        stage: Stage::Painting,
        layer: "masked",
        mask: true,
        frame: 0,
        own: Some((34.0, 70.0)),
    },
    Row {
        name: "adjustment",
        about: "an adjustment layer (curves)",
        stage: Stage::Painting,
        layer: "curves",
        mask: false,
        frame: 0,
        own: None,
    },
    Row {
        name: "bones",
        about: "a bones layer with a chain of two bones",
        stage: Stage::Painting,
        layer: "bones",
        mask: false,
        frame: 0,
        own: Some((87.0, 70.0)),
    },
    Row {
        name: "map",
        about: "a paint layer that draws the normal map, light maps on",
        stage: Stage::Lit,
        layer: "relief",
        mask: false,
        frame: 0,
        own: Some((154.0, 110.0)),
    },
    Row {
        name: "reference picture",
        about: "an imported picture marked as a reference",
        stage: Stage::Painting,
        layer: "photo",
        mask: false,
        frame: 0,
        own: Some((214.0, 30.0)),
    },
    Row {
        name: "locked paint",
        about: "a locked paint layer with a stroke",
        stage: Stage::Painting,
        layer: "locked",
        mask: false,
        frame: 0,
        own: Some((154.0, 70.0)),
    },
    Row {
        name: "locked pixel",
        about: "a locked pixel layer with a block of pixels",
        stage: Stage::Sprite,
        layer: "locked pixels",
        mask: false,
        frame: 0,
        own: Some((35.5, 12.5)),
    },
    Row {
        name: "locked vector",
        about: "a locked vector layer with a rectangle",
        stage: Stage::Painting,
        layer: "locked shapes",
        mask: false,
        frame: 0,
        own: Some((94.0, 110.0)),
    },
    Row {
        name: "hidden paint",
        about: "a hidden paint layer with a stroke",
        stage: Stage::Painting,
        layer: "hidden",
        mask: false,
        frame: 0,
        own: Some((214.0, 70.0)),
    },
    Row {
        name: "empty frame",
        about: "a paint layer on a frame where it has no drawing",
        stage: Stage::Painting,
        layer: "timed",
        mask: false,
        frame: 2,
        own: None,
    },
    Row {
        name: "link",
        about: "a paint layer on a frame that shows a link to an earlier drawing",
        stage: Stage::Painting,
        layer: "timed",
        mask: false,
        frame: 4,
        own: Some((34.0, 110.0)),
    },
    Row {
        name: "nothing active",
        about: "a drawing of vector layers only, as it opens: no layer is picked",
        stage: Stage::Shapes,
        layer: "",
        mask: false,
        frame: 0,
        own: Some((94.0, 30.0)),
    },
];

impl Stage {
    fn other(self) -> Option<(f32, f32)> {
        match self {
            Stage::Sprite => Some((30.5, 38.5)),
            Stage::Shapes => None,
            _ => Some((124.0, 160.0)),
        }
    }

    fn empty(self) -> (f32, f32) {
        match self {
            Stage::Sprite => (54.5, 24.5),
            _ => (214.0, 110.0),
        }
    }

    fn reach(self) -> (f32, f32) {
        match self {
            Stage::Sprite => (5.0, 3.0),
            _ => (20.0, 14.0),
        }
    }

    fn around(self) -> (f32, f32) {
        match self {
            Stage::Sprite => (9.0, 6.0),
            _ => (24.0, 13.0),
        }
    }
}

impl Row {
    fn places(&self) -> Vec<(Place, (f32, f32))> {
        [
            self.own.map(|at| (Place::Own, at)),
            self.stage.other().map(|at| (Place::Other, at)),
            Some((Place::Empty, self.stage.empty())),
        ]
        .into_iter()
        .flatten()
        .collect()
    }
}

impl Gesture {
    fn name(self) -> &'static str {
        match self {
            Gesture::Click => "click",
            Gesture::Drag => "drag",
            Gesture::Loop => "loop",
        }
    }
}

impl Place {
    fn name(self) -> &'static str {
        match self {
            Place::Own => "on its own mark",
            Place::Other => "on another layer's paint",
            Place::Empty => "on empty canvas",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Dead,
    Idle,
}

struct Known {
    hands: &'static [&'static str],
    rows: &'static [&'static str],
    gestures: &'static [Gesture],
    places: &'static [Place],
    verdict: &'static str,
    kind: Kind,
    why: &'static str,
}

const ANYWHERE: &[Place] = &[Place::Own, Place::Other, Place::Empty];
const OWN: &[Place] = &[Place::Own];
const AWAY: &[Place] = &[Place::Other, Place::Empty];
const NOWHERE: &[Place] = &[Place::Empty];
const CLICK: &[Gesture] = &[Gesture::Click];
const DRAG: &[Gesture] = &[Gesture::Drag];

const KNOWN: [Known; 9] = [
    Known {
        hands: &["eraser"],
        rows: &["pixel"],
        gestures: PRESS,
        places: AWAY,
        verdict: "quiet",
        kind: Kind::Idle,
        why: "nothing is there to erase",
    },
    Known {
        hands: &["select and move"],
        rows: &[
            "paint",
            "pixel",
            "vector",
            "group",
            "mask",
            "adjustment",
            "map",
            "reference picture",
            "empty frame",
            "link",
            "nothing active",
        ],
        gestures: CLICK,
        places: NOWHERE,
        verdict: "quiet",
        kind: Kind::Idle,
        why: "nothing is there to take",
    },
    Known {
        hands: &["select and move"],
        rows: &[
            "paint",
            "pixel",
            "vector",
            "mask",
            "map",
            "reference picture",
            "link",
            "nothing active",
        ],
        gestures: &[Gesture::Drag, Gesture::Loop],
        places: NOWHERE,
        verdict: "quiet",
        kind: Kind::Idle,
        why: "nothing is there to take",
    },
    Known {
        hands: &["edit points"],
        rows: &[
            "paint",
            "pixel",
            "vector",
            "group",
            "mask",
            "adjustment",
            "map",
            "reference picture",
            "empty frame",
            "link",
            "nothing active",
        ],
        gestures: AROUND,
        places: NOWHERE,
        verdict: "quiet",
        kind: Kind::Idle,
        why: "nothing is there to take",
    },
    Known {
        hands: &["select strokes"],
        rows: &["paint", "pixel", "mask", "map", "link"],
        gestures: AROUND,
        places: NOWHERE,
        verdict: "quiet",
        kind: Kind::Idle,
        why: "nothing is there to take",
    },
    Known {
        hands: &["select strokes"],
        rows: &["mask"],
        gestures: DRAG,
        places: OWN,
        verdict: "quiet",
        kind: Kind::Dead,
        why: "the drag starts on the stroke the click before took, so it moves it, and here the move does nothing and says nothing",
    },
    Known {
        hands: &["rectangles and ellipses"],
        rows: &[
            "paint",
            "vector",
            "group",
            "mask",
            "adjustment",
            "bones",
            "map",
            "reference picture",
            "locked paint",
            "locked vector",
            "hidden paint",
            "empty frame",
            "link",
            "nothing active",
        ],
        gestures: CLICK,
        places: ANYWHERE,
        verdict: "quiet",
        kind: Kind::Idle,
        why: "a click without a drag draws nothing",
    },
    Known {
        hands: &["bones"],
        rows: &[
            "paint",
            "pixel",
            "vector",
            "group",
            "mask",
            "adjustment",
            "map",
            "reference picture",
            "locked paint",
            "locked pixel",
            "locked vector",
            "hidden paint",
            "empty frame",
            "link",
            "nothing active",
        ],
        gestures: CLICK,
        places: ANYWHERE,
        verdict: "quiet",
        kind: Kind::Idle,
        why: "a click without a drag draws nothing",
    },
    Known {
        hands: &["bones"],
        rows: &["bones"],
        gestures: CLICK,
        places: AWAY,
        verdict: "quiet",
        kind: Kind::Idle,
        why: "a click without a drag draws nothing",
    },
];

impl Known {
    fn holds(&self, cell: &Cell) -> bool {
        self.hands.contains(&cell.hand)
            && (self.rows.is_empty() || self.rows.contains(&cell.row))
            && self.gestures.contains(&cell.gesture)
            && self.places.contains(&cell.place)
    }
}

fn apply(bundle: &mut Bundle, lines: &str) {
    bundle
        .apply(&parse_lines(lines).unwrap_or_else(|e| panic!("{lines}: {e:#}")))
        .unwrap_or_else(|e| panic!("{lines}: {e:#}"));
}

fn id_of(bundle: &Bundle, name: &str) -> String {
    bundle.doc.find_layer(name).unwrap().id.clone()
}

fn add_layer(bundle: &mut Bundle, kind: &str, name: &str) -> String {
    apply(
        bundle,
        &format!(r#"{{"op":"add","layer":{{"kind":"{kind}","name":"{name}","cels":[{{"frame":0}}]}}}}"#),
    );
    id_of(bundle, name)
}

fn stroke(bundle: &mut Bundle, layer: &str, (x, y): (u32, u32)) {
    apply(
        bundle,
        &format!(
            r#"{{"op":"stroke","layer":"{layer}","path":"M{} {y} L{} {y}","size":6}}"#,
            x - 15,
            x + 15
        ),
    );
}

fn rect(bundle: &mut Bundle, layer: &str, (x, y): (u32, u32)) {
    apply(
        bundle,
        &format!(
            r##"{{"op":"add","node":{{"type":"rect","x":{},"y":{},"w":30,"h":20,"fill":"#a04030"}},"in":"{layer}","frame":0}}"##,
            x - 15,
            y - 10
        ),
    );
}

fn painting(dir: &Path, lit: bool) {
    let blank = Blank {
        width: 256,
        height: 192,
        frames: 6,
        ..Blank::default()
    };
    let mut bundle = Bundle::blank(dir, &blank, "cli").unwrap();
    let ground = bundle.doc.layers[0].id.clone();
    apply(
        &mut bundle,
        &format!(
            r#"{{"op":"set","id":"{ground}","name":"ground"}}
{{"op":"stroke","layer":"{ground}","path":"M10 160 L246 160","size":12}}"#
        ),
    );
    let paint = add_layer(&mut bundle, "paint", "paint");
    stroke(&mut bundle, &paint, (34, 30));
    let vector = add_layer(&mut bundle, "vector", "vector");
    rect(&mut bundle, &vector, (94, 30));
    apply(
        &mut bundle,
        r#"{"op":"add","layer":{"kind":"group","name":"set","children":[{"kind":"paint","name":"inner","cels":[{"frame":0}]}]}}"#,
    );
    let inner = id_of(&bundle, "inner");
    stroke(&mut bundle, &inner, (154, 30));
    let grey = klad_render::assets::Rgba {
        width: 30,
        height: 20,
        pixels: vec![150; 30 * 20 * 4],
    };
    let png = klad_render::assets::encode_png(&grey).unwrap();
    let (src, _, _) = klad_render::assets::import_image(&bundle.root, &png).unwrap();
    apply(
        &mut bundle,
        &format!(
            r#"{{"op":"add","layer":{{"kind":"vector","name":"photo","reference":true,"cels":[{{"frame":0,"nodes":[{{"type":"image","src":"{src}","x":199,"y":20,"w":30,"h":20}}]}}]}}}}"#
        ),
    );
    let masked = add_layer(&mut bundle, "paint", "masked");
    stroke(&mut bundle, &masked, (34, 70));
    apply(
        &mut bundle,
        &format!(
            r#"{{"op":"add","mask":{{"name":"mask"}},"in":"{masked}"}}
{{"op":"add","layer":{{"id":"sk","kind":"bones","name":"bones"}}}}
{{"op":"bone","layer":"sk","chain":["upper","lower"],"through":[[79,70],[94,70],[109,70]]}}"#
        ),
    );
    let locked = add_layer(&mut bundle, "paint", "locked");
    stroke(&mut bundle, &locked, (154, 70));
    apply(&mut bundle, &format!(r#"{{"op":"set","id":"{locked}","locked":true}}"#));
    let hidden = add_layer(&mut bundle, "paint", "hidden");
    stroke(&mut bundle, &hidden, (214, 70));
    apply(
        &mut bundle,
        &format!(
            r#"{{"op":"set","id":"{hidden}","visible":false}}
{{"op":"add","layer":{{"kind":"paint","name":"timed","cels":[{{"frame":0,"hold":2}}]}}}}"#
        ),
    );
    let timed = id_of(&bundle, "timed");
    stroke(&mut bundle, &timed, (34, 110));
    let first = bundle.doc.find_layer("timed").unwrap().cels()[0].id.clone();
    apply(
        &mut bundle,
        &format!(r#"{{"op":"cel","link":"{first}","frame":4,"hold":2}}"#),
    );
    let shapes = add_layer(&mut bundle, "vector", "locked shapes");
    rect(&mut bundle, &shapes, (94, 110));
    apply(&mut bundle, &format!(r#"{{"op":"set","id":"{shapes}","locked":true}}"#));
    if lit {
        apply(&mut bundle, r#"{"op":"maps","surface":{"bevel":8}}"#);
        let relief = add_layer(&mut bundle, "paint", "relief");
        stroke(&mut bundle, &relief, (154, 110));
        apply(
            &mut bundle,
            &format!(r#"{{"op":"set","id":"{relief}","map":"normal"}}"#),
        );
    }
    apply(
        &mut bundle,
        r#"{"op":"add","layer":{"kind":"adjust","name":"curves","adjust":{"curves":{}}}}"#,
    );
}

fn sprite(dir: &Path) {
    let blank = Blank {
        width: 64,
        height: 48,
        pixel: true,
        frames: 6,
        ..Blank::default()
    };
    let mut bundle = Bundle::blank(dir, &blank, "cli").unwrap();
    let ground = bundle.doc.layers[0].id.clone();
    apply(
        &mut bundle,
        &format!(
            r##"{{"op":"set","id":"{ground}","name":"ground"}}
{{"op":"pixel","layer":"{ground}","color":"#6b705c","rect":[4,36,56,4],"filled":true}}"##
        ),
    );
    let pixels = add_layer(&mut bundle, "pixel", "pixels");
    apply(
        &mut bundle,
        &format!(r##"{{"op":"pixel","layer":"{pixels}","color":"#2b2d42","rect":[8,8,12,8],"filled":true}}"##),
    );
    let locked = add_layer(&mut bundle, "pixel", "locked pixels");
    apply(
        &mut bundle,
        &format!(
            r##"{{"op":"pixel","layer":"{locked}","color":"#ef233c","rect":[30,8,10,8],"filled":true}}
{{"op":"set","id":"{locked}","locked":true}}"##
        ),
    );
}

fn shapes(dir: &Path) {
    let blank = Blank {
        width: 256,
        height: 192,
        ..Blank::default()
    };
    let mut bundle = Bundle::blank(dir, &blank, "cli").unwrap();
    let first = bundle.doc.layers[0].id.clone();
    let vector = add_layer(&mut bundle, "vector", "vector");
    rect(&mut bundle, &vector, (94, 30));
    apply(&mut bundle, &format!(r#"{{"op":"rm","id":"{first}"}}"#));
}

fn scratch(name: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "klad-sweep-{}-{}-{name}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

type Masters = (usize, Option<BTreeMap<Stage, PathBuf>>);

static MASTERS: Mutex<Masters> = Mutex::new((0, None));

fn masters() -> std::sync::MutexGuard<'static, Masters> {
    MASTERS.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

struct Sweeping;

impl Sweeping {
    fn begin() -> Self {
        masters().0 += 1;
        Self
    }
}

impl Drop for Sweeping {
    fn drop(&mut self) {
        let mut held = masters();
        held.0 -= 1;
        if held.0 == 0 {
            held.1
                .take()
                .into_iter()
                .flat_map(BTreeMap::into_values)
                .filter_map(|dir| dir.parent().map(Path::to_path_buf))
                .for_each(|dir| {
                    let _ = std::fs::remove_dir_all(dir);
                });
        }
    }
}

fn master(stage: Stage) -> PathBuf {
    masters()
        .1
        .get_or_insert_with(|| {
            [Stage::Painting, Stage::Lit, Stage::Sprite, Stage::Shapes]
                .into_iter()
                .map(|stage| {
                    let dir = scratch("master").join("sweep.klad");
                    match stage {
                        Stage::Painting => painting(&dir, false),
                        Stage::Lit => painting(&dir, true),
                        Stage::Sprite => sprite(&dir),
                        Stage::Shapes => shapes(&dir),
                    }
                    (stage, dir)
                })
                .collect()
        })
        .get(&stage)
        .unwrap()
        .clone()
}

fn fresh_only() -> bool {
    std::env::var_os("KLAD_SWEEP_FRESH").is_some_and(|v| !v.is_empty())
}

fn out_dir() -> Option<PathBuf> {
    std::env::var_os("KLAD_SWEEP_OUT")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
}

struct Desk {
    h: Harness<'static, Editor>,
    folder: PathBuf,
    stage: Stage,
    moving: bool,
}

impl Drop for Desk {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.folder);
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Snap {
    drawing: String,
    picture: u64,
    selection: String,
    mode: String,
    frame: u32,
    flags: String,
    screen: Vec<String>,
}

impl Snap {
    fn differs(&self, before: &Snap) -> Vec<String> {
        [
            (self.drawing != before.drawing).then(|| format!("drawing: {} was {}", self.drawing, before.drawing)),
            (self.picture != before.picture).then(|| "picture".to_string()),
            (self.selection != before.selection)
                .then(|| format!("selection: {} was {}", self.selection, before.selection)),
            (self.mode != before.mode).then(|| format!("tool or view: {} was {}", self.mode, before.mode)),
            (self.frame != before.frame).then(|| format!("frame: {} was {}", self.frame, before.frame)),
            (self.flags != before.flags).then(|| format!("switches: {} was {}", self.flags, before.flags)),
            (self.screen != before.screen)
                .then(|| format!("screen: {}", differences(&before.screen, &self.screen).join(" ; "))),
        ]
        .into_iter()
        .flatten()
        .collect()
    }
}

impl Desk {
    fn open(stage: Stage) -> Self {
        let folder = scratch("cell");
        let path = folder.join("sweep.klad");
        copy_dir(&master(stage), &path);
        let mut h = harness(editor(&path), WINDOW);
        h.state_mut().used_colour();
        h.run_steps(1);
        Self {
            h,
            folder,
            stage,
            moving: false,
        }
    }

    fn ed(&self) -> &Editor {
        self.h.state()
    }

    fn settle(&mut self) {
        let started = Instant::now();
        let moving = self.moving;
        let waiting = |ed: &Editor| {
            (!moving && !ed.canvas_current()) || !ed.jobs.is_empty() || ed.preview.is_some() || ed.path_commit.is_some()
        };
        self.h.run_steps(1);
        while waiting(self.ed()) && started.elapsed() < Duration::from_secs(8) {
            self.h.run_steps(1);
            std::thread::sleep(Duration::from_millis(1));
        }
        self.h.run_steps(2);
    }

    fn pick(&mut self, row: &Row) {
        let wanted = (!row.layer.is_empty()).then(|| {
            let doc = &self.ed().scene.bundle.doc;
            let id = doc.find_layer(row.layer).unwrap().id.clone();
            let path = match doc.locate(&id) {
                Some(Loc::Layer(path)) => path,
                _ => panic!("no layer {}", row.layer),
            };
            match row.mask {
                true => path.into_iter().chain([MASK]).collect::<Vec<usize>>(),
                false => path,
            }
        });
        let moved = wanted
            .filter(|path| *path != self.ed().scene.active)
            .map(|path| self.h.state_mut().activate(path).unwrap())
            .is_some();
        let stepped = self.ed().scene.frame != row.frame;
        if stepped {
            self.h.state_mut().go_to(row.frame).unwrap();
        }
        if moved || stepped {
            self.settle();
        }
    }

    fn take(&mut self, hand: &Hand) {
        if self.ed().is_on(hand.cmd) != Some(true) {
            self.h.state_mut().invoke(hand.cmd).unwrap();
            self.h.run_steps(2);
        }
    }

    fn hush(&mut self) -> u64 {
        let ids: Vec<u64> = self.ed().messages.log().map(|m| m.id).collect();
        ids.iter().for_each(|id| self.h.state_mut().messages.dismiss(*id));
        ids.into_iter().max().unwrap_or(0)
    }

    fn at(&self, (x, y): (f32, f32)) -> Pos2 {
        canvas_at(&self.h, x, y)
    }

    fn labels(&self, keep: impl Fn(egui::Rect, Role) -> bool) -> Vec<String> {
        let said: BTreeSet<String> = self.ed().messages.log().map(|m| m.text.clone()).collect();
        let canvas = self.ed().canvas_rect;
        let mut found: Vec<String> = self
            .h
            .query_all(By::new().predicate(|n| written(n).is_some()))
            .filter(|n| keep(n.rect(), n.accesskit_node().role()))
            .filter_map(|n| {
                let node = n.accesskit_node();
                let text = written(&node)?;
                let message = said.contains(&text) && n.rect().top() >= canvas.top();
                (!message).then(|| match node.role() {
                    Role::Label => text,
                    role => format!("{role:?} {text}"),
                })
            })
            .collect();
        found.sort();
        found
    }

    fn bar(&self) -> Vec<String> {
        let canvas = self.ed().canvas_rect;
        let top = self
            .h
            .query_all_by_label("File")
            .next()
            .map_or(0.0, |n| n.rect().bottom() + 3.0);
        self.labels(|r, role| {
            let c = r.center();
            role == Role::Label && c.y > top && c.y < canvas.top() && c.x > canvas.left() && c.x < canvas.right()
        })
    }

    fn snap(&mut self) -> Snap {
        self.hush();
        self.h.run_steps(1);
        let ed = self.ed();
        let doc = &ed.scene.bundle.doc;
        let undo = klad_core::history::entries(&ed.scene.bundle, Stack::Undo).map_or(0, |e| e.len());
        let picture = picture(ed);
        let floor = self.h.ctx.content_rect().bottom() - layout::STATUS;
        let json = serde_json::to_string(doc).unwrap_or_default();
        Snap {
            drawing: format!(
                "{} undo {undo} marks {}",
                json.bytes().fold(FNV, |hash, b| mix(hash, u64::from(b))),
                ed.scene.total_marks()
            ),
            picture,
            selection: format!(
                "{:?} {:?} {:?} {:?} {:?}",
                ed.picked, ed.selection, ed.group, ed.direct, ed.timeline.span
            ),
            mode: format!(
                "{:?} {} {:?} {:?} pen {:?} typing {:?} note {:?} {:?} {} bones {:?} {:?} {} {} shift {} play {} rename {} find {} colour {:?} size {} mirror {:?}",
                ed.tool,
                ed.brush,
                ed.scene.active,
                ed.view,
                ed.pen_path.as_ref().map(|p| p.anchors.len()),
                ed.text_edit.as_ref().map(|t| t.text.text.clone()),
                ed.note_at,
                ed.note_open,
                ed.pins.len(),
                ed.bones.mode,
                ed.bones.selected,
                ed.bones.chain.is_some(),
                ed.bones.dabs.len(),
                ed.shifting.is_some(),
                ed.timeline.player.is_some(),
                ed.renaming.is_some(),
                ed.palette.is_some(),
                ed.color,
                ed.size,
                ed.mirror
            ),
            frame: ed.scene.frame,
            flags: format!(
                "{} wants {:?} links {} copied {} {} {} jobs {} windows {} {} {} {} range {:?} ask {:?} developer {} swatch {:?} second {:?} pencil {}",
                Cmd::ALL
                    .iter()
                    .map(|cmd| match ed.is_on(*cmd) {
                        Some(true) => '1',
                        Some(false) => '0',
                        None => '-',
                    })
                    .collect::<String>(),
                ed.wants,
                ed.opened.len(),
                ed.clipboard.is_some(),
                ed.shape_clip.is_some(),
                ed.timeline.cells.is_some(),
                !ed.jobs.is_empty(),
                ed.windows.import,
                ed.windows.palettes,
                ed.windows.clip,
                ed.windows.between,
                ed.timeline.range,
                ed.timeline.ask,
                ed.settings.developer,
                ed.swatch_picked,
                ed.second,
                ed.pixel_size
            ),
            screen: self.labels(|r, _| r.center().y < floor),
        }
    }

    fn said_since(&self, heard: u64) -> Vec<String> {
        self.ed()
            .messages
            .log()
            .filter(|m| m.id > heard)
            .map(|m| {
                let tone = match m.tone {
                    Tone::Info => "info",
                    Tone::Done => "done",
                    Tone::Refused => "refused",
                    Tone::Error => "error",
                };
                format!("[{tone}] {}", m.text.replace('\n', " / "))
            })
            .collect()
    }

    fn hover(&mut self, at: Pos2) {
        self.h.hover_at(at);
        self.h.run_steps(1);
    }

    fn stroke(&mut self, points: &[Pos2]) {
        self.hover(points[0]);
        self.h.drag_at(points[0]);
        self.h.run_steps(1);
        points.windows(2).for_each(|pair| {
            (1..=2).for_each(|i| self.hover(pair[0] + (pair[1] - pair[0]) * (i as f32 / 2.0)));
        });
        self.h.drop_at(points[points.len() - 1]);
        self.h.run_steps(1);
    }

    fn act(&mut self, hand: &Hand, gesture: Gesture, at: (f32, f32)) {
        let (dx, dy) = self.stage.reach();
        let path: Vec<(f32, f32)> = match gesture {
            Gesture::Click => vec![at],
            Gesture::Drag => vec![at, (at.0 + dx, at.1 + dy)],
            Gesture::Loop => {
                let (rx, ry) = self.stage.around();
                (0..=16)
                    .map(|i| {
                        let turn = std::f32::consts::PI + i as f32 / 16.0 * std::f32::consts::TAU;
                        (at.0 + rx * turn.cos(), at.1 + ry * turn.sin())
                    })
                    .collect()
            }
        };
        let screen: Vec<Pos2> = path.iter().map(|p| self.at(*p)).collect();
        self.h.state_mut().marquee = match gesture {
            Gesture::Loop => super::marquee::Marquee::Loop,
            _ => super::marquee::Marquee::Box,
        };
        self.stroke(&screen);
        match hand.finish {
            Finish::Nothing => {}
            Finish::Path => {
                let next = self.at((at.0 + dx * 1.5, at.1 - dy * 0.5));
                self.stroke(&[next]);
                self.h.key_press(Key::Enter);
                self.h.run_steps(2);
            }
            Finish::Typed => {
                self.h.event(egui::Event::Text("hi".to_string()));
                self.h.run_steps(1);
                self.h.key_press(Key::Escape);
                self.h.run_steps(2);
            }
            Finish::Pinned => {
                self.h.run_steps(1);
                self.h.event(egui::Event::Text("look here".to_string()));
                self.h.run_steps(2);
                if let Some(pin) = self.h.query_all_by_label("pin it").next() {
                    pin.click();
                }
                self.h.run_steps(2);
            }
        }
        self.settle();
    }

    fn back(&mut self, hand: &Hand, row: &Row, before: &Snap, after: &Snap, home: Pos2) -> Vec<String> {
        if after == before {
            return Vec::new();
        }
        self.h.key_press(Key::Escape);
        self.h.run_steps(2);
        (0..3).for_each(|_| {
            if self.snap().drawing != before.drawing {
                let _ = self.h.state_mut().invoke(Cmd::Undo);
                self.settle();
            }
        });
        let pins: Vec<u32> = self.ed().pins.iter().map(|n| n.id).collect();
        pins.iter().for_each(|id| {
            let _ = klad_core::notes::remove(&self.ed().scene.bundle, *id);
        });
        if !pins.is_empty() {
            self.h.state_mut().reread_pins();
        }
        self.pick(row);
        self.take(hand);
        self.hover(home);
        self.snap().differs(before)
    }
}

#[derive(Clone, Debug)]
struct Cell {
    hand: &'static str,
    row: &'static str,
    gesture: Gesture,
    place: Place,
    drawing: bool,
    picture: bool,
    selection: bool,
    mode: bool,
    screen: Vec<String>,
    said: Vec<String>,
    bar: Vec<String>,
    took: Duration,
    reused: bool,
    left: Vec<String>,
}

impl Cell {
    fn seen(&self) -> bool {
        self.picture || self.selection || self.mode || !self.screen.is_empty()
    }

    fn verdict(&self) -> &'static str {
        let failed = self.said.iter().any(|m| m.starts_with("[error]"));
        match (failed, self.seen(), self.drawing, self.said.is_empty()) {
            (true, ..) => "error",
            (_, true, _, _) => "changes",
            (_, false, true, _) => "unseen",
            (_, false, false, false) => "says",
            (_, false, false, true) => "quiet",
        }
    }

    fn known(&self) -> Option<&'static Known> {
        KNOWN.iter().find(|k| k.verdict == self.verdict() && k.holds(self))
    }

    fn shown(&self) -> String {
        match (self.verdict(), self.known().map(|k| k.kind)) {
            (verdict, Some(Kind::Dead)) => format!("**{verdict}**"),
            (verdict, Some(Kind::Idle)) => format!("{verdict}, nothing to do"),
            (verdict, None) => verdict.to_string(),
        }
    }

    fn key(&self) -> String {
        format!(
            "{} / {} / {} / {}",
            self.hand,
            self.row,
            self.gesture.name(),
            self.place.name()
        )
    }

    fn what(&self) -> String {
        [
            (self.picture, "picture"),
            (self.drawing, "drawing"),
            (self.selection, "selection"),
            (self.mode, "tool or view"),
            (!self.screen.is_empty(), "screen"),
        ]
        .iter()
        .filter(|(on, _)| *on)
        .map(|(_, name)| *name)
        .collect::<Vec<_>>()
        .join(", ")
    }
}

fn differences(before: &[String], after: &[String]) -> Vec<String> {
    let count = |list: &[String]| {
        list.iter().fold(BTreeMap::<String, i32>::new(), |mut counts, label| {
            *counts.entry(label.clone()).or_default() += 1;
            counts
        })
    };
    let (old, new) = (count(before), count(after));
    let names: BTreeSet<&String> = old.keys().chain(new.keys()).collect();
    names
        .into_iter()
        .filter_map(|name| {
            let by = new.get(name).copied().unwrap_or(0) - old.get(name).copied().unwrap_or(0);
            match by {
                0 => None,
                by if by > 0 => Some(format!("+{name}")),
                _ => Some(format!("-{name}")),
            }
        })
        .collect()
}

type Press<'a> = (&'a Row, Place, (f32, f32), Gesture);

fn cell(hand: &Hand, (row, place, at, gesture): Press, kept: Option<Desk>) -> (Cell, Option<Desk>) {
    let started = Instant::now();
    let reused = kept.is_some();
    let mut desk = kept.unwrap_or_else(|| Desk::open(row.stage));
    desk.pick(row);
    desk.take(hand);
    let home = desk.at(at);
    desk.hover(home);
    let heard = desk.hush();
    let bar = desk.bar();
    let before = desk.snap();
    desk.act(hand, gesture, at);
    desk.hover(home);
    let said = desk.said_since(heard);
    let after = desk.snap();
    let mut made = Cell {
        hand: hand.name,
        row: row.name,
        gesture,
        place,
        drawing: before.drawing != after.drawing,
        picture: before.picture != after.picture,
        selection: before.selection != after.selection,
        mode: before.mode != after.mode || before.frame != after.frame || before.flags != after.flags,
        screen: differences(&before.screen, &after.screen),
        said,
        bar,
        took: started.elapsed(),
        reused,
        left: Vec::new(),
    };
    if fresh_only() {
        return (made, None);
    }
    made.left = desk.back(hand, row, &before, &after, home);
    let clean = made.left.is_empty();
    (made, clean.then_some(desk))
}

fn cells_of(hand: &Hand) -> Vec<Cell> {
    let presses: Vec<Press> = ROWS
        .iter()
        .flat_map(|row| {
            row.places()
                .into_iter()
                .flat_map(move |(place, at)| hand.gestures.iter().map(move |gesture| (row, place, at, *gesture)))
        })
        .collect();
    presses
        .into_iter()
        .fold((Vec::new(), None::<Desk>), |(mut cells, kept), press| {
            let kept = kept.filter(|desk| desk.stage == press.0.stage);
            let (made, kept) = cell(hand, press, kept);
            cells.push(made);
            (cells, kept)
        })
        .0
}

fn grid_report(cells: &[Cell]) -> String {
    let mut lines = vec![
        "# The grid: every tool on every kind of layer".to_string(),
        String::new(),
        "A cell is one act with a tool while a kind of layer is active. changes: something a person can see changed. says: nothing changed and a message says why. quiet: nothing changed and nothing was said. unseen: the drawing changed and nothing on screen did. error: a message in the error tone. Bold cells are dead ends; the list under the tables says why for each.".to_string(),
        String::new(),
        "## The layers".to_string(),
        String::new(),
    ];
    ROWS.iter()
        .for_each(|row| lines.push(format!("- **{}**: {}", row.name, row.about)));
    lines.push(String::new());
    HANDS.iter().for_each(|hand| {
        lines.push(format!("## {} ({})", hand.name, hand.key));
        lines.push(String::new());
        let columns: Vec<(Place, Gesture)> = [Place::Own, Place::Other, Place::Empty]
            .into_iter()
            .flat_map(|place| hand.gestures.iter().map(move |g| (place, *g)))
            .collect();
        lines.push(format!(
            "| layer | {} |",
            columns
                .iter()
                .map(|(place, g)| format!("{}, {}", g.name(), place.name()))
                .collect::<Vec<_>>()
                .join(" | ")
        ));
        lines.push(format!("|---|{}", "---|".repeat(columns.len())));
        ROWS.iter().for_each(|row| {
            let across: Vec<String> = columns
                .iter()
                .map(|(place, gesture)| {
                    cells
                        .iter()
                        .find(|c| {
                            c.hand == hand.name && c.row == row.name && c.place == *place && c.gesture == *gesture
                        })
                        .map_or_else(|| "-".to_string(), Cell::shown)
                })
                .collect();
            lines.push(format!("| {} | {} |", row.name, across.join(" | ")));
        });
        lines.push(String::new());
    });
    lines.push("## The quiet cells, and why".to_string());
    lines.push(String::new());
    lines.push("| what | tools | layers | acts | where | cells | why |".to_string());
    lines.push("|---|---|---|---|---|---|---|".to_string());
    KNOWN.iter().for_each(|known| {
        let held = cells.iter().filter(|c| known.holds(c)).count();
        let names = |list: Vec<&str>| list.join(", ");
        lines.push(format!(
            "| {} | {} | {} | {} | {} | {held} | {} |",
            match known.kind {
                Kind::Dead => "dead end",
                Kind::Idle => "nothing to do",
            },
            names(known.hands.to_vec()),
            match known.rows.is_empty() {
                true => "every layer".to_string(),
                false => names(known.rows.to_vec()),
            },
            names(known.gestures.iter().map(|g| g.name()).collect()),
            names(known.places.iter().map(|p| p.name()).collect()),
            known.why
        ));
    });
    lines.push(String::new());
    lines.push("## Every cell".to_string());
    lines.push(String::new());
    cells.iter().for_each(|c| {
        lines.push(format!("### {}", c.key()));
        lines.push(format!("- verdict: {}", c.verdict()));
        if let Some(known) = c.known() {
            lines.push(format!("- known: {}", known.why));
        }
        if c.seen() || c.drawing {
            lines.push(format!("- changed: {}", c.what()));
        }
        c.said.iter().for_each(|m| lines.push(format!("- said {m}")));
        if !c.screen.is_empty() {
            let words: String = c.screen.join(" ; ").chars().take(SHOWN).collect();
            lines.push(format!("- screen: {words}"));
        }
        lines.push(format!("- the options bar reads: {}", c.bar.join(" ; ")));
        c.left.iter().for_each(|l| {
            let words: String = l.chars().take(SHOWN).collect();
            lines.push(format!("- after Esc and undo, still changed: {words}"));
        });
        lines.push(format!(
            "- took: {} ms{}",
            c.took.as_millis(),
            if c.reused {
                ", same editor as the cell before"
            } else {
                ""
            }
        ));
        lines.push(String::new());
    });
    lines.join("\n") + "\n"
}

#[test]
fn every_tool_on_every_kind_of_layer_changes_something_or_says_why() {
    let _sweeping = Sweeping::begin();
    let cells: Vec<Cell> = std::thread::scope(|scope| {
        let running: Vec<_> = HANDS.iter().map(|hand| scope.spawn(move || cells_of(hand))).collect();
        running.into_iter().flat_map(|r| r.join().unwrap()).collect()
    });
    if let Some(out) = out_dir() {
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(out.join("grid.md"), grid_report(&cells)).unwrap();
    }
    let new: Vec<String> = cells
        .iter()
        .filter(|c| matches!(c.verdict(), "quiet" | "unseen" | "error"))
        .filter(|c| KNOWN.iter().filter(|k| k.verdict == c.verdict() && k.holds(c)).count() != 1)
        .map(|c| format!("{} ({})", c.key(), c.verdict()))
        .collect();
    let gone: Vec<String> = cells
        .iter()
        .filter(|c| KNOWN.iter().any(|k| k.holds(c) && k.verdict != c.verdict()))
        .map(|c| format!("{} (now {})", c.key(), c.verdict()))
        .collect();
    assert!(
        new.is_empty() && gone.is_empty(),
        "acts that now do nothing without a word, change the drawing unseen or end in an error, and are not on the list: {new:#?}\nacts on the list that now answer (take them off it): {gone:#?}"
    );
}

struct State {
    name: &'static str,
    about: &'static str,
    row: &'static str,
    keys: bool,
    moving: bool,
    enter: fn(&mut Desk),
}

fn hold(desk: &mut Desk, cmd: Cmd) {
    if desk.ed().is_on(cmd) != Some(true) {
        desk.h.state_mut().invoke(cmd).unwrap();
        desk.h.run_steps(2);
    }
}

fn tap(desk: &mut Desk, at: (f32, f32)) {
    let at = desk.at(at);
    desk.stroke(&[at]);
    desk.settle();
}

fn frame_cell(desk: &Desk, layer: &str, frame: u32) -> Option<Pos2> {
    let ed = desk.ed();
    let row = ed
        .layer_rows(super::layers::Where::Timeline)
        .iter()
        .position(|r| r.name == layer)?;
    ed.timeline.geom.map(|g| g.cell_centre(frame, row))
}

fn painting_now(desk: &mut Desk) {
    hold(desk, Cmd::Brush);
}

fn strokes_now(desk: &mut Desk) {
    hold(desk, Cmd::Strokes);
    if desk.ed().picked.is_none() {
        tap(desk, (34.0, 30.0));
    }
}

fn shape_now(desk: &mut Desk) {
    hold(desk, Cmd::Shapes);
    if desk.ed().selection.is_none() {
        tap(desk, (94.0, 30.0));
    }
}

fn points_now(desk: &mut Desk) {
    hold(desk, Cmd::Points);
    if desk.ed().direct.is_none() {
        tap(desk, (79.0, 30.0));
        let corners = [desk.at((72.0, 14.0)), desk.at((86.0, 26.0))];
        desk.stroke(&corners);
        desk.settle();
    }
}

fn pixels_now(desk: &mut Desk) {
    hold(desk, Cmd::Strokes);
    if desk.ed().picked.is_none() {
        let around = [desk.at((6.5, 6.5)), desk.at((21.5, 17.5))];
        desk.stroke(&around);
        desk.settle();
    }
}

fn frames_now(desk: &mut Desk) {
    hold(desk, Cmd::Brush);
    let cells = [frame_cell(desk, "timed", 1), frame_cell(desk, "timed", 0)];
    if let (true, [Some(from), Some(to)]) = (desk.ed().timeline.span.is_none(), cells) {
        desk.stroke(&[from, to]);
        desk.settle();
    }
}

fn locked_now(desk: &mut Desk) {
    hold(desk, Cmd::Shapes);
    if desk.ed().picked.is_none() {
        tap(desk, (154.0, 70.0));
    }
}

fn typing_now(desk: &mut Desk) {
    hold(desk, Cmd::TextTool);
    if desk.ed().text_edit.is_none() {
        tap(desk, (214.0, 110.0));
    }
}

fn path_now(desk: &mut Desk) {
    hold(desk, Cmd::Pen);
    if desk.ed().pen_path.is_none() {
        tap(desk, (190.0, 100.0));
        tap(desk, (230.0, 116.0));
    }
}

fn settings_now(desk: &mut Desk) {
    hold(desk, Cmd::Brush);
    hold(desk, Cmd::Settings);
}

fn playing_now(desk: &mut Desk) {
    hold(desk, Cmd::Brush);
    hold(desk, Cmd::Play);
}

fn bone_now(desk: &mut Desk) {
    hold(desk, Cmd::BoneTool);
    if desk.ed().bones.selected.is_none() {
        tap(desk, (87.0, 70.0));
    }
}

fn developer_now(desk: &mut Desk) {
    hold(desk, Cmd::Brush);
    desk.h.state_mut().settings.developer = true;
    desk.h.run_steps(1);
}

const STATES: [State; 23] = [
    State {
        name: "painting",
        about: "a paint layer is active, the brush in hand, nothing selected",
        row: "paint",
        keys: true,
        moving: false,
        enter: painting_now,
    },
    State {
        name: "a stroke selected",
        about: "a stroke of the paint layer is selected with S",
        row: "paint",
        keys: true,
        moving: false,
        enter: strokes_now,
    },
    State {
        name: "a shape selected",
        about: "the rectangle of the vector layer is selected with V",
        row: "vector",
        keys: false,
        moving: false,
        enter: shape_now,
    },
    State {
        name: "points selected",
        about: "a corner of the rectangle is selected with A",
        row: "vector",
        keys: false,
        moving: false,
        enter: points_now,
    },
    State {
        name: "a pixel layer",
        about: "a pixel layer of a sprite is active, the pencil in hand",
        row: "pixel",
        keys: false,
        moving: false,
        enter: painting_now,
    },
    State {
        name: "pixels selected",
        about: "a block of pixels is selected with S",
        row: "pixel",
        keys: false,
        moving: false,
        enter: pixels_now,
    },
    State {
        name: "frames selected",
        about: "two frames of a layer are selected in the timeline, which has the keys",
        row: "link",
        keys: true,
        moving: false,
        enter: frames_now,
    },
    State {
        name: "a mask",
        about: "the mask of a paint layer is picked to paint on",
        row: "mask",
        keys: false,
        moving: false,
        enter: painting_now,
    },
    State {
        name: "a group",
        about: "a group is the active layer",
        row: "group",
        keys: false,
        moving: false,
        enter: painting_now,
    },
    State {
        name: "an adjustment layer",
        about: "an adjustment layer is active",
        row: "adjustment",
        keys: false,
        moving: false,
        enter: painting_now,
    },
    State {
        name: "a reference picture",
        about: "an imported picture marked as a reference is the active layer",
        row: "reference picture",
        keys: false,
        moving: false,
        enter: painting_now,
    },
    State {
        name: "an empty frame",
        about: "a paint layer is active on a frame where it has no drawing",
        row: "empty frame",
        keys: false,
        moving: false,
        enter: painting_now,
    },
    State {
        name: "a link",
        about: "a paint layer is active on a frame that shows a link",
        row: "link",
        keys: false,
        moving: false,
        enter: painting_now,
    },
    State {
        name: "a locked layer, its stroke selected",
        about: "a stroke of a locked paint layer is selected with V",
        row: "locked paint",
        keys: true,
        moving: false,
        enter: locked_now,
    },
    State {
        name: "a hidden layer",
        about: "a hidden paint layer is active, the brush in hand",
        row: "hidden paint",
        keys: false,
        moving: false,
        enter: painting_now,
    },
    State {
        name: "typing a text",
        about: "the text tool has a text open on the canvas",
        row: "vector",
        keys: true,
        moving: false,
        enter: typing_now,
    },
    State {
        name: "a path in progress",
        about: "the pen has placed two points of a path",
        row: "vector",
        keys: true,
        moving: false,
        enter: path_now,
    },
    State {
        name: "a map layer",
        about: "a layer that draws the normal map is active, light maps on",
        row: "map",
        keys: false,
        moving: false,
        enter: painting_now,
    },
    State {
        name: "nothing active",
        about: "a drawing of vector layers only, as it opens",
        row: "nothing active",
        keys: false,
        moving: false,
        enter: painting_now,
    },
    State {
        name: "settings open",
        about: "the settings window is open",
        row: "paint",
        keys: true,
        moving: false,
        enter: settings_now,
    },
    State {
        name: "playing",
        about: "the timeline is playing",
        row: "paint",
        keys: true,
        moving: true,
        enter: playing_now,
    },
    State {
        name: "a bone selected",
        about: "a bone of the bones layer is selected with the bone tool",
        row: "bones",
        keys: false,
        moving: false,
        enter: bone_now,
    },
    State {
        name: "developer tools on",
        about: "painting, with the developer tools turned on in the settings",
        row: "paint",
        keys: false,
        moving: false,
        enter: developer_now,
    },
];

const OUTSIDE: [Cmd; 2] = [Cmd::InstallCommand, Cmd::Quit];
const NO_KEY: [Cmd; 2] = [Cmd::HideAll, Cmd::HideDock];

struct Calm {
    cmd: &'static str,
    kind: Kind,
    why: &'static str,
}

const fn calm(cmd: &'static str, kind: Kind, why: &'static str) -> Calm {
    Calm { cmd, kind, why }
}

const IN_HAND: &str = "the tool is in hand already";
const THERE: &str = "the playhead is on that frame already";

const CALM: [Calm; 19] = [
    calm("tool.brush", Kind::Idle, IN_HAND),
    calm("tool.strokes", Kind::Idle, IN_HAND),
    calm("tool.shapes", Kind::Idle, IN_HAND),
    calm("tool.points", Kind::Idle, IN_HAND),
    calm("tool.pen", Kind::Idle, IN_HAND),
    calm("tool.text", Kind::Idle, IN_HAND),
    calm("tool.bones", Kind::Idle, IN_HAND),
    calm("edit.deselect", Kind::Idle, "nothing is selected"),
    calm(
        "edit.select-all",
        Kind::Idle,
        "everything on the layer is selected already",
    ),
    calm("frame.previous", Kind::Idle, THERE),
    calm("frame.first", Kind::Idle, THERE),
    calm("frame.next", Kind::Idle, THERE),
    calm("frame.last", Kind::Idle, THERE),
    calm("expose.twos", Kind::Idle, "new drawings are on twos already"),
    calm("expose.ones", Kind::Idle, "new drawings are on ones already"),
    calm("view.upright", Kind::Idle, "the view is upright already"),
    calm("view.fit", Kind::Idle, "the canvas fits the window already"),
    calm("brush.smaller", Kind::Idle, "the pencil is one pixel already"),
    calm("numbers.reset", Kind::Idle, "the numbers are not on screen"),
];

const FAILS: [(&str, &str); 0] = [];

const DEAF: [&str; 2] = ["typing a text", "settings open"];

#[derive(Clone, Copy, Debug, PartialEq)]
enum Route {
    Menu,
    Key(crate::commands::Keys),
}

impl Route {
    fn name(&self) -> String {
        match self {
            Route::Menu => "menu".to_string(),
            Route::Key(keys) => keys.label(),
        }
    }
}

#[derive(Clone, Debug)]
struct Run {
    state: &'static str,
    cmd: Cmd,
    route: Route,
    refusal: Option<String>,
    changed: Vec<String>,
    said: Vec<String>,
    took: Duration,
    fresh: bool,
}

impl Run {
    fn verdict(&self) -> &'static str {
        let failed = self.said.iter().any(|m| m.starts_with("[error]"));
        match (failed, self.changed.is_empty(), self.said.is_empty()) {
            (true, ..) => "error",
            (_, false, _) => "changes",
            (_, true, false) => "says",
            (_, true, true) => "quiet",
        }
    }

    fn key(&self) -> String {
        format!("{} / {} / {}", self.state, self.cmd.spec().id, self.route.name())
    }

    fn silent(&self) -> bool {
        self.verdict() == "quiet" && self.refusal.is_none()
    }

    fn kinds(&self) -> String {
        self.changed
            .iter()
            .filter_map(|c| c.split(':').next())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

struct Keep {
    view: Option<crate::view::View>,
    size: f32,
    pencil: u32,
    on: Vec<Option<bool>>,
}

impl Desk {
    fn enter(&mut self, state: &State) {
        let row = ROWS.iter().find(|r| r.name == state.row).unwrap();
        self.moving = state.moving;
        self.pick(row);
        (state.enter)(self);
        let middle = self.ed().canvas_rect.center();
        self.hover(middle);
    }

    fn keep(&self) -> Keep {
        let ed = self.ed();
        Keep {
            view: ed.view,
            size: ed.size,
            pencil: ed.pixel_size,
            on: Cmd::ALL.iter().map(|cmd| ed.is_on(*cmd)).collect(),
        }
    }

    fn calm(&mut self, moving: bool) -> Snap {
        let snap = self.snap();
        match moving {
            true => Snap {
                picture: 0,
                frame: 0,
                screen: Vec::new(),
                ..snap
            },
            false => snap,
        }
    }

    fn switch_back(&mut self, keep: &Keep, wanted: bool) {
        let off: Vec<Cmd> = Cmd::ALL
            .iter()
            .zip(&keep.on)
            .filter(|(cmd, was)| **was == Some(wanted) && self.ed().is_on(**cmd) == Some(!wanted))
            .map(|(cmd, _)| *cmd)
            .collect();
        off.into_iter().for_each(|cmd| {
            if self.ed().is_on(cmd) == Some(!wanted) {
                let _ = self.h.state_mut().invoke(cmd);
                self.h.run_steps(2);
            }
        });
    }

    fn undo_to(&mut self, before: &Snap) {
        (0..3).for_each(|_| {
            if self.snap().drawing != before.drawing {
                let _ = self.h.state_mut().invoke(Cmd::Undo);
                self.settle();
            }
        });
    }

    fn restore(&mut self, state: &State, before: &Snap, keep: &Keep) -> bool {
        self.h.key_press(Key::Escape);
        self.h.run_steps(2);
        self.undo_to(before);
        self.switch_back(keep, true);
        self.switch_back(keep, false);
        {
            let ed = self.h.state_mut();
            ed.wants = None;
            ed.opened.clear();
            ed.palette = None;
            ed.clipboard = None;
            ed.shape_clip = None;
            ed.timeline.cells = None;
            ed.view = keep.view;
            ed.zoom_wanted = None;
            ed.size = keep.size;
            ed.pixel_size = keep.pencil;
        }
        self.h.run_steps(1);
        self.enter(state);
        self.settle();
        self.calm(state.moving) == *before
    }

    fn send(&mut self, cmd: Cmd, route: Route) {
        match route {
            Route::Menu => self.h.state_mut().run_later(cmd),
            Route::Key(keys) => {
                let modifiers = egui::Modifiers {
                    alt: keys.alt,
                    ctrl: false,
                    shift: keys.shift,
                    mac_cmd: false,
                    command: keys.command,
                };
                self.h.key_press_modifiers(modifiers, keys.key);
            }
        }
        self.h.run_steps(2);
        self.settle();
    }
}

fn routes(state: &State, cmd: Cmd, bindings: &crate::commands::Bindings) -> Vec<Route> {
    let keys = bindings
        .keys(cmd)
        .into_iter()
        .filter(|_| state.keys && !NO_KEY.contains(&cmd))
        .map(Route::Key);
    std::iter::once(Route::Menu).chain(keys).collect()
}

fn runs_of(state: &State) -> Vec<Run> {
    let stage = ROWS.iter().find(|r| r.name == state.row).unwrap().stage;
    let open = || {
        let mut desk = Desk::open(stage);
        desk.enter(state);
        desk.settle();
        desk
    };
    let first = open();
    let bindings = first.ed().bindings.clone();
    let jobs: Vec<(Cmd, Route)> = Cmd::ALL
        .into_iter()
        .filter(|cmd| !OUTSIDE.contains(cmd))
        .flat_map(|cmd| routes(state, cmd, &bindings).into_iter().map(move |route| (cmd, route)))
        .collect();
    jobs.into_iter()
        .fold((Vec::new(), Some(first)), |(mut runs, kept), (cmd, route)| {
            let started = Instant::now();
            let fresh = kept.is_none();
            let mut desk = kept.unwrap_or_else(open);
            let heard = desk.hush();
            let before = desk.calm(state.moving);
            let keep = desk.keep();
            let refusal = desk.ed().can_run(cmd);
            desk.send(cmd, route);
            let said = desk.said_since(heard);
            let after = desk.calm(state.moving);
            let changed = after.differs(&before);
            let clean = changed.is_empty() || (!fresh_only() && desk.restore(state, &before, &keep));
            runs.push(Run {
                state: state.name,
                cmd,
                route,
                refusal,
                changed,
                said,
                took: started.elapsed(),
                fresh,
            });
            (runs, clean.then_some(desk))
        })
        .0
}

fn mute(runs: &[Run]) -> Vec<&Run> {
    runs.iter()
        .filter(|r| r.route != Route::Menu && r.verdict() == "quiet")
        .filter(|r| {
            runs.iter()
                .any(|m| m.route == Route::Menu && m.state == r.state && m.cmd == r.cmd && m.verdict() != "quiet")
        })
        .collect()
}

fn sweep_report(runs: &[Run]) -> String {
    let mut lines = vec![
        "# The command sweep: every command in every prepared state".to_string(),
        String::new(),
        "Each command is run the way a menu, a button or the command finder runs it, and from its keys where the state is marked for keys. changes: something a person can see changed. says: nothing changed and a message was said. quiet: nothing changed and nothing was said. error: a message in the error tone.".to_string(),
        String::new(),
        "## The states".to_string(),
        String::new(),
    ];
    STATES.iter().for_each(|state| {
        lines.push(format!(
            "- **{}**: {}{}",
            state.name,
            state.about,
            if state.keys { " (keys pressed too)" } else { "" }
        ));
    });
    lines.push(String::new());
    lines.push(format!(
        "Not run: {} (they reach outside klad). The keys of {} are not pressed: the test window has no hook for Tab.",
        OUTSIDE.iter().map(|c| c.spec().name).collect::<Vec<_>>().join(", "),
        NO_KEY.iter().map(|c| c.spec().name).collect::<Vec<_>>().join(", ")
    ));
    lines.push(String::new());
    lines.push("## Commands that can run, do nothing and say nothing".to_string());
    lines.push(String::new());
    lines.push("| command | what | why | states |".to_string());
    lines.push("|---|---|---|---|".to_string());
    CALM.iter().for_each(|calm| {
        let states: Vec<&str> = runs
            .iter()
            .filter(|r| r.silent() && r.route == Route::Menu && r.cmd.spec().id == calm.cmd)
            .map(|r| r.state)
            .collect();
        lines.push(format!(
            "| {} ({}) | {} | {} | {} |",
            Cmd::find(calm.cmd).map_or("", |c| c.spec().name),
            calm.cmd,
            match calm.kind {
                Kind::Dead => "dead end",
                Kind::Idle => "nothing to do",
            },
            calm.why,
            match states.len() == STATES.len() {
                true => "every state".to_string(),
                false => states.join(", "),
            }
        ));
    });
    lines.push(String::new());
    lines.push("## Commands that end in an error".to_string());
    lines.push(String::new());
    runs.iter().filter(|r| r.verdict() == "error").for_each(|r| {
        lines.push(format!("- {}: {}", r.key(), r.said.join(" / ")));
    });
    lines.push(String::new());
    lines.push("## Keys that say nothing where the menu answers".to_string());
    lines.push(String::new());
    mute(runs)
        .into_iter()
        .for_each(|r| lines.push(format!("- {}", r.key())));
    lines.push(String::new());
    STATES.iter().for_each(|state| {
        lines.push(format!("## {}", state.name));
        lines.push(String::new());
        lines.push("| command | how | verdict | what | ms |".to_string());
        lines.push("|---|---|---|---|---|".to_string());
        runs.iter().filter(|r| r.state == state.name).for_each(|r| {
            let what = match (r.verdict(), &r.refusal) {
                ("changes", _) => r.kinds(),
                _ => String::new(),
            };
            let said = r.said.join(" / ").replace('|', "/");
            lines.push(format!(
                "| {} ({}) | {} | {}{} | {} | {}{} |",
                r.cmd.spec().name,
                r.cmd.spec().id,
                r.route.name(),
                r.verdict(),
                if r.refusal.is_some() { ", cannot run" } else { "" },
                [what, said]
                    .into_iter()
                    .filter(|t| !t.is_empty())
                    .collect::<Vec<_>>()
                    .join(": "),
                r.took.as_millis(),
                if r.fresh { " fresh" } else { "" }
            ));
        });
        lines.push(String::new());
    });
    lines.join("\n") + "\n"
}

#[test]
#[ignore = "slow: just sweep"]
fn every_command_in_every_state_changes_something_or_says_why() {
    let _sweeping = Sweeping::begin();
    let only = std::env::var("KLAD_SWEEP_STATE").unwrap_or_default();
    let runs: Vec<Run> = std::thread::scope(|scope| {
        let running: Vec<_> = STATES
            .iter()
            .filter(|state| only.is_empty() || state.name == only)
            .map(|state| scope.spawn(move || runs_of(state)))
            .collect();
        running.into_iter().flat_map(|r| r.join().unwrap()).collect()
    });
    if let Some(out) = out_dir() {
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(out.join("sweep.md"), sweep_report(&runs)).unwrap();
    }
    let keys = |list: Vec<&Run>| list.into_iter().map(Run::key).collect::<Vec<_>>();
    let ran = |name: &str| only.is_empty() || only == name;
    let refused: Vec<String> = keys(
        runs.iter()
            .filter(|r| r.refusal.is_some() && r.route == Route::Menu && !r.changed.is_empty())
            .collect(),
    );
    assert!(
        refused.is_empty(),
        "commands that say they cannot run and change something all the same: {refused:#?}"
    );
    let failed: Vec<String> = keys(
        runs.iter()
            .filter(|r| r.verdict() == "error" && !FAILS.contains(&(r.state, r.cmd.spec().id)))
            .collect(),
    );
    let mended: Vec<&(&str, &str)> = FAILS
        .iter()
        .filter(|(state, _)| ran(state))
        .filter(|(state, cmd)| {
            !runs
                .iter()
                .any(|r| r.state == *state && r.cmd.spec().id == *cmd && r.verdict() == "error")
        })
        .collect();
    assert!(
        failed.is_empty() && mended.is_empty(),
        "commands that end in an error and are not on the list: {failed:#?}\nlisted ones that no longer do (take them off it): {mended:#?}"
    );
    let silent: Vec<String> = keys(
        runs.iter()
            .filter(|r| r.silent() && r.route == Route::Menu)
            .filter(|r| !CALM.iter().any(|calm| calm.cmd == r.cmd.spec().id))
            .collect(),
    );
    let loud: Vec<&str> = CALM
        .iter()
        .filter(|_| only.is_empty())
        .filter(|calm| !runs.iter().any(|r| r.silent() && r.cmd.spec().id == calm.cmd))
        .map(|calm| calm.cmd)
        .collect();
    assert!(
        silent.is_empty() && loud.is_empty(),
        "commands that can run, change nothing and say nothing, and are not on the list: {silent:#?}\nlisted ones that now answer everywhere (take them off it): {loud:#?}"
    );
    let unheard: Vec<String> = keys(mute(&runs).into_iter().filter(|r| !DEAF.contains(&r.state)).collect());
    assert!(
        unheard.is_empty(),
        "keys that say nothing where the menu answers, outside a text and the settings: {unheard:#?}"
    );
}
