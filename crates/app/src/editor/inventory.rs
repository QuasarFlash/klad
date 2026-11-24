use super::bones::Mode;
use super::export::What;
use super::prefs::Section;
use super::ui_tests::{CONTROLS, activate, canvas_at, drag, editor, harness, pixel_sample, sample};
use super::{Direct, Editor, Tool};
use crate::commands::Cmd;
use crate::home::Home;
use crate::layout::{self, Pane};
use crate::settings::Settings;
use crate::theme;
use egui::accesskit::{Role, Toggled};
use egui::{Rect, Vec2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{By, NodeT, Queryable};
use klad_core::ops::parse_lines;
use std::collections::{BTreeMap, BTreeSet};

const SIZE: Vec2 = vec2(1600.0, 1400.0);
const SHELL: [&str; 4] = ["menu bar", "tool strip", "timeline", "status line"];
const TOOL: [&str; 2] = ["options bar", "over the canvas"];
const DOCK: [&str; 1] = ["dock"];
const FLOATING: [&str; 3] = ["window", "dialog", "menu"];

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Seen {
    place: String,
    role: String,
    label: String,
    value: String,
    hint: String,
}

fn icons() -> BTreeMap<char, String> {
    include_str!("../theme.rs")
        .lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("pub const ")?;
            let (name, value) = rest.split_once(": &str = \"\\u{")?;
            let code = u32::from_str_radix(value.split('}').next()?, 16).ok()?;
            Some((char::from_u32(code)?, name.to_lowercase().replace('_', " ")))
        })
        .collect()
}

fn plain(text: &str, icons: &BTreeMap<char, String>) -> String {
    text.chars()
        .map(|c| match (icons.get(&c), c) {
            (Some(name), _) => format!("[{name}]"),
            (None, '\u{E000}'..='\u{F8FF}') => format!("[icon {:04X}]", c as u32),
            (None, '|') => "/".to_string(),
            (None, '\n') => " ".to_string(),
            (None, c) => c.to_string(),
        })
        .collect::<String>()
        .trim()
        .to_string()
}

fn housing(node: &egui_kittest::Node<'_>) -> Option<String> {
    std::iter::successors(node.parent(), |n| n.parent()).find_map(|n| {
        let inner = n.accesskit_node();
        match inner.role() {
            Role::Window => inner
                .label()
                .filter(|title| !title.is_empty())
                .map(|title| format!("window \"{title}\"")),
            Role::Dialog => Some("dialog".to_string()),
            Role::Menu | Role::ListBox => Some("menu".to_string()),
            Role::Tooltip => Some("tooltip".to_string()),
            _ => None,
        }
    })
}

fn region(rect: Rect, canvas: Rect, menu_bottom: f32, window: Vec2) -> &'static str {
    let c = rect.center();
    match () {
        _ if c.y < menu_bottom => "menu bar",
        _ if c.y > window.y - layout::STATUS => "status line",
        _ if c.y > canvas.bottom() => "timeline",
        _ if c.x < canvas.left() => "tool strip",
        _ if c.x > canvas.right() => "dock",
        _ if c.y < canvas.top() => "options bar",
        _ => "over the canvas",
    }
}

fn value_of(node: &egui_kittest::Node<'_>) -> String {
    let inner = node.accesskit_node();
    let state = inner.toggled().map(|t| {
        match t {
            Toggled::True => "on",
            Toggled::False => "off",
            Toggled::Mixed => "mixed",
        }
        .to_string()
    });
    let greyed = inner.is_disabled().then(|| "greyed out".to_string());
    [
        inner.value(),
        inner.numeric_value().map(|v| format!("{v}")),
        state,
        greyed,
    ]
    .into_iter()
    .flatten()
    .filter(|v| !v.is_empty())
    .collect::<Vec<_>>()
    .join(", ")
}

struct Frame {
    icons: BTreeMap<char, String>,
    canvas: Option<Rect>,
    menu_bottom: f32,
    window: Vec2,
}

impl Frame {
    fn of<S>(h: &Harness<'static, S>, canvas: Option<Rect>) -> Self {
        Self {
            icons: icons(),
            canvas,
            menu_bottom: h
                .query_all_by_label("File")
                .next()
                .map_or(0.0, |n| n.rect().bottom() + 3.0),
            window: h.ctx.content_rect().size(),
        }
    }

    fn place(&self, node: &egui_kittest::Node<'_>) -> Option<String> {
        let around = self
            .canvas
            .map(|canvas| region(node.rect(), canvas, self.menu_bottom, self.window));
        match (housing(node), around) {
            (_, Some("menu bar")) => Some("menu bar".to_string()),
            (Some(place), _) if place == "tooltip" => None,
            (Some(place), _) => Some(plain(&place, &self.icons)),
            (None, Some(around)) => Some(around.to_string()),
            (None, None) => Some("start window".to_string()),
        }
    }
}

fn seen<S>(h: &Harness<'static, S>, canvas: Option<Rect>) -> Vec<Seen> {
    let frame = Frame::of(h, canvas);
    let hints = theme::hints(&h.ctx);
    let found: BTreeSet<Seen> = h
        .query_all(By::new().predicate(|n| CONTROLS.contains(&n.role())))
        .filter_map(|n| {
            let rect = n.rect();
            let place = frame.place(&n)?;
            let inner = n.accesskit_node();
            let hint = hints
                .iter()
                .filter(|(r, _)| r.expand(1.5).contains_rect(rect))
                .min_by(|a, b| a.0.area().total_cmp(&b.0.area()))
                .map_or_else(String::new, |(_, text)| plain(text, &frame.icons));
            Some(Seen {
                place,
                role: format!("{:?}", inner.role()),
                label: plain(&inner.label().unwrap_or_default(), &frame.icons),
                value: plain(&value_of(&n), &frame.icons),
                hint,
            })
        })
        .collect();
    found.into_iter().collect()
}

fn popped<S>(h: &Harness<'static, S>) -> BTreeSet<String> {
    h.query_all(By::new().predicate(|n| n.label().is_some_and(|l| !l.is_empty())))
        .filter(|n| housing(n).is_some_and(|place| place == "dialog" || place == "menu"))
        .filter_map(|n| n.accesskit_node().label())
        .collect()
}

struct Choice {
    place: String,
    name: String,
    options: Vec<String>,
}

fn choices<S>(h: &mut Harness<'static, S>, canvas: Option<Rect>) -> Vec<Choice> {
    let frame = Frame::of(h, canvas);
    let combos = h.query_all(By::new().role(Role::ComboBox)).count();
    (0..combos)
        .filter_map(|i| {
            let before = popped(h);
            let (place, name) = {
                let node = h.query_all(By::new().role(Role::ComboBox)).nth(i)?;
                let place = frame.place(&node)?;
                let name = format!(
                    "{} = {}",
                    plain(&node.accesskit_node().label().unwrap_or_default(), &frame.icons),
                    plain(&node.accesskit_node().value().unwrap_or_default(), &frame.icons)
                );
                node.click();
                (place, name)
            };
            h.run_steps(3);
            let options: Vec<String> = popped(h).difference(&before).map(|l| plain(l, &frame.icons)).collect();
            if let Some(node) = h.query_all(By::new().role(Role::ComboBox)).nth(i) {
                node.click();
            }
            h.run_steps(3);
            if popped(h) != before {
                h.key_press(egui::Key::Escape);
                h.run_steps(3);
            }
            (!options.is_empty()).then_some(Choice { place, name, options })
        })
        .collect()
}

fn cell(text: &str) -> String {
    match text.is_empty() {
        true => " ".to_string(),
        false => text.to_string(),
    }
}

const BRIEF: [&str; 2] = ["settings: Keys", "the command palette"];

fn section(name: &str, how: &str, rows: &[Seen], keep: &[&str], lists: &[Choice]) -> String {
    let kept = |place: &str| {
        keep.is_empty()
            || keep
                .iter()
                .any(|k| place == *k || place.starts_with(&format!("{k} \"")))
    };
    let brief = BRIEF.contains(&name);
    let table: Vec<String> = rows
        .iter()
        .filter(|r| kept(&r.place) && !(brief && r.role == "Button"))
        .map(|r| {
            format!(
                "| {} | {} | {} | {} | {} |",
                r.place,
                r.role,
                cell(&r.label),
                cell(&r.value),
                cell(&r.hint)
            )
        })
        .collect();
    let buttons = rows
        .iter()
        .filter(|r| kept(&r.place) && brief && r.role == "Button")
        .count();
    let lists: Vec<String> = lists
        .iter()
        .filter(|c| kept(&c.place))
        .map(|c| format!("- choices of `{}`: {}", c.name, c.options.join(", ")))
        .chain((buttons > 0).then(|| format!("- and {buttons} buttons, one or more per command, not listed")))
        .collect();
    format!(
        "### {name}\n\n{how}\n\n| where | control | label | value | hint |\n|---|---|---|---|---|\n{}\n{}\n",
        match table.is_empty() {
            true => "| (no controls) | | | | |".to_string(),
            false => table.join("\n"),
        },
        match lists.is_empty() {
            true => String::new(),
            false => format!("\n{}\n", lists.join("\n")),
        }
    )
}

fn shot(
    out: &mut Vec<String>,
    name: &str,
    how: &str,
    pixel: bool,
    keep: &[&str],
    set: impl FnOnce(&mut Harness<'static, Editor>),
) {
    let path = match pixel {
        true => pixel_sample("inventory"),
        false => sample("inventory"),
    };
    let mut h = harness(editor(&path), SIZE);
    if !pixel {
        activate(&mut h, "lines");
    }
    set(&mut h);
    h.run_steps(4);
    let canvas = h.state().canvas_rect;
    let rows = seen(&h, Some(canvas));
    let lists = choices(&mut h, Some(canvas));
    out.push(section(name, how, &rows, keep, &lists));
    let _ = std::fs::remove_dir_all(&path);
}

fn run(h: &mut Harness<'static, Editor>, cmd: Cmd) {
    if let Err(e) = h.state_mut().invoke(cmd) {
        h.state_mut()
            .say(crate::messages::Tone::Error, format!("{cmd:?} did not run: {e:#}"));
    }
    h.run_steps(3);
}

fn only(h: &mut Harness<'static, Editor>, pane: Pane) {
    let layout = &mut h.state_mut().settings.layout;
    layout.panes.iter_mut().for_each(|p| {
        p.open = p.pane == pane;
        p.collapsed = false;
    });
    layout.dock = true;
    layout.timeline = false;
    h.run_steps(3);
}

fn first_shape(h: &Harness<'static, Editor>) -> String {
    let layer = h.state().scene.bundle.doc.find_layer("shapes").unwrap();
    layer.cels()[0].nodes[0].id.clone()
}

fn tools(out: &mut Vec<String>) {
    shot(
        out,
        "brush on a paint layer",
        "B on a paint layer.",
        false,
        &TOOL,
        |h| {
            h.state_mut().tool = Tool::Brush;
        },
    );
    shot(
        out,
        "brush, its more menu",
        "B on a paint layer, then the more button of the options bar.",
        false,
        &["menu", "dialog"],
        |h| {
            h.state_mut().tool = Tool::Brush;
            h.run_steps(2);
            let more = h
                .query_all(By::new().role(Role::Button))
                .find(|n| n.accesskit_node().label().is_some_and(|l| l.ends_with(" more")));
            if let Some(more) = more {
                more.click();
            }
        },
    );
    shot(
        out,
        "eraser on a paint layer",
        "E on a paint layer.",
        false,
        &TOOL,
        |h| {
            run(h, Cmd::Eraser);
        },
    );
    shot(
        out,
        "brush with mirror painting on",
        "B on a paint layer, Tools > Mirror painting.",
        false,
        &TOOL,
        |h| run(h, Cmd::Symmetry),
    );
    shot(
        out,
        "brush on a pixel layer (the pencil)",
        "B on a pixel layer.",
        true,
        &TOOL,
        |h| h.state_mut().tool = Tool::Brush,
    );
    shot(
        out,
        "eraser on a pixel layer",
        "E on a pixel layer.",
        true,
        &TOOL,
        |h| {
            run(h, Cmd::Eraser);
        },
    );
    shot(out, "fill on a paint layer", "G on a paint layer.", false, &TOOL, |h| {
        h.state_mut().tool = Tool::Fill;
    });
    shot(out, "fill on a pixel layer", "G on a pixel layer.", true, &TOOL, |h| {
        h.state_mut().tool = Tool::Fill;
    });
    shot(
        out,
        "select and move, nothing picked",
        "V on a paint layer.",
        false,
        &TOOL,
        |h| h.state_mut().tool = Tool::Select,
    );
    shot(
        out,
        "select and move, a shape picked",
        "V on a vector layer with a rectangle picked: the options bar and the bar at the selection.",
        false,
        &TOOL,
        |h| {
            activate(h, "shapes");
            let shape = first_shape(h);
            h.state_mut().tool = Tool::Select;
            h.state_mut().selection = Some(shape);
        },
    );
    shot(
        out,
        "select strokes, nothing picked",
        "S on a paint layer.",
        false,
        &TOOL,
        |h| h.state_mut().tool = Tool::Strokes,
    );
    shot(
        out,
        "select strokes, strokes picked",
        "S on a paint layer, cmd A: the options bar and the bar at the selection.",
        false,
        &TOOL,
        |h| {
            h.state_mut().tool = Tool::Strokes;
            h.state_mut().select_all_strokes().unwrap();
        },
    );
    shot(
        out,
        "select strokes on a pixel layer, pixels picked",
        "S on a pixel layer, cmd A.",
        true,
        &TOOL,
        |h| {
            h.state_mut().tool = Tool::Strokes;
            h.state_mut().select_all_strokes().unwrap();
        },
    );
    shot(
        out,
        "edit points, points picked",
        "A on a vector layer with two points of a rectangle picked.",
        false,
        &TOOL,
        |h| {
            activate(h, "shapes");
            let node = first_shape(h);
            h.state_mut().tool = Tool::Direct;
            h.state_mut().direct = Some(Direct {
                node,
                points: BTreeSet::from([0, 1]),
                also: Default::default(),
            });
        },
    );
    shot(
        out,
        "pen on a vector layer",
        "P on a vector layer.",
        false,
        &TOOL,
        |h| {
            activate(h, "shapes");
            h.state_mut().tool = Tool::Pen;
        },
    );
    shot(
        out,
        "rectangles and ellipses on a vector layer",
        "U on a vector layer.",
        false,
        &TOOL,
        |h| {
            activate(h, "shapes");
            h.state_mut().tool = Tool::Shape;
        },
    );
    shot(
        out,
        "rectangles and ellipses on a pixel layer",
        "U on a pixel layer.",
        true,
        &TOOL,
        |h| h.state_mut().tool = Tool::Shape,
    );
    shot(
        out,
        "text on a vector layer",
        "T on a vector layer.",
        false,
        &TOOL,
        |h| {
            activate(h, "shapes");
            h.state_mut().tool = Tool::Text;
        },
    );
    shot(
        out,
        "text, while typing",
        "T on a vector layer, a click on the canvas.",
        false,
        &TOOL,
        |h| {
            activate(h, "shapes");
            h.state_mut().tool = Tool::Text;
            h.run_steps(2);
            let _ = h.state_mut().text_down(vec2(200.0, 200.0));
        },
    );
    shot(out, "note", "N.", false, &TOOL, |h| h.state_mut().tool = Tool::Note);
    shot(
        out,
        "bones, no skeleton yet",
        "J on a paint layer in a drawing without bones.",
        false,
        &TOOL,
        |h| h.state_mut().tool = Tool::Bone,
    );
    [
        (Mode::Draw, "bones, draw mode, a bone picked"),
        (Mode::Weights, "bones, weights mode"),
        (Mode::Pose, "bones, pose mode"),
    ]
    .into_iter()
    .for_each(|(mode, name)| {
        shot(
            out,
            name,
            "J, a bone dragged on the canvas, then the mode.",
            false,
            &TOOL,
            |h| {
                h.state_mut().tool = Tool::Bone;
                h.run_steps(2);
                let (a, b) = (canvas_at(h, 40.0, 40.0), canvas_at(h, 170.0, 120.0));
                drag(h, a, b);
                h.run_steps(2);
                h.state_mut().bones.mode = mode;
            },
        );
    });
    shot(
        out,
        "transform the whole layer",
        "cmd T on a paint layer.",
        false,
        &TOOL,
        |h| run(h, Cmd::TransformLayer),
    );
    shot(
        out,
        "brush while painting on a mask",
        "A layer with a mask, Layer > Mask > Paint on mask.",
        false,
        &TOOL,
        |h| {
            activate(h, "shade");
            run(h, Cmd::PaintMask);
        },
    );
}

fn panes(out: &mut Vec<String>) {
    shot(
        out,
        "panel: Colour",
        "Window > Colour, alone in the dock, in a paint drawing.",
        false,
        &DOCK,
        |h| only(h, Pane::Tool),
    );
    shot(
        out,
        "panel: Colour, in a pixel drawing",
        "Window > Colour, alone in the dock, in a pixel drawing.",
        true,
        &DOCK,
        |h| only(h, Pane::Tool),
    );
    shot(
        out,
        "panel: Brush",
        "Window > Brush, alone in the dock.",
        false,
        &DOCK,
        |h| only(h, Pane::Brush),
    );
    shot(
        out,
        "panel: Layers",
        "Window > Layers, alone in the dock.",
        false,
        &DOCK,
        |h| only(h, Pane::Layers),
    );
    shot(
        out,
        "panel: Layer settings, a paint layer",
        "Window > Layer settings on a paint layer, nothing picked.",
        false,
        &DOCK,
        |h| only(h, Pane::Inspector),
    );
    shot(
        out,
        "panel: Layer settings, strokes picked",
        "Layer settings with every stroke of a paint layer picked.",
        false,
        &DOCK,
        |h| {
            only(h, Pane::Inspector);
            h.state_mut().tool = Tool::Strokes;
            h.state_mut().select_all_strokes().unwrap();
        },
    );
    shot(
        out,
        "panel: Layer settings, a layer with a mask",
        "Layer settings on a paint layer that has a mask.",
        false,
        &DOCK,
        |h| {
            only(h, Pane::Inspector);
            activate(h, "shade");
        },
    );
    shot(
        out,
        "panel: Layer settings, a vector layer",
        "Layer settings on a vector layer, nothing picked.",
        false,
        &DOCK,
        |h| {
            only(h, Pane::Inspector);
            activate(h, "shapes");
        },
    );
    shot(
        out,
        "panel: Layer settings, a rectangle picked",
        "Layer settings on a vector layer with a rectangle picked.",
        false,
        &DOCK,
        |h| {
            only(h, Pane::Inspector);
            activate(h, "shapes");
            let shape = first_shape(h);
            h.state_mut().tool = Tool::Select;
            h.state_mut().selection = Some(shape);
        },
    );
    [
        (Cmd::NewGroupLayer, "a group"),
        (Cmd::NewAdjustLayer, "an adjustment layer"),
        (Cmd::NewBonesLayer, "a bones layer"),
        (Cmd::NewMapLayer, "a map layer"),
        (Cmd::NewPixelLayer, "a pixel layer"),
    ]
    .into_iter()
    .for_each(|(cmd, what)| {
        shot(
            out,
            &format!("panel: Layer settings, {what}"),
            &format!("Layer settings right after Layer > New for {what}."),
            false,
            &DOCK,
            |h| {
                only(h, Pane::Inspector);
                run(h, cmd);
            },
        );
    });
    shot(
        out,
        "panel: Notes for the agent",
        "Window > Notes, alone in the dock.",
        false,
        &DOCK,
        |h| only(h, Pane::Notes),
    );
    shot(
        out,
        "panel: History",
        "Window > History, alone in the dock.",
        false,
        &DOCK,
        |h| only(h, Pane::History),
    );
    shot(
        out,
        "panel: Real size",
        "Window > Real size, alone in the dock, in a pixel drawing.",
        true,
        &DOCK,
        |h| only(h, Pane::Preview),
    );
    shot(
        out,
        "panel: Light maps, off",
        "Window > Light maps, alone in the dock, before the maps are turned on.",
        false,
        &DOCK,
        |h| only(h, Pane::Maps),
    );
    shot(
        out,
        "panel: Light maps, on",
        "Window > Light maps with the light maps box ticked.",
        false,
        &DOCK,
        |h| {
            only(h, Pane::Maps);
            if let Some(switch) = h.query_all_by_label("light maps").next() {
                switch.click();
            }
        },
    );
}

fn windows(out: &mut Vec<String>) {
    Section::ALL.into_iter().for_each(|part| {
        shot(
            out,
            &format!("settings: {}", part.name()),
            "cmd , then the section in the sidebar.",
            false,
            &["dialog"],
            |h| {
                h.state_mut().windows.settings = true;
                h.state_mut().settings_section = part;
            },
        );
    });
    shot(
        out,
        "settings: Layout, with developer tools on",
        "Settings > Layout with developer tools ticked.",
        false,
        &["dialog"],
        |h| {
            h.state_mut().settings.developer = true;
            h.state_mut().windows.settings = true;
            h.state_mut().settings_section = Section::Layout;
        },
    );
    [
        (Cmd::BrushWindow, "window: edit the brush", "F5 or Tools > Edit brush…."),
        (Cmd::MessageLog, "window: message log", "Window > Message log."),
        (
            Cmd::OnionSettings,
            "window: onion skin settings",
            "Animation > Onion settings….",
        ),
        (
            Cmd::Link,
            "window: link a drawing here",
            "Frame > Link a drawing here….",
        ),
        (
            Cmd::ClipSettings,
            "window: clip settings",
            "Animation > Clip > Clip settings….",
        ),
        (
            Cmd::ImportPicture,
            "window: import a picture",
            "File > Import picture….",
        ),
        (
            Cmd::Pixelize,
            "window: pixels of a picture",
            "File > Pixels of a picture….",
        ),
        (Cmd::LoadPalette, "window: load a palette", "Tools > Load a palette…."),
        (
            Cmd::TileSize,
            "window: cut into tiles",
            "View > Tileset: cut into tiles….",
        ),
        (Cmd::About, "window: about", "Help > About klad and licences."),
        (
            Cmd::ReportProblem,
            "window: report a problem",
            "Help > Report a problem….",
        ),
        (Cmd::AddFont, "window: add a font", "File > Add font…."),
        (Cmd::PaintOver, "window: paint over", "Layer > Paint over…."),
        (Cmd::FirstSteps, "window: first steps", "Help > First steps."),
        (
            Cmd::Inbetween,
            "window: in-between",
            "Animation > Selected frames > In-between…, nothing selected.",
        ),
    ]
    .into_iter()
    .for_each(|(cmd, name, how)| {
        shot(out, name, how, false, &FLOATING, |h| run(h, cmd));
    });
    [
        (What::Image, "this frame", "File > Export > Image… (cmd E)."),
        (What::Frames, "numbered frames", "File > Export > Frames…"),
        (What::Sheet, "a sprite sheet", "File > Export > Sprite sheet…"),
        (What::Gif, "a GIF", "File > Export > GIF…"),
        (What::Aseprite, "an Aseprite file", "File > Export > Aseprite file…"),
        (What::Godot, "Godot", "File > Export > Godot…"),
    ]
    .into_iter()
    .for_each(|(what, name, how)| {
        shot(out, &format!("window: export, {name}"), how, false, &FLOATING, |h| {
            h.state_mut().windows.export = true;
            h.state_mut().export.what = what;
        });
    });
    shot(
        out,
        "window: export, in a pixel drawing",
        "File > Export > Image… (cmd E) in a pixel drawing.",
        true,
        &FLOATING,
        |h| h.state_mut().windows.export = true,
    );
    shot(
        out,
        "window: in-between, two keys selected",
        "Two drawings selected in the timeline, then Animation > Selected frames > In-between….",
        false,
        &FLOATING,
        |h| {
            let lines = h.state().scene.bundle.doc.find_layer("lines").unwrap().id.clone();
            h.state_mut()
                .apply(parse_lines(&format!(r#"{{"op":"cel","new":"{lines}","frame":6}}"#)).unwrap())
                .unwrap();
            h.state_mut().timeline.span = Some(super::tweens::Span {
                from: 0,
                to: 6,
                rows: vec![lines],
                anchor: (0, 0),
            });
            h.state_mut().windows.between = true;
        },
    );
    shot(
        out,
        "the command palette",
        "cmd K: only its field is listed here, its rows are the commands above.",
        false,
        &["dialog"],
        |h| run(h, Cmd::Palette),
    );
}

fn shell(out: &mut Vec<String>) {
    shot(
        out,
        "the window around the canvas",
        "A paint drawing with the dock closed: the menu bar's titles, the tool strip, the timeline's toolbars and the status line.",
        false,
        &SHELL,
        |h| h.state_mut().settings.layout.dock = false,
    );
    shot(
        out,
        "the timeline with frames selected",
        "Frames 0 to 6 of one layer selected in the timeline.",
        false,
        &["timeline"],
        |h| {
            let lines = h.state().scene.bundle.doc.find_layer("lines").unwrap().id.clone();
            h.state_mut()
                .apply(parse_lines(&format!(r#"{{"op":"cel","new":"{lines}","frame":6}}"#)).unwrap())
                .unwrap();
            h.state_mut().timeline.span = Some(super::tweens::Span {
                from: 0,
                to: 6,
                rows: vec![lines],
                anchor: (0, 0),
            });
            h.state_mut().settings.layout.dock = false;
        },
    );
    shot(
        out,
        "the window around a pixel drawing",
        "A pixel drawing with the dock closed.",
        true,
        &SHELL,
        |h| h.state_mut().settings.layout.dock = false,
    );
}

fn start(out: &mut Vec<String>) {
    let folder = std::env::temp_dir().join(format!("klad-inventory-start-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    let home = |welcomed: bool| {
        let settings = Settings {
            drawings: Some(folder.display().to_string()),
            check_updates: false,
            welcomed,
            ..Settings::default()
        };
        let mut home = Home::with(settings, None, None);
        home.begin(&egui::Context::default(), None);
        let mut h = Harness::builder()
            .with_size(SIZE)
            .build_ui_state(|ui, home: &mut Home| home.frame(ui), home);
        h.run_steps(4);
        h
    };
    let mut record = |name: &str, how: &str, h: &mut Harness<'static, Home>| {
        h.run_steps(4);
        let rows: Vec<Seen> = seen(h, None)
            .into_iter()
            .map(|r| Seen {
                place: "start window".to_string(),
                ..r
            })
            .collect();
        let lists: Vec<Choice> = choices(h, None)
            .into_iter()
            .map(|c| Choice {
                place: "start window".to_string(),
                ..c
            })
            .collect();
        out.push(section(name, how, &rows, &[], &lists));
    };
    let mut h = home(false);
    h.run_steps(60);
    record(
        "welcome: the first launch",
        "klad started with fresh settings: the welcome's first screen, once the K is painted.",
        &mut h,
    );
    let mut h = home(true);
    record(
        "start window: new drawing",
        "klad started after the welcome was dismissed.",
        &mut h,
    );
    if let Some(start) = h.state_mut().start.as_mut() {
        start.pixel = true;
    }
    record(
        "start window: new pixel drawing",
        "The pixel art choice of a new drawing.",
        &mut h,
    );
    let _ = std::fs::remove_dir_all(&folder);
}

fn flattened(prefix: &str, value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) if !map.is_empty() => map.iter().for_each(|(key, inner)| {
            let path = match prefix.is_empty() {
                true => key.clone(),
                false => format!("{prefix}.{key}"),
            };
            flattened(&path, inner, out);
        }),
        other => out.push(format!("| `{prefix}` | `{}` |", other.to_string().replace('|', "/"))),
    }
}

fn settings_file() -> String {
    let mut rows = Vec::new();
    flattened("", &serde_json::to_value(Settings::default()).unwrap(), &mut rows);
    format!(
        "### the settings file\n\nEvery key a fresh settings file holds, with its starting value (keys left out while empty are not listed).\n\n| key | starts as |\n|---|---|\n{}\n",
        rows.join("\n")
    )
}

#[test]
#[ignore]
fn editor_inventory() {
    let out = std::env::var("KLAD_INVENTORY_OUT").expect("set KLAD_INVENTORY_OUT to the file to write");
    let mut parts = vec![format!(
        "## Every control, by where it shows\n\nMade by opening the editor without a window at {} by {} and reading its controls: the place, the kind of control, its label (icons by name in square brackets), its value and its hover hint. Repeated rows (one per layer or frame) are listed once.\n",
        SIZE.x, SIZE.y
    )];
    shell(&mut parts);
    tools(&mut parts);
    panes(&mut parts);
    windows(&mut parts);
    start(&mut parts);
    parts.push(settings_file());
    std::fs::write(out, parts.join("\n")).unwrap();
}
