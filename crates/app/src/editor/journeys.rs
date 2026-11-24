use super::testkit::{Journey, Rows};
use super::{Editor, Tool};
use egui::{Key, Modifiers, vec2};
use egui_kittest::kittest::{NodeT, Queryable};
use klad_core::stroke::Mark;

const WINDOW: egui::Vec2 = vec2(1440.0, 900.0);
const CMD: Modifiers = Modifiers::COMMAND;
const CMD_SHIFT: Modifiers = Modifiers {
    alt: false,
    ctrl: false,
    shift: true,
    mac_cmd: false,
    command: true,
};

fn marks(j: &Journey) -> usize {
    j.ed().map_or(0, |ed| ed.scene.active_marks().len())
}

fn zoom(j: &Journey) -> f32 {
    j.ed().and_then(|ed| ed.view).map_or(0.0, |v| v.zoom)
}

fn centre(j: &Journey) -> egui::Vec2 {
    j.ed().and_then(|ed| ed.view).map_or(egui::Vec2::ZERO, |v| v.center)
}

fn last_brush(ed: &Editor) -> String {
    ed.scene
        .active_marks()
        .last()
        .and_then(|m| m.as_stroke())
        .map_or_else(String::new, |s| s.brush.clone())
}

fn last_colour(ed: &Editor) -> [u16; 4] {
    ed.scene
        .active_marks()
        .last()
        .and_then(|m| m.as_stroke())
        .map_or([0; 4], |s| s.color)
}

fn pngs(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|x| x == "png"))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn the_first_ten_minutes_of_someone_who_draws() {
    let mut j = Journey::first_launch("first-ten-minutes-draw", WINDOW);

    j.act("klad starts for the first time: the welcome");
    j.settle();
    let welcomed = j.shown("Get started");
    j.expect("welcome-first", welcomed, "the first start opens on the welcome");
    j.look("welcome");

    j.act("someone who draws by hand goes through the welcome and skips the agent and the first steps");
    ["Get started", "Skip", "Skip"].into_iter().for_each(|button| {
        j.click(button);
        j.settle();
    });
    j.look("start");
    let can_start = j.shown("Name") && j.shown("Create");
    j.expect("start-offers-new", can_start, "the start window offers a new drawing");

    j.act("make a drawing called mushroom, typing the name into the field as it is");
    j.type_into("Name", "mushroom");
    j.click("Create");
    j.look("new-drawing");
    let open = j.ed().is_some();
    j.expect("new-opens", open, "the new drawing opens in the editor");
    if !open {
        return j.finish(&[]);
    }
    let named = j.folder.join("mushroom.klad").exists();
    j.expect("name-typed-replaces", named, "the drawing is called mushroom, as typed");
    let brush = j.ed().is_some_and(|ed| ed.tool == Tool::Brush);
    j.expect("starts-on-brush", brush, "the brush is the tool in hand");

    j.act("paint three strokes");
    j.drag(&[(300.0, 300.0), (500.0, 220.0), (700.0, 320.0)]);
    j.drag(&[(320.0, 420.0), (680.0, 430.0)]);
    j.drag(&[(500.0, 240.0), (500.0, 600.0)]);
    j.look("three-strokes");
    j.expect("strokes-land", marks(&j) == 3, "three strokes are on the drawing");

    j.act("pick red by typing its hex, then paint");
    j.retype("hex", "#d03030");
    j.drag(&[(900.0, 300.0), (1200.0, 700.0)]);
    j.look("red-stroke");
    let red = j.ed().map(last_colour).is_some_and(|c| c[0] > 40000 && c[1] < 20000);
    j.expect(
        "hex-sets-colour",
        red,
        "the stroke painted after typing a hex colour is that colour",
    );

    j.act("pick the red stroke with V and drag it");
    j.key(Key::V);
    let before = j
        .ed()
        .and_then(|ed| ed.scene.active_marks().last().and_then(|m| m.bounds()));
    j.drag(&[(1050.0, 500.0), (1250.0, 560.0)]);
    j.look("moved");
    let after = j
        .ed()
        .and_then(|ed| ed.scene.active_marks().last().and_then(|m| m.bounds()));
    j.note(format!("the red stroke's box went from {before:?} to {after:?}"));
    let moved = matches!((before, after), (Some(a), Some(b)) if (b[0] - a[0]).abs() > 50.0);
    j.expect(
        "v-drags-a-stroke",
        moved,
        "V picks the stroke under the pointer and drags it",
    );

    j.act("cut the picked stroke with cmd X, take the cut back with cmd Z and pick the stroke again");
    let before = marks(&j);
    j.chord(CMD, Key::X);
    j.look("cut");
    j.expect("cmd-x-cuts", marks(&j) + 1 == before, "cmd X cuts what is picked");
    j.chord(CMD, Key::Z);
    j.look("cut-undone");
    j.expect(
        "undo-takes-the-cut-back",
        marks(&j) == before,
        "cmd Z brings the cut stroke back",
    );
    j.drag(&[(1250.0, 560.0), (1250.0, 560.0)]);

    j.act("delete the picked stroke with Backspace, then undo");
    let before = marks(&j);
    j.key(Key::Backspace);
    j.look("deleted");
    j.expect(
        "backspace-deletes",
        marks(&j) + 1 == before,
        "Backspace deletes what is picked",
    );
    j.chord(CMD, Key::Z);
    j.look("delete-undone");
    j.expect(
        "undo-brings-back",
        marks(&j) == before,
        "cmd Z brings the deleted stroke back",
    );
    j.key(Key::Escape);
    j.key(Key::B);

    j.act("pick a colour by clicking on the colour wheel, then paint");
    let before = j.ed().map(|ed| ed.color);
    j.click_inside("colour wheel", 0.5, 0.06);
    j.drag(&[(900.0, 800.0), (1300.0, 900.0)]);
    j.look("wheel-colour");
    let changed = j.ed().map(|ed| ed.color) != before;
    j.expect(
        "wheel-sets-colour",
        changed,
        "a click on the colour wheel changes the colour",
    );

    j.act("make the brush bigger with ] three times, then paint");
    let size = j.ed().map_or(0.0, |ed| ed.size);
    (0..3).for_each(|_| j.key(Key::CloseBracket));
    j.drag(&[(300.0, 900.0), (700.0, 1000.0)]);
    j.look("bigger-brush");
    let grew = j.ed().is_some_and(|ed| ed.size > size);
    j.expect("bracket-sizes-brush", grew, "] makes the brush bigger");

    j.act("erase across the first strokes with E");
    let painted = marks(&j);
    j.key(Key::E);
    j.drag(&[(400.0, 200.0), (600.0, 650.0)]);
    j.look("erased");
    let erased = j.ed().map(last_brush).unwrap_or_default();
    j.note(format!("the mark it made uses the brush \"{erased}\""));
    j.expect(
        "e-erases",
        marks(&j) == painted + 1 && erased.contains("eraser"),
        "E erases",
    );

    j.act("go back to the brush with B and paint");
    j.key(Key::B);
    j.drag(&[(1300.0, 300.0), (1700.0, 500.0)]);
    j.look("brush-again");
    let back = j.ed().map(last_brush).unwrap_or_default();
    j.note(format!("the mark it made uses the brush \"{back}\""));
    j.expect(
        "b-paints-again",
        !back.contains("eraser"),
        "B brings the brush back after the eraser",
    );

    j.act("undo twice with cmd Z, redo once with cmd shift Z");
    let before = marks(&j);
    j.chord(CMD, Key::Z);
    j.chord(CMD, Key::Z);
    j.look("undone");
    j.expect(
        "cmd-z-undoes",
        marks(&j) + 2 == before,
        "cmd Z twice takes two strokes away",
    );
    j.chord(CMD_SHIFT, Key::Z);
    j.look("redone");
    j.expect(
        "cmd-shift-z-redoes",
        marks(&j) + 1 == before,
        "cmd shift Z brings one back",
    );

    j.act("redo with cmd Y, the Windows habit");
    j.chord(CMD, Key::Y);
    j.look("redo-y");
    j.expect("cmd-y-redoes", marks(&j) == before, "cmd Y redoes");

    j.act("zoom in with cmd +");
    let fit = zoom(&j);
    j.chord(CMD, Key::Plus);
    j.chord(CMD, Key::Equals);
    j.look("cmd-plus");
    j.expect("cmd-plus-zooms", zoom(&j) > fit, "cmd + zooms the canvas in");

    j.act("zoom out with cmd -");
    let zoomed = zoom(&j);
    j.chord(CMD, Key::Minus);
    j.look("cmd-minus");
    j.expect("cmd-minus-zooms", zoom(&j) < zoomed, "cmd - zooms the canvas out");

    j.act("zoom with cmd and scroll, then fit with cmd 0");
    j.wheel(1000.0, 1000.0, vec2(0.0, 120.0), CMD);
    j.look("cmd-scroll");
    let zoomed = zoom(&j);
    j.expect(
        "cmd-scroll-zooms",
        (zoomed - fit).abs() > 1e-3,
        "cmd and scroll zooms the canvas",
    );
    j.chord(CMD, Key::Num0);
    j.look("cmd-zero");
    j.expect(
        "cmd-zero-fits",
        (zoom(&j) - fit).abs() < 1e-3,
        "cmd 0 fits the canvas in the window",
    );

    j.act("zoom with a pinch, pan with a two-finger scroll, fit with F");
    let z = zoom(&j);
    j.pinch(1000.0, 1000.0, 1.5);
    j.look("pinched");
    j.expect("pinch-zooms", zoom(&j) > z, "a pinch zooms");
    let c = centre(&j);
    j.wheel(1000.0, 1000.0, vec2(-60.0, -40.0), Modifiers::NONE);
    j.look("scrolled");
    j.expect("scroll-pans", centre(&j) != c, "a two-finger scroll pans");
    j.key(Key::F);
    j.look("fit");
    j.expect(
        "f-fits",
        (zoom(&j) - fit).abs() < 1e-3,
        "F fits the canvas in the window",
    );

    j.act("right click on the canvas");
    j.tap_with(1000.0, 1000.0, egui::PointerButton::Secondary, Modifiers::NONE);
    j.look("right-click");
    j.expect(
        "right-click-menu",
        j.shown("Move to new layer") && j.shown("Select all"),
        "a right click on the canvas opens a menu",
    );
    j.key(Key::Escape);

    j.act("save with cmd S");
    let heard = j.ed().and_then(|ed| ed.messages.latest().map(|m| m.id));
    j.chord(CMD, Key::S);
    j.look("saved");
    let told = j.ed().and_then(|ed| ed.messages.latest().map(|m| m.id)) != heard;
    j.expect(
        "cmd-s-answers",
        told,
        "cmd S gets an answer, such as that klad saves as you go",
    );

    j.act("add a layer from the Layer menu and paint on it");
    let layers = j.ed().map_or(0, |ed| ed.scene.bundle.doc.layers.len());
    j.click("Layer");
    j.click("New paint layer");
    j.look("new-layer");
    let added = j.ed().map_or(0, |ed| ed.scene.bundle.doc.layers.len()) == layers + 1;
    j.expect("layer-menu-adds", added, "Layer > New paint layer adds a layer");
    j.drag(&[(1400.0, 1500.0), (1800.0, 1700.0)]);
    j.look("painted-on-new-layer");
    j.expect(
        "new-layer-takes-paint",
        marks(&j) == 1,
        "the next stroke lands on the new layer",
    );
    let rows =
        j.h.query_all_by_label("paint 1")
            .chain(j.h.query_all_by_label("paint 2"))
            .filter(|n| n.rect().left() > 1040.0 && n.rect().bottom() < 540.0)
            .count();
    j.note(format!("layer rows in sight in the Layers panel: {rows}"));
    j.expect(
        "layers-panel-shows-layers",
        rows == 2,
        "the Layers panel shows both paint layers in a 1440 by 900 window",
    );

    j.act("export a PNG from File > Export > Image");
    j.menu("File", "Export", "Image… cmd E");
    j.look("export-window");
    j.click("Export");
    j.look("exported");
    let written: Vec<String> = pngs(&j.folder)
        .iter()
        .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .collect();
    j.note(format!("PNG files next to the drawing: {written:?}"));
    j.expect(
        "export-writes-png",
        written.len() == 1,
        "export writes one PNG next to the drawing",
    );
    let internal = j.ed().is_some_and(|ed| {
        ed.messages
            .latest()
            .is_some_and(|m| m.text.contains(" ms") || m.text.contains("dithered"))
    });
    j.expect(
        "export-says-it-plainly",
        !internal,
        "the export message says where the file is without render times",
    );
    j.key(Key::Escape);

    j.act("press + and then - without cmd, as someone reaching for zoom would");
    let frames = j.ed().map_or(0, |ed| ed.scene.bundle.doc.timeline.frames);
    j.key(Key::Equals);
    j.look("plain-plus");
    let after = j.ed().map_or(0, |ed| ed.scene.bundle.doc.timeline.frames);
    j.note(format!("the timeline was {frames} frames long and is now {after}"));
    j.expect(
        "plain-plus-leaves-the-drawing",
        after == frames,
        "+ without cmd does not change the drawing",
    );
    j.key(Key::Minus);
    j.look("plain-minus");
    let back = j.ed().map_or(0, |ed| ed.scene.bundle.doc.timeline.frames);
    j.note(format!("after - the timeline is {back} frames long"));
    j.expect("minus-takes-plus-back", back == frames, "- takes back what + did");

    j.act("type a new hex and press Return while the drawing has several frames");
    j.retype("hex", "#208040");
    j.look("return-in-a-field");
    let playing = j.ed().is_some_and(|ed| ed.timeline.player.is_some());
    j.expect(
        "return-in-a-field-stays-there",
        !playing,
        "Return in a text field only confirms the field and does not start playback",
    );
    if playing {
        j.key(Key::Enter);
    }

    j.act("close the drawing with cmd W and open it again from the recent list");
    let total = j.ed().map_or(0, |ed| ed.scene.total_marks());
    j.chord(CMD, Key::W);
    j.look("closed");
    j.expect(
        "close-goes-to-start",
        j.ed().is_none(),
        "cmd W goes back to the start window",
    );
    j.click_containing("mushroom");
    j.look("reopened");
    let kept = j.ed().map_or(0, |ed| ed.scene.total_marks());
    j.note(format!("{total} marks before closing, {kept} after opening it again"));
    j.expect(
        "work-is-kept",
        j.ed().is_some() && kept == total,
        "the drawing opens again from the recent list with everything in it",
    );

    j.finish(&["layers-panel-shows-layers"]);
}

fn spot(j: &Journey, pick: impl Fn(&klad_core::stroke::Mark) -> bool) -> Option<(u32, f32, f32)> {
    j.ed().and_then(|ed| {
        ed.scene.active_marks().iter().filter(|m| pick(m)).find_map(|m| {
            let stroke = m.as_stroke()?;
            let (x, y) = stroke.samples.get(stroke.samples.len() / 2)?.pos();
            Some((m.id(), x, y))
        })
    })
}

fn bounds_of(j: &Journey, id: u32) -> Option<[f32; 4]> {
    j.ed().and_then(|ed| {
        ed.scene
            .active_marks()
            .iter()
            .find(|m| m.id() == id)
            .and_then(|m| m.bounds())
    })
}

fn colour_of(j: &Journey, id: u32) -> Option<[u16; 4]> {
    j.ed().and_then(|ed| {
        ed.scene
            .active_marks()
            .iter()
            .find(|m| m.id() == id)
            .and_then(|m| m.as_stroke())
            .map(|s| s.color)
    })
}

fn picked(j: &Journey) -> usize {
    j.ed().and_then(|ed| ed.picked.as_ref()).map_or(0, |p| p.ids.len())
}

#[test]
fn the_first_ten_minutes_of_someone_who_edits_what_an_agent_painted() {
    let mut j = Journey::begin("first-ten-minutes-edit", WINDOW, Some("lantern.klad"));

    j.act("open the lantern an agent painted");
    j.look("opened");
    let open = j.ed().is_some();
    j.expect("drawing-opens", open, "the drawing opens");
    if !open {
        return j.finish(&[]);
    }

    j.act("go to the lines layer by clicking its name");
    j.click("lines");
    j.look("lines-layer");
    let on_lines = j
        .ed()
        .and_then(|ed| ed.scene.active_layer().map(|l| l.name.clone()))
        .is_some_and(|n| n == "lines");
    j.expect(
        "layer-click-activates",
        on_lines,
        "a click on a layer's name makes it the layer in hand",
    );
    let Some((id, x, y)) = spot(&j, |m| m.as_stroke().is_some_and(|s| s.samples.len() > 8)) else {
        j.note("no stroke to work on");
        return j.finish(&[]);
    };

    j.act("click a line with the tool in hand, as someone who wants to select it");
    let before = j.ed().map_or(0, |ed| ed.scene.total_marks());
    let tool = j.ed().map(|ed| format!("{:?}", ed.tool)).unwrap_or_default();
    j.tap(x, y);
    j.look("first-click");
    let after = j.ed().map_or(0, |ed| ed.scene.total_marks());
    j.note(format!(
        "the tool in hand was {tool}; the drawing had {before} marks and has {after}"
    ));
    j.expect(
        "first-click-leaves-the-drawing",
        after == before,
        "a first click on someone else's drawing does not paint on it",
    );
    if after != before {
        j.chord(CMD, Key::Z);
    }

    j.act("pick that line with V");
    j.key(Key::V);
    j.tap(x, y);
    j.look("picked");
    j.expect(
        "v-click-picks",
        picked(&j) == 1,
        "V and a click pick the line under the pointer",
    );
    let id = j
        .ed()
        .and_then(|ed| ed.picked.as_ref().and_then(|p| p.ids.first().copied()))
        .unwrap_or(id);

    j.act("give it another colour by choosing the colour while it is picked");
    let was = colour_of(&j, id);
    j.retype("hex", "#d03030");
    j.look("colour-while-picked");
    j.expect(
        "colour-applies-to-selection",
        colour_of(&j, id) != was,
        "choosing a colour recolours what is picked",
    );
    let playing = j.ed().is_some_and(|ed| ed.timeline.player.is_some());
    j.expect(
        "return-in-a-field-stays-there",
        !playing,
        "Return in a text field only confirms the field and does not start playback",
    );
    if playing {
        j.key(Key::Enter);
    }

    j.act("recolour it from Edit > Strokes > Recolour");
    let sub = j.menu("Edit", "Strokes", "Recolour");
    j.look("recoloured");
    j.expect(
        "recolour-command-works",
        sub && colour_of(&j, id) != was,
        "Edit > Strokes > Recolour recolours it",
    );

    j.act("nudge it with the arrow keys");
    let was = bounds_of(&j, id);
    j.key(Key::ArrowRight);
    j.chord(Modifiers::SHIFT, Key::ArrowDown);
    j.look("nudged");
    let now = bounds_of(&j, id);
    j.note(format!("its box went from {was:?} to {now:?}"));
    let nudged =
        matches!((was, now), (Some(a), Some(b)) if (b[0] - a[0] - 1.0).abs() < 0.6 && (b[1] - a[1] - 10.0).abs() < 0.6);
    j.expect("arrows-nudge", nudged, "the arrows move it 1 px, and 10 px with shift");

    j.act("drag it somewhere else");
    let was = bounds_of(&j, id);
    let from = was.map_or((x, y), |b| ((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0));
    let (px, py) = spot(&j, |m| m.id() == id).map_or(from, |(_, x, y)| (x, y));
    j.drag(&[(px, py), (px + 120.0, py - 80.0)]);
    j.look("dragged");
    let now = bounds_of(&j, id);
    let dragged = matches!((was, now), (Some(a), Some(b)) if (b[0] - a[0]).abs() > 60.0);
    j.expect("drag-moves", dragged, "a drag moves the picked line");

    j.act("copy it with cmd C and paste it with cmd V");
    let before = marks(&j);
    j.chord(CMD, Key::C);
    j.chord(CMD, Key::V);
    j.look("pasted");
    j.expect("copy-paste", marks(&j) == before + 1, "cmd C then cmd V makes a copy");

    j.act("duplicate with cmd D, then delete the copy with Delete");
    let before = marks(&j);
    j.chord(CMD, Key::D);
    j.look("duplicated");
    j.expect(
        "cmd-d-duplicates",
        marks(&j) == before + 1,
        "cmd D duplicates what is picked",
    );
    j.key(Key::Delete);
    j.look("deleted");
    j.expect("delete-deletes", marks(&j) == before, "Delete removes what is picked");

    j.act("select everything with cmd A, then nothing with Esc");
    j.chord(CMD, Key::A);
    j.look("all-picked");
    let all = picked(&j);
    j.expect("cmd-a-picks-all", all > 1, "cmd A picks every stroke on the layer");
    j.key(Key::Escape);
    j.look("none-picked");
    j.expect("esc-clears", picked(&j) == 0, "Esc clears the selection");

    j.act("drag a loop around part of the drawing to pick several lines");
    j.drag(&[
        (300.0, 300.0),
        (760.0, 300.0),
        (760.0, 700.0),
        (300.0, 700.0),
        (300.0, 310.0),
    ]);
    j.look("looped");
    j.note(format!("{} lines picked by the loop", picked(&j)));
    j.expect(
        "loop-picks",
        picked(&j) >= 1,
        "a loop dragged on the canvas picks the lines inside",
    );
    j.key(Key::Escape);

    j.act("hide the lines layer from Layer > Show, and show it again");
    j.click("Layer");
    j.click("Show");
    j.look("hidden");
    let hidden = j.ed().and_then(|ed| ed.scene.active_layer().map(|l| !l.visible));
    j.expect("layer-hides", hidden == Some(true), "Layer > Show hides the layer");
    j.click("Layer");
    j.click("Show");

    j.act("undo five times and redo five times");
    let before = j.ed().map_or(0, |ed| ed.scene.total_marks());
    (0..5).for_each(|_| j.chord(CMD, Key::Z));
    j.look("undone");
    (0..5).for_each(|_| j.chord(CMD_SHIFT, Key::Z));
    j.look("redone");
    let after = j.ed().map_or(0, |ed| ed.scene.total_marks());
    j.expect(
        "undo-redo-round-trip",
        after == before,
        "five undos and five redos end where they began",
    );

    j.finish(&["first-click-leaves-the-drawing", "colour-applies-to-selection"]);
}

#[test]
fn the_first_ten_minutes_of_someone_who_makes_pixel_art() {
    let mut j = Journey::first_launch("first-ten-minutes-pixel", WINDOW);

    j.act("klad starts for the first time; leave the welcome with Esc");
    j.settle();
    j.key(Key::Escape);
    j.settle();
    let left = !j.shown("Get started") && j.shown("Pixel art");
    j.expect("welcome-escape", left, "Esc leaves the welcome for the start window");

    j.act("choose pixel art");
    j.click("Pixel art");
    j.look("pixel-start");

    j.act("make a sprite called slime");
    j.retype("Name", "slime");
    j.click("Create");
    j.look("new-sprite");
    let open = j.ed().is_some();
    j.expect("sprite-opens", open, "the new sprite opens in the editor");
    if !open {
        return j.finish(&[]);
    }
    let (w, h) = j.ed().map_or((0, 0), |ed| ed.scene.size());
    j.note(format!("the sprite is {w} by {h}"));
    let pixel = j
        .ed()
        .and_then(|ed| ed.scene.active_layer().map(|l| l.pixel()))
        .unwrap_or(false);
    j.expect("starts-on-a-pixel-layer", pixel, "the layer in hand is a pixel layer");

    j.act("draw a few pixels");
    let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
    j.drag(&[(cx - 6.0, cy), (cx + 6.0, cy)]);
    j.drag(&[(cx, cy - 6.0), (cx, cy + 6.0)]);
    j.look("pixels");
    j.expect("pixels-land", marks(&j) >= 1, "dragging draws pixels");
    let ring = j.ed().and_then(|ed| ed.ring());
    let pencil = j.ed().map_or(0, |ed| ed.pixel_size);
    j.note(format!(
        "the pencil is {pencil} px wide; the brush ring around the pointer: {ring:?}"
    ));
    j.expect(
        "pixel-pointer-is-the-pencil",
        ring.is_none(),
        "on a pixel layer the pointer outlines the pencil's pixels, with no brush ring",
    );

    j.act("pick a green by its hex and draw with it");
    let swatches = j.ed().map_or(0, |ed| ed.scene.bundle.doc.palette.len());
    j.retype("hex", "#40a060");
    let playing = j.ed().is_some_and(|ed| ed.timeline.player.is_some());
    if playing {
        j.key(Key::Enter);
    }
    j.drag(&[(cx - 10.0, cy + 10.0), (cx + 10.0, cy + 10.0)]);
    j.look("green-pixels");
    let now = j.ed().map_or(0, |ed| ed.scene.bundle.doc.palette.len());
    j.note(format!("the palette had {swatches} colours and has {now}"));
    j.expect(
        "new-colour-joins-palette",
        now == swatches + 1,
        "a colour drawn with joins the palette",
    );

    j.act("erase a few pixels with E, then fill an area with G");
    j.key(Key::E);
    j.drag(&[(cx - 3.0, cy), (cx + 3.0, cy)]);
    j.look("erased");
    j.key(Key::G);
    j.tap(cx - 20.0, cy - 20.0);
    j.look("filled");
    let tool = j.ed().map(|ed| ed.tool);
    j.expect(
        "g-fills",
        tool == Some(Tool::Fill) && marks(&j) >= 2,
        "G fills the area that is clicked",
    );
    j.chord(CMD, Key::Z);

    j.act("zoom with a pinch");
    j.key(Key::B);
    j.pinch(cx, cy, 1.6);
    j.look("pinched");
    let px = zoom(&j) * j.h.ctx.pixels_per_point();
    j.note(format!("a pixel of the sprite is {px:.2} screen pixels wide"));
    j.expect(
        "zoom-in-whole-steps",
        (px - px.round()).abs() < 1e-3,
        "the zoom stays on whole steps",
    );

    j.act("select some pixels by dragging a box with S and nudge them");
    j.key(Key::S);
    j.drag(&[(cx - 8.0, cy - 8.0), (cx + 8.0, cy + 8.0)]);
    j.look("boxed");
    j.note(format!("{} marks picked", picked(&j)));
    j.expect("box-picks-pixels", picked(&j) >= 1, "a box dragged with S picks pixels");
    j.key(Key::ArrowRight);
    j.look("nudged");
    j.key(Key::Escape);

    j.act("look in the Frame menu for a new frame");
    j.click("Frame");
    j.look("frame-menu");
    j.key(Key::Escape);

    j.act("add a frame after this one with shift K and draw on it");
    j.key(Key::B);
    let frames = j.ed().map_or(0, |ed| ed.scene.bundle.doc.timeline.frames);
    j.chord(Modifiers::SHIFT, Key::K);
    j.look("second-frame");
    let now = j.ed().map_or(0, |ed| ed.scene.bundle.doc.timeline.frames);
    j.note(format!("the timeline was {frames} frames long and is now {now}"));
    j.expect(
        "shift-k-adds-a-frame",
        now > frames,
        "shift K adds a frame after this one",
    );
    j.drag(&[(cx - 6.0, cy - 12.0), (cx + 6.0, cy - 12.0)]);
    j.look("drawn-on-second-frame");

    j.act("play with the space bar, then with Return");
    j.key(Key::Space);
    j.look("space");
    let space = j.ed().is_some_and(|ed| ed.timeline.player.is_some());
    j.expect("space-plays", space, "the space bar plays the animation");
    if !space {
        j.key(Key::Enter);
    }
    j.look("playing");
    let playing = j.ed().is_some_and(|ed| ed.timeline.player.is_some());
    j.expect("return-plays", playing, "Return plays the animation");
    j.key(Key::Enter);

    j.act("export a GIF from File > Export > GIF");
    j.menu("File", "Export", "GIF…");
    j.look("export-gif");
    j.click("Export");
    j.look("exported-gif");
    let gifs: Vec<(String, u16, u16)> = std::fs::read_dir(&j.folder)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|x| x == "gif"))
                .filter_map(|p| {
                    let bytes = std::fs::read(&p).ok()?;
                    let side = |at: usize| u16::from_le_bytes([bytes[at], bytes[at + 1]]);
                    Some((p.file_name()?.to_string_lossy().to_string(), side(6), side(8)))
                })
                .collect()
        })
        .unwrap_or_default();
    j.note(format!("GIF files next to the drawing, with their sizes: {gifs:?}"));
    j.expect(
        "gif-exports",
        gifs.len() == 1,
        "export writes one GIF next to the drawing",
    );

    j.finish(&["space-plays"]);
}

#[test]
fn what_a_cold_first_session_tripped_on() {
    let mut j = Journey::begin("cold-session", WINDOW, None);

    j.act("klad starts; make a drawing");
    j.type_into("Name", "house");
    j.click("Create");
    j.look("new-drawing");
    if j.ed().is_none() {
        return j.finish(&[]);
    }

    j.act("click once on the canvas with the brush, without dragging");
    j.tap(600.0, 600.0);
    j.look("dot");
    let dot = j
        .ed()
        .and_then(|ed| ed.scene.active_marks().last().and_then(|m| m.bounds()));
    j.note(format!("the click left a mark with the box {dot:?}"));
    j.expect(
        "a-click-paints-a-dot",
        dot.is_some_and(|b| b[2] - b[0] > 2.0),
        "one click with the brush leaves a dot as wide as the brush",
    );

    j.act("hold alt over the paint, as someone who wants to pick its colour");
    j.hover_with(600.0, 600.0, Modifiers::ALT);
    j.look("alt-held");
    j.let_go();

    j.act("double click the layer's name and type a new one");
    j.double_click("paint 1");
    j.look("rename-started");
    j.type_text("roof");
    j.key(Key::Enter);
    j.look("renamed");
    let name = j
        .ed()
        .and_then(|ed| ed.scene.active_layer().map(|l| l.name.clone()))
        .unwrap_or_default();
    j.note(format!("the layer is now called \"{name}\""));
    j.expect(
        "double-click-renames-by-typing",
        name == "roof",
        "a double click on a layer's name selects it, so typing replaces it",
    );

    j.act("look for other ways to mix a colour for a shadow");
    j.click("Lab");
    j.look("colour-lab");
    let lab =
        j.h.query_all_by_role_and_label(egui::accesskit::Role::Slider, "L")
            .count()
            == 1
            && j.h
                .query_all_by_role_and_label(egui::accesskit::Role::Slider, "b")
                .count()
                == 1;
    j.expect(
        "colour-models",
        lab,
        "the Colour panel mixes in HSL, RGB and Lab as well as on the wheel",
    );
    j.click("HSV");

    j.act("draw a roof, select its strokes and right click to split them off");
    j.drag(&[(300.0, 500.0), (800.0, 200.0)]);
    j.drag(&[(800.0, 200.0), (1300.0, 500.0)]);
    let layers = j.ed().map_or(0, |ed| ed.scene.bundle.doc.all_layers().len());
    j.key(Key::S);
    j.chord(CMD, Key::A);
    j.tap_with(1500.0, 1500.0, egui::PointerButton::Secondary, Modifiers::NONE);
    j.look("right-click-with-a-selection");
    j.click("Move to new layer");
    j.look("moved-to-new-layer");
    let split = j.ed().is_some_and(|ed| {
        ed.scene.bundle.doc.all_layers().len() == layers + 1
            && ed.scene.active_layer().is_some_and(|l| l.name == "roof part")
            && ed.scene.active_marks().len() == 3
    });
    j.expect(
        "right-click-moves-to-new-layer",
        split,
        "right click > Move to new layer puts the selected strokes on a layer of their own",
    );

    j.act("recolour them with the button at the selection");
    let was = j.ed().and_then(|ed| ed.scene.active_marks().first().map(|m| m.color()));
    j.click("the colour of the selected strokes");
    j.look("recolour-picker");
    let picker = j.h.query_all_by_label("colour wheel").count() == 2;
    j.expect(
        "recolour-opens-a-picker",
        picker && j.ed().and_then(|ed| ed.scene.active_marks().first().map(|m| m.color())) == was,
        "the recolour button opens a colour picker at the selection and changes nothing before a choice",
    );
    j.key(Key::Escape);

    j.act("draw a rectangle with U while the paint layer is the active one");
    j.key(Key::U);
    j.drag(&[(400.0, 700.0), (1200.0, 1500.0)]);
    j.look("shape-on-a-paint-layer");
    let shape = j.ed().is_some_and(|ed| {
        ed.scene
            .active_layer()
            .is_some_and(|l| l.kind() == klad_core::doc::Kind::Vector && l.cels()[0].nodes.len() == 1)
    });
    j.expect(
        "shape-makes-its-vector-layer",
        shape,
        "a shape drawn while a paint layer is active lands on a new vector layer",
    );

    j.act("copy the rectangle with cmd C and paste it with cmd V");
    j.key(Key::V);
    j.tap(800.0, 1100.0);
    j.chord(CMD, Key::C);
    j.chord(CMD, Key::V);
    j.look("shape-pasted");
    let pasted = j
        .ed()
        .is_some_and(|ed| ed.scene.active_layer().is_some_and(|l| l.cels()[0].nodes.len() == 2));
    j.expect("shape-copy-paste", pasted, "cmd C and cmd V copy a vector shape");
    j.key(Key::Escape);

    j.act("turn the view with two fingers, then put it back");
    j.twist(1000.0, 1000.0, 23.0, Modifiers::NONE);
    j.twist(1000.0, 1000.0, 4.0, Modifiers::SHIFT);
    j.look("turned");
    let angle = |j: &Journey| j.ed().and_then(|ed| ed.view).map_or(0.0, |v| v.angle.to_degrees());
    j.expect(
        "shift-snaps-the-twist",
        (angle(&j) - 30.0).abs() < 0.01,
        "holding shift while twisting snaps the view to 15 degree steps",
    );
    j.click_containing("upright");
    j.look("upright");
    j.expect(
        "turned-view-goes-back",
        angle(&j).abs() < 0.01,
        "a button on the canvas puts a turned view upright again",
    );

    j.act("press I on the canvas, as someone reaching for an eyedropper");
    let cels = |j: &Journey| {
        j.ed()
            .map_or(0, |ed| ed.scene.active_layer().map_or(0, |l| l.cels().len()))
    };
    let frames = |j: &Journey| j.ed().map_or(0, |ed| ed.scene.bundle.doc.timeline.frames);
    let (was_cels, was_frames) = (cels(&j), frames(&j));
    j.key(Key::I);
    j.look("i-on-the-canvas");
    j.expect(
        "frame-keys-wait-for-the-timeline",
        (cels(&j), frames(&j)) == (was_cels, was_frames) && j.heard().contains("timeline key"),
        "I changes no timing while the canvas is in use, and says where it works",
    );

    j.act("make three drawings, select every frame in the timeline and press cmd D");
    j.click("roof");
    j.key(Key::B);
    j.chord(Modifiers::SHIFT, Key::K);
    j.drag(&[(300.0, 300.0), (500.0, 300.0)]);
    j.chord(Modifiers::SHIFT, Key::K);
    j.drag(&[(300.0, 400.0), (500.0, 400.0)]);
    j.look("three-drawings");
    let length = frames(&j);
    let drawings = cels(&j);
    j.drag_frames("roof", 1, 1);
    j.chord(CMD, Key::A);
    j.look("frames-selected");
    j.chord(CMD, Key::D);
    j.look("frames-repeated");
    j.note(format!(
        "{drawings} drawings on {length} frames became {} drawings on {} frames",
        cels(&j),
        frames(&j)
    ));
    j.expect(
        "cmd-d-repeats-selected-frames",
        cels(&j) == drawings * 2 && frames(&j) == length * 2,
        "cmd D with frames selected in the timeline repeats all of them after themselves",
    );

    j.act("press + and then - on a frame in the timeline");
    j.drag_frames("roof", 1, 1);
    let holds = |j: &Journey| -> Vec<(u32, Option<u32>)> {
        j.ed().map_or(Vec::new(), |ed| {
            ed.scene
                .bundle
                .doc
                .all_layers()
                .iter()
                .flat_map(|l| l.cels().iter().map(|c| (c.frame, c.hold)))
                .collect()
        })
    };
    let (was_holds, was_frames) = (holds(&j), frames(&j));
    j.key(Key::Equals);
    j.look("plus-on-a-frame");
    let longer = frames(&j);
    j.key(Key::Minus);
    j.look("minus-on-a-frame");
    j.note(format!(
        "the timeline went from {was_frames} frames to {longer} and back to {}",
        frames(&j)
    ));
    j.expect(
        "plus-and-minus-undo-each-other",
        longer == was_frames + 1 && frames(&j) == was_frames && holds(&j) == was_holds,
        "on a frame in the timeline + makes the drawing one frame longer and - takes exactly that back",
    );

    j.act("save with cmd S");
    j.chord(CMD, Key::S);
    j.look("saved");
    j.expect(
        "cmd-s-says-it-is-saved",
        j.heard().contains("nothing to save") && j.heard().contains("house.klad"),
        "cmd S says the drawing is saved as you go, and where it is",
    );

    j.act("export the picture from File > Export > Image, with a name");
    j.menu("File", "Export", "Image… cmd E");
    j.type_into("save as", "roof-shot");
    j.look("export-image");
    let middle =
        j.h.query_all_by_label_contains("Export image")
            .find(|n| n.accesskit_node().role() == egui::accesskit::Role::Window)
            .map(|n| n.rect().center());
    j.expect(
        "export-opens-in-the-middle",
        middle.is_some_and(|c| (c - egui::pos2(WINDOW.x / 2.0, WINDOW.y / 2.0)).length() < 4.0),
        "the export window opens in the middle of the window",
    );
    j.click("Export");
    j.look("exported");
    let written = pngs(&j.folder)
        .iter()
        .any(|p| p.file_name().is_some_and(|n| n == "roof-shot.png"));
    j.expect(
        "export-takes-a-name-and-says-where",
        written && j.heard().starts_with("exported to ") && j.heard().ends_with("roof-shot.png"),
        "the export is written under the name typed and the message says where it is",
    );
    j.menu("File", "Export", "Godot…");
    j.look("export-godot");
    j.expect(
        "godot-has-its-own-window",
        j.shown("Godot folder") && !j.shown("16 bits per channel"),
        "what Godot needs is in its own export window",
    );

    j.finish(&[]);
}

fn layer_marks(j: &Journey, layer: &str) -> Vec<Mark> {
    j.ed().map_or_else(Vec::new, |ed| {
        ed.scene
            .bundle
            .doc
            .find_layer(layer)
            .ok()
            .and_then(|l| l.content_at(ed.scene.frame))
            .map(|c| ed.scene.marks_of(&c.id).to_vec())
            .unwrap_or_default()
    })
}

fn mark_of(j: &Journey, layer: &str, id: u32) -> Option<Mark> {
    layer_marks(j, layer).into_iter().find(|m| m.id() == id)
}

fn free_spot(marks: &[Mark], id: u32) -> Option<(f32, f32)> {
    let own = marks.iter().find(|m| m.id() == id)?.as_stroke()?;
    let others: Vec<(f32, f32)> = marks
        .iter()
        .filter(|m| m.id() != id)
        .filter_map(|m| m.as_stroke())
        .flat_map(|s| s.samples.iter().step_by(3).map(|p| p.pos()))
        .collect();
    let count = own.samples.len();
    own.samples
        .iter()
        .skip(count / 5)
        .take((count * 3 / 5).max(1))
        .map(|p| p.pos())
        .map(|(x, y)| {
            let near = others
                .iter()
                .map(|(ox, oy)| (ox - x).hypot(oy - y))
                .fold(f32::MAX, f32::min);
            (near, x, y)
        })
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, x, y)| (x, y))
}

fn picked_ids(j: &Journey) -> Vec<u32> {
    j.ed()
        .and_then(|ed| ed.picked.as_ref())
        .map_or_else(Vec::new, |p| p.ids.clone())
}

fn active_name(j: &Journey) -> String {
    j.ed()
        .and_then(|ed| ed.scene.active_layer().map(|l| l.name.clone()))
        .unwrap_or_else(|| "no layer".into())
}

fn hex_of(mark: &Mark) -> String {
    klad_core::color::to_hex(mark.color())
}

fn box_of(marks: &[Mark], ids: &[u32]) -> Option<[f32; 4]> {
    marks
        .iter()
        .filter(|m| ids.contains(&m.id()))
        .filter_map(|m| m.bounds())
        .reduce(|a, b| [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])])
}

fn handle_corners(j: &Journey) -> Option<[egui::Pos2; 4]> {
    let (_, corners) = j.ed()?.handles()?;
    Some(corners.map(|c| j.at(c.x, c.y)))
}

fn near(a: Option<[f32; 4]>, b: Option<[f32; 4]>, by: f32) -> bool {
    matches!((a, b), (Some(a), Some(b)) if a.iter().zip(b).all(|(a, b)| (a - b).abs() <= by))
}

#[test]
fn someone_changes_one_stroke_and_several_in_what_an_agent_painted() {
    let mut j = Journey::begin("edit-strokes", WINDOW, Some("lantern.klad"));

    j.act("open the lantern an agent painted and take the select tool");
    j.key(Key::V);
    j.look("opened");
    if j.ed().is_none() {
        return j.finish(&[]);
    }
    let lines = layer_marks(&j, "lines");
    let blades: Vec<(u32, f32, f32)> = lines
        .iter()
        .filter(|m| m.bounds().is_some_and(|b| b[2] < 330.0 && b[1] > 1100.0))
        .filter_map(|m| free_spot(&lines, m.id()).map(|(x, y)| (m.id(), x, y)))
        .take(3)
        .collect();
    j.note(format!("the blades of grass left of the stone: {blades:?}"));
    if blades.len() < 3 {
        return j.finish(&[]);
    }

    j.act("zoom in on the grass left of the stone, to work on single blades");
    j.pinch(268.0, 1160.0, 2.0);
    j.pinch(268.0, 1160.0, 2.0);
    j.look("zoomed");
    let spot = j.at(268.0, 1160.0);
    let in_view = j.ed().is_some_and(|ed| ed.canvas_rect.contains(spot));
    j.note(format!("zoom is {:.2}", zoom(&j)));
    j.expect(
        "zoom-keeps-the-spot",
        in_view,
        "after zooming in on the grass it is still in view",
    );

    j.act("click one blade");
    j.tap(blades[0].1, blades[0].2);
    j.look("one-blade");
    j.note(format!("picked {:?} on {}", picked_ids(&j), active_name(&j)));
    j.expect(
        "click-picks-the-blade",
        picked_ids(&j) == [blades[0].0],
        "a click picks the blade under the pointer",
    );

    j.act("shift click two more blades");
    j.tap_with(blades[1].1, blades[1].2, egui::PointerButton::Primary, Modifiers::SHIFT);
    j.tap_with(blades[2].1, blades[2].2, egui::PointerButton::Primary, Modifiers::SHIFT);
    j.look("three-blades");
    j.note(format!("picked {:?}", picked_ids(&j)));
    j.expect(
        "shift-click-adds",
        picked_ids(&j).len() == 3,
        "shift click adds to the selection",
    );

    j.act("shift click the second one again, to take it out");
    j.tap_with(blades[1].1, blades[1].2, egui::PointerButton::Primary, Modifiers::SHIFT);
    j.look("two-blades");
    let two = picked_ids(&j);
    j.note(format!("picked {two:?}"));
    j.expect(
        "shift-click-takes-out",
        two.len() == 2 && !two.contains(&blades[1].0),
        "shift click on a picked stroke takes it out of the selection",
    );

    j.act("make the two thinner with the minus in the bar at the selection, twice");
    let sizes = |j: &Journey, ids: &[u32]| -> Vec<f32> {
        layer_marks(j, "lines")
            .iter()
            .filter(|m| ids.contains(&m.id()))
            .filter_map(|m| m.as_stroke().map(|s| s.size))
            .collect()
    };
    let was = sizes(&j, &two);
    j.press_hinted("make the selected strokes thinner");
    j.press_hinted("make the selected strokes thinner");
    j.look("thinner");
    let now = sizes(&j, &two);
    j.note(format!("their sizes went from {was:?} to {now:?}"));
    j.expect(
        "thinner-thins",
        was.len() == 2 && was.iter().zip(&now).all(|(a, b)| (b / a - 0.64).abs() < 0.02),
        "each click on minus makes the selected strokes a fifth thinner",
    );

    j.act("give them the stone colour from the palette, through the bar's colour button");
    let opened = j.press_hinted("the colour of the selected strokes");
    j.look("picker");
    j.expect(
        "colour-button-opens-a-picker",
        opened && j.shown("use the current colour"),
        "the bar's colour button opens a picker",
    );
    let popup =
        j.h.query_all_by_label("use the current colour")
            .next()
            .map(|n| n.rect())
            .zip(
                j.h.query_all_by_label("the colour of the selected strokes")
                    .find(|n| n.accesskit_node().role() == egui::accesskit::Role::Label)
                    .map(|n| n.rect()),
            )
            .map(|(low, title)| low.union(title));
    let selection = handle_corners(&j).map(|c| egui::Rect::from_points(&c));
    j.note(format!(
        "the picker is at {popup:?}, the selection's box at {selection:?}"
    ));
    j.expect(
        "the-picker-leaves-the-selection-in-sight",
        !matches!((popup, selection), (Some(p), Some(s)) if p.intersects(s)),
        "the picker opens beside what it recolours, so the change can be seen",
    );
    j.click_leftmost("#6b7584");
    j.look("recoloured");
    let colours: Vec<String> = layer_marks(&j, "lines")
        .iter()
        .filter(|m| two.contains(&m.id()))
        .map(hex_of)
        .collect();
    j.note(format!("their colours are {colours:?}"));
    j.expect(
        "palette-colour-recolours-several",
        colours.len() == 2 && colours.iter().all(|c| c == "#6b7584"),
        "a palette colour in the picker recolours every selected stroke",
    );

    j.act("close the picker with Esc, to go on with the two blades");
    j.key(Key::Escape);
    j.look("picker-closed");
    j.note(format!("picked {:?}", picked_ids(&j)));
    j.expect(
        "esc-closes-the-picker-first",
        !j.shown("use the current colour") && picked_ids(&j).len() == 2,
        "Esc closes the picker and keeps the selection",
    );
    if picked_ids(&j).len() != 2 {
        j.tap(blades[0].1, blades[0].2);
        j.tap_with(blades[2].1, blades[2].2, egui::PointerButton::Primary, Modifiers::SHIFT);
        j.note(format!("picked the two again: {:?}", picked_ids(&j)));
    }

    j.act("turn them a little by the knob above their box");
    let before_turn = box_of(&layer_marks(&j, "lines"), &two);
    let knob_of = |c: [egui::Pos2; 4]| {
        let top = c[0].lerp(c[1], 0.5);
        let centre = (c[0].to_vec2() + c[1].to_vec2() + c[2].to_vec2() + c[3].to_vec2()) / 4.0;
        top + (top.to_vec2() - centre).normalized() * 30.0
    };
    if let Some(c) = handle_corners(&j) {
        let knob = knob_of(c);
        j.drag_screen(
            knob,
            knob + vec2(40.0, 10.0),
            "the knob above the selection's box to the right",
        );
    } else {
        j.note("no box around the selection to turn it by");
    }
    j.look("turned");
    let after_turn = box_of(&layer_marks(&j, "lines"), &two);
    j.note(format!("their box went from {before_turn:?} to {after_turn:?}"));
    j.expect(
        "knob-turns-under-the-bar",
        !near(before_turn, after_turn, 0.5),
        "the knob turns the selection, also when the bar sits above the box",
    );

    let turned_first = !near(before_turn, after_turn, 0.5);
    let after_turn = match turned_first {
        true => after_turn,
        false => {
            j.act("the bar is on the knob: scroll the grass up to the middle of the canvas and try the knob again");
            let low = j.at(268.0, 1160.0).y;
            j.wheel(268.0, 1160.0, vec2(0.0, -220.0), Modifiers::NONE);
            if j.at(268.0, 1160.0).y >= low {
                j.wheel(268.0, 1160.0, vec2(0.0, 440.0), Modifiers::NONE);
            }
            j.note(format!(
                "the grass went from y {low:.0} to y {:.0} on screen",
                j.at(268.0, 1160.0).y
            ));
            if let Some(c) = handle_corners(&j) {
                let knob = knob_of(c);
                j.drag_screen(
                    knob,
                    knob + vec2(40.0, 10.0),
                    "the knob above the selection's box to the right",
                );
            }
            j.look("turned-with-room");
            let after_turn = box_of(&layer_marks(&j, "lines"), &two);
            j.note(format!("their box went from {before_turn:?} to {after_turn:?}"));
            after_turn
        }
    };
    j.expect(
        "knob-turns",
        !near(before_turn, after_turn, 0.5) && sizes(&j, &two) == now,
        "the knob turns the selection and leaves the strokes as thick as they were",
    );

    j.act("make them bigger by a corner of the box");
    if let Some(c) = handle_corners(&j) {
        let centre = (c[0].to_vec2() + c[1].to_vec2() + c[2].to_vec2() + c[3].to_vec2()) / 4.0;
        let out = (c[0].to_vec2() - centre).normalized() * 40.0;
        j.drag_screen(c[0], c[0] + out, "the top left corner of the box outwards");
    }
    j.look("scaled");
    let after_scale = box_of(&layer_marks(&j, "lines"), &two);
    let grew = matches!((after_turn, after_scale), (Some(a), Some(b)) if (b[2] - b[0]) > (a[2] - a[0]) * 1.1);
    j.note(format!(
        "their box is now {after_scale:?}, their sizes {:?}",
        sizes(&j, &two)
    ));
    j.expect("corner-scales", grew, "a corner of the box scales the selection");

    j.act("take the turn and the scale back with cmd Z, twice");
    j.chord(CMD, Key::Z);
    j.chord(CMD, Key::Z);
    j.look("turn-and-scale-undone");
    let back = box_of(&layer_marks(&j, "lines"), &two);
    j.expect(
        "undo-takes-back-the-transforms",
        near(before_turn, back, 0.5),
        "two undos put the blades back where they were before the turn",
    );
    j.key(Key::Escape);

    j.act("see the whole lantern again with F and click the pale body of the lamp, to change its colour");
    j.key(Key::F);
    j.tap(560.0, 650.0);
    j.settle();
    if j.heard().contains("still loading") {
        j.note(format!("it said: {}", j.heard()));
        j.tap(560.0, 650.0);
    }
    j.look("clicked-the-body");
    let got = j.ed().map_or_else(String::new, |ed| {
        format!(
            "strokes or fills {:?}, shape {:?}, on {}",
            ed.picked.as_ref().map(|p| p.ids.clone()),
            ed.selection,
            active_name(&j)
        )
    });
    j.note(format!("the click took: {got}"));
    let body = mark_of(&j, "flats", 6);
    j.expect(
        "click-on-a-flat-colour-picks-it",
        active_name(&j) == "flats" && picked_ids(&j) == [6],
        "a click on the lamp's pale body picks that flat colour and goes to its layer",
    );

    j.act("give the body a warmer colour by typing a hex in the picker");
    j.press_hinted("the colour of the selected strokes");
    j.retype_leftmost("hex", "#e8c890");
    j.look("body-recoloured");
    let now_body = mark_of(&j, "flats", 6).map(|m| hex_of(&m));
    j.note(format!(
        "the body went from {:?} to {now_body:?}",
        body.as_ref().map(hex_of)
    ));
    j.expect(
        "hex-in-the-picker-recolours-a-fill",
        now_body.as_deref() == Some("#e8c890"),
        "a hex typed in the picker recolours the selected fill",
    );
    j.key(Key::Escape);
    j.key(Key::Escape);

    j.act("reshape the outline of the stone: A, click the line, drag one of its points up");
    j.click("lines");
    j.key(Key::A);
    let stone = free_spot(&layer_marks(&j, "lines"), 6);
    let was_stone = mark_of(&j, "lines", 6).and_then(|m| m.bounds());
    if let Some((x, y)) = stone {
        j.tap(x, y);
    }
    j.look("points");
    let anchors: Vec<[f32; 2]> = j
        .ed()
        .and_then(|ed| ed.shaped.as_ref())
        .map(|s| {
            klad_core::pathedit::anchors(&s.subs)
                .iter()
                .map(|(_, a)| a.at)
                .collect()
        })
        .unwrap_or_default();
    j.note(format!("the line shows {} points", anchors.len()));
    j.expect(
        "a-shows-the-points-of-a-stroke",
        anchors.len() > 2,
        "A and a click show the points of a brush stroke",
    );
    if let Some(top) = anchors.iter().min_by(|a, b| a[1].total_cmp(&b[1])) {
        let from = j.at(top[0], top[1]);
        j.drag_screen(
            from,
            from - vec2(0.0, 14.0),
            "the topmost point of the stone's outline 14 px up",
        );
        j.wait("the reshaped line", |ed| {
            ed.point_drag.is_none() && ed.path_commit.is_none()
        });
    }
    j.look("reshaped");
    let now_stone = mark_of(&j, "lines", 6).and_then(|m| m.bounds());
    j.note(format!("the outline's box went from {was_stone:?} to {now_stone:?}"));
    j.expect(
        "a-point-drag-reshapes-the-stroke",
        matches!((was_stone, now_stone), (Some(a), Some(b)) if b[1] < a[1] - 20.0),
        "dragging a point reshapes the stroke",
    );
    j.key(Key::Escape);

    j.act("put the grass right of the stone on a layer of its own: zoom in, S, and a box around it");
    j.pinch(750.0, 1150.0, 2.0);
    j.key(Key::S);
    let count_before = layer_marks(&j, "lines").len();
    let all_before = j.ed().map_or(0, |ed| ed.scene.total_marks());
    let around = [(694.0, 1108.0), (806.0, 1212.0)];
    j.drag(&around);
    j.look("looped-the-grass");
    let grass: Vec<u32> = (28..=34).collect();
    let cut = |j: &Journey| {
        j.ed()
            .and_then(|ed| ed.picked.as_ref())
            .is_some_and(|p| p.cut.is_some())
    };
    let elsewhere = |j: &Journey| -> Vec<(String, usize)> {
        j.ed()
            .and_then(|ed| ed.picked.as_ref())
            .map(|p| p.more.iter().map(|part| (part.layer.clone(), part.ids.len())).collect())
            .unwrap_or_default()
    };
    j.note(format!(
        "the box took {:?} on lines{}, and on other layers {:?}; the grass there is {grass:?}",
        picked_ids(&j),
        if cut(&j) {
            " with the parts of other strokes inside it"
        } else {
            ""
        },
        elsewhere(&j)
    ));
    j.expect(
        "a-loop-takes-the-grass",
        grass.iter().all(|id| picked_ids(&j).contains(id)),
        "a box around the grass takes every blade",
    );

    j.act("the box took whole strokes only, as its cut is off by default: drag it again and check");
    j.key(Key::Escape);
    j.drag(&around);
    j.look("looped-whole-strokes");
    let shading = elsewhere(&j);
    let taken = picked_ids(&j).len() + shading.iter().map(|(_, n)| n).sum::<usize>();
    j.note(format!(
        "the box took {:?} on lines and on other layers {shading:?}: {taken} marks",
        picked_ids(&j)
    ));
    let mut whole = picked_ids(&j);
    whole.sort_unstable();
    j.expect(
        "a-loop-without-the-cut-takes-whole-strokes",
        whole == grass && !cut(&j),
        "with the cut off, the box takes the strokes wholly inside it and nothing else on this layer",
    );

    j.act("move them: cmd X, a new paint layer, cmd V");
    let grass_box = box_of(&layer_marks(&j, "lines"), &grass);
    j.chord(CMD, Key::X);
    j.look("cut");
    let cut_away = layer_marks(&j, "lines").len() + grass.len() == count_before;
    j.expect("cmd-x-cuts", cut_away, "cmd X cuts what is picked");

    j.click("Layer");
    j.click("New paint layer");
    j.chord(CMD, Key::V);
    j.look("pasted-after-cut");
    let new_layer = active_name(&j);
    let all_now = j.ed().map_or(0, |ed| ed.scene.total_marks());
    j.note(format!(
        "the layer in hand is {new_layer} with {} marks; the drawing had {all_before} marks and has {all_now}",
        layer_marks(&j, &new_layer).len()
    ));
    j.expect(
        "cut-and-paste-moves-the-strokes",
        all_now == all_before && layer_marks(&j, &new_layer).len() >= grass.len(),
        "what was cut pastes onto the new layer, so the drawing keeps every mark",
    );

    j.act("take that back and do it the other way round: copy, a new layer, paste, and delete the originals after");
    (0..4).for_each(|_| {
        if layer_marks(&j, "lines").len() < count_before {
            j.chord(CMD, Key::Z);
        }
    });
    if active_name(&j) != "lines" {
        j.click("lines");
    }
    j.key(Key::S);
    j.drag(&around);
    j.chord(CMD, Key::C);
    j.click("Layer");
    j.click("New paint layer");
    j.chord(CMD, Key::V);
    j.look("pasted-on-new-layer");
    let new_layer = active_name(&j);
    let pasted: Vec<Mark> = layer_marks(&j, &new_layer);
    let pasted_box = pasted
        .iter()
        .filter_map(|m| m.bounds())
        .reduce(|a, b| [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]);
    j.note(format!(
        "the layer in hand is {new_layer} with {} marks in {pasted_box:?}; the grass was in {grass_box:?}; lines has {} marks (it had {count_before}); the drawing has {} marks (it had {all_before})",
        pasted.len(),
        layer_marks(&j, "lines").len(),
        j.ed().map_or(0, |ed| ed.scene.total_marks())
    ));
    j.expect(
        "paste-lands-on-the-layer-in-hand",
        new_layer != "lines" && pasted.len() >= grass.len() && layer_marks(&j, "lines").len() == count_before,
        "strokes copied from several layers are pasted on the layer in hand",
    );

    j.act("the glow in the lamp's window is a shape: see everything with F, click it with V");
    j.key(Key::F);
    j.key(Key::V);
    j.click("all layers");
    j.tap(650.0, 655.0);
    j.look("clicked-the-glow");
    let shape = j.ed().and_then(|ed| ed.selection.clone());
    j.note(format!(
        "picked the shape {shape:?}; the layer in hand is {}; strokes picked {:?}",
        active_name(&j),
        picked_ids(&j)
    ));
    j.expect(
        "click-picks-a-shape-among-paint",
        shape.is_some(),
        "a click on the glow picks the shape",
    );
    j.expect(
        "picking-a-shape-goes-to-its-layer",
        active_name(&j) == "light",
        "the layer of the picked shape becomes the layer in hand, as it does for a stroke",
    );

    j.act("type a size for the glow: look for its number fields in Layer settings");
    let fields = ["x", "y", "rotate", "sx", "sy"];
    let seen = |j: &Journey| -> Vec<&str> { fields.iter().copied().filter(|f| j.in_sight(f)).collect() };
    j.look("number-fields");
    j.note(format!("number fields in sight: {:?}", seen(&j)));
    j.expect(
        "shape-number-fields-in-sight",
        seen(&j).len() == fields.len(),
        "the picked shape's position, turn and scale fields are in sight",
    );
    j.press_hinted("fold Colour to its title");
    j.press_hinted("fold Brush to its title");
    j.look("number-fields-after-folding");
    j.note(format!(
        "with Colour and Brush folded, number fields in sight: {:?}",
        seen(&j)
    ));
    j.expect(
        "shape-number-fields-after-folding",
        seen(&j).len() == fields.len(),
        "with two panels folded the fields are in sight",
    );

    j.act("make the glow warmer with the bar's fill button, typing a hex");
    let fill_of = |j: &Journey, id: &str| -> Option<String> {
        j.ed().and_then(|ed| match ed.scene.bundle.doc.item(id) {
            Some(klad_core::doc::Item::Node(_, _, node)) => Some(format!("{:?}", node.fill)),
            _ => None,
        })
    };
    let was_fill = shape.as_deref().and_then(|id| fill_of(&j, id));
    j.press_hinted("the shape's fill");
    j.look("glow-picker");
    j.retype_leftmost("hex", "#ffd9a0");
    j.look("glow-warmer");
    let now_fill = shape.as_deref().and_then(|id| fill_of(&j, id));
    j.note(format!("its fill went from {was_fill:?} to {now_fill:?}"));
    j.expect(
        "shape-fill-recolours",
        shape.is_some() && was_fill != now_fill,
        "the bar's fill button recolours the shape",
    );
    let soft = |fill: &Option<String>| {
        fill.as_ref()
            .is_some_and(|f| f.contains("Radial") || f.contains("Linear"))
    };
    j.expect(
        "recolouring-a-gradient-keeps-it-a-gradient",
        !soft(&was_fill) || soft(&now_fill),
        "a soft glow stays a soft glow when it is given another colour",
    );

    j.act("that made a flat disc of it: take it back with cmd Z");
    j.chord(CMD, Key::Z);
    j.look("one-undo");
    let after_one = shape.as_deref().and_then(|id| fill_of(&j, id));
    j.expect(
        "one-recolour-is-one-undo",
        after_one == was_fill,
        "one cmd Z takes back one recolour",
    );
    if after_one != was_fill {
        j.chord(CMD, Key::Z);
        j.look("second-undo");
        let after_two = shape.as_deref().and_then(|id| fill_of(&j, id));
        j.note(format!(
            "after a second cmd Z the glow is {}",
            if after_two == was_fill {
                "as it was"
            } else {
                "still changed"
            }
        ));
    }

    j.act("delete the glow and bring it back");
    j.key(Key::Escape);
    j.key(Key::Escape);
    j.tap(650.0, 655.0);
    j.key(Key::Delete);
    j.look("glow-deleted");
    let gone = shape.as_deref().is_some_and(|id| fill_of(&j, id).is_none());
    j.expect("delete-removes-the-shape", gone, "Delete removes the picked shape");
    j.chord(CMD, Key::Z);
    j.look("glow-back");
    let again = shape.as_deref().and_then(|id| fill_of(&j, id));
    j.expect(
        "undo-brings-the-shape-back",
        again.is_some() && again == was_fill,
        "cmd Z brings the shape back as it was",
    );

    j.finish(&[]);
}

fn tree(j: &Journey) -> Vec<String> {
    j.ed().map_or_else(Vec::new, |ed| {
        ed.layer_rows(super::layers::Where::Panel)
            .iter()
            .filter(|r| !r.mask)
            .map(|r| format!("{}{}", "  ".repeat(r.depth), r.name))
            .collect()
    })
}

fn names(j: &Journey) -> Vec<String> {
    tree(j).iter().map(|n| n.trim().to_string()).collect()
}

fn layer_box(j: &Journey, layer: &str) -> Option<[f32; 4]> {
    layer_marks(j, layer)
        .iter()
        .filter_map(|m| m.bounds())
        .reduce(|a, b| [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])])
}

fn shifted(a: Option<[f32; 4]>, b: Option<[f32; 4]>) -> Option<(f32, f32)> {
    match (a, b) {
        (Some(a), Some(b)) => Some((b[0] - a[0], b[1] - a[1])),
        _ => None,
    }
}

#[test]
fn someone_rearranges_the_layers_of_an_agents_drawing_and_transforms_parts() {
    let mut j = Journey::begin("edit-layers", WINDOW, Some("lantern.klad"));

    j.act("open the lantern and look for its layers in the Layers panel");
    j.look("opened");
    if j.ed().is_none() {
        return j.finish(&[]);
    }
    let all = names(&j);
    let seen = j.rows_in_sight(&all, Rows::Panel);
    j.note(format!("the drawing's layers, top first: {all:?}"));
    j.note(format!(
        "in sight in the Layers panel: {seen:?}; in the timeline: {:?}",
        j.rows_in_sight(&all, Rows::Timeline)
    ));
    j.expect(
        "layers-panel-shows-the-layers",
        seen.len() == all.len(),
        "the Layers panel shows the drawing's layers",
    );

    j.act("make room for them: fold Colour and Brush");
    j.press_hinted("fold Colour to its title");
    j.press_hinted("fold Brush to its title");
    j.look("two-folded");
    let seen = j.rows_in_sight(&all, Rows::Panel);
    j.note(format!("in sight in the Layers panel: {seen:?}"));
    j.expect(
        "layers-panel-grows-when-others-fold",
        seen.len() == all.len(),
        "with Colour and Brush folded the Layers panel shows every layer",
    );

    j.act("still cut off: drag the edge between Layers and Layer settings down");
    if let Some(title) = j.spot_of("fold Layer settings to its title") {
        let grip = egui::pos2(title.center().x + 120.0, title.center().y - 13.0);
        j.drag_screen(
            grip,
            grip + vec2(0.0, 170.0),
            "the top edge of the Layer settings title 170 px down",
        );
    }
    j.look("layers-taller");
    let seen = j.rows_in_sight(&all, Rows::Panel);
    j.note(format!("in sight in the Layers panel: {seen:?}"));
    j.expect(
        "the-edge-between-panels-drags",
        seen.len() == all.len(),
        "dragging the edge under the Layers panel makes room for every layer",
    );
    j.press_hinted("fold Layer settings to its title");
    j.look("three-folded");
    let seen = j.rows_in_sight(&all, Rows::Panel);
    j.note(format!(
        "with Layer settings folded as well, in sight in the Layers panel: {seen:?}"
    ));
    j.expect(
        "layers-panel-takes-the-room-of-a-folded-neighbour",
        seen.len() == all.len(),
        "with the panel under it folded too, the Layers panel shows every layer",
    );
    j.press_hinted("open Layer settings");
    j.act("the timeline shows every layer's row: open it with its arrow, folded as it is on a drawing of one frame");
    j.press_hinted("open the timeline: its layers and frames");
    j.look("timeline-opened");
    let rows = Rows::Timeline;
    j.note(format!(
        "in sight in the timeline: {:?}; working with its rows from here",
        j.rows_in_sight(&all, rows)
    ));

    j.act("put the halo in front of the lines: take its row by the name and drag it above lines");
    let before = j.picture();
    j.drag_row_from("halo", "lines", 0.1, rows, true);
    j.look("halo-dragged-by-name");
    j.note(format!("the layers are now {:?}; it said: {}", names(&j), j.heard()));
    let by_name = names(&j) != all;
    j.expect(
        "a-row-drags-by-its-name",
        by_name,
        "a row taken by its name can be dragged",
    );
    if by_name {
        j.chord(CMD, Key::Z);
    }

    j.act("take the row beside its name and drag it above lines");
    j.drag_row("halo", "lines", 0.1, rows);
    j.look("halo-above-lines");
    let order = names(&j);
    j.note(format!("the layers are now {order:?}"));
    let above = order
        .iter()
        .position(|n| n == "halo")
        .zip(order.iter().position(|n| n == "lines"))
        .is_some_and(|(halo, lines)| halo + 1 == lines);
    j.expect("a-dragged-row-moves", above, "a row dragged above another lands there");
    let shown = j.picture();
    j.expect(
        "moving-a-layer-changes-the-picture",
        shown != before,
        "the picture shows the new order",
    );
    j.chord(CMD, Key::Z);
    j.look("halo-back");
    let shown = j.picture();
    j.expect(
        "undo-puts-the-row-back",
        names(&j) == all && shown == before,
        "cmd Z puts the layer back where it was",
    );

    j.act("put the two shading layers in a group: click shade, the group button, then drag rim into the group");
    j.click("shade");
    j.press_hinted("put the layer in a new group");
    j.look("shade-grouped");
    let grouped = tree(&j);
    j.note(format!("the layers are now {grouped:?}"));
    let group = names(&j).into_iter().find(|n| !all.contains(n)).unwrap_or_default();
    j.expect(
        "group-button-wraps-the-layer",
        !group.is_empty() && grouped.iter().any(|n| n == "  shade"),
        "the button puts the layer in a new group",
    );
    let shown = j.picture();
    j.expect(
        "grouping-keeps-the-picture",
        shown == before,
        "putting a layer in a group leaves the picture as it was",
    );
    j.drag_row("rim", &group, 0.5, rows);
    j.look("rim-in-group");
    let grouped = tree(&j);
    j.note(format!("the layers are now {grouped:?}"));
    j.expect(
        "a-row-dropped-on-a-group-goes-in",
        grouped.iter().any(|n| n == "  rim"),
        "a row dropped on the middle of a group goes into it",
    );
    let shown = j.picture();
    j.expect(
        "moving-into-the-group-keeps-the-picture",
        shown == before,
        "the shading still shows as it did, now from inside the group",
    );

    j.act("name the group: double click its name, type shading, Return");
    j.double_click(&group);
    j.type_text("shading");
    j.key(Key::Enter);
    j.look("group-named");
    j.expect(
        "double-click-renames",
        names(&j).iter().any(|n| n == "shading"),
        "a double click on the name renames the group",
    );
    let group = match names(&j).iter().any(|n| n == "shading") {
        true => "shading".to_string(),
        false => group,
    };

    j.act("hide the group with its eye, and show it again");
    let with = j.picture();
    j.press_hinted(&format!("show or hide {group}"));
    j.look("group-hidden");
    let shown = j.picture();
    j.expect(
        "the-eye-hides-a-group",
        shown != with,
        "the eye hides everything in the group",
    );
    j.press_hinted(&format!("show or hide {group}"));
    j.look("group-shown");
    let shown = j.picture();
    j.expect("the-eye-shows-it-again", shown == with, "and shows it again as it was");

    j.act("make the halo fainter: click halo, drag its opacity down, then choose another blend mode");
    j.click("halo");
    let layer = |j: &Journey, name: &str| j.ed().and_then(|ed| ed.scene.bundle.doc.find_layer(name).ok().cloned());
    let was = layer(&j, "halo");
    let field =
        j.h.query_all_by_label("opacity")
            .map(|n| n.rect())
            .find(|r| j.in_sight("opacity") && r.center().y < 200.0);
    match field {
        Some(r) => j.drag_screen(
            r.center(),
            r.center() - vec2(60.0, 0.0),
            "the opacity number 60 px to the left",
        ),
        None => j.note("no opacity field in sight"),
    }
    j.look("halo-fainter");
    let now = layer(&j, "halo");
    j.note(format!(
        "the halo's opacity went from {:?} to {:?}",
        was.as_ref().map(|l| l.opacity),
        now.as_ref().map(|l| l.opacity)
    ));
    j.expect(
        "opacity-drags",
        matches!((&was, &now), (Some(a), Some(b)) if b.opacity < a.opacity),
        "dragging the opacity number makes the layer fainter",
    );
    j.click("how the active layer mixes with what is below it");
    j.click("add");
    j.look("halo-add");
    let blended = layer(&j, "halo");
    j.expect(
        "blend-mode-changes",
        matches!((&now, &blended), (Some(a), Some(b)) if a.blend != b.blend),
        "the list beside the opacity changes the blend mode",
    );

    j.act("move the lantern to the left: go to lines, cmd T, drag inside the box");
    j.click("lines");
    j.chord(CMD, Key::T);
    j.look("transform-lines");
    let said = j.heard();
    j.note(format!("it said: {said}"));
    let lines_before = layer_box(&j, "lines");
    let flats_before = layer_box(&j, "flats");
    if let Some(c) = handle_corners(&j) {
        let middle = (c[0].to_vec2() + c[2].to_vec2()) / 2.0;
        j.drag_screen(
            middle.to_pos2(),
            middle.to_pos2() - vec2(60.0, 0.0),
            "from the middle of the layer's box 60 px to the left",
        );
    } else {
        j.note("no box around the layer");
    }
    j.look("lines-moved");
    let moved = shifted(lines_before, layer_box(&j, "lines"));
    let left = shifted(flats_before, layer_box(&j, "flats"));
    j.note(format!("lines moved by {moved:?}, flats by {left:?}"));
    j.expect(
        "cmd-t-moves-the-layer",
        moved.is_some_and(|(dx, _)| dx < -100.0),
        "cmd T and a drag inside the box move the layer",
    );
    j.expect(
        "the-lantern-moves-as-one",
        left == moved,
        "the colour under the lines moves with them",
    );
    j.key(Key::Escape);
    j.chord(CMD, Key::Z);

    j.act("the lantern is on several layers: put them all in one group, to move them together");
    j.click("flats");
    j.press_hinted("put the layer in a new group");
    let whole = names(&j)
        .into_iter()
        .find(|n| !all.contains(n) && *n != group)
        .unwrap_or_default();
    let dragged = ["lines", group.as_str(), "light", "halo"]
        .into_iter()
        .filter(|name| j.drag_row(name, &whole, 0.5, rows))
        .count();
    j.look("all-in-one-group");
    let grouped = tree(&j);
    j.note(format!("{dragged} rows dragged; the layers are now {grouped:?}"));
    let inside = ["lines", "light", "halo", "flats"]
        .iter()
        .all(|n| grouped.iter().any(|g| g.trim() == *n && g.starts_with("  ")));
    j.expect(
        "several-layers-go-into-one-group",
        inside,
        "five rows dragged one by one end up in the group",
    );

    j.act("move the whole lantern: click the group, cmd T, drag inside the box");
    j.click(&whole);
    j.chord(CMD, Key::T);
    j.look("transform-group");
    let lines_before = layer_box(&j, "lines");
    let flats_before = layer_box(&j, "flats");
    let shade_before = layer_box(&j, "shade");
    let pictured = j.picture();
    if let Some(c) = handle_corners(&j) {
        let middle = (c[0].to_vec2() + c[2].to_vec2()) / 2.0;
        j.drag_screen(
            middle.to_pos2(),
            middle.to_pos2() - vec2(60.0, 0.0),
            "from the middle of the group's box 60 px to the left",
        );
    } else {
        j.note("no box around the group");
    }
    j.look("group-moved");
    let by = [
        shifted(lines_before, layer_box(&j, "lines")),
        shifted(flats_before, layer_box(&j, "flats")),
        shifted(shade_before, layer_box(&j, "shade")),
    ];
    j.note(format!("lines, flats and shade moved by {by:?}"));
    j.expect(
        "cmd-t-on-a-group-moves-everything-in-it",
        by[0].is_some_and(|(dx, _)| dx < -100.0)
            && by.iter().all(
                |b| matches!((b, by[0]), (Some(b), Some(a)) if (b.0 - a.0).abs() < 0.5 && (b.1 - a.1).abs() < 0.5),
            ),
        "cmd T on a group moves every layer in it by the same amount",
    );

    j.act("make it smaller by a corner, then turn it by dragging outside the box");
    if let Some(c) = handle_corners(&j) {
        let centre = (c[0].to_vec2() + c[2].to_vec2()) / 2.0;
        let inward = (centre - c[2].to_vec2()).normalized() * 50.0;
        j.drag_screen(c[2], c[2] + inward, "the bottom right corner 50 px inwards");
    }
    j.look("group-smaller");
    let small = layer_box(&j, "lines");
    let width = |b: Option<[f32; 4]>| b.map_or(0.0, |b| b[2] - b[0]);
    j.note(format!("the box of lines is {small:?}"));
    j.expect(
        "a-corner-scales-the-group",
        width(small) < width(lines_before) * 0.95,
        "a corner of the box scales the group",
    );
    if let Some(c) = handle_corners(&j) {
        let centre = (c[0].to_vec2() + c[2].to_vec2()) / 2.0;
        let out = c[1] + (c[1].to_vec2() - centre).normalized() * 28.0;
        j.drag_screen(
            out,
            out + vec2(0.0, 60.0),
            "from just outside the top right corner 60 px down",
        );
    }
    j.look("group-dragged-outside");
    let turned = layer_box(&j, "lines");
    let still = j.ed().is_some_and(|ed| ed.shifting.is_some());
    j.note(format!(
        "the box of lines is {turned:?}; the transform is {}; it said: {}",
        if still { "still on" } else { "over" },
        j.heard()
    ));
    let outside_turned = !near(small, turned, 1.0);
    j.expect(
        "outside-the-box-turns-as-the-message-says",
        outside_turned,
        "a drag from outside the box turns the group, as the message says",
    );

    j.act("that ended the transform: cmd T again and turn it by the knob above the box");
    if !still {
        j.chord(CMD, Key::T);
    }
    if let Some(c) = handle_corners(&j) {
        let top = c[0].lerp(c[1], 0.5);
        let centre = (c[0].to_vec2() + c[2].to_vec2()) / 2.0;
        let knob = top + (top.to_vec2() - centre).normalized() * 30.0;
        j.drag_screen(knob, knob + vec2(50.0, 10.0), "the knob above the box to the right");
    }
    j.look("group-turned");
    let turned = layer_box(&j, "lines");
    j.note(format!("the box of lines is {turned:?}"));
    j.expect(
        "the-knob-turns-the-group",
        !near(small, turned, 1.0),
        "the knob above the box turns the group",
    );
    j.key(Key::Escape);
    j.look("transform-done");
    (0..3 + usize::from(outside_turned)).for_each(|_| j.chord(CMD, Key::Z));
    j.look("transforms-undone");
    let shown = j.picture();
    j.expect(
        "three-undos-take-the-transforms-back",
        near(lines_before, layer_box(&j, "lines"), 0.5) && shown == pictured,
        "an undo per transform (three, and one more for the turn from outside) puts the lantern back where it was",
    );

    j.act("throw the agent's hidden sketch away: click sketch, the bin, and take it back");
    if !j.click("sketch") {
        j.press_hinted(&format!("fold or open the group {group}"));
        j.press_hinted(&format!("fold or open the group {whole}"));
        j.note("folded the two groups in the timeline to bring the sketch's row in sight");
        j.click("sketch");
    }
    j.note(format!("the layer in hand is {}", active_name(&j)));
    j.press_hinted("delete the layer");
    j.look("sketch-deleted");
    j.note(format!("it said: {}", j.heard()));
    j.expect(
        "the-bin-deletes-the-layer",
        !names(&j).iter().any(|n| n == "sketch"),
        "the bin deletes the layer in hand",
    );
    j.chord(CMD, Key::Z);
    j.look("sketch-back");
    j.expect(
        "undo-brings-the-layer-back",
        names(&j).iter().any(|n| n == "sketch"),
        "cmd Z brings the layer back",
    );

    j.act("make a second halo: click halo and the duplicate button");
    j.click("halo");
    let count = names(&j).len();
    j.press_hinted("duplicate the layer");
    j.look("halo-twice");
    let after = tree(&j);
    j.note(format!(
        "the layers are now {after:?}; the layer in hand is {}",
        active_name(&j)
    ));
    j.expect(
        "duplicate-copies-the-layer",
        names(&j).len() == count + 1,
        "the duplicate button makes a copy of the layer",
    );

    j.finish(&[
        "grouping-keeps-the-picture",
        "layers-panel-shows-the-layers",
        "moving-into-the-group-keeps-the-picture",
        "the-lantern-moves-as-one",
    ]);
}

fn count_on(j: &Journey, layer: &str) -> usize {
    layer_marks(j, layer).len()
}

fn history(j: &mut Journey, take: usize) -> Vec<String> {
    j.ed_mut().map_or_else(Vec::new, |ed| {
        ed.history_rows()
            .iter()
            .take(take)
            .map(|r| format!("{}{} {}", if r.undone { "undone " } else { "" }, r.who, r.what))
            .collect()
    })
}

#[test]
fn someone_works_on_a_drawing_while_their_agent_edits_it() {
    let mut j = Journey::begin("edit-with-agent", WINDOW, Some("lantern.klad"));

    j.act("open the lantern and copy its path for the agent, from the File menu");
    j.look("opened");
    if j.ed().is_none() {
        return j.finish(&[]);
    }
    let drawing = j.drawing();
    j.click("File");
    let found = j.click("Copy path");
    j.look("path-copied");
    let copied = j.copied();
    j.note(format!(
        "on the clipboard: {:?}; it said: {}",
        copied
            .iter()
            .map(|c| c.replace(&drawing, "<the drawing's path>"))
            .collect::<Vec<_>>(),
        j.heard().replace(&drawing, "<the drawing's path>")
    ));
    j.expect(
        "copy-path-says-what-it-copied",
        found && j.heard().contains(&drawing),
        "File > Copy path says that it copied the drawing's path",
    );

    j.act("pin a note for the agent on the lamp: N, a click, the words, pin it");
    j.key(Key::N);
    j.tap(650.0, 640.0);
    j.look("note-open");
    j.type_text("make this glow warmer");
    j.click("pin it");
    j.look("note-pinned");
    let pins = j.ed().map_or(0, |ed| ed.pins.len());
    j.expect("a-note-pins", pins == 1, "the note is pinned on the canvas");
    let (_, read) = j.agent(&["notes", &drawing]);
    j.expect(
        "the-agent-reads-the-note",
        read.contains("make this glow warmer"),
        "klad notes gives the agent the note",
    );

    j.act("select two lines for the agent to look at: V, a click, a shift click");
    j.key(Key::V);
    let lines = layer_marks(&j, "lines");
    let spots: Vec<(u32, f32, f32)> = [4u32, 6]
        .iter()
        .filter_map(|id| free_spot(&lines, *id).map(|(x, y)| (*id, x, y)))
        .collect();
    if spots.len() == 2 {
        j.tap(spots[0].1, spots[0].2);
        j.tap_with(spots[1].1, spots[1].2, egui::PointerButton::Primary, Modifiers::SHIFT);
    }
    j.look("two-selected");
    let held = picked_ids(&j);
    j.note(format!("picked {held:?} on {}", active_name(&j)));
    let (_, read) = j.agent(&["selection", &drawing]);
    j.expect(
        "the-agent-reads-the-selection",
        held.len() == 2 && held.iter().all(|id| read.contains(&format!("s{id}"))),
        "klad selection gives the agent the two selected lines",
    );

    j.act("the agent paints a red line while the two lines are still selected");
    let before = count_on(&j, "lines");
    let last = j.heard();
    j.agent(&[
        "stroke",
        &drawing,
        "lines",
        "--brush",
        "ink",
        "--size",
        "14",
        "--color",
        "#d03030",
        "--path",
        "M 300 330 L 460 300",
    ]);
    let arrived = j.wait("the agent's line in the editor", |ed| {
        ed.scene
            .bundle
            .doc
            .find_layer("lines")
            .ok()
            .and_then(|l| l.content_at(ed.scene.frame))
            .is_some_and(|c| ed.scene.marks_of(&c.id).len() > 34)
    });
    j.look("agent-painted");
    let told = j.heard();
    j.note(format!(
        "lines has {} marks (it had {before}); the last message is: {told}",
        count_on(&j, "lines")
    ));
    j.expect(
        "the-agents-stroke-shows",
        arrived && count_on(&j, "lines") == before + 1,
        "the agent's line shows in the open editor",
    );
    j.expect(
        "the-selection-survives-an-agent-edit",
        picked_ids(&j) == held,
        "what was selected stays selected when the agent paints elsewhere",
    );
    j.expect(
        "an-agent-edit-is-announced",
        told != last,
        "a word on screen says the agent changed the drawing",
    );

    j.act("the agent marks the note as done");
    let id = j.ed().and_then(|ed| ed.pins.first().map(|n| n.id)).unwrap_or_default();
    j.agent(&["notes", &drawing, "--done", &id.to_string()]);
    let done = j.wait("the note marked done", |ed| ed.pins.iter().any(|n| n.done));
    j.look("note-done");
    j.expect(
        "the-pin-turns-done",
        done,
        "the pin on the canvas shows that the agent handled it",
    );

    j.act("paint a line yourself with B, then the agent paints another, then press cmd Z to take your own line back");
    j.key(Key::Escape);
    j.key(Key::B);
    j.click("lines");
    j.drag(&[(300.0, 420.0), (460.0, 400.0)]);
    j.settle();
    let mine = layer_marks(&j, "lines").last().map(|m| m.id());
    j.agent(&[
        "stroke",
        &drawing,
        "lines",
        "--color",
        "#3060c0",
        "--size",
        "14",
        "--path",
        "M 300 500 L 460 480",
    ]);
    j.wait("the agent's second line", |ed| {
        ed.scene
            .bundle
            .doc
            .find_layer("lines")
            .ok()
            .and_then(|l| l.content_at(ed.scene.frame))
            .is_some_and(|c| ed.scene.marks_of(&c.id).len() > 36)
    });
    j.look("both-painted");
    let theirs = layer_marks(&j, "lines").last().map(|m| m.id());
    j.chord(CMD, Key::Z);
    j.look("one-undo");
    let left: Vec<u32> = layer_marks(&j, "lines").iter().map(|m| m.id()).collect();
    let said = j.heard();
    j.note(format!(
        "my line is {mine:?}, the agent's {theirs:?}; after cmd Z the layer still has mine: {}, the agent's: {}; it said: {said}",
        mine.is_some_and(|id| left.contains(&id)),
        theirs.is_some_and(|id| left.contains(&id))
    ));
    j.expect(
        "undo-says-whose-step-it-took",
        said.contains("agent"),
        "cmd Z says that the step it took back was the agent's",
    );
    j.chord(CMD_SHIFT, Key::Z);

    j.act("look for the History panel, which says who did what");
    let title = j.hint_in_sight("open History") || j.hint_in_sight("fold History to its title");
    j.look("looking-for-history");
    j.expect(
        "history-panel-is-in-the-window",
        title,
        "the History panel, or its title, is in the window",
    );

    j.act("it is not there: try Window > History");
    j.click("Window");
    j.click("History");
    j.look("window-history");
    let listed = |j: &Journey| j.in_sight_containing("agent  stroke") && j.in_sight_containing("you  stroke");
    j.note(format!(
        "the history is {}; the panel is {}; it said: {}",
        if listed(&j) { "in sight" } else { "not in sight" },
        match j
            .ed()
            .map(|ed| ed.settings.layout.is_open(crate::layout::Pane::History))
        {
            Some(true) => "in the dock",
            _ => "closed",
        },
        j.heard()
    ));
    j.expect(
        "window-menu-brings-up-history-at-once",
        listed(&j),
        "Window > History shows the history",
    );
    let rows = history(&mut j, 6);
    j.note(format!("the history, newest first: {rows:?}"));

    if !listed(&j) {
        j.act("nothing came up: fold Colour, Layer settings and Layers to make room");
        j.press_hinted("fold Colour to its title");
        j.press_hinted("fold Layer settings to its title");
        j.press_hinted("fold Layers to its title");
        j.look("history-with-room");
        j.expect(
            "history-names-you-and-the-agent",
            listed(&j),
            "the History panel names the agent's steps and yours",
        );
        j.press_hinted("open Layers");
    }

    j.act("draw a long line while the agent paints in the middle of it");
    let before = count_on(&j, "lines");
    j.pen_down(300.0, 900.0);
    j.pen_move(360.0, 880.0);
    j.pen_move(420.0, 900.0);
    j.agent(&[
        "stroke",
        &drawing,
        "lines",
        "--color",
        "#30a050",
        "--size",
        "14",
        "--path",
        "M 300 980 L 460 960",
    ]);
    j.pen_move(480.0, 880.0);
    j.pen_move(540.0, 900.0);
    j.pen_up(540.0, 900.0);
    j.wait("both lines", |ed| {
        ed.scene
            .bundle
            .doc
            .find_layer("lines")
            .ok()
            .and_then(|l| l.content_at(ed.scene.frame))
            .is_some_and(|c| ed.scene.marks_of(&c.id).len() >= 39)
    });
    j.look("drawn-through-an-agent-edit");
    j.note(format!(
        "lines had {before} marks and has {}; the last message is: {}",
        count_on(&j, "lines"),
        j.heard()
    ));
    j.expect(
        "a-stroke-drawn-through-an-agent-edit-keeps-both",
        count_on(&j, "lines") == before + 2,
        "the line being drawn and the agent's line are both kept",
    );

    j.act("select a line with V; the agent deletes that very line; then press Delete");
    j.key(Key::V);
    let target = free_spot(&layer_marks(&j, "lines"), 8);
    if let Some((x, y)) = target {
        j.tap(x, y);
    }
    j.look("picked-for-the-agent");
    let picked_one = picked_ids(&j);
    j.note(format!("picked {picked_one:?}"));
    let cel = j.ed().and_then(|ed| ed.scene.active_cel()).unwrap_or_default();
    let address = picked_one.first().map(|id| format!("{cel}:s{id}")).unwrap_or_default();
    let before = count_on(&j, "lines");
    j.agent(&["rm", &drawing, &address]);
    j.wait("the line gone", |ed| {
        ed.scene
            .bundle
            .doc
            .find_layer("lines")
            .ok()
            .and_then(|l| l.content_at(ed.scene.frame))
            .is_some_and(|c| ed.scene.marks_of(&c.id).len() < 39)
    });
    j.look("agent-deleted-my-selection");
    j.note(format!(
        "lines had {before} marks and has {}; picked {:?}; the last message is: {}",
        count_on(&j, "lines"),
        picked_ids(&j),
        j.heard()
    ));
    j.expect(
        "a-selection-the-agent-removed-is-dropped",
        picked_ids(&j).is_empty(),
        "a line the agent deleted is no longer selected",
    );
    j.key(Key::Delete);
    j.look("delete-after");
    j.note(format!("Delete said: {}", j.heard()));
    j.expect(
        "delete-after-the-agent-removed-it-is-calm",
        count_on(&j, "lines") == before - 1,
        "Delete after that removes nothing else",
    );

    j.act("make a layer of your own and paint on it; the agent removes that layer");
    j.click("Layer");
    j.click("New paint layer");
    j.key(Key::B);
    j.drag(&[(820.0, 300.0), (900.0, 400.0)]);
    j.look("my-layer");
    let own = active_name(&j);
    let own_id = j.ed().and_then(|ed| ed.scene.active_id()).unwrap_or_default();
    j.agent(&["rm", &drawing, &own_id]);
    j.wait("the layer gone", |ed| ed.scene.bundle.doc.find_layer(&own_id).is_err());
    j.look("agent-removed-my-layer");
    j.note(format!(
        "the layer in hand was {own}; now it is {}; the last message is: {}",
        active_name(&j),
        j.heard()
    ));
    j.expect(
        "a-layer-the-agent-removed-leaves-a-layer-in-hand",
        active_name(&j) != own && active_name(&j) != "no layer",
        "after the agent removed the layer in hand, another layer is in hand",
    );
    let total = j.ed().map_or(0, |ed| ed.scene.total_marks());
    j.drag(&[(820.0, 500.0), (900.0, 600.0)]);
    j.look("painted-after");
    j.note(format!(
        "a stroke after that: the drawing had {total} marks and has {}, on {}; it said: {}",
        j.ed().map_or(0, |ed| ed.scene.total_marks()),
        active_name(&j),
        j.heard()
    ));
    j.expect(
        "painting-goes-on-after-the-layer-is-gone",
        j.ed().map_or(0, |ed| ed.scene.total_marks()) == total + 1,
        "the next stroke lands on the layer now in hand",
    );

    j.act("the agent undoes the last step, which was yours");
    let total = j.ed().map_or(0, |ed| ed.scene.total_marks());
    j.agent(&["undo", &drawing]);
    j.wait("the undo", |ed| ed.scene.total_marks() < total);
    j.look("agent-undid-mine");
    j.note(format!(
        "the drawing had {total} marks and has {}; the last message is: {}",
        j.ed().map_or(0, |ed| ed.scene.total_marks()),
        j.heard()
    ));
    j.chord(CMD_SHIFT, Key::Z);
    j.look("redone");
    j.expect(
        "redo-brings-back-what-the-agent-undid",
        j.ed().map_or(0, |ed| ed.scene.total_marks()) == total,
        "cmd shift Z brings back the stroke the agent's undo took",
    );

    j.act("paint right after the agent writes, before the editor has shown its line");
    let total = j.ed().map_or(0, |ed| ed.scene.total_marks());
    j.agent(&[
        "stroke",
        &drawing,
        "lines",
        "--color",
        "#a040a0",
        "--size",
        "14",
        "--path",
        "M 300 1040 L 460 1020",
    ]);
    j.drag(&[(820.0, 700.0), (900.0, 800.0)]);
    j.wait("both strokes", |ed| ed.scene.total_marks() >= total + 2);
    j.look("painted-right-after");
    j.note(format!(
        "the drawing had {total} marks and has {}; the last message is: {}",
        j.ed().map_or(0, |ed| ed.scene.total_marks()),
        j.heard()
    ));
    j.expect(
        "a-stroke-right-after-the-agents-write-keeps-both",
        j.ed().map_or(0, |ed| ed.scene.total_marks()) == total + 2,
        "the agent's line and the stroke painted right after it are both kept",
    );

    j.finish(&[]);
}

const GODOT_CHECK: &str = r#"extends SceneTree

func _init() -> void:
	var packed: PackedScene = load("res://art/SCENE.tscn")
	if packed == null:
		print("FAILED the scene did not load")
		quit(1)
		return
	var node: Node = packed.instantiate()
	var sprite: AnimatedSprite2D = node as AnimatedSprite2D
	if sprite == null or sprite.sprite_frames == null:
		print("FAILED the scene's root is a ", node.get_class(), " without frames")
		node.free()
		quit(1)
		return
	var frames: SpriteFrames = sprite.sprite_frames
	for name in frames.get_animation_names():
		var length: float = 0.0
		for i in frames.get_frame_count(name):
			length += frames.get_frame_duration(name, i)
		print("ANIMATION ", name, " frames ", length, " fps ", frames.get_animation_speed(name), " loops ", frames.get_animation_loop(name))
	print("ROOT ", node.get_class(), " named ", node.name, " plays ", sprite.animation)
	node.free()
	quit(0)
"#;

fn godot(j: &mut Journey, project: &std::path::Path, scene: &str) -> Option<String> {
    let godot = std::env::var_os("KLAD_GODOT").filter(|g| !g.is_empty())?;
    std::fs::write(
        project.join("project.godot"),
        "config_version=5\n\n[application]\nconfig/name=\"check\"\n",
    )
    .ok()?;
    std::fs::write(project.join("check.gd"), GODOT_CHECK.replace("SCENE", scene)).ok()?;
    let run = |args: &[&str]| {
        std::process::Command::new(&godot)
            .arg("--headless")
            .arg("--path")
            .arg(project)
            .args(args)
            .output()
            .map(|out| String::from_utf8_lossy(&out.stdout).to_string())
            .unwrap_or_default()
    };
    run(&["--import"]);
    let said = run(&["--script", "res://check.gd"]);
    j.note("opened the folder as a Godot project and loaded the scene there");
    said.lines()
        .filter(|l| l.starts_with("ANIMATION") || l.starts_with("ROOT") || l.starts_with("FAILED"))
        .for_each(|l| j.note(format!("Godot: {l}")));
    Some(said)
}

fn files_in(dir: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().to_string()))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

fn uids(dir: &std::path::Path) -> Vec<String> {
    files_in(dir)
        .iter()
        .filter(|n| n.ends_with(".tscn") || n.ends_with(".tres"))
        .flat_map(|n| {
            std::fs::read_to_string(dir.join(n))
                .unwrap_or_default()
                .split("uid=\"")
                .skip(1)
                .filter_map(|rest| rest.split('"').next().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .collect()
}

#[test]
fn someone_exports_an_agents_animation_for_a_game() {
    let mut j = Journey::begin("export-for-a-game", WINDOW, Some("creature.klad"));

    j.act("open the creature an agent animated and choose File > Export > Godot…");
    j.look("opened");
    if j.ed().is_none() {
        return j.finish(&[]);
    }
    let home = j.folder.clone();
    let opened = j.menu("File", "Export", "Godot…");
    j.look("godot-window");
    j.expect(
        "the-godot-window-opens",
        opened && j.shown("Godot folder"),
        "File > Export > Godot… opens its window",
    );
    j.expect(
        "export-says-what-it-needs-first",
        j.shown("choose the Godot folder first"),
        "the window says that it needs a folder before it can export",
    );
    j.note(format!(
        "a Choose… button for the folder is {}",
        if j.shown("Choose…") {
            "there"
        } else {
            "not there in this build: the folder is typed"
        }
    ));

    j.act("give it a folder in the game, game/art, and export the animated sprite");
    j.type_into("Godot folder", "game/art");
    j.look("folder-typed");
    let place = j.shown("goes into");
    j.expect(
        "the-window-says-where-it-will-go",
        place,
        "the window says where the files will go",
    );
    j.click("Export");
    j.look("exported");
    let art = home.join("game").join("art");
    let written = files_in(&art);
    j.note(format!(
        "in game/art: {written:?}; it said: {}",
        j.heard().replace(&home.display().to_string(), "")
    ));
    j.expect(
        "the-sprite-is-written",
        ["creature.png", "creature.tres", "creature.tscn"]
            .iter()
            .all(|f| written.iter().any(|w| w == f)),
        "the sheet, the SpriteFrames and the scene are in the folder",
    );
    j.expect(
        "the-export-says-where-it-went",
        j.heard().contains("game/art") || j.heard().contains("game\\art"),
        "the message names the folder the files went to",
    );

    j.act("load it in Godot");
    match godot(&mut j, &home.join("game"), "creature") {
        Some(said) => {
            j.expect(
                "godot-loads-the-scene",
                said.contains("ROOT AnimatedSprite2D"),
                "Godot loads the scene as an animated sprite",
            );
            let clip = |name: &str, frames: u32| {
                said.lines().any(|l| {
                    l.starts_with(&format!("ANIMATION {name} frames {frames}"))
                        && l.contains("fps 12")
                        && l.contains("loops true")
                })
            };
            j.expect(
                "godot-has-both-clips",
                clip("idle", 12) && clip("walk", 16),
                "the sprite has the idle and walk animations at their length and speed",
            );
        }
        None => j.note("Godot was not given (KLAD_GODOT): the files were not loaded there"),
    }

    j.act("change the drawing and send it again with File > Export > Godot sprite, same folder");
    let ids = uids(&art);
    let sheet = std::fs::read(art.join("creature.png")).unwrap_or_default();
    let shut = |j: &mut Journey, window: &str| {
        let hint = format!("close {} {window}", crate::theme::icon::EXPORT);
        if j.hint_in_sight(&hint) {
            j.press_hinted(&hint);
        }
    };
    j.key(Key::Escape);
    j.look("after-esc");
    let open = j.shown("Godot folder");
    j.note(format!(
        "after Esc the Godot window is {}",
        if open { "still open" } else { "closed" }
    ));
    shut(&mut j, "Export for Godot");
    j.click("lines");
    j.key(Key::B);
    j.drag(&[(300.0, 300.0), (420.0, 360.0)]);
    j.settle();
    let again = j.menu("File", "Export", "Godot sprite, same folder");
    j.look("exported-again");
    j.note(format!(
        "it said: {}",
        j.heard().replace(&home.display().to_string(), "")
    ));
    let now = std::fs::read(art.join("creature.png")).unwrap_or_default();
    j.expect(
        "export-again-writes-the-change",
        again && !sheet.is_empty() && now != sheet,
        "the second export writes the changed picture",
    );
    j.note(format!("the ids in the files were {ids:?} and are {:?}", uids(&art)));
    j.expect(
        "export-again-keeps-godots-ids",
        !ids.is_empty() && ids.iter().all(|id| uids(&art).contains(id)),
        "the second export keeps the ids Godot knows the files by",
    );
    j.chord(CMD, Key::Z);

    j.act("a sprite sheet of the walk alone: File > Export > Sprite sheet…, the walk button, a folder, Export");
    j.menu("File", "Export", "Sprite sheet…");
    j.look("sheet-window");
    j.click("walk");
    j.retype("folder", "game/sheets");
    j.look("sheet-ready");
    j.click("Export");
    j.look("sheet-exported");
    let sheets = files_in(&home.join("game").join("sheets"));
    j.note(format!(
        "in game/sheets: {sheets:?}; it said: {}",
        j.heard().replace(&home.display().to_string(), "")
    ));
    j.expect(
        "a-sheet-with-its-json",
        sheets.iter().any(|f| f.ends_with(".png")) && sheets.iter().any(|f| f.ends_with(".json")),
        "the sheet and its JSON file are written",
    );
    let json = sheets
        .iter()
        .find(|f| f.ends_with(".json"))
        .and_then(|f| std::fs::read_to_string(home.join("game").join("sheets").join(f)).ok())
        .unwrap_or_default();
    j.expect(
        "the-sheet-holds-the-clip-asked-for",
        json.contains("\"walk\"") && !json.contains("\"idle\""),
        "the JSON names the walk and not the idle",
    );
    shut(&mut j, "Export sprite sheet");

    j.act("numbered frames of everything: File > Export > Frames…, a folder, Export");
    j.menu("File", "Export", "Frames…");
    j.look("frames-window");
    let field = |j: &Journey, label: &str| -> String {
        j.h.query_all_by_label(label)
            .filter(|n| n.accesskit_node().role() == egui::accesskit::Role::TextInput)
            .filter_map(|n| n.accesskit_node().value())
            .next()
            .unwrap_or_default()
    };
    let carried = (field(&j, "frames"), field(&j, "folder"));
    j.note(format!(
        "the window opens with frames \"{}\" and folder \"{}\"",
        carried.0, carried.1
    ));
    j.expect(
        "a-new-export-window-starts-with-all-frames",
        carried.0 == "all",
        "another kind of export starts with all frames, not with the clip chosen for the last one",
    );
    j.retype("frames", "all");
    j.retype("folder", "game/frames");
    j.look("frames-ready");
    j.click("Export");
    j.look("frames-exported");
    let frames_home = home.join("game").join("frames");
    let inside: Vec<String> = files_in(&frames_home)
        .iter()
        .flat_map(|name| {
            let sub = frames_home.join(name);
            match sub.is_dir() {
                true => files_in(&sub).into_iter().map(|f| format!("{name}/{f}")).collect(),
                false => vec![name.clone()],
            }
        })
        .collect();
    j.note(format!(
        "in game/frames: {} files, the first {:?}, the last {:?}; it said: {}",
        inside.len(),
        inside.first(),
        inside.last(),
        j.heard().replace(&home.display().to_string(), "")
    ));
    j.expect(
        "a-png-for-every-frame",
        inside.iter().filter(|f| f.ends_with(".png")).count() == 28,
        "the 28 frames are written as 28 numbered pictures",
    );
    shut(&mut j, "Export frames");

    j.act("the shadow alone as a picture: File > Export > Image…, the layer, a name, Export");
    j.menu("File", "Export", "Image… cmd E");
    j.look("image-window");
    j.click("export the whole image or one layer or group");
    j.click_leftmost("shadow");
    j.retype("folder", "game");
    j.retype("save as", "shadow");
    j.look("image-ready");
    let size = j.h.query_all_by_label_contains(" px").count();
    j.note(format!("lines on screen that give a size in px: {size}"));
    j.click("Export");
    j.look("image-exported");
    let game = files_in(&home.join("game"));
    j.note(format!(
        "in game: {game:?}; it said: {}",
        j.heard().replace(&home.display().to_string(), "")
    ));
    j.expect(
        "one-layer-as-a-picture",
        game.iter().any(|f| f.starts_with("shadow") && f.ends_with(".png")),
        "the layer is written under the name given",
    );
    shut(&mut j, "Export image");

    j.act("the sheet again, later: File > Export > Sprite sheet… and Export, without looking at the fields");
    let picture = std::fs::read(home.join("game").join("shadow.png")).unwrap_or_default();
    j.menu("File", "Export", "Sprite sheet…");
    j.look("sheet-window-again");
    let carried = (field(&j, "save as"), field(&j, "folder"), field(&j, "frames"));
    j.note(format!(
        "the window opens with save as \"{}\", folder \"{}\", frames \"{}\"",
        carried.0, carried.1, carried.2
    ));
    j.click("Export");
    j.look("sheet-over-the-picture");
    let after = std::fs::read(home.join("game").join("shadow.png")).unwrap_or_default();
    j.note(format!(
        "in game: {:?}; it said: {}",
        files_in(&home.join("game")),
        j.heard().replace(&home.display().to_string(), "")
    ));
    j.expect(
        "a-later-export-leaves-the-earlier-file",
        !picture.is_empty() && picture == after,
        "the picture exported before is still the picture, not overwritten by the sheet",
    );
    shut(&mut j, "Export sprite sheet");

    j.act("a folder that cannot be made, inside a file: File > Export > Image… into creature.klad/doc.json/out");
    j.menu("File", "Export", "Image… cmd E");
    j.retype("folder", "creature.klad/doc.json/out");
    j.click("Export");
    j.look("export-failed");
    let said = j.heard();
    j.note(format!("it said: {said}"));
    j.expect(
        "a-failed-export-says-so",
        !said.is_empty() && !said.starts_with("exported"),
        "an export that cannot be written says so",
    );
    j.expect(
        "a-failed-export-speaks-plainly",
        !said.contains("os error") && !said.contains("klad export") && !said.contains("--"),
        "the message is in plain words, without the system's error text or a command line",
    );
    shut(&mut j, "Export image");

    j.finish(&[]);
}

fn start_error(j: &Journey) -> String {
    j.h.state()
        .start
        .as_ref()
        .and_then(|s| s.error.clone())
        .unwrap_or_default()
}

fn on_disk(path: &std::path::Path) -> Option<usize> {
    let bundle = klad_core::bundle::Bundle::open(path).ok()?;
    Some(
        bundle
            .doc
            .all_layers()
            .iter()
            .flat_map(|l| {
                l.cels()
                    .iter()
                    .map(|c| (l.id.clone(), c.id.clone()))
                    .collect::<Vec<_>>()
            })
            .filter_map(|(layer, cel)| bundle.read_marks(&layer, &cel).ok())
            .map(|marks| marks.len())
            .sum(),
    )
}

fn new_drawing(j: &mut Journey, name: &str) -> bool {
    j.click("Size");
    j.click("1024 x 1024  sprite");
    j.retype("Name", name);
    if j.ed().is_none() {
        j.click("Create");
    }
    j.settle();
    j.ed().is_some()
}

#[test]
fn someone_keeps_their_drawings_in_order() {
    let mut j = Journey::begin("files", WINDOW, None);
    let home = j.folder.clone();
    let shown = |text: String| text.replace(&format!("{}/", home.display()), "");

    j.act("klad starts: make a drawing called poster, paint two strokes, close it with cmd W");
    let made = new_drawing(&mut j, "poster");
    j.expect("a-drawing-is-made", made, "the new drawing opens");
    if !made {
        return j.finish(&[]);
    }
    j.drag(&[(200.0, 300.0), (500.0, 320.0)]);
    j.drag(&[(220.0, 420.0), (520.0, 400.0)]);
    j.look("poster");
    j.chord(CMD, Key::W);
    j.look("closed");
    j.expect(
        "a-closed-drawing-is-in-recent",
        j.ed().is_none() && j.shown("poster"),
        "after cmd W the drawing is among the recent ones",
    );
    j.expect(
        "closing-kept-the-strokes",
        on_disk(&home.join("poster.klad")) == Some(2),
        "the two strokes are in the drawing on disk",
    );

    j.act("make another drawing with the same name");
    j.retype("Name", "poster");
    j.look("same-name");
    let said = shown(start_error(&j));
    j.note(format!("it said: {said}"));
    j.expect(
        "a-name-in-use-is-refused",
        j.ed().is_none() && said.contains("already exists"),
        "a second drawing of the same name is refused, and the first is left alone",
    );

    j.act("call it sketchbook, paint a stroke, close it");
    let made = new_drawing(&mut j, "sketchbook");
    j.drag(&[(300.0, 600.0), (700.0, 620.0)]);
    j.look("sketchbook");
    j.chord(CMD, Key::W);
    j.look("two-recent");
    let place = |j: &Journey, name: &str| j.h.query_all_by_label(name).next().map(|n| n.rect().left());
    let order = (place(&j, "sketchbook"), place(&j, "poster"));
    j.note(format!(
        "in the recent row, sketchbook is at x {:?} and poster at x {:?}",
        order.0, order.1
    ));
    j.expect(
        "recent-lists-the-newest-first",
        made && matches!(order, (Some(a), Some(b)) if a < b),
        "the drawing closed last is the first of the recent ones",
    );

    j.act("open poster again from the recent row");
    j.click("poster");
    j.look("poster-again");
    j.expect(
        "recent-reopens-a-drawing",
        j.ed()
            .is_some_and(|ed| ed.scene.bundle.root.ends_with("poster.klad") && ed.scene.total_marks() == 2),
        "a click on its card opens the drawing as it was left",
    );

    j.act("go to the other drawing without closing this one: File > Open recent > sketchbook");
    let folder = home
        .file_name()
        .map_or_else(String::new, |f| f.to_string_lossy().to_string());
    let switched = j.menu("File", "Open recent", &format!("sketchbook  ({folder})"));
    j.look("switched");
    j.expect(
        "open-recent-switches-drawings",
        switched
            && j.ed()
                .is_some_and(|ed| ed.scene.bundle.root.ends_with("sketchbook.klad")),
        "File > Open recent switches to another drawing",
    );

    j.act("look at all of them over the open drawing, and go back: File > Open recent > All recent drawings…");
    j.menu("File", "Open recent", "All recent drawings…");
    j.look("recent-over-the-drawing");
    j.expect(
        "recent-shows-over-the-drawing",
        j.shown("Back to this drawing"),
        "the recent drawings show over the open one, with a way back",
    );
    j.click("Back to this drawing");
    j.look("back");
    j.expect(
        "back-keeps-the-drawing",
        j.ed()
            .is_some_and(|ed| ed.scene.bundle.root.ends_with("sketchbook.klad"))
            && !j.shown("Back to this drawing"),
        "Back to this drawing leaves the drawing open",
    );
    j.chord(CMD, Key::W);
    j.look("poster-still-open");
    j.expect(
        "closing-shows-the-other-open-drawing",
        j.ed().is_some_and(|ed| ed.scene.bundle.root.ends_with("poster.klad")),
        "poster stayed open in its tab while sketchbook was open, and shows when sketchbook closes",
    );
    j.chord(CMD, Key::W);

    j.act("in the file browser, rename poster to poster final; then look at klad's recent row");
    std::fs::rename(home.join("poster.klad"), home.join("poster final.klad")).unwrap();
    j.note("renamed the folder poster.klad to poster final.klad outside klad");
    j.look("renamed-outside");
    j.expect(
        "a-renamed-drawing-shows-as-moved",
        j.shown("moved or deleted"),
        "the recent row says that poster was moved or deleted",
    );

    j.act("open it where it is now: paste its path under Open");
    let moved = home.join("poster final.klad").display().to_string();
    j.type_into("Or paste its path", &moved);
    j.click("Open");
    j.look("opened-by-path");
    j.expect(
        "a-pasted-path-opens",
        j.ed()
            .is_some_and(|ed| ed.scene.bundle.root.ends_with("poster final.klad") && ed.scene.total_marks() == 2),
        "the drawing opens from its new place with its strokes",
    );
    j.chord(CMD, Key::W);
    j.look("recent-after-the-move");
    let stale = j.shown("Remove poster");
    j.note(format!(
        "the old card is {}",
        if stale {
            "still there, with a Remove button"
        } else {
            "gone"
        }
    ));
    if stale {
        j.click("Remove poster");
        j.look("old-card-removed");
        j.expect(
            "remove-forgets-the-old-card",
            !j.shown("moved or deleted"),
            "Remove takes the old card out of the row",
        );
    }

    j.act("open sketchbook, paint, then rename its folder in the file browser while it is open, and paint again");
    j.click("sketchbook");
    j.settle();
    j.drag(&[(300.0, 200.0), (700.0, 220.0)]);
    j.settle();
    let before = j.ed().map_or(0, |ed| ed.scene.total_marks());
    let renamed = std::fs::rename(home.join("sketchbook.klad"), home.join("sketchbook moved.klad")).is_ok();
    j.note(match renamed {
        true => "renamed the folder sketchbook.klad to sketchbook moved.klad outside klad",
        false => "the system refused to rename the folder of an open drawing: nothing to see here",
    });
    j.h.run_steps(6);
    j.look("moved-under-the-editor");
    j.note(format!("after the rename it said: {}", shown(j.heard())));
    j.drag(&[(300.0, 300.0), (700.0, 320.0)]);
    j.look("painted-after-the-move");
    let said = shown(j.heard());
    let shows = j.ed().map_or(0, |ed| ed.scene.total_marks());
    let there = on_disk(&home.join("sketchbook moved.klad"));
    let old = on_disk(&home.join("sketchbook.klad"));
    j.note(format!(
        "the editor shows {shows} strokes (it had {before}); on disk sketchbook moved.klad has {there:?} and sketchbook.klad has {old:?}; in the folder: {:?}; it said: {said}",
        files_in(&home)
    ));
    let inside = |name: &str| -> Vec<String> {
        let root = home.join(name);
        let mut found: Vec<String> = files_in(&root)
            .into_iter()
            .filter(|f| f != ".cache")
            .flat_map(|f| match root.join(&f).is_dir() {
                true => files_in(&root.join(&f))
                    .into_iter()
                    .map(|g| format!("{f}/{g}"))
                    .collect(),
                false => vec![f],
            })
            .collect();
        found.sort();
        found
    };
    j.note(format!(
        "sketchbook moved.klad holds {:?}; the sketchbook.klad that came back holds {:?}",
        inside("sketchbook moved.klad"),
        inside("sketchbook.klad")
    ));
    if renamed {
        j.expect(
            "a-stroke-after-the-move-is-kept-or-refused-aloud",
            there == Some(shows) || (shows == before && !said.is_empty()),
            "the stroke painted after the move is in the renamed drawing, or the editor says that it could not keep it",
        );
        j.expect(
            "the-move-leaves-no-second-drawing",
            old.is_none() && !home.join("sketchbook.klad").exists(),
            "no second drawing appears at the old place",
        );
    }
    j.chord(CMD, Key::W);
    j.look("closed-after-the-move");

    j.act("put an older copy back under the editor, as a sync tool or git would");
    let third = new_drawing(&mut j, "roll");
    j.drag(&[(200.0, 200.0), (600.0, 220.0)]);
    j.settle();
    let roll = home.join("roll.klad");
    let backup = home.join("backup of roll");
    super::testkit::copy_dir(&roll, &backup);
    j.note("kept a copy of the drawing with one stroke");
    j.drag(&[(200.0, 400.0), (600.0, 420.0)]);
    j.drag(&[(200.0, 600.0), (600.0, 620.0)]);
    j.look("three-strokes");
    super::testkit::copy_dir(&backup, &roll);
    j.note("put that copy back over the drawing's files");
    j.wait("the older copy in the editor", |ed| ed.scene.total_marks() == 1);
    j.look("older-copy-back");
    let shows = j.ed().map_or(0, |ed| ed.scene.total_marks());
    j.note(format!(
        "the editor shows {shows} strokes, the files hold {:?}; it said: {}",
        on_disk(&roll),
        shown(j.heard())
    ));
    j.expect(
        "the-editor-follows-the-files",
        third && on_disk(&roll) == Some(shows),
        "the editor shows what the files hold after they were put back",
    );
    j.drag(&[(200.0, 800.0), (600.0, 820.0)]);
    j.look("painted-on-the-older-copy");
    let shows = j.ed().map_or(0, |ed| ed.scene.total_marks());
    j.note(format!(
        "after one more stroke the editor shows {shows}, the files hold {:?}; it said: {}",
        on_disk(&roll),
        shown(j.heard())
    ));
    j.expect(
        "painting-goes-on-from-the-older-copy",
        on_disk(&roll) == Some(shows),
        "the next stroke is saved on top of what the files hold",
    );
    j.chord(CMD, Key::S);
    j.look("cmd-s");
    j.note(format!("cmd S said: {}", shown(j.heard())));
    j.chord(CMD, Key::W);

    j.act("a drawing whose file is damaged: open it from the recent row");
    std::fs::write(roll.join("doc.json"), "{ \"version\": ").unwrap();
    j.note("cut the drawing's doc.json short outside klad");
    j.click("roll");
    j.look("damaged");
    let said = shown(start_error(&j));
    j.note(format!("it said: {said}"));
    j.expect(
        "a-damaged-drawing-is-not-opened",
        j.ed().is_none() && !said.is_empty(),
        "klad stays in the start window and says something",
    );
    j.expect(
        "a-damaged-drawing-is-explained",
        !said.contains("EOF") && !said.contains("line 1") && !said.contains("column"),
        "the message says in plain words that the drawing is damaged and what to do",
    );

    j.act("give Open the path of a picture: klad makes a drawing of it next to it and opens that");
    let (_, rendered) = j.agent(&["render", &home.join("poster final.klad").display().to_string()]);
    let picture = rendered
        .lines()
        .find(|l| l.trim().ends_with(".png"))
        .map(|l| l.trim().to_string())
        .unwrap_or_default();
    j.retype("Or paste its path", &format!("{}/{picture}", home.display()));
    j.click("Open");
    j.look("picture-to-open");
    let stem = std::path::Path::new(&picture)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let opened = j.ed().map(|ed| ed.scene.bundle.root.clone());
    j.note(format!("it opened {opened:?}"));
    j.expect(
        "a-picture-given-to-open-becomes-a-drawing",
        opened.is_some_and(|p| {
            p.extension().is_some_and(|e| e == "klad") && p.file_stem().is_some_and(|s| s.to_string_lossy() == stem)
        }),
        "the picture opens as a drawing made next to it",
    );

    j.finish(&[]);
}

#[test]
fn someone_renames_and_moves_a_drawing_while_it_is_open() {
    let mut j = Journey::begin("renamed-while-open", WINDOW, Some("lantern.klad"));
    let home = j.folder.clone();
    let shown = |text: String| text.replace(&format!("{}/", home.display()), "");
    let (old, new) = (home.join("lantern.klad"), home.join("lantern at night.klad"));
    let holds = |j: &Journey| j.ed().map_or(0, |ed| ed.scene.total_marks());
    let open_at = |j: &Journey| j.ed().map(|ed| ed.scene.bundle.root.clone());

    j.act("open the lantern, a drawing of several layers, and give its folder another name in the file browser");
    j.settle();
    j.look("opened");
    let before = holds(&j);
    j.note(format!(
        "the editor holds {before} marks, the folder {:?}",
        on_disk(&old)
    ));
    if std::fs::rename(&old, &new).is_err() {
        j.note("the system refused to rename the folder of an open drawing: nothing to see here");
        return j.finish(&[]);
    }
    j.h.run_steps(6);
    j.look("renamed");
    let said = shown(j.heard());
    j.note(format!("after the rename it said: {said}"));
    j.expect(
        "the-editor-follows-the-renamed-folder",
        open_at(&j).as_deref() == Some(new.as_path()),
        "klad goes on in the folder under its new name",
    );
    j.expect(
        "the-rename-is-told-in-plain-words",
        said.contains("lantern at night") && !said.contains("os error") && !said.contains("doc.json"),
        "klad says that the drawing has another name now, without a path to a file or an error number",
    );

    j.act("paint a stroke");
    j.key(Key::B);
    j.drag(&[(300.0, 300.0), (700.0, 320.0)]);
    j.look("painted-after-the-rename");
    j.note(format!(
        "the editor holds {} marks; lantern at night.klad holds {:?}; in the folder: {:?}; it said: {}",
        holds(&j),
        on_disk(&new),
        files_in(&home),
        shown(j.heard())
    ));
    j.expect(
        "a-stroke-after-the-rename-lands-in-the-renamed-drawing",
        holds(&j) == before + 1 && on_disk(&new) == Some(before + 1),
        "the stroke is in the editor and in the renamed folder, with everything that was there",
    );
    j.expect(
        "the-rename-leaves-no-second-drawing",
        !old.exists(),
        "no folder comes back under the old name",
    );

    j.act("take the stroke back with cmd Z");
    j.chord(CMD, Key::Z);
    j.look("undone-after-the-rename");
    j.note(format!(
        "the editor holds {} marks; lantern at night.klad holds {:?}; in the folder: {:?}",
        holds(&j),
        on_disk(&new),
        files_in(&home)
    ));
    j.expect(
        "an-undo-after-the-rename-keeps-every-layer",
        holds(&j) == before && on_disk(&new) == Some(before) && !old.exists(),
        "the editor and the renamed folder hold the drawing as it was, on every layer",
    );

    j.act("close it: the recent drawings know it under its new name");
    j.chord(CMD, Key::W);
    j.look("closed-after-the-rename");
    j.expect(
        "recent-knows-the-new-name",
        j.shown("lantern at night") && !j.shown("moved or deleted"),
        "the recent row shows the drawing under its new name, and no card for a drawing that is gone",
    );

    j.act("open it again, paint, and move its folder into another folder while it is open");
    j.click("lantern at night");
    j.settle();
    j.expect(
        "the-renamed-drawing-opens-from-recent",
        j.ed().is_some(),
        "a click on its card opens the drawing under its new name",
    );
    if j.ed().is_none() {
        return j.finish(&[]);
    }
    j.key(Key::B);
    j.drag(&[(300.0, 400.0), (700.0, 420.0)]);
    j.settle();
    let kept = holds(&j);
    let away = home.join("archive").join("lantern at night.klad");
    std::fs::create_dir_all(home.join("archive")).unwrap();
    std::fs::rename(&new, &away).unwrap();
    j.h.run_steps(6);
    j.look("moved-away");
    let said = shown(start_error(&j));
    j.note(format!(
        "the editor is {}; it said: {said}",
        if j.ed().is_some() { "still open" } else { "closed" }
    ));
    j.expect(
        "a-drawing-moved-away-is-closed-with-a-word",
        j.ed().is_none() && said.contains("lantern at night") && said.contains("moved") && !said.contains("os error"),
        "klad closes a drawing whose folder it can no longer find, and says so in plain words",
    );
    j.h.run_steps(30);
    j.expect(
        "a-move-leaves-nothing-behind",
        !new.exists() && on_disk(&away) == Some(kept),
        "nothing comes back at the old place, and the moved folder holds the whole drawing",
    );
    j.finish(&[]);
}

fn finder_offers(query: &str) -> Vec<&'static str> {
    crate::commands::rank(query, &[])
        .into_iter()
        .filter(|c| !crate::commands::DEVELOPER.contains(c))
        .take(3)
        .map(|c| c.spec().name)
        .collect()
}

#[test]
fn someone_looks_for_help_and_finds_their_way_back() {
    let mut j = Journey::begin("help-and-recovery", WINDOW, Some("lantern.klad"));

    j.act("open the lantern and look at Help > First steps");
    j.look("opened");
    if j.ed().is_none() {
        return j.finish(&[]);
    }
    j.click("Help");
    let found = j.click_containing("First steps");
    j.look("first-steps");
    j.expect(
        "first-steps-open",
        found && j.shown("copy a request"),
        "Help > First steps opens its window",
    );
    j.press_hinted(&format!("close {} First steps", crate::theme::icon::INFO));

    j.act("look for a command by typing what it does: cmd K, export");
    j.chord(CMD, Key::K);
    let field = format!("{} Find a command", crate::theme::icon::SEARCH);
    j.type_into(&field, "export");
    j.look("finder-export");
    let offers = finder_offers("export");
    j.note(format!("for \"export\" the finder offers first: {offers:?}"));
    j.expect(
        "the-finder-finds-a-command-by-its-name",
        offers.first().is_some_and(|name| name.contains("export")),
        "typing export offers the export commands",
    );

    j.act("type the words people bring from other apps");
    let words = [
        "resize",
        "canvas size",
        "crop",
        "merge",
        "save as",
        "zoom in",
        "deselect",
        "preferences",
        "shortcuts",
        "rotate",
        "mirror",
        "transparency",
        "new layer",
        "rename",
        "help",
    ];
    let offered: Vec<(&str, Vec<&str>)> = words.iter().map(|w| (*w, finder_offers(w))).collect();
    offered
        .iter()
        .for_each(|(word, offers)| j.note(format!("\"{word}\": {offers:?}")));
    j.key(Key::Escape);
    j.chord(CMD, Key::K);
    j.type_into(&field, "spreadsheet");
    j.look("finder-nothing");
    j.expect(
        "the-finder-says-when-it-has-nothing",
        j.shown("nothing matches"),
        "a word no command answers to says that nothing matches",
    );
    let fitting = [
        ("deselect", "clear the selection"),
        ("preferences", "settings"),
        ("shortcuts", "every key"),
        ("rotate", "turn"),
        ("help", "first steps"),
    ];
    let missed: Vec<&str> = fitting
        .iter()
        .filter(|(word, name)| {
            !crate::commands::rank(word, &[])
                .iter()
                .take(5)
                .any(|c| c.spec().name.contains(name))
        })
        .map(|(word, _)| *word)
        .collect();
    j.note(format!(
        "words for things klad has under another name that do not lead to them: {missed:?}"
    ));
    j.expect(
        "the-finder-knows-the-words-people-bring",
        missed.is_empty(),
        "a usual word for something klad has leads to it",
    );

    j.act("type deselect and press Return, with two lines selected");
    j.key(Key::Escape);
    j.key(Key::V);
    let lines = layer_marks(&j, "lines");
    if let (Some(a), Some(b)) = (free_spot(&lines, 4), free_spot(&lines, 6)) {
        j.tap(a.0, a.1);
        j.tap_with(b.0, b.1, egui::PointerButton::Primary, Modifiers::SHIFT);
    }
    let before = count_on(&j, "lines");
    let held = picked_ids(&j).len();
    j.chord(CMD, Key::K);
    j.type_into(&field, "deselect");
    j.look("finder-deselect");
    j.key(Key::Enter);
    j.look("deselect-run");
    j.note(format!(
        "{held} lines were selected and lines had {before} marks; now {} are selected and it has {}; it said: {}",
        picked_ids(&j).len(),
        count_on(&j, "lines"),
        j.heard()
    ));
    j.expect(
        "deselect-in-the-finder-does-not-delete",
        count_on(&j, "lines") == before,
        "typing deselect and Return does not delete what is selected",
    );
    if count_on(&j, "lines") != before {
        j.chord(CMD, Key::Z);
    }
    j.key(Key::Escape);

    j.act("look up the keys: Help > Keyboard shortcuts, and type undo in its field");
    j.click("Help");
    let keys = j.click_containing("Keyboard shortcuts");
    j.look("keys");
    let list = j.h.ctx.memory(|m| m.area_rect(egui::Id::unique("palette")));
    j.note(format!(
        "the list of every key is the command finder with nothing typed; its window is {:.0} px high",
        list.map_or(0.0, |r| r.height())
    ));
    j.expect(
        "the-key-list-opens",
        keys && j.shown("every key"),
        "Help > Keyboard shortcuts opens the list of keys",
    );
    j.expect(
        "the-key-list-shows-its-rows",
        list.is_some_and(|r| r.height() > 300.0),
        "the list shows more than a few of its two hundred rows at a time",
    );
    j.key(Key::Escape);

    j.act("fold a panel, then put everything back from the settings: cmd comma, Layout, reset the layout");
    j.press_hinted("fold Colour to its title");
    j.chord(CMD, Key::Comma);
    j.look("settings");
    j.click_containing(" Layout");
    j.look("settings-layout");
    j.click("reset the layout");
    j.look("layout-reset");
    let unfolded = j
        .ed()
        .and_then(|ed| ed.settings.layout.pane(crate::layout::Pane::Tool).map(|p| !p.collapsed));
    j.expect(
        "reset-the-layout-unfolds",
        unfolded == Some(true),
        "reset the layout opens the folded panel again",
    );
    j.key(Key::Escape);

    j.act("close the Layers panel by accident and get it back from the Window menu");
    j.press_hinted("close Layers; cmd K brings it back");
    j.look("layers-closed");
    j.click("Window");
    j.click("Layers");
    j.look("layers-back");
    j.expect(
        "the-window-menu-brings-a-panel-back",
        j.ed()
            .is_some_and(|ed| ed.settings.layout.is_open(crate::layout::Pane::Layers)),
        "Window > Layers brings the closed panel back",
    );

    j.act("hide everything with Window > Hide panels, then look for the way back");
    j.click("Window");
    j.click_containing("Hide panels");
    j.look("panels-hidden");
    let bar = j.in_sight("Window");
    j.note(format!(
        "with the panels hidden the menu bar is {}; the status line says: {}",
        if bar { "still there" } else { "gone" },
        j.heard()
    ));
    j.expect(
        "hidden-panels-leave-a-way-back-in-sight",
        bar || j.shown("Tab"),
        "something on screen says how to get the panels back",
    );
    if bar {
        j.click("Window");
        j.click_containing("Hide panels");
    } else {
        j.click_containing("show the panels");
    }
    j.look("panels-back");
    j.expect(
        "the-panels-come-back",
        j.in_sight("Window") && j.ed().is_some_and(|ed| !ed.settings.layout.hidden),
        "the panels come back",
    );

    j.act("lose the drawing: zoom far in and scroll away, then press F");
    j.pinch(512.0, 640.0, 8.0);
    j.wheel(512.0, 640.0, vec2(0.0, -4000.0), Modifiers::NONE);
    j.look("lost");
    let middle = j.at(512.0, 640.0);
    let lost = j.ed().is_some_and(|ed| !ed.canvas_rect.contains(middle));
    j.note(format!(
        "the middle of the drawing is {} the canvas area, zoom {:.2}",
        if lost { "outside" } else { "inside" },
        zoom(&j)
    ));
    j.key(Key::F);
    j.look("found-again");
    let middle = j.at(512.0, 640.0);
    j.expect(
        "f-brings-the-drawing-back",
        j.ed().is_some_and(|ed| ed.canvas_rect.contains(middle)) && zoom(&j) < 1.0,
        "F shows the whole drawing again",
    );

    j.act("make three mistakes, then read them back in Window > Message log");
    j.key(Key::Escape);
    j.key(Key::Delete);
    j.chord(CMD, Key::V);
    j.key(Key::ArrowLeft);
    j.look("mistakes");
    j.click("Window");
    j.click_containing("Message log");
    j.look("message-log");
    let kept = ["select a shape or strokes with V first", "nothing is copied yet"]
        .iter()
        .all(|text| j.h.query_all_by_label_contains(text).count() > 0);
    j.expect(
        "the-message-log-keeps-what-was-said",
        kept,
        "the message log lists what klad said",
    );
    j.press_hinted(&format!("close {} Messages", crate::theme::icon::LOG));

    j.act("something goes wrong: an export into a folder that cannot be made; then Help > Report a problem…");
    j.menu("File", "Export", "Image… cmd E");
    j.retype("folder", "lantern.klad/doc.json/out");
    j.click("Export");
    j.settle();
    let error = j.heard();
    j.press_hinted(&format!("close {} Export image", crate::theme::icon::EXPORT));
    j.click("Help");
    let report = j.click_containing("Report a problem");
    j.look("report");
    let holds = j.h.query_all_by_label_contains("could not be made").count() > 0
        || j.h
            .query_all(
                egui_kittest::kittest::By::new()
                    .predicate(|n| n.value().is_some_and(|v| v.contains("could not be made"))),
            )
            .count()
            > 0;
    j.note(format!("the error was: {error}"));
    j.expect(
        "the-report-holds-the-error",
        report && holds,
        "the report holds the error that was just shown",
    );
    j.click("email it to support");
    j.look("report-emailed");
    let sent = j.ed().and_then(|ed| ed.opened.last().cloned()).unwrap_or_default();
    j.note(format!("it opened: {}", sent.chars().take(80).collect::<String>()));
    j.expect(
        "the-report-goes-to-an-email",
        sent.starts_with("mailto:"),
        "email it to support opens an email",
    );
    j.press_hinted(&format!("close {} Report a problem", crate::theme::icon::LOG));

    j.act("the other ways to reach help: Help > Guides, What's new, Contact support, About");
    let links: Vec<String> = ["Guides", "What's new", "Contact support"]
        .iter()
        .map(|item| {
            j.click("Help");
            j.click_containing(item);
            j.ed().and_then(|ed| ed.opened.last().cloned()).unwrap_or_default()
        })
        .collect();
    j.note(format!(
        "they open: {:?}",
        links
            .iter()
            .map(|l| l.chars().take(60).collect::<String>())
            .collect::<Vec<_>>()
    ));
    j.expect(
        "help-links-lead-somewhere",
        links[0].starts_with("https://") && links[1].starts_with("https://") && links[2].starts_with("mailto:"),
        "Guides and What's new open the site, Contact support an email",
    );
    j.click("Help");
    j.click_containing("About klad");
    j.look("about");
    j.expect(
        "about-shows-the-version",
        j.h.query_all_by_label_contains(crate::VERSION).count() > 0,
        "About shows the version",
    );
    j.press_hinted(&format!("close {} About klad", crate::theme::icon::INFO));

    j.act("leave and come back: go to flats with V, close with cmd W, open it again from the recent row");
    j.click("flats");
    j.key(Key::V);
    j.pinch(512.0, 640.0, 2.0);
    let left = (active_name(&j), j.ed().map(|ed| format!("{:?}", ed.tool)), zoom(&j));
    j.chord(CMD, Key::W);
    j.look("closed");
    j.click("lantern");
    j.look("reopened");
    let back = (active_name(&j), j.ed().map(|ed| format!("{:?}", ed.tool)), zoom(&j));
    j.note(format!(
        "left on {} with {:?} at zoom {:.2}; came back on {} with {:?} at zoom {:.2}",
        left.0, left.1, left.2, back.0, back.1, back.2
    ));
    j.expect(
        "reopening-opens-the-drawing",
        j.ed().is_some(),
        "the drawing opens again from the recent row",
    );

    j.finish(&[]);
}

fn ring(cx: f32, cy: f32, rx: f32, ry: f32, gap: f32) -> Vec<(f32, f32, f32)> {
    let steps = 28;
    (0..=steps)
        .map(|i| i as f32 / steps as f32 * (std::f32::consts::TAU - gap))
        .map(|a| (cx + rx * a.cos(), cy + ry * a.sin(), 0.45 + 0.4 * (a * 0.5).sin().abs()))
        .collect()
}

fn last_stroke(j: &Journey, layer: &str) -> Option<klad_core::stroke::Stroke> {
    layer_marks(j, layer).iter().rev().find_map(|m| m.as_stroke().cloned())
}

fn fills(j: &Journey, layer: &str) -> Vec<[f32; 4]> {
    layer_marks(j, layer)
        .iter()
        .filter(|m| m.as_fill().is_some())
        .filter_map(|m| m.bounds())
        .collect()
}

fn brush_named(j: &mut Journey, name: &str) -> bool {
    if j.click(name) {
        return true;
    }
    j.pick_in_list("the brush; the brush panel shows them with a sample stroke", name)
}

fn layer_of(j: &Journey, name: &str) -> Option<klad_core::doc::Layer> {
    j.ed().and_then(|ed| ed.scene.bundle.doc.find_layer(name).ok().cloned())
}

fn rename_layer(j: &mut Journey, from: &str, to: &str) -> bool {
    j.double_click(from);
    j.type_text(to);
    j.key(Key::Enter);
    names(j).iter().any(|n| n == to)
}

#[test]
fn someone_sketches_inks_and_colours_a_picture() {
    let mut j = Journey::begin("illustrate-sketch-ink-colour", WINDOW, None);

    j.act("make a drawing called mushroom and look at what it starts with");
    let made = new_drawing(&mut j, "mushroom");
    j.look("new");
    if !made {
        return j.finish(&[]);
    }
    j.note(format!(
        "it starts with the layers {:?}, on {}",
        tree(&j),
        active_name(&j)
    ));

    j.act("sketch on the layer called sketch: click it, take the Pencil, draw the cap with a pen");
    j.click("sketch");
    let pencil = brush_named(&mut j, "Pencil");
    let before = j.ed().map_or(0, |ed| ed.scene.total_marks());
    j.pen_stroke(&ring(512.0, 400.0, 220.0, 150.0, 0.0));
    j.look("sketched-on-sketch");
    let landed = j.ed().map_or(0, |ed| ed.scene.total_marks()) > before;
    j.note(format!(
        "the Pencil was {}; the layer in hand is {}; it said: {}",
        if pencil { "found" } else { "not found" },
        active_name(&j),
        j.heard()
    ));
    j.expect(
        "the-sketch-layer-takes-a-pencil",
        landed,
        "a pencil stroke with the layer called sketch in hand lands in the drawing",
    );

    j.act("sketch on the paint layer then: the cap, the stem and the ground");
    j.click("paint 1");
    j.pen_stroke(&ring(512.0, 400.0, 220.0, 150.0, 0.0));
    j.pen_stroke(&[
        (440.0, 540.0, 0.4),
        (430.0, 760.0, 0.7),
        (600.0, 760.0, 0.7),
        (585.0, 540.0, 0.4),
    ]);
    j.pen_stroke(&[(200.0, 780.0, 0.3), (512.0, 770.0, 0.8), (830.0, 785.0, 0.3)]);
    j.look("sketched");
    let sketch = layer_marks(&j, "paint 1");
    let pressed: Vec<f32> = sketch
        .first()
        .and_then(|m| m.as_stroke())
        .map(|s| s.samples.iter().map(|p| p.pressure()).collect())
        .unwrap_or_default();
    let (light, hard) = pressed
        .iter()
        .fold((f32::MAX, 0.0f32), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
    j.note(format!(
        "three strokes: {}; the first is a {} stroke pressed {light:.2} to {hard:.2}",
        sketch.len() == 3,
        sketch
            .first()
            .and_then(|m| m.as_stroke())
            .map_or("no", |s| s.brush.as_str())
    ));
    j.expect(
        "a-pen-sketches-with-its-pressure",
        sketch.len() == 3 && hard - light > 0.2,
        "three pencil strokes land with the pen's pressure in them",
    );

    j.act("turn the sketch down to see through it: drag the layer's opacity");
    let field =
        j.h.query_all_by_label("opacity")
            .map(|n| n.rect())
            .find(|r| r.center().x > 1050.0);
    match field {
        Some(r) => j.drag_screen(
            r.center(),
            r.center() - vec2(70.0, 0.0),
            "the layer's opacity number 70 px to the left",
        ),
        None => j.note("no layer opacity in sight"),
    }
    j.look("sketch-faint");
    let faint = layer_of(&j, "paint 1").map(|l| l.opacity);
    j.note(format!("the sketch layer's opacity is {faint:?}"));
    j.expect(
        "layer-opacity-turns-the-sketch-down",
        faint.is_some_and(|o| o < 0.6),
        "dragging the layer's opacity makes the sketch faint",
    );

    j.act("a new layer for the ink, called ink");
    j.click("Layer");
    j.click("New paint layer");
    let fresh = active_name(&j);
    let named = rename_layer(&mut j, &fresh, "ink");
    j.look("ink-layer");
    j.note(format!("the layers are now {:?}", tree(&j)));
    j.expect(
        "a-new-layer-is-named-by-double-click",
        named && active_name(&j) == "ink",
        "the new layer is above the sketch and takes its name from a double click",
    );

    j.act("ink the cap and the stem with the Inking pen at size 8");
    let ink = brush_named(&mut j, "Inking pen");
    j.click("8");
    j.pen_stroke(&ring(512.0, 400.0, 220.0, 150.0, 0.0));
    j.pen_stroke(&[
        (440.0, 545.0, 0.5),
        (430.0, 760.0, 0.8),
        (600.0, 760.0, 0.8),
        (585.0, 545.0, 0.5),
    ]);
    j.look("inked");
    let line = last_stroke(&j, "ink");
    j.note(format!(
        "the ink layer has {} strokes; the last is {:?} at size {:?}",
        count_on(&j, "ink"),
        line.as_ref().map(|s| s.brush.clone()),
        line.as_ref().map(|s| s.size)
    ));
    j.expect(
        "ink-lands-on-the-ink-layer",
        ink && count_on(&j, "ink") == 2 && line.is_some_and(|s| s.brush == "ink" && (s.size - 8.0).abs() < 0.1),
        "two inked lines land on the ink layer at the size picked",
    );

    j.act("a line is too long: erase a piece of it with E, then go back to the brush with B");
    let inked = j.picture();
    j.key(Key::E);
    j.drag(&[(515.0, 725.0), (515.0, 795.0)]);
    j.look("erased");
    let erased = j.picture();
    j.key(Key::B);
    let back = j.ed().map(|ed| (ed.tool, ed.brush.clone()));
    j.note(format!("after B the tool and brush are {back:?}"));
    j.expect(
        "the-eraser-takes-ink-off",
        erased != inked,
        "an eraser stroke across the line changes the picture",
    );
    j.expect(
        "b-brings-the-ink-brush-back",
        back.is_some_and(|(tool, brush)| tool == Tool::Brush && brush == "ink"),
        "B after the eraser is the Inking pen again",
    );

    j.act("a layer for the flat colours, under the ink: click the sketch layer, then Layer > New paint layer");
    j.click("paint 1");
    j.click("Layer");
    j.click("New paint layer");
    let fresh = active_name(&j);
    let named = rename_layer(&mut j, &fresh, "flats");
    j.look("flats-layer");
    let order = names(&j);
    j.note(format!("the layers are now {order:?}"));
    let under = order
        .iter()
        .position(|n| n == "ink")
        .zip(order.iter().position(|n| n == "flats"))
        .is_some_and(|(ink, flats)| ink < flats);
    j.expect(
        "a-new-layer-lands-above-the-layer-in-hand",
        named && under,
        "a layer made with the sketch in hand lands above the sketch and under the ink",
    );

    j.act("fill the cap with red: G, a red, a click inside the cap");
    j.key(Key::G);
    j.retype("hex", "#c8342c");
    j.tap(512.0, 400.0);
    j.look("first-fill");
    let first = fills(&j, "flats");
    j.note(format!(
        "the fill covers {first:?}; the cap's line covers about 292,250 to 732,550; it said: {}",
        j.heard()
    ));
    let inside = |b: &[f32; 4]| b[0] > 250.0 && b[1] > 200.0 && b[2] < 780.0 && b[3] < 600.0;
    j.expect(
        "a-fill-on-the-colour-layer-stops-at-the-ink-above",
        first.len() == 1 && first.iter().all(inside),
        "a click inside the inked cap, on the layer under the ink, fills the cap",
    );

    j.act("it filled the whole page: undo, tell the fill to stop at everything visible, and click again");
    if !(first.len() == 1 && first.iter().all(inside)) {
        j.chord(CMD, Key::Z);
    }
    j.pick_in_list(
        "the lines the fill stops at: this layer, everything visible, or a layer such as your lines",
        "all visible layers",
    );
    if fills(&j, "flats").is_empty() {
        j.tap(512.0, 400.0);
    }
    j.look("cap-filled");
    let cap = fills(&j, "flats");
    j.note(format!("the fill covers {cap:?}"));
    j.expect(
        "a-fill-stops-at-visible-lines",
        cap.len() == 1 && cap.iter().all(inside),
        "with everything visible as its lines the fill stays inside the cap",
    );

    j.act("the stem in a cream colour; its line was left open at the top");
    j.retype("hex", "#e8dcc0");
    j.tap(512.0, 660.0);
    j.look("stem-filled");
    let stem = fills(&j, "flats");
    j.note(format!("the fills cover {stem:?}; it said: {}", j.heard()));
    j.expect(
        "a-fill-in-an-open-shape-is-held-or-explained",
        stem.len() == 2 && stem.last().is_some_and(|b| b[2] - b[0] < 400.0),
        "the stem, closed by the cap above it, fills without running over the page",
    );

    j.act("shade the cap: a new layer above the flats, clipped to them and set to multiply, a soft grey stroke");
    j.click("Layer");
    j.click("New paint layer");
    let fresh = active_name(&j);
    rename_layer(&mut j, &fresh, "shade");
    j.click("Layer");
    let clipped = j.click_containing("Clip to layer below");
    j.pick_in_list("how the active layer mixes with what is below it", "multiply");
    j.key(Key::B);
    let soft = brush_named(&mut j, "Soft");
    j.click("64");
    j.retype("hex", "#808080");
    let flat = j.picture();
    j.pen_stroke(&[(330.0, 470.0, 0.6), (512.0, 520.0, 0.9), (700.0, 470.0, 0.6)]);
    j.look("shaded");
    let shade = layer_of(&j, "shade");
    j.note(format!(
        "the shade layer: clip {:?}, blend {:?}, {} strokes",
        shade.as_ref().map(|l| l.clip),
        shade.as_ref().map(|l| l.blend.name()),
        count_on(&j, "shade")
    ));
    let shown = j.picture();
    j.expect(
        "a-clipped-multiply-layer-shades",
        clipped && soft && shade.is_some_and(|l| l.clip) && count_on(&j, "shade") == 1 && shown != flat,
        "a soft stroke on a layer clipped to the flats shades them",
    );

    j.act("take the cap's red back from the picture: hold alt and click the cap");
    j.tap_with(420.0, 340.0, egui::PointerButton::Primary, Modifiers::ALT);
    j.look("picked-colour");
    let colour = j.ed().map(|ed| crate::color::hex(ed.color));
    j.note(format!("the colour in hand is {colour:?}"));
    j.expect(
        "alt-click-picks-a-colour-from-the-picture",
        colour.as_deref().is_some_and(|c| c != "#808080"),
        "alt and a click take the colour under the pointer",
    );

    j.act("hide the sketch and look at the picture");
    let with = j.picture();
    j.scroll_to_hinted("show or hide paint 1");
    j.press_hinted("show or hide paint 1");
    j.look("sketch-hidden");
    let without = j.picture();
    j.expect(
        "the-eye-hides-the-sketch",
        without != with && layer_of(&j, "paint 1").is_some_and(|l| !l.visible),
        "the eye hides the sketch layer",
    );

    j.finish(&[]);
}

fn view_of(j: &Journey) -> (f32, bool) {
    j.ed()
        .and_then(|ed| ed.view)
        .map_or((0.0, false), |v| (v.angle.to_degrees(), v.flip))
}

fn rendered(j: &mut Journey, drawing: &str) -> String {
    let (_, said) = j.agent(&["render", drawing]);
    said.split_whitespace()
        .skip_while(|w| *w != "hash")
        .nth(1)
        .unwrap_or_default()
        .to_string()
}

#[test]
fn someone_fixes_and_adjusts_a_painting() {
    let mut j = Journey::begin("illustrate-fix-adjust", WINDOW, Some("ink-shapes.klad"));

    j.act("open an inked drawing and flip the view with H, to see it fresh");
    j.look("opened");
    if j.ed().is_none() {
        return j.finish(&[]);
    }
    let drawing = j.drawing();
    let said = j.heard();
    j.key(Key::H);
    j.look("flipped");
    let flipped = view_of(&j).1;
    j.note(format!("the view is flipped: {flipped}; it said: {}", j.heard()));
    j.expect("h-flips-the-view", flipped, "H mirrors the view");
    j.expect(
        "a-flipped-view-says-so",
        j.heard() != said || j.shown("flipped") || j.shown("mirrored"),
        "something on screen says that the view is mirrored, not the drawing",
    );
    j.key(Key::H);

    j.act("turn the view with R to draw at an easier angle, and draw a line with the pen");
    j.key(Key::R);
    j.key(Key::R);
    j.look("turned");
    j.note(format!("the view is turned {:.0} degrees", view_of(&j).0));
    j.click("lines");
    let before = count_on(&j, "lines");
    j.pen_stroke(&[(300.0, 620.0, 0.5), (420.0, 640.0, 0.9), (540.0, 620.0, 0.5)]);
    j.look("drawn-turned");
    let line = last_stroke(&j, "lines");
    let start = line.as_ref().and_then(|s| s.samples.first().map(|p| p.pos()));
    j.note(format!("the line starts at {start:?}; it was drawn from 300,620"));
    j.expect(
        "a-line-lands-under-the-pen-in-a-turned-view",
        count_on(&j, "lines") == before + 1
            && start.is_some_and(|(x, y)| (x - 300.0).abs() < 6.0 && (y - 620.0).abs() < 6.0),
        "a line drawn in a turned view lands where the pen was",
    );
    j.chord(Modifiers::ALT, Key::R);
    j.look("upright");
    j.expect(
        "alt-r-puts-the-view-upright",
        view_of(&j).0.abs() < 0.5,
        "alt R puts the view upright again",
    );
    j.chord(CMD, Key::Z);

    j.act("the shape at the top left sits wrong: drag a box around it with S and mirror it with the bar's flip");
    j.key(Key::S);
    j.drag(&[(70.0, 30.0), (270.0, 215.0)]);
    j.look("looped");
    let taken = picked_ids(&j);
    let first = taken.first().copied().unwrap_or_default();
    let head = |j: &Journey| {
        mark_of(j, "lines", first).and_then(|m| m.as_stroke().and_then(|s| s.samples.first().map(|p| p.pos())))
    };
    let was = (box_of(&layer_marks(&j, "lines"), &taken), head(&j));
    j.press_hinted("flip the selection left-right");
    j.look("flipped-selection");
    let now = (box_of(&layer_marks(&j, "lines"), &taken), head(&j));
    j.note(format!(
        "the box took {} strokes; their box went from {:?} to {:?}, the first stroke's start from {:?} to {:?}",
        taken.len(),
        was.0,
        now.0,
        was.1,
        now.1
    ));
    j.expect(
        "the-bar-flips-a-selection-in-place",
        !taken.is_empty() && near(was.0, now.0, 1.5) && was.1 != now.1,
        "the flip mirrors the selected strokes inside their own box",
    );

    j.act("make it smaller by a corner of its box");
    if let Some(c) = handle_corners(&j) {
        let centre = (c[0].to_vec2() + c[2].to_vec2()) / 2.0;
        let inward = (centre - c[2].to_vec2()).normalized() * 30.0;
        j.drag_screen(c[2], c[2] + inward, "the bottom right corner 30 px inwards");
    }
    j.look("smaller");
    let small = box_of(&layer_marks(&j, "lines"), &taken);
    let width = |b: Option<[f32; 4]>| b.map_or(0.0, |b| b[2] - b[0]);
    j.expect(
        "a-corner-makes-the-selection-smaller",
        width(small) < width(now.0) * 0.95,
        "a corner dragged inwards makes the selection smaller",
    );
    j.key(Key::Escape);

    j.act("rub out a part with a bigger eraser: E, ] three times, a drag over the lines");
    j.key(Key::E);
    let size = j.ed().map_or(0.0, |ed| ed.size);
    (0..3).for_each(|_| j.key(Key::CloseBracket));
    let pictured = j.picture();
    j.drag(&[(380.0, 480.0), (560.0, 600.0)]);
    j.look("rubbed-out");
    let grown = j.ed().map_or(0.0, |ed| ed.size);
    let after = j.picture();
    j.note(format!("the eraser went from {size:.1} to {grown:.1} px"));
    j.expect(
        "a-bigger-eraser-rubs-out",
        grown > size * 1.5 && after != pictured,
        "the eraser grows with ] and rubs the lines out",
    );
    j.key(Key::B);
    let brush_size = j.ed().map_or(0.0, |ed| ed.size);
    j.note(format!("back on the brush its size is {brush_size:.1} px"));
    j.expect(
        "the-eraser-keeps-its-own-size",
        (brush_size - size).abs() < 0.1,
        "making the eraser bigger leaves the brush as it was",
    );

    j.act(
        "take the colour out of everything with an adjustment layer: Layer > New adjustment layer, hue and saturation",
    );
    let painting = j.picture();
    j.click("Layer");
    j.click("New adjustment layer");
    j.look("adjustment-added");
    let list = "what this adjustment layer does to the layers under it in its group (or the one it is clipped to)";
    let seen = j.in_sight(list);
    j.note(format!(
        "the layer in hand is {}; its settings are {}",
        active_name(&j),
        if seen { "in sight" } else { "below the fold" }
    ));
    j.expect(
        "an-adjustment-layers-settings-are-in-sight",
        seen,
        "the new adjustment layer's settings are in sight",
    );
    if !seen {
        j.press_hinted("fold Colour to its title");
        j.press_hinted("fold Brush to its title");
        j.scroll_to(list);
    }
    let before = j.picture();
    j.note(format!(
        "the new adjustment layer, as it comes, {} the picture",
        if before == painting { "leaves" } else { "changes" }
    ));
    let chosen = j.pick_in_list(list, "hue and saturation");
    j.scroll_to("lightness");
    j.click_inside("lightness", 0.95, 0.5);
    j.look("adjusted");
    let after = j.picture();
    j.expect(
        "an-adjustment-changes-the-picture",
        chosen && after != before,
        "hue and saturation on an adjustment layer changes the picture under it",
    );
    j.press_hinted(&format!("show or hide {}", active_name(&j)));
    let hidden = j.picture();
    j.expect(
        "hiding-the-adjustment-brings-the-picture-back",
        hidden == painting,
        "with the adjustment layer hidden the picture is as it was",
    );

    j.act("hide a part without erasing it: a mask on the lines, painted with the brush");
    j.click("lines");
    let masked = j.menu("Layer", "Mask", "Add mask");
    j.look("mask-added");
    j.note(format!(
        "the layer in hand is {}; it said: {}",
        active_name(&j),
        j.heard()
    ));
    let strokes = count_on(&j, "lines");
    let before = j.picture();
    j.key(Key::B);
    (0..4).for_each(|_| j.key(Key::CloseBracket));
    j.drag(&[(640.0, 80.0), (860.0, 200.0)]);
    j.look("masked");
    let after = j.picture();
    j.expect(
        "a-mask-hides-without-erasing",
        masked && after != before && count_on(&j, "lines") == strokes,
        "painting on the mask hides a part of the lines and leaves the strokes as they were",
    );
    let back = j.menu("Layer", "Mask", "Back to layer");
    j.note(format!(
        "back on the layer: {back}; the layer in hand is {}",
        active_name(&j)
    ));

    j.act("bring in a picture to draw from: File > Import picture…, then mark it as a reference");
    let hash = rendered(&mut j, &drawing);
    let photo = j.folder.join("photo.png").display().to_string();
    let (_, shot) = j.agent(&["render", &drawing, "--out", &photo]);
    j.note(format!(
        "made a picture to bring in: {}",
        shot.lines().next().unwrap_or_default()
    ));
    j.click("File");
    j.click_containing("Import picture");
    j.look("import-window");
    j.type_into("PNG or JPEG file", &photo);
    j.click("import");
    j.look("imported");
    let layers = names(&j);
    j.note(format!(
        "the layers are now {layers:?}; the layer in hand is {}; it said: {}",
        active_name(&j),
        j.heard()
    ));
    j.expect(
        "a-picture-comes-in-as-a-layer",
        layers.len() == 4,
        "the picture is on a new layer",
    );
    j.scroll_to("reference");
    let ticked = j.click("reference");
    j.look("reference");
    let (_, listed) = j.agent(&["ls", &drawing]);
    j.note(format!(
        "the agent sees the layers: {}",
        listed.lines().take(4).collect::<Vec<_>>().join(" / ")
    ));
    let reference = layer_of(&j, &active_name(&j)).is_some_and(|l| l.reference);
    j.expect(
        "a-layer-is-marked-as-a-reference",
        ticked && reference,
        "the reference box in Layer settings marks the layer",
    );
    let now = rendered(&mut j, &drawing);
    j.expect(
        "a-reference-stays-out-of-the-picture",
        !hash.is_empty() && now == hash,
        "the rendered picture is the same with the reference in the drawing",
    );

    j.act("put the picture away and soften an edge with the Smudge brush");
    j.press_hinted("show or hide photo");
    j.click("lines");
    let smudge = brush_named(&mut j, "Smudge");
    let before = (count_on(&j, "lines"), j.picture());
    j.drag(&[(120.0, 500.0), (260.0, 560.0)]);
    j.look("smudged");
    let after = (count_on(&j, "lines"), j.picture());
    j.note(format!(
        "lines went from {} to {} marks; the last one uses {:?}",
        before.0,
        after.0,
        last_stroke(&j, "lines").map(|s| s.brush)
    ));
    j.expect(
        "smudge-moves-the-paint",
        smudge && after.0 == before.0 + 1 && after.1 != before.1,
        "a smudge stroke over the lines changes them",
    );

    j.finish(&[]);
}

#[test]
fn someone_paints_fast_with_brushes_sizes_and_opacity() {
    let mut j = Journey::begin("illustrate-brushes", WINDOW, None);
    let size = |j: &Journey| j.ed().map_or(0.0, |ed| ed.size);
    let brush = |j: &Journey| j.ed().map_or_else(String::new, |ed| ed.brush.clone());

    j.act("make a drawing and change the brush size with the keys: ] three times, [ once");
    let made = new_drawing(&mut j, "brushes");
    if !made {
        return j.finish(&[]);
    }
    let first = size(&j);
    (0..3).for_each(|_| j.key(Key::CloseBracket));
    let bigger = size(&j);
    j.key(Key::OpenBracket);
    let smaller = size(&j);
    j.look("sizes");
    j.note(format!(
        "the size went from {first:.1} to {bigger:.1} and back to {smaller:.1}"
    ));
    j.expect(
        "the-bracket-keys-size-the-brush",
        bigger > first * 1.8 && smaller < bigger,
        "] makes the brush bigger and [ smaller",
    );

    j.act("pick a size from the Brush panel, and half the opacity in the bar");
    j.click("32");
    let slider = |j: &Journey| {
        j.h.query_all_by_label("opacity")
            .filter(|n| n.accesskit_node().role() == egui::accesskit::Role::Slider)
            .map(|n| n.rect())
            .find(|r| r.center().y < 70.0)
    };
    if let Some(r) = slider(&j) {
        j.drag_screen(
            r.center(),
            r.center(),
            "a click in the middle of the bar's opacity slider",
        );
    }
    j.drag(&[(120.0, 120.0), (400.0, 150.0)]);
    j.look("size-and-opacity");
    let stroke = last_stroke(&j, "paint 1");
    j.note(format!(
        "the stroke has size {:?} and opacity {:?}",
        stroke.as_ref().map(|s| s.size),
        stroke.as_ref().map(|s| s.opacity)
    ));
    j.expect(
        "a-size-tile-and-the-opacity-slider-set-the-stroke",
        stroke.is_some_and(|s| (s.size - 32.0).abs() < 0.1 && s.opacity > 0.3 && s.opacity < 0.7),
        "the stroke takes the size of the tile and the opacity of the slider",
    );
    if let Some(r) = slider(&j) {
        let end = egui::pos2(r.right() - 1.0, r.center().y);
        j.drag_screen(
            r.center(),
            end + vec2(30.0, 0.0),
            "the opacity slider all the way to the right",
        );
    }
    j.click("12");

    j.act("paint a line with every brush in the list, top to bottom");
    let listed: Vec<(String, String, bool)> = j.ed().map_or_else(Vec::new, |ed| {
        ed.brushes
            .defs()
            .map(|(id, def)| {
                let erases = ed
                    .brushes
                    .get(id)
                    .is_ok_and(|b| b.blend == klad_brush::brush::StrokeBlend::Erase);
                (id.to_string(), def.name.clone(), erases)
            })
            .collect()
    });
    let mut wrong = Vec::new();
    listed.iter().enumerate().for_each(|(i, (id, name, erases))| {
        let before = count_on(&j, "paint 1");
        let picked = j.pick_in_list("the brush; the brush panel shows them with a sample stroke", name);
        let y = 220.0 + (i % 12) as f32 * 62.0;
        let x = 100.0 + (i / 12) as f32 * 460.0;
        j.pen_stroke(&[(x, y, 0.4), (x + 180.0, y + 12.0, 0.9), (x + 360.0, y, 0.5)]);
        let used = last_stroke(&j, "paint 1").map(|s| s.brush);
        let kept = match erases {
            true => count_on(&j, "paint 1") == before,
            false => count_on(&j, "paint 1") == before + 1 && used.as_deref() == Some(id.as_str()),
        };
        if !picked || !kept {
            wrong.push(format!(
                "{name}: picked {picked}, marks {} to {}, used {used:?}",
                before,
                count_on(&j, "paint 1")
            ));
        }
    });
    j.look("every-brush");
    j.note(format!(
        "{} brushes in the list; not as expected: {wrong:?}",
        listed.len()
    ));
    j.expect(
        "every-brush-in-the-list-paints",
        listed.len() > 20 && wrong.is_empty(),
        "each brush of the list can be picked and leaves its own stroke; an eraser over empty paper leaves none",
    );

    j.act("press lightly and then hard with the Inking pen");
    brush_named(&mut j, "Inking pen");
    j.pen_stroke(&[(560.0, 100.0, 0.08), (700.0, 110.0, 0.5), (900.0, 100.0, 1.0)]);
    j.look("pressure");
    let pressed: Vec<f32> = last_stroke(&j, "paint 1")
        .map(|s| s.samples.iter().map(|p| p.pressure()).collect())
        .unwrap_or_default();
    let (light, hard) = pressed
        .iter()
        .fold((f32::MAX, 0.0f32), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
    j.note(format!("the stroke holds pressures from {light:.2} to {hard:.2}"));
    j.expect(
        "the-pens-pressure-is-in-the-stroke",
        light < 0.3 && hard > 0.8,
        "a stroke pressed lightly and then hard keeps both",
    );

    j.act("swap the two colours with X and back");
    let colour = |j: &Journey| j.ed().map(|ed| crate::color::hex(ed.color)).unwrap_or_default();
    let ink = colour(&j);
    j.key(Key::X);
    let swapped = colour(&j);
    j.key(Key::X);
    j.note(format!(
        "the colour went from {ink} to {swapped} and back to {}",
        colour(&j)
    ));
    j.expect(
        "x-swaps-the-colours",
        swapped != ink && colour(&j) == ink,
        "X swaps the colour in hand with the second one",
    );

    j.act("take the Marker, rub out with E, and go back with B");
    brush_named(&mut j, "Marker");
    j.key(Key::E);
    let erasing = brush(&j);
    j.key(Key::B);
    j.look("marker-again");
    j.note(format!("E gave {erasing}, B gave {}", brush(&j)));
    j.expect(
        "b-returns-to-the-brush-in-hand",
        erasing == "eraser" && brush(&j) == "marker",
        "B after the eraser is the brush that was in hand",
    );

    j.act("open the brush's more menu and give it a taper at the start");
    let opened = j.press_hinted("hardness, feather, pressure curve, taper, stabiliser and smoothing");
    j.look("more");
    j.click_inside("taper in px", 0.5, 0.5);
    j.key(Key::Escape);
    j.drag(&[(560.0, 160.0), (900.0, 170.0)]);
    j.look("tapered");
    let taper = last_stroke(&j, "paint 1").map(|s| s.taper);
    j.note(format!("the stroke's taper is {taper:?}"));
    j.expect(
        "the-more-menu-sets-a-taper",
        opened && taper.is_some_and(|t| t[0] > 0.0),
        "a taper set in the more menu is in the next stroke",
    );

    j.act("paint with the mirror on");
    let before = count_on(&j, "paint 1");
    let ticked = j.click("mirror");
    j.drag(&[(160.0, 960.0), (300.0, 990.0)]);
    j.look("mirrored");
    j.note(format!("one drag made {} marks", count_on(&j, "paint 1") - before));
    j.expect(
        "mirror-paints-both-sides",
        ticked && count_on(&j, "paint 1") == before + 2,
        "with mirror on, one stroke lands on both sides",
    );
    j.click("mirror");

    j.act("size the brush on the canvas: hold cmd and alt and drag sideways");
    let before = (size(&j), count_on(&j, "paint 1"));
    j.drag_with(
        &[(500.0, 500.0), (580.0, 500.0)],
        Modifiers {
            alt: true,
            ..Modifiers::COMMAND
        },
    );
    j.look("sized-on-canvas");
    j.note(format!("the size went from {:.1} to {:.1}", before.0, size(&j)));
    j.expect(
        "cmd-alt-drag-sizes-the-brush",
        size(&j) > before.0 && count_on(&j, "paint 1") == before.1,
        "a sideways drag with cmd and alt held changes the size and paints nothing",
    );

    j.act("twelve quick strokes, then twelve undos");
    let before = count_on(&j, "paint 1");
    (0..12).for_each(|i| {
        let y = 300.0 + i as f32 * 40.0;
        j.drag(&[(620.0, y), (940.0, y + 10.0)]);
    });
    let painted = count_on(&j, "paint 1");
    (0..12).for_each(|_| j.chord(CMD, Key::Z));
    j.look("undone");
    j.expect(
        "twelve-strokes-and-twelve-undos",
        painted == before + 12 && count_on(&j, "paint 1") == before,
        "twelve strokes land and twelve undos take them off",
    );

    j.act("move the paper with the space bar held, then draw a line that starts off the paper");
    let before = (centre(&j), count_on(&j, "paint 1"));
    j.hold(Key::Space, true);
    j.drag(&[(500.0, 500.0), (600.0, 560.0)]);
    j.hold(Key::Space, false);
    j.look("panned");
    let after = (centre(&j), count_on(&j, "paint 1"));
    j.note(format!(
        "the middle of the view went from {:.0},{:.0} to {:.0},{:.0}",
        before.0.x, before.0.y, after.0.x, after.0.y
    ));
    j.expect(
        "space-and-a-drag-move-the-paper",
        (after.0 - before.0).length() > 30.0 && after.1 == before.1,
        "with space held a drag moves the view and paints nothing",
    );
    j.key(Key::F);
    j.drag(&[(-60.0, 880.0), (140.0, 900.0)]);
    j.look("from-off-the-paper");
    let off = last_stroke(&j, "paint 1").and_then(|s| s.samples.first().map(|p| p.pos()));
    j.note(format!(
        "paint 1 went from {} to {} marks; the last one starts at {off:?}",
        after.1,
        count_on(&j, "paint 1")
    ));
    j.expect(
        "a-line-from-off-the-paper-lands",
        count_on(&j, "paint 1") == after.1 + 1,
        "a line that starts beside the paper and runs onto it is painted",
    );

    j.act("pick a colour on the wheel, keep it in the palette, and take it again from there");
    let ink = colour(&j);
    j.click_inside("colour wheel", 0.5, 0.06);
    let picked = colour(&j);
    let added = j.click_containing("add colour");
    j.look("palette");
    let kept = j.ed().map_or(0, |ed| ed.scene.bundle.doc.palette.len());
    j.retype("hex", "#101010");
    let swatch = j.click(&picked);
    j.look("from-the-palette");
    j.note(format!(
        "the wheel gave {picked} (it was {ink}); the palette holds {kept} colours; after a click on its square the colour is {}",
        colour(&j)
    ));
    j.expect(
        "the-wheel-and-the-palette-keep-a-colour",
        picked != ink && added && kept == 1 && swatch && colour(&j) == picked,
        "a colour from the wheel goes into the palette and comes back from it",
    );

    j.act("look at the brush itself: F5");
    j.key(Key::F5);
    j.look("brush-window");
    j.expect(
        "f5-opens-the-brush-window",
        j.ed().is_some_and(|ed| ed.windows.brush),
        "F5 opens the window that edits the brush",
    );
    j.key(Key::F5);

    j.finish(&[]);
}

fn shapes_in(drawing: &str) -> Vec<(String, String, [f32; 4])> {
    let out = std::process::Command::new(crate::start::test_binary())
        .args(["ls", drawing])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    out.lines()
        .filter(|l| l.starts_with("  "))
        .filter_map(|l| {
            let words: Vec<&str> = l.split_whitespace().collect();
            let (id, kind) = (words.first()?, words.get(1)?);
            let place: Vec<f32> = words.get(2)?.split(',').filter_map(|v| v.parse().ok()).collect();
            let size: Vec<f32> = words.get(3)?.split('x').filter_map(|v| v.parse().ok()).collect();
            (place.len() == 2 && size.len() == 2)
                .then(|| (id.to_string(), kind.to_string(), [place[0], place[1], size[0], size[1]]))
        })
        .collect()
}

fn shown_as(drawing: &str, id: &str) -> String {
    std::process::Command::new(crate::start::test_binary())
        .args(["show", drawing, id])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default()
}

#[test]
fn someone_draws_shapes_and_text() {
    let mut j = Journey::begin("illustrate-shapes-text", WINDOW, None);

    j.act("make a drawing and drag a rectangle with U");
    let made = new_drawing(&mut j, "label");
    if !made {
        return j.finish(&[]);
    }
    let drawing = j.drawing();
    let kinds = |drawing: &str, kind: &str| -> Vec<(String, [f32; 4])> {
        shapes_in(drawing)
            .into_iter()
            .filter(|(_, k, _)| k == kind)
            .map(|(id, _, b)| (id, b))
            .collect()
    };
    j.key(Key::U);
    j.drag(&[(200.0, 200.0), (500.0, 400.0)]);
    j.look("rectangle");
    let rects = kinds(&drawing, "rect");
    j.note(format!(
        "rectangles: {rects:?}; the layers are {:?}, in hand {}; it said: {}",
        tree(&j),
        active_name(&j),
        j.heard()
    ));
    j.expect(
        "u-drags-a-rectangle",
        rects.len() == 1
            && rects
                .iter()
                .all(|(_, b)| (b[2] - 300.0).abs() < 8.0 && (b[3] - 200.0).abs() < 8.0),
        "a drag with U makes a rectangle of the size dragged",
    );

    j.act("a circle: the ellipse button, and shift held while dragging");
    j.click("ellipse");
    j.drag_with(&[(600.0, 200.0), (800.0, 330.0)], Modifiers::SHIFT);
    j.look("circle");
    let circles = kinds(&drawing, "ellipse");
    j.note(format!("ellipses: {circles:?}"));
    j.expect(
        "shift-makes-a-circle",
        circles.len() == 1 && circles.iter().all(|(_, b)| (b[2] - b[3]).abs() < 3.0),
        "with shift held the ellipse is a circle",
    );

    j.act("pick the rectangle with V and give it a green fill and a dark outline from the bar");
    j.key(Key::V);
    j.tap(350.0, 300.0);
    j.look("rectangle-picked");
    let picked = j.ed().and_then(|ed| ed.selection.clone());
    j.press_hinted("the shape's fill");
    j.retype_leftmost("hex", "#3a7a4a");
    j.key(Key::Escape);
    if j.ed().is_some_and(|ed| ed.selection.is_none()) {
        j.tap(350.0, 300.0);
    }
    j.press_hinted("the shape's outline");
    j.retype_leftmost("hex", "#202020");
    j.look("rectangle-coloured");
    let look = picked.as_deref().map(|id| shown_as(&drawing, id)).unwrap_or_default();
    j.note(format!(
        "the rectangle is now: {}",
        look.split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(260)
            .collect::<String>()
    ));
    j.expect(
        "the-bar-colours-a-shape",
        look.contains("3a7a4a") && look.contains("202020"),
        "the bar's two colour buttons set the shape's fill and outline",
    );
    j.key(Key::Escape);
    j.key(Key::Escape);

    j.act("make the rectangle wider by its right edge, and turn it by the knob");
    let was = kinds(&drawing, "rect").first().map_or(0.0, |(_, b)| b[2]);
    let step = 40.0 / j.ed().and_then(|ed| ed.view).map_or(1.0, |v| v.zoom);
    j.tap(350.0, 300.0);
    if let Some(c) = handle_corners(&j) {
        let edge = c[1].lerp(c[2], 0.5);
        j.drag_screen(
            edge,
            edge + vec2(40.0, 0.0),
            "the middle of the right edge 40 px to the right",
        );
    }
    j.look("wider");
    let wide = kinds(&drawing, "rect");
    j.note(format!("rectangles: {wide:?}"));
    j.expect(
        "an-edge-stretches-a-shape",
        wide.iter()
            .any(|(_, b)| b[2] > was + step * 0.8 && (b[3] - 200.0).abs() < 8.0),
        "dragging an edge makes the rectangle wider and no taller",
    );
    if let Some(c) = handle_corners(&j) {
        let top = c[0].lerp(c[1], 0.5);
        let centre = (c[0].to_vec2() + c[2].to_vec2()) / 2.0;
        let knob = top + (top.to_vec2() - centre).normalized() * 30.0;
        j.drag_screen(
            knob,
            knob + vec2(40.0, 12.0),
            "the knob above the rectangle to the right",
        );
    }
    j.look("turned");
    let turned = picked.as_deref().map(|id| shown_as(&drawing, id)).unwrap_or_default();
    j.expect(
        "the-knob-turns-a-shape",
        turned.contains("\"rotate\"") && !turned.contains("\"rotate\": 0.0"),
        "the knob turns the rectangle",
    );
    j.chord(CMD, Key::Z);
    j.key(Key::Escape);

    j.act("draw a roof with the pen: three clicks and a click on the first point to close it");
    j.key(Key::P);
    j.tap(200.0, 620.0);
    j.tap(350.0, 520.0);
    j.tap(500.0, 620.0);
    j.tap(200.0, 620.0);
    j.look("roof");
    let paths = kinds(&drawing, "path");
    j.note(format!("paths: {paths:?}; it said: {}", j.heard()));
    j.expect(
        "the-pen-closes-a-path",
        paths.len() == 1 && paths.iter().all(|(_, b)| (b[2] - 300.0).abs() < 12.0),
        "three clicks and a click on the first point make a closed shape",
    );

    j.act("lift the top of the roof: A, a click on the shape, a drag on its top point");
    let tall = kinds(&drawing, "path").first().map_or(0.0, |(_, b)| b[3]);
    let lift = 30.0 / j.ed().and_then(|ed| ed.view).map_or(1.0, |v| v.zoom);
    j.key(Key::A);
    j.tap(350.0, 590.0);
    j.look("roof-points");
    let anchors: Vec<[f32; 2]> = j
        .ed()
        .and_then(|ed| ed.shaped.as_ref())
        .map(|s| {
            klad_core::pathedit::anchors(&s.subs)
                .iter()
                .map(|(_, a)| a.at)
                .collect()
        })
        .unwrap_or_default();
    j.note(format!("the roof shows {} points", anchors.len()));
    if let Some(top) = anchors.iter().min_by(|a, b| a[1].total_cmp(&b[1])) {
        let from = j.at(top[0], top[1]);
        j.drag_screen(from, from - vec2(0.0, 30.0), "the top point 30 px up");
        j.wait("the reshaped roof", |ed| {
            ed.point_drag.is_none() && ed.path_commit.is_none()
        });
    }
    j.look("roof-lifted");
    let lifted = kinds(&drawing, "path");
    j.note(format!("paths: {lifted:?}"));
    j.expect(
        "a-moves-a-point-of-a-shape",
        anchors.len() == 3 && lifted.iter().any(|(_, b)| b[3] > tall + lift * 0.8),
        "A shows the roof's three points and a drag lifts its top",
    );
    j.key(Key::Escape);

    j.act("write two lines of text: T, a click, Mushroom, Return, house, Esc");
    j.key(Key::T);
    j.tap(560.0, 560.0);
    j.type_text("Mushroom");
    j.key(Key::Enter);
    j.type_text("house");
    j.look("typing");
    j.key(Key::Escape);
    j.look("text");
    let texts = kinds(&drawing, "text");
    let words = texts.first().map(|(id, _)| shown_as(&drawing, id)).unwrap_or_default();
    j.note(format!("texts: {texts:?}; it said: {}", j.heard()));
    j.expect(
        "t-types-two-lines",
        texts.len() == 1 && words.contains("Mushroom") && words.contains("house"),
        "the text holds both lines",
    );

    j.act("add to the text: click it with T, type an exclamation mark, Esc");
    j.tap(600.0, 590.0);
    j.type_text("!");
    j.key(Key::Escape);
    j.look("text-edited");
    let again = kinds(&drawing, "text");
    let words = again.first().map(|(id, _)| shown_as(&drawing, id)).unwrap_or_default();
    j.note(format!("texts: {again:?}"));
    j.expect(
        "a-click-on-a-text-edits-it",
        again.len() == 1 && words.contains('!'),
        "a click on the text with T edits that text, not a new one",
    );

    j.act("line the rectangle and the circle up: V, a click, a shift click, align left edges");
    j.key(Key::V);
    j.tap(350.0, 300.0);
    j.tap_with(700.0, 265.0, egui::PointerButton::Primary, Modifiers::SHIFT);
    j.look("two-shapes");
    let both = j.ed().map_or(0, |ed| ed.group.len());
    let aligned = j.press_hinted("align left edges");
    j.look("aligned");
    let lefts: Vec<f32> = kinds(&drawing, "rect")
        .iter()
        .chain(kinds(&drawing, "ellipse").iter())
        .map(|(_, b)| b[0])
        .collect();
    j.note(format!("{both} shapes selected; their left edges are at {lefts:?}"));
    j.expect(
        "two-shapes-align",
        both == 2 && aligned && lefts.len() == 2 && (lefts[0] - lefts[1]).abs() < 1.5,
        "two selected shapes line up on their left edges",
    );
    j.chord(CMD, Key::Z);
    j.key(Key::Escape);

    j.act("copy the circle: cmd C and cmd V, then cmd D, then Delete");
    j.tap(700.0, 265.0);
    j.chord(CMD, Key::C);
    j.chord(CMD, Key::V);
    let pasted = kinds(&drawing, "ellipse").len();
    j.chord(CMD, Key::D);
    let doubled = kinds(&drawing, "ellipse").len();
    j.key(Key::Delete);
    j.look("copies");
    let left = kinds(&drawing, "ellipse").len();
    j.note(format!("ellipses: 1, then {pasted}, then {doubled}, then {left}"));
    j.expect(
        "shapes-copy-paste-duplicate-and-delete",
        pasted == 2 && doubled == 3 && left == 2,
        "cmd C and cmd V, cmd D and Delete work on a shape",
    );

    j.finish(&[]);
}

fn pixels(drawing: &str, layer: &str, frame: u32) -> (Vec<String>, String) {
    let text = std::process::Command::new(crate::start::test_binary())
        .args(["pixels", drawing, layer, "--frame", &frame.to_string()])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    let rows = text
        .lines()
        .filter(|l| {
            let mut words = l.split_whitespace();
            words.next().is_some_and(|w| w.parse::<u32>().is_ok()) && words.next().is_some()
        })
        .filter_map(|l| l.split_whitespace().nth(1).map(str::to_string))
        .filter(|row| row.len() > 10)
        .collect();
    (rows, text)
}

fn painted(rows: &[String]) -> usize {
    rows.iter().map(|r| r.chars().filter(|c| *c != '.').count()).sum()
}

fn cell_of(rows: &[String], x: usize, y: usize) -> char {
    rows.get(y).and_then(|r| r.chars().nth(x)).unwrap_or('?')
}

fn png_size(path: &std::path::Path) -> Option<(u32, u32)> {
    let bytes = std::fs::read(path).ok()?;
    let word = |at: usize| Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?));
    Some((word(16)?, word(20)?))
}

fn layer_id(j: &Journey) -> String {
    j.ed().and_then(|ed| ed.scene.active_id()).unwrap_or_default()
}

#[test]
fn someone_draws_a_small_sprite() {
    let mut j = Journey::begin("pixel-sprite", WINDOW, None);

    j.act("klad starts: Pixel art, the small sprite size, the name knight");
    j.click("Pixel art");
    j.click("Size");
    j.click("32 x 32  small sprite");
    j.retype("Name", "knight");
    if j.ed().is_none() {
        j.click("Create");
    }
    j.settle();
    j.look("new-sprite");
    if j.ed().is_none() {
        return j.finish(&[]);
    }
    let drawing = j.drawing();
    let layer = layer_id(&j);
    let colours = |j: &Journey| j.ed().map_or(0, |ed| ed.scene.bundle.doc.palette.len());
    j.note(format!(
        "the sprite is {:?} px, on {}; the palette holds {} colours",
        j.ed().map(|ed| ed.scene.size()),
        active_name(&j),
        colours(&j)
    ));
    j.expect(
        "a-small-sprite-is-made",
        j.ed().is_some_and(|ed| ed.scene.size() == (32, 32)),
        "the sprite is 32 by 32",
    );

    j.act("take a ready palette: the palettes button of the Colour panel, then PICO-8");
    let button = j.hint_in_sight("load a palette");
    j.expect(
        "the-palettes-button-is-in-sight",
        button,
        "the Colour panel's palettes button is in sight on a new sprite",
    );
    let opened = match button {
        true => j.press_hinted("load a palette"),
        false => {
            j.click("Tools");
            j.click_containing("Load a palette")
        }
    };
    j.look("palettes");
    j.click("PICO-8");
    j.look("palette-loaded");
    j.note(format!(
        "the palette holds {} colours; it said: {}",
        colours(&j),
        j.heard()
    ));
    j.expect(
        "a-ready-palette-loads",
        opened && colours(&j) >= 16,
        "the palette takes the sixteen colours",
    );
    if j.shown("or a palette file") {
        j.key(Key::Escape);
    }
    let hexes: Vec<String> = j.ed().map_or_else(Vec::new, |ed| {
        ed.scene.bundle.doc.palette.iter().map(|c| c.color.clone()).collect()
    });
    let reachable = hexes.iter().filter(|hex| j.in_sight(hex)).count();
    j.note(format!(
        "{reachable} of the {} palette colours are in sight in the Colour panel",
        hexes.len()
    ));
    j.expect(
        "the-whole-palette-is-in-sight",
        reachable == hexes.len(),
        "every colour of a sixteen colour palette is in sight",
    );

    j.act("the body's outline with the rectangle of U");
    j.key(Key::U);
    j.drag(&[(8.5, 10.5), (22.5, 26.5)]);
    j.look("outline");
    let (rows, _) = pixels(&drawing, &layer, 0);
    j.note(format!("{} pixels are painted", painted(&rows)));
    j.expect(
        "u-draws-a-rectangle-of-pixels",
        painted(&rows) == 60 && cell_of(&rows, 8, 10) != '.' && cell_of(&rows, 15, 18) == '.',
        "the rectangle is a one pixel outline from 8,10 to 22,26",
    );

    j.act("fill it with orange from the palette: its square, G, a click inside");
    let orange = j.click("#ffa300");
    j.key(Key::G);
    j.tap(15.5, 18.5);
    j.look("filled");
    let (rows, _) = pixels(&drawing, &layer, 0);
    j.note(format!("{} pixels are painted", painted(&rows)));
    j.expect(
        "g-fills-inside-the-outline",
        orange && painted(&rows) == 255 && cell_of(&rows, 15, 18) != cell_of(&rows, 8, 10),
        "the fill covers the inside and stops at the outline",
    );

    j.act("two eyes at once: the pencil in black with mirror on, one click");
    j.key(Key::B);
    j.click("#000000");
    let mirror = j.click("mirror");
    j.tap(12.5, 15.5);
    j.look("eyes");
    let (rows, _) = pixels(&drawing, &layer, 0);
    let eyes = (cell_of(&rows, 12, 15), cell_of(&rows, 19, 15), cell_of(&rows, 15, 18));
    j.note(format!(
        "the pixels at 12,15 and 19,15 are {:?} and {:?}; the body is {:?}",
        eyes.0, eyes.1, eyes.2
    ));
    j.expect(
        "mirror-draws-the-other-eye",
        mirror && eyes.0 == eyes.1 && eyes.0 != eyes.2,
        "one click with mirror on draws the pixel and its mirror image",
    );
    j.click("mirror");

    j.act("the ground: a line with the line button of U");
    j.key(Key::U);
    let line = j.click("line");
    j.drag(&[(6.5, 28.5), (24.5, 28.5)]);
    j.look("ground");
    let (rows, _) = pixels(&drawing, &layer, 0);
    let ground = rows.get(28).map_or(0, |r| r.chars().filter(|c| *c != '.').count());
    j.expect(
        "u-draws-a-line-of-pixels",
        line && ground == 19,
        "the line is nineteen pixels long on row 28",
    );
    j.click("rectangle");

    j.act("shade the body's lower rows: the pencil's darker button, a drag across");
    j.key(Key::B);
    let darker = j.click("darker");
    let before = pixels(&drawing, &layer, 0).0;
    j.drag(&[(9.5, 25.5), (21.5, 25.5)]);
    j.look("shaded");
    let after = pixels(&drawing, &layer, 0).0;
    j.note(format!(
        "row 25 went from {:?} to {:?}; it said: {}",
        before.get(25),
        after.get(25),
        j.heard()
    ));
    j.expect(
        "darker-shades-or-says-what-it-needs",
        darker && (before.get(25) != after.get(25) || j.heard().contains("ramp")),
        "the darker pencil shades the pixels, or says that it needs a ramp",
    );
    j.expect(
        "the-ramp-refusal-points-at-the-editor",
        !j.heard().contains("klad palette"),
        "the refusal names the ramp button, not a command line option",
    );
    j.click("colour");

    j.act("move the whole knight two pixels to the right: S, a box around it, the right arrow twice");
    j.key(Key::S);
    j.drag(&[(5.5, 8.5), (26.5, 30.5)]);
    j.key(Key::ArrowRight);
    j.key(Key::ArrowRight);
    j.look("moved");
    let moved = pixels(&drawing, &layer, 0).0;
    j.note(format!(
        "the pixel at 8,10 is {:?} and at 10,10 {:?}",
        cell_of(&moved, 8, 10),
        cell_of(&moved, 10, 10)
    ));
    j.expect(
        "a-box-and-the-arrows-move-pixels",
        cell_of(&moved, 8, 10) == '.' && cell_of(&moved, 10, 10) != '.' && painted(&moved) == painted(&after),
        "the selected pixels move two pixels and none is lost",
    );
    j.key(Key::Escape);

    j.act("a dark line around it: Layer > Outline the pixels");
    let before = painted(&pixels(&drawing, &layer, 0).0);
    j.click("Layer");
    let outlined = j.click("Outline the pixels");
    j.look("outlined");
    let after = painted(&pixels(&drawing, &layer, 0).0);
    j.note(format!("{before} pixels before, {after} after; it said: {}", j.heard()));
    j.expect(
        "outline-the-pixels-adds-an-outline",
        outlined && after > before,
        "Outline the pixels draws a line of pixels around the sprite",
    );

    j.act("have klad look it over: Layer > Check the pixels");
    j.click("Layer");
    let checked = j.click("Check the pixels");
    j.look("checked");
    j.note(format!("it said: {}", j.heard()));
    j.expect(
        "check-the-pixels-answers",
        checked && !j.heard().is_empty(),
        "Check the pixels says what it found",
    );
    j.expect(
        "a-clean-check-says-so",
        !j.heard().contains("nothing to look at"),
        "a check that finds no fault says that the sprite is clean, in words that do not read as an empty layer",
    );

    j.act("export it enlarged: File > Export > Image…, the size slider to the right, Export");
    j.menu("File", "Export", "Image… cmd E");
    j.look("export-window");
    j.click_inside("times real size", 0.5, 0.5);
    j.look("export-size");
    j.click("Export");
    j.look("exported");
    let made = pngs(&j.folder)
        .into_iter()
        .find(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("knight")));
    let size = made.as_deref().and_then(png_size);
    j.note(format!(
        "{:?} is {size:?} px; it said: {}",
        made.as_ref().and_then(|p| p.file_name()),
        j.heard().replace(&j.folder.display().to_string(), "")
    ));
    j.expect(
        "a-sprite-exports-in-whole-steps",
        size.is_some_and(|(w, h)| w == h && w % 32 == 0 && w > 32),
        "the exported picture is the sprite enlarged by a whole number",
    );

    j.finish(&[]);
}

fn frame_now(j: &Journey) -> (u32, u32) {
    j.ed()
        .map_or((0, 0), |ed| (ed.scene.frame, ed.scene.bundle.doc.timeline.frames))
}

fn clip_names(j: &Journey) -> Vec<String> {
    j.ed().map_or_else(Vec::new, |ed| {
        ed.scene
            .bundle
            .doc
            .timeline
            .clips
            .iter()
            .map(|c| c.name.clone())
            .collect()
    })
}

#[test]
fn someone_animates_a_sprite_and_exports_it() {
    let mut j = Journey::begin("pixel-animate", WINDOW, Some("sprout.klad"));

    j.act("open a sprite with two clips and play it with Return, then stop");
    j.look("opened");
    if j.ed().is_none() {
        return j.finish(&[]);
    }
    let drawing = j.drawing();
    j.note(format!(
        "frame {} of {}; clips {:?}; layers {:?}",
        frame_now(&j).0,
        frame_now(&j).1,
        clip_names(&j),
        names(&j)
    ));
    j.key(Key::Enter);
    j.h.run_steps(8);
    let playing = j.ed().is_some_and(|ed| ed.timeline.player.is_some());
    j.look("playing");
    j.key(Key::Enter);
    let stopped = j.ed().is_some_and(|ed| ed.timeline.player.is_none());
    j.expect(
        "return-plays-and-stops",
        playing && stopped,
        "Return plays and Return stops",
    );
    j.h.run_steps(3);
    j.note(format!("after stopping it said: {}", j.heard()));
    j.expect(
        "stopping-says-nothing-or-something-plain",
        !j.heard().contains("dropped"),
        "stopping the playback says nothing, or something a person who animates can read",
    );

    j.act("step to the third frame with the full stop key, and turn the onion skin on with O");
    j.key(Key::Home);
    j.key(Key::Period);
    j.key(Key::Period);
    j.key(Key::O);
    j.look("onion");
    j.expect(
        "the-keys-step-and-o-shows-the-onion",
        frame_now(&j).0 == 2 && j.ed().is_some_and(|ed| ed.scene.onion_on),
        "the full stop steps a frame and O turns the onion skin on",
    );

    j.act("draw one pixel on the body on this frame and see which frames it shows on");
    j.click("body");
    j.key(Key::B);
    let body = layer_id(&j);
    j.tap(3.5, 3.5);
    j.look("one-pixel");
    let on: Vec<u32> = (0..frame_now(&j).1)
        .filter(|f| cell_of(&pixels(&drawing, &body, *f).0, 3, 3) != '.')
        .collect();
    let told =
        j.h.query_all_by_label_contains("draws on body")
            .next()
            .and_then(|n| super::testkit::written(&n.accesskit_node()))
            .unwrap_or_default();
    j.note(format!(
        "the pixel shows on frames {on:?}; the status line reads \"{told}\""
    ));
    j.expect(
        "the-status-line-says-which-frames-a-drawing-shows-on",
        !on.is_empty() && (on.len() == 1 || told.contains(&format!("frames {} to {}", on[0], on[on.len() - 1]))),
        "the status line names the frames the drawing in hand shows on",
    );
    j.chord(CMD, Key::Z);

    j.act("copy this drawing to the next frame: a click on its frame in the timeline, cmd D");
    let before = frame_now(&j);
    j.drag_frames("body", before.0, before.0);
    j.chord(CMD, Key::D);
    j.look("copied-frame");
    let after = frame_now(&j);
    let same = pixels(&drawing, &body, before.0).0 == pixels(&drawing, &body, after.0).0;
    j.note(format!(
        "frame {} of {} before, frame {} of {} after; the two frames hold the same pixels: {same}; it said: {}",
        before.0,
        before.1,
        after.0,
        after.1,
        j.heard()
    ));
    j.expect(
        "cmd-d-copies-a-frame",
        after.1 > before.1 && same,
        "cmd D with the timeline in use makes a copy of the drawing on the next frame",
    );

    j.act("change the copy: one pixel, and the frame before stays as it was");
    let was = pixels(&drawing, &body, before.0).0;
    j.tap(3.5, 3.5);
    j.look("copy-changed");
    let here = frame_now(&j).0;
    j.expect(
        "a-copied-frame-is-its-own-drawing",
        pixels(&drawing, &body, before.0).0 == was && cell_of(&pixels(&drawing, &body, here).0, 3, 3) != '.',
        "a pixel drawn on the copy leaves the frame it was copied from alone",
    );

    j.act("hold the drawing a frame longer with = and take that back with -");
    let length = frame_now(&j).1;
    let rows = |j: &Journey| -> Vec<String> {
        j.ed().map_or_else(Vec::new, |ed| {
            ed.scene
                .bundle
                .doc
                .all_layers()
                .iter()
                .map(|l| {
                    format!(
                        "{}: {:?}",
                        l.name,
                        l.cels().iter().map(|c| (c.frame, c.hold)).collect::<Vec<_>>()
                    )
                })
                .collect()
        })
    };
    let was_rows = rows(&j);
    j.drag_frames("body", here, here);
    j.key(Key::Equals);
    let longer = frame_now(&j).1;
    let long_rows = rows(&j);
    j.key(Key::Minus);
    j.look("held");
    j.note(format!(
        "the timeline was {length} frames, {longer} after =, {} after -",
        frame_now(&j).1
    ));
    j.note(format!("drawings and holds before: {was_rows:?}"));
    j.note(format!("after =: {long_rows:?}"));
    j.note(format!("after -: {:?}", rows(&j)));
    j.expect(
        "equals-and-minus-undo-each-other",
        longer == length + 1 && frame_now(&j).1 == length,
        "= holds the drawing one frame longer and - takes it back",
    );

    j.act("select the last three frames of the body by dragging over them in the timeline");
    let end = frame_now(&j).1 - 2;
    let was_rows = rows(&j);
    let body_row = |j: &Journey| rows(j).into_iter().find(|r| r.starts_with("body")).unwrap_or_default();
    j.drag_frames("body", end - 2, end);
    j.look("dragged-over-frames");
    let selected = j.ed().is_some_and(|ed| ed.timeline.span.is_some());
    j.note(format!(
        "frames selected: {selected}; the body's drawings were {:?} and are {:?}; it said: {}",
        was_rows.iter().find(|r| r.starts_with("body")),
        body_row(&j),
        j.heard()
    ));
    j.expect(
        "a-drag-over-frames-selects-them",
        selected && rows(&j) == was_rows,
        "a drag over frames selects them and moves nothing",
    );
    if rows(&j) != was_rows {
        j.chord(CMD, Key::Z);
    }

    j.act("the same drag, pressing a little left of the middle of the first frame");
    j.drag_frames_off("body", end - 2, end, egui::vec2(-5.0, 0.0), Modifiers::NONE);
    j.look("dragged-left-of-the-middle");
    let selected = j.ed().is_some_and(|ed| ed.timeline.span.is_some());
    j.note(format!(
        "frames selected: {selected}; the body's drawings are {:?}; it said: {}",
        body_row(&j),
        j.heard()
    ));
    j.expect(
        "a-drag-from-the-left-of-a-frame-selects",
        selected && rows(&j) == was_rows,
        "a drag that starts left of the middle of a frame selects frames and changes no drawing",
    );
    if rows(&j) != was_rows {
        j.chord(CMD, Key::Z);
    }

    j.act("the same drag, pressing near the top edge of the row");
    j.drag_frames_off("body", end - 2, end, egui::vec2(0.0, -10.0), Modifiers::NONE);
    j.look("dragged-along-the-top");
    let span = j.ed().and_then(|ed| ed.timeline.span.as_ref().map(|s| (s.from, s.to)));
    j.note(format!(
        "the selection is {span:?}; the body's drawings are {:?}",
        body_row(&j)
    ));
    j.expect(
        "a-drag-along-the-top-of-the-row-selects",
        span == Some((end - 2, end)) && rows(&j) == was_rows,
        "a drag that starts in the top few pixels of the row selects the frames",
    );
    if rows(&j) != was_rows {
        j.chord(CMD, Key::Z);
    }

    j.act("the same drag with shift held, as the hint of the retime button says");
    j.drag_frames_with("body", end - 2, end, Modifiers::SHIFT);
    j.look("shift-dragged-over-frames");
    let selected = j.ed().is_some_and(|ed| ed.timeline.span.is_some());
    j.note(format!(
        "frames selected: {selected}; the body's drawings are {:?}; it said: {}",
        body_row(&j),
        j.heard()
    ));
    j.expect(
        "a-shift-drag-over-frames-selects-them",
        selected && rows(&j) == was_rows,
        "a drag over frames with shift held selects them and moves nothing",
    );
    if rows(&j) != was_rows {
        j.chord(CMD, Key::Z);
    }

    j.act("a click on the first and a shift click on the last, then the clip button");
    j.drag_frames("body", end - 2, end - 2);
    j.drag_frames_with("body", end, end, Modifiers::SHIFT);
    j.look("shift-clicked-frames");
    let span = j.ed().and_then(|ed| ed.timeline.span.as_ref().map(|s| (s.from, s.to)));
    j.note(format!(
        "the selection is {span:?}; frames {} to {end} were meant",
        end - 2
    ));
    j.expect(
        "a-click-and-a-shift-click-select-frames",
        span == Some((end - 2, end)),
        "a click and a shift click select the frames between them",
    );
    let before = clip_names(&j);
    let clip = j.press_hinted("new clip from the selection");
    j.look("clip-made");
    let made = clip_names(&j)
        .into_iter()
        .find(|c| !before.contains(c))
        .unwrap_or_default();
    j.note(format!("clips: {:?}; it said: {}", clip_names(&j), j.heard()));
    j.expect(
        "selected-frames-become-a-clip",
        clip && !made.is_empty(),
        "the selected frames become a clip",
    );
    j.expect(
        "a-new-clip-asks-its-name",
        j.shown("clip")
            && j.h
                .query_all_by_label("clip")
                .any(|n| n.accesskit_node().role() == egui::accesskit::Role::TextInput),
        "a new clip offers its name to type, where it is made",
    );

    j.act("name it hop: a click on its bar above the frames, the name field");
    j.click_clip(&made);
    j.look("clip-window");
    j.retype("clip", "hop");
    j.look("clip-named");
    j.note(format!("clips: {:?}", clip_names(&j)));
    j.expect(
        "a-click-on-a-clips-bar-opens-its-name",
        clip_names(&j).iter().any(|c| c == "hop"),
        "a click on the clip's bar opens its settings, where the name is typed",
    );
    j.key(Key::Escape);

    j.act("export the walk as a GIF: File > Export > GIF…, the walk button, Export");
    j.menu("File", "Export", "GIF…");
    j.click("walk");
    j.look("gif-ready");
    j.click("Export");
    j.look("gif-exported");
    let gifs: Vec<String> = files_in(&j.folder)
        .into_iter()
        .filter(|f| f.ends_with(".gif"))
        .collect();
    j.note(format!(
        "GIFs: {gifs:?}; it said: {}",
        j.heard().replace(&j.folder.display().to_string(), "")
    ));
    j.expect(
        "a-clip-exports-as-a-gif",
        gifs.len() == 1,
        "the walk is written as a GIF",
    );
    j.press_hinted(&format!("close {} Export GIF", crate::theme::icon::EXPORT));

    j.act("and everything as a sprite sheet with its JSON");
    j.menu("File", "Export", "Sprite sheet…");
    j.retype("frames", "all");
    j.click("Export");
    j.look("sheet-exported");
    let json = files_in(&j.folder)
        .into_iter()
        .find(|f| f.ends_with(".json"))
        .and_then(|f| std::fs::read_to_string(j.folder.join(f)).ok())
        .unwrap_or_default();
    j.note(format!(
        "the JSON beside the sheet names {:?}",
        clip_names(&j)
            .into_iter()
            .filter(|c| json.contains(&format!("\"{c}\"")))
            .collect::<Vec<_>>()
    ));
    j.expect(
        "the-sheet-names-every-clip",
        !json.is_empty() && clip_names(&j).iter().all(|c| json.contains(&format!("\"{c}\""))),
        "the sheet's JSON has a tag for each clip",
    );
    j.press_hinted(&format!("close {} Export sprite sheet", crate::theme::icon::EXPORT));

    j.act("throw one drawing away with the timeline's bin and bring it back");
    j.drag_frames("body", 1, 1);
    let was = pixels(&drawing, &body, 1).0;
    let binned = j.press_hinted("delete the drawing on this frame");
    j.look("binned");
    let gone = pixels(&drawing, &body, 1).0;
    j.note(format!("it said: {}", j.heard()));
    j.chord(CMD, Key::Z);
    j.look("back");
    j.expect(
        "the-bin-takes-a-drawing-and-undo-brings-it-back",
        binned && gone != was && pixels(&drawing, &body, 1).0 == was,
        "the bin removes the drawing on the frame and cmd Z brings it back",
    );

    j.finish(&[]);
}

fn drawings(j: &Journey, layer: &str) -> Vec<(u32, Option<u32>, bool)> {
    layer_of(j, layer).map_or_else(Vec::new, |l| {
        l.cels().iter().map(|c| (c.frame, c.hold, c.link.is_some())).collect()
    })
}

fn box_at(j: &mut Journey, layer: &str, frame: u32) -> Option<[f32; 4]> {
    let back = frame_now(j).0;
    if let Some(ed) = j.ed_mut() {
        let _ = ed.go_to(frame);
    }
    j.settle();
    let found = layer_box(j, layer);
    if let Some(ed) = j.ed_mut() {
        let _ = ed.go_to(back);
    }
    j.settle();
    found
}

#[test]
fn someone_animates_frame_by_frame() {
    let mut j = Journey::begin("animate-frame-by-frame", WINDOW, None);

    j.act("a new painting called bounce, and a ball near the top with the brush");
    if !new_drawing(&mut j, "bounce") {
        j.look("no-drawing");
        return j.finish(&[]);
    }
    let layer = active_name(&j);
    j.key(Key::B);
    j.drag(&[
        (470.0, 200.0),
        (512.0, 160.0),
        (554.0, 200.0),
        (512.0, 240.0),
        (470.0, 200.0),
    ]);
    j.look("first-key");
    let first = layer_box(&j, &layer);
    j.note(format!(
        "layers {:?}; drawings on {layer}: {:?}; the ball is at {first:?}; frame {} of {}",
        names(&j),
        drawings(&j, &layer),
        frame_now(&j).0,
        frame_now(&j).1
    ));
    j.expect(
        "the-first-drawing-is-there",
        first.is_some(),
        "the stroke is on the first frame",
    );

    j.act("the timeline of a drawing with one frame is folded to its bar: open it with its arrow");
    let folded = j.ed().is_some_and(|ed| ed.settings.layout.timeline_folded);
    j.press_hinted("open the timeline: its layers and frames");
    j.look("timeline-opened");
    j.expect(
        "a-still-opens-with-the-timeline-folded",
        folded,
        "a new drawing of one frame shows the timeline folded to its bar",
    );
    j.expect(
        "the-arrow-opens-the-timeline",
        j.ed().is_some_and(|ed| !ed.settings.layout.timeline_folded),
        "its arrow opens the timeline",
    );

    j.act("the second key from the first: a click on its frame in the timeline, cmd D");
    j.drag_frames(&layer, 0, 0);
    j.chord(CMD, Key::D);
    j.look("second-key");
    let copy = frame_now(&j).0;
    j.note(format!(
        "drawings on {layer}: {:?}; the playhead is on frame {copy} of {}; it said: {}",
        drawings(&j, &layer),
        frame_now(&j).1,
        j.heard()
    ));
    j.expect(
        "cmd-d-makes-the-next-key",
        drawings(&j, &layer).len() == 2 && copy > 0 && layer_box(&j, &layer).is_some(),
        "cmd D makes a copy of the drawing on the next frame and goes there",
    );

    j.act("move the copy down to the floor: V, a drag on the ball");
    j.key(Key::V);
    j.drag(&[(512.0, 160.0), (512.0, 400.0), (512.0, 700.0)]);
    j.look("second-key-moved");
    let second = layer_box(&j, &layer);
    let stays = box_at(&mut j, &layer, 0);
    j.note(format!(
        "the ball on this frame is at {second:?}; on the first frame it is at {stays:?}"
    ));
    j.expect(
        "the-copy-moves-alone",
        shifted(first, second).is_some_and(|(_, dy)| dy > 400.0) && near(stays, first, 2.0),
        "the copy moves down and the first drawing stays",
    );
    j.key(Key::Escape);

    j.act("room between the keys: a click on the first frame, = three times");
    j.drag_frames(&layer, 0, 0);
    j.key(Key::Equals);
    j.key(Key::Equals);
    j.key(Key::Equals);
    j.look("room");
    let keys = drawings(&j, &layer);
    let last = keys.last().map_or(0, |k| k.0);
    j.note(format!(
        "drawings on {layer}: {keys:?}; the timeline has {} frames",
        frame_now(&j).1
    ));
    j.expect(
        "equals-makes-room-between-keys",
        last >= 4,
        "= holds the first key longer and pushes the second later",
    );

    j.act("in-betweens: a click on the first key, a shift click on the second, the in-between button, make");
    j.drag_frames(&layer, 0, 0);
    j.drag_frames_with(&layer, last, last, Modifiers::SHIFT);
    j.look("keys-selected");
    let asked = j.press_hinted("in-between the selection");
    j.look("in-between-window");
    let shape = |j: &mut Journey, frame: u32| -> (Option<[f32; 4]>, usize) {
        let found = box_at(j, &layer, frame);
        let back = frame_now(j).0;
        if let Some(ed) = j.ed_mut() {
            let _ = ed.go_to(frame);
        }
        let samples = last_stroke(j, &layer).map_or(0, |s| s.samples.len());
        if let Some(ed) = j.ed_mut() {
            let _ = ed.go_to(back);
        }
        j.settle();
        (found, samples)
    };
    let keys_before = (shape(&mut j, 0), shape(&mut j, last));
    let made = j.click("make the in-betweens");
    let done = j.wait("the in-betweens", |ed| {
        ed.scene.bundle.doc.find_layer(&layer).is_ok_and(|l| l.cels().len() > 2)
            && !ed.jobs.busy(crate::jobs::Tag::Inbetween)
    });
    j.look("in-betweens");
    let between: Vec<Option<[f32; 4]>> = (1..last).map(|f| box_at(&mut j, &layer, f)).collect();
    let tops: Vec<f32> = between.iter().flatten().map(|b| b[1]).collect();
    j.note(format!(
        "drawings on {layer}: {:?}; the ball's top on the frames between: {tops:?}; it said: {}",
        drawings(&j, &layer),
        j.heard()
    ));
    j.expect(
        "the-in-betweens-lie-between-the-keys",
        asked
            && made
            && done
            && tops.len() as u32 == last - 1
            && tops.windows(2).all(|w| w[0] < w[1])
            && tops.first().is_some_and(|t| first.is_some_and(|f| *t > f[1]))
            && tops.last().is_some_and(|t| second.is_some_and(|s| *t < s[1])),
        "each frame between the keys gets a drawing, each lower than the one before",
    );
    let keys_after = (shape(&mut j, 0), shape(&mut j, last));
    j.note(format!(
        "the two keys (box, points of the stroke) before: {keys_before:?}; after: {keys_after:?}"
    ));
    j.expect(
        "making-in-betweens-leaves-the-keys-as-drawn",
        keys_before == keys_after,
        "the two keys are the strokes they were before the in-betweens were made",
    );
    j.press_hinted(&format!("close {} In-betweens", crate::theme::icon::INBETWEEN));

    j.act("see the neighbours while drawing: O, on a frame in the middle, and O again");
    j.drag_frames(&layer, 2, 2);
    let plain = j.picture();
    j.key(Key::O);
    j.look("onion");
    let on = j.ed().is_some_and(|ed| ed.scene.onion_on);
    let with_onion = j.picture();
    j.key(Key::O);
    let back = j.picture();
    j.expect(
        "o-shows-the-neighbouring-frames",
        on && with_onion != plain && back == plain,
        "O shows the frames before and after, and O again shows the drawing alone",
    );

    j.act("play it with Return, then stop");
    j.key(Key::Enter);
    j.h.run_steps(10);
    let playing = j.ed().is_some_and(|ed| ed.timeline.player.is_some());
    j.look("playing");
    j.key(Key::Enter);
    j.expect(
        "return-plays-the-painting",
        playing && j.ed().is_some_and(|ed| ed.timeline.player.is_none()),
        "Return plays and stops",
    );

    j.act("a floor behind the ball, added while looking at the last frame: Layer > New paint layer, one stroke");
    j.key(Key::End);
    let at = frame_now(&j).0;
    j.click("Layer");
    j.click("New paint layer");
    let ground = active_name(&j);
    j.key(Key::B);
    j.drag(&[(150.0, 900.0), (500.0, 910.0), (880.0, 900.0)]);
    j.look("floor-made-on-the-last-frame");
    let total = frame_now(&j).1;
    let on: Vec<u32> = (0..total).filter(|f| box_at(&mut j, &ground, *f).is_some()).collect();
    j.note(format!(
        "made on frame {at}; layers {:?}; the stroke on {ground} shows on frames {on:?} of {total}; drawings on {ground}: {:?}",
        names(&j),
        drawings(&j, &ground)
    ));
    j.expect(
        "a-new-layer-shows-on-every-frame",
        on.len() as u32 == total,
        "a layer added on a later frame shows what is drawn on it on every frame",
    );

    j.act("take both back, go to the first frame with Home and make the floor again");
    j.chord(CMD, Key::Z);
    j.chord(CMD, Key::Z);
    j.key(Key::Home);
    j.click("Layer");
    j.click("New paint layer");
    let ground = active_name(&j);
    j.key(Key::B);
    j.drag(&[(150.0, 900.0), (500.0, 910.0), (880.0, 900.0)]);
    j.look("floor-made-on-the-first-frame");
    let on: Vec<u32> = (0..total).filter(|f| box_at(&mut j, &ground, *f).is_some()).collect();
    j.note(format!(
        "layers {:?}; the stroke on {ground} shows on frames {on:?} of {total}",
        names(&j)
    ));
    j.expect(
        "a-layer-made-on-the-first-frame-holds-across-the-timeline",
        on.len() as u32 == total,
        "a stroke on a layer made on the first frame shows on every frame",
    );

    j.act("the ball back up at the end: the ball's layer, End, shift K for a new frame, the link button, the first drawing");
    j.click(&layer);
    j.drag_frames(&layer, last, last);
    j.key(Key::End);
    j.chord(Modifiers::SHIFT, Key::K);
    let here = frame_now(&j).0;
    let opened = j.press_hinted("show a drawing here (link)");
    j.look("link-window");
    let offered = j.in_sight_containing(&format!("frame {here} will show the drawing you pick"));
    let picked = j.click("frame 0");
    j.look("link-on-a-new-frame");
    j.note(format!(
        "frame {here} of {}; the window offered this frame: {offered}; drawings on {layer}: {:?}; it said: {}",
        frame_now(&j).1,
        drawings(&j, &layer),
        j.heard()
    ));
    j.expect(
        "the-link-window-does-what-it-offers",
        opened && picked && drawings(&j, &layer).iter().any(|d| d.2),
        "the window that says this frame will show the picked drawing makes the link, also on a frame shift K just made",
    );
    j.expect(
        "a-refused-link-speaks-plainly",
        !j.heard().starts_with("op ") && !j.heard().contains(" c1"),
        "a refusal says what is in the way in words, without internal ids",
    );

    j.act("the other way: take the new frame back, hold the last key a frame longer with =, End, the link button");
    if !drawings(&j, &layer).iter().any(|d| d.2) {
        j.chord(CMD, Key::Z);
        j.drag_frames(&layer, last, last);
        j.key(Key::Equals);
        j.key(Key::End);
        if !j.in_sight_containing("will show the drawing you pick") {
            j.press_hinted("show a drawing here (link)");
        }
        j.look("link-window-again");
        j.click("frame 0");
    }
    j.look("linked");
    let here = frame_now(&j).0;
    let up = layer_box(&j, &layer);
    j.note(format!(
        "frame {here} of {}; drawings on {layer}: {:?}; the ball here is at {up:?}; it said: {}",
        frame_now(&j).1,
        drawings(&j, &layer),
        j.heard()
    ));
    j.expect(
        "a-link-shows-the-first-drawing-again",
        drawings(&j, &layer).iter().any(|d| d.2) && near(up, keys_after.0.0, 2.0),
        "on a held frame the link shows the first drawing again",
    );

    j.act("a dot on the linked frame, and a look at the first frame");
    let before = box_at(&mut j, &layer, 0);
    j.key(Key::B);
    j.tap(800.0, 300.0);
    j.look("dot-on-the-link");
    let after = box_at(&mut j, &layer, 0);
    j.note(format!(
        "the first frame's drawing reached {before:?} and now reaches {after:?}; it said: {}",
        j.heard()
    ));
    j.expect(
        "drawing-on-a-link-changes-both",
        after != before,
        "a mark on the linked frame shows on the first frame too, as the button's hint says",
    );
    j.act("export it as a GIF: File > Export > GIF…, Export");
    j.menu("File", "Export", "GIF…");
    j.look("gif-window");
    j.click("Export");
    let folder = j.folder.clone();
    let written = j.wait("the GIF", |_| files_in(&folder).iter().any(|f| f.ends_with(".gif")));
    j.look("gif-exported");
    j.note(format!(
        "it said: {}",
        j.heard().replace(&j.folder.display().to_string(), "")
    ));
    j.expect(
        "a-painted-animation-exports-as-a-gif",
        written,
        "the animation is written as a GIF",
    );

    j.finish(&[]);
}

const GODOT_TILES: &str = r#"extends SceneTree

func _init() -> void:
	var path := ""
	var dir := DirAccess.open("res://FOLDER")
	if dir:
		for name in dir.get_files():
			if name.ends_with(".tres"):
				path = "res://FOLDER/" + name
	var set = load(path) if path != "" else null
	if set == null or not (set is TileSet):
		print("FAILED no TileSet loads from ", path)
		quit(1)
		return
	var source: TileSetAtlasSource = set.get_source(set.get_source_id(0))
	print("TILESET ", path.get_file(), " tile size ", set.tile_size, " tiles ", source.get_tiles_count(), " texture ", source.texture.get_size())
	quit(0)
"#;

fn godot_tiles(j: &mut Journey, project: &std::path::Path, folder: &str) -> Option<String> {
    let godot = std::env::var_os("KLAD_GODOT").filter(|g| !g.is_empty())?;
    std::fs::write(
        project.join("project.godot"),
        "config_version=5\n\n[application]\nconfig/name=\"check\"\n",
    )
    .ok()?;
    std::fs::write(project.join("tiles.gd"), GODOT_TILES.replace("FOLDER", folder)).ok()?;
    let run = |args: &[&str]| {
        std::process::Command::new(&godot)
            .arg("--headless")
            .arg("--path")
            .arg(project)
            .args(args)
            .output()
            .map(|out| String::from_utf8_lossy(&out.stdout).to_string())
            .unwrap_or_default()
    };
    run(&["--import"]);
    let said = run(&["--script", "res://tiles.gd"]);
    j.note("opened the folder as a Godot project and loaded the tile set there");
    said.lines()
        .filter(|l| l.starts_with("TILESET") || l.starts_with("FAILED"))
        .for_each(|l| j.note(format!("Godot: {l}")));
    Some(said)
}

fn tile_of(j: &Journey) -> Option<[u32; 2]> {
    j.ed().and_then(|ed| ed.scene.bundle.doc.canvas.tile)
}

#[test]
fn someone_makes_a_tileset_for_a_game() {
    let mut j = Journey::begin("pixel-tileset", WINDOW, None);

    j.act("klad starts: Pixel art, the 64 px sprite size, the name ground");
    j.click("Pixel art");
    j.click("Size");
    j.click("64 x 64  sprite");
    j.retype("Name", "ground");
    if j.ed().is_none() {
        j.click("Create");
    }
    j.settle();
    j.look("new-drawing");
    if j.ed().is_none() {
        return j.finish(&[]);
    }
    let drawing = j.drawing();
    let layer = layer_id(&j);

    j.act("cut it into tiles of 16 px: View > Tileset: cut into tiles…, the cut button");
    j.click("View");
    let asked = j.click("Tileset: cut into tiles…");
    j.look("tileset-window");
    let cut = j.click("cut into tiles");
    j.look("cut");
    j.note(format!(
        "the drawing's tile size is {:?}; it said: {}",
        tile_of(&j),
        j.heard()
    ));
    j.expect(
        "a-drawing-cuts-into-tiles",
        asked && cut && tile_of(&j) == Some([16, 16]),
        "the drawing is cut into tiles of 16 by 16",
    );
    j.press_hinted(&format!("close {} Tileset", crate::theme::icon::PIXELS));

    j.act("a grass tile and a stone tile: a palette, then two filled squares with U and G");
    j.click("Tools");
    j.click_containing("Load a palette");
    j.click("PICO-8");
    if j.shown("or a palette file") {
        j.key(Key::Escape);
    }
    j.click("#008751");
    j.key(Key::U);
    j.drag(&[(0.5, 0.5), (15.5, 15.5)]);
    j.key(Key::G);
    j.tap(8.5, 8.5);
    j.click("#5f574f");
    j.key(Key::U);
    j.drag(&[(16.5, 0.5), (31.5, 15.5)]);
    j.key(Key::G);
    j.tap(24.5, 8.5);
    j.look("two-tiles");
    let (rows, _) = pixels(&drawing, &layer, 0);
    j.note(format!(
        "{} pixels are painted; row 8 reads {:?}",
        painted(&rows),
        rows.get(8)
    ));
    j.expect(
        "two-tiles-are-drawn",
        painted(&rows) == 512 && cell_of(&rows, 8, 8) != cell_of(&rows, 24, 8) && cell_of(&rows, 40, 8) == '.',
        "the first two tiles are filled, each in its colour, and the third is empty",
    );

    j.act("a fill in an empty tile: G, a click in the third tile");
    j.click("#29adff");
    j.key(Key::G);
    j.tap(40.5, 8.5);
    j.look("fill-in-a-tile");
    let (rows, _) = pixels(&drawing, &layer, 0);
    let filled = painted(&rows).saturating_sub(512);
    j.note(format!("the fill painted {filled} pixels; a tile holds 256"));
    j.expect(
        "a-fill-stays-in-its-tile",
        filled == 256,
        "in a drawing cut into tiles a fill in an empty tile fills that tile",
    );
    if filled != 256 {
        j.chord(CMD, Key::Z);
    }

    j.act("make a tile that repeats: View > Tile: wrap at the edges, a pencil line past the right edge");
    j.click("View");
    let wrap = j.click("Tile: wrap at the edges");
    j.key(Key::B);
    j.click("#ffec27");
    j.drag(&[(58.5, 40.5), (62.5, 40.5), (66.5, 40.5), (69.5, 40.5)]);
    j.look("wrapped-line");
    let (rows, _) = pixels(&drawing, &layer, 0);
    let row = rows.get(40).cloned().unwrap_or_default();
    j.note(format!("row 40 reads {row:?}"));
    j.expect(
        "a-line-past-the-edge-comes-back-in",
        wrap && cell_of(&rows, 62, 40) != '.' && cell_of(&rows, 2, 40) != '.',
        "with wrap on, a line drawn past the right edge goes on from the left edge",
    );
    j.click("View");
    j.click("Tile: wrap at the edges");

    j.act("export the tiles for Godot: File > Export > Godot…, a tile set, the folder game, Export");
    j.menu("File", "Export", "Godot…");
    j.look("godot-window");
    let offered = j.click("a tile set");
    j.retype("Godot folder", "game");
    j.look("godot-ready");
    j.click("Export");
    let project = j.folder.join("game");
    let folder = project.clone();
    let written = j.wait("the tile set", |_| {
        files_in(&folder).iter().any(|f| f.ends_with(".tres"))
    });
    j.look("godot-exported");
    let files = files_in(&project);
    j.note(format!(
        "files in game: {files:?}; it said: {}",
        j.heard().replace(&j.folder.display().to_string(), "")
    ));
    let tres = files
        .iter()
        .find(|f| f.ends_with(".tres"))
        .and_then(|f| std::fs::read_to_string(project.join(f)).ok())
        .unwrap_or_default();
    let tiles = tres.lines().filter(|l| l.ends_with("/0 = 0")).count();
    j.note(format!("the tile set lists {tiles} tiles"));
    j.expect(
        "the-tile-set-holds-the-drawn-tiles",
        offered && written && tres.contains("type=\"TileSet\"") && tiles >= 3,
        "the export writes a Godot TileSet with a tile for each tile drawn on",
    );
    let root = j.folder.clone();
    if let Some(said) = godot_tiles(&mut j, &root, "game") {
        j.expect(
            "godot-loads-the-tile-set",
            said.contains("TILESET") && said.contains("tile size (16, 16)") && said.contains(&format!("tiles {tiles}")),
            "Godot loads the tile set with its tile size and its tiles",
        );
    }
    j.press_hinted(&format!("close {} Export for Godot", crate::theme::icon::EXPORT));

    j.act("one picture again: View > Tileset: cut into tiles…, no tiles");
    j.click("View");
    j.click("Tileset: cut into tiles…");
    let whole = j.click("no tiles");
    j.look("no-tiles");
    j.expect(
        "no-tiles-makes-it-one-picture",
        whole && tile_of(&j).is_none(),
        "no tiles makes the drawing one picture again",
    );

    j.finish(&["a-fill-stays-in-its-tile"]);
}

fn bones_of(j: &Journey) -> Vec<(String, Option<String>)> {
    j.ed().map_or_else(Vec::new, |ed| {
        klad_core::rig::skeletons(&ed.scene.bundle.doc)
            .first()
            .map_or_else(Vec::new, |l| {
                l.bones.iter().map(|b| (b.name.clone(), b.parent.clone())).collect()
            })
    })
}

fn rig_of(j: &Journey, layer: &str) -> Option<Option<String>> {
    layer_of(j, layer).and_then(|l| l.rig.map(|link| link.bone))
}

#[test]
fn someone_gives_an_arm_bones_and_poses_it() {
    let mut j = Journey::begin("bones-pose", WINDOW, None);

    j.act("a new painting called arm, and a thick line for the arm");
    if !new_drawing(&mut j, "arm") {
        j.look("no-drawing");
        return j.finish(&[]);
    }
    let layer = active_name(&j);
    j.key(Key::B);
    j.click("64");
    j.drag(&[(300.0, 500.0), (500.0, 500.0), (700.0, 500.0)]);
    j.look("arm");
    let rest = layer_box(&j, &layer);
    j.note(format!("layers {:?}; the arm on {layer} is at {rest:?}", names(&j)));
    j.expect("the-arm-is-drawn", rest.is_some(), "the line is on the canvas");

    j.act("bones: J, a drag along the upper arm, a click at the hand, Esc");
    j.key(Key::J);
    j.look("bone-tool");
    j.drag(&[(300.0, 500.0), (500.0, 500.0)]);
    j.tap(700.0, 500.0);
    j.key(Key::Escape);
    j.look("two-bones");
    j.note(format!(
        "bones {:?}; layers {:?}; the layer in hand is {}; it said: {}",
        bones_of(&j),
        names(&j),
        active_name(&j),
        j.heard()
    ));
    j.expect(
        "a-drag-and-a-click-draw-a-chain-of-two-bones",
        bones_of(&j).len() == 2 && bones_of(&j)[1].1.as_deref() == Some(bones_of(&j)[0].0.as_str()),
        "the drag makes a bone and the click a second one that hangs from it",
    );

    j.act("pose it right away: the pose button, a drag of the hand upwards");
    let pose = j.click("pose");
    j.drag(&[(700.0, 500.0), (690.0, 420.0), (650.0, 340.0)]);
    j.h.run_steps(20);
    j.look("posed-with-nothing-attached");
    let turns = |j: &Journey| -> Vec<(String, f32)> {
        j.ed().map_or_else(Vec::new, |ed| {
            ed.bones.pose.iter().map(|(k, v)| (k.clone(), *v)).collect()
        })
    };
    let words = j.screen().join(" | ");
    j.note(format!("turns {:?}; it said: {}", turns(&j), j.heard()));
    j.expect(
        "a-drag-of-the-hand-turns-the-forearm",
        pose && turns(&j).iter().any(|(_, t)| t.abs() > 15.0),
        "dragging the end of the second bone turns it",
    );
    j.expect(
        "posing-with-no-layer-attached-says-so",
        ["attach", "no layer", "follows"]
            .iter()
            .any(|w| words.contains(*w) || j.heard().contains(*w)),
        "posing a skeleton that no layer follows says that nothing is attached yet, and where to attach",
    );

    j.act("keep it anyway: make this pose a drawing");
    j.click("make this pose a drawing");
    j.wait("the pose as a drawing", |ed| !ed.jobs.busy(crate::jobs::Tag::Edit));
    j.settle();
    j.look("pose-made-of-nothing");
    j.note(format!("it said: {}", j.heard()));
    j.expect(
        "a-pose-of-nothing-is-not-called-done",
        !j.heard().contains("posed 0 drawings"),
        "with nothing attached the button says what is missing, not that it is done",
    );
    j.click("back to rest");

    j.act("let the arm bend with the bones: the bone button on the arm's row in the Layers panel");
    let hint = format!("{layer} is not rigged");
    let row = j.hint_in_sight(&hint);
    j.expect(
        "the-rows-bone-button-is-in-reach",
        row,
        "the bone button on the arm's row in the Layers panel is in sight",
    );
    if !row && !j.scroll_to_hinted(&hint) {
        j.note("the row lies under the timeline; folding the Colour and Brush panels to make room");
        j.press_hinted("fold Colour to its title");
        j.press_hinted("fold Brush to its title");
        j.scroll_to_hinted(&hint);
    }
    let pressed = j.press_hinted(&hint);
    j.look("rig-menu");
    let bends = pressed && j.click("bends with bones");
    j.look("attached");
    j.note(format!(
        "{layer} is rigged as {:?}; it said: {}",
        rig_of(&j, &layer),
        j.heard()
    ));
    j.expect(
        "the-arm-bends-with-the-bones",
        bends && rig_of(&j, &layer) == Some(None),
        "the row's bone button attaches the arm so that it bends with the skeleton",
    );

    j.act("pose again: a drag of the hand upwards");
    j.click("bones");
    j.key(Key::J);
    j.click("pose");
    j.drag(&[(700.0, 500.0), (690.0, 420.0), (650.0, 340.0)]);
    let shown = j.wait("the posed drawing", |ed| {
        ed.preview
            .as_ref()
            .is_some_and(|p| p.posed.is_some() && p.job.is_none() && p.done.is_some())
    });
    j.look("posed");
    j.note(format!("turns {:?}; it said: {}", turns(&j), j.heard()));
    j.expect(
        "the-drawing-bends-while-posing",
        shown,
        "the arm is shown bent while the bone is dragged",
    );

    j.act("keep it: make this pose a drawing");
    j.click("make this pose a drawing");
    j.wait("the pose as a drawing", |ed| !ed.jobs.busy(crate::jobs::Tag::Edit));
    j.settle();
    j.look("pose-on-the-rest-frame");
    j.note(format!(
        "drawings on {layer}: {:?}; it said: {}",
        drawings(&j, &layer),
        j.heard()
    ));
    j.expect(
        "a-pose-on-the-first-frame-is-kept-or-refused-plainly",
        drawings(&j, &layer).len() > 1 || (!j.heard().contains("op 1") && !j.heard().contains(" c2")),
        "the first pose is kept, or the refusal says in plain words to go to another frame first",
    );

    j.act("try the button once more, then look for the export in the File menu while the two messages are there");
    j.click("make this pose a drawing");
    j.wait("the pose as a drawing", |ed| !ed.jobs.busy(crate::jobs::Tag::Edit));
    j.settle();
    j.click("File");
    j.click("Export");
    j.look("menu-under-the-messages");
    let covered: Vec<String> =
        j.h.query_all(egui_kittest::kittest::By::new().predicate(|n| {
            super::testkit::written(n).is_some_and(|label| {
                [
                    "Godot…",
                    "Aseprite…",
                    "GIF…",
                    "Quit klad",
                    "Install the klad command",
                    "Copy path",
                ]
                .iter()
                .any(|item| label.starts_with(item))
            })
        }))
        .filter(|n| {
            j.h.ctx
                .layer_id_at(n.rect().center())
                .is_none_or(|layer| layer.order != egui::Order::Foreground)
        })
        .filter_map(|n| super::testkit::written(&n.accesskit_node()))
        .collect();
    j.note(format!("menu items lying under a message: {covered:?}"));
    j.expect(
        "messages-leave-open-menus-free",
        covered.is_empty(),
        "a message on the canvas does not lie over the items of an open menu",
    );
    j.key(Key::Escape);
    j.key(Key::Escape);

    j.act("close the messages with their crosses; then, as they say, another frame: a click on the arm's frame in the timeline, shift I for a frame more, the full stop key");
    (0..3).for_each(|_| {
        if j.hint_in_sight("dismiss; the message log keeps it") {
            j.press_hinted("dismiss; the message log keeps it");
        }
    });
    j.click("back to rest");
    if j.hint_in_sight("open the timeline: its layers and frames") {
        j.press_hinted("open the timeline: its layers and frames");
    }
    j.drag_frames(&layer, 0, 0);
    j.chord(Modifiers::SHIFT, Key::I);
    j.key(Key::Period);
    let at = frame_now(&j);
    j.note(format!("frame {} of {}; it said: {}", at.0, at.1, j.heard()));
    j.expect(
        "shift-i-makes-a-second-frame",
        at.0 == 1 && at.1 >= 2,
        "shift I makes the timeline a frame longer and the full stop key goes there (the first pose may have made frame 1 already)",
    );

    j.act("the pose again on this frame: the bones layer, J, pose, a drag of the hand, make this pose a drawing");
    j.click("bones");
    j.key(Key::J);
    j.click("pose");
    j.drag(&[(700.0, 500.0), (690.0, 420.0), (650.0, 340.0)]);
    j.wait("the posed drawing", |ed| {
        ed.preview
            .as_ref()
            .is_some_and(|p| p.posed.is_some() && p.job.is_none() && p.done.is_some())
    });
    let made = j.click("make this pose a drawing");
    j.wait("the pose as a drawing", |ed| !ed.jobs.busy(crate::jobs::Tag::Edit));
    j.settle();
    j.look("pose-made");
    let bent = layer_box(&j, &layer);
    j.note(format!(
        "frame {} of {}; the arm was at {rest:?} and is at {bent:?} here; drawings on {layer}: {:?}; it said: {}",
        at.0,
        at.1,
        drawings(&j, &layer),
        j.heard()
    ));
    j.expect(
        "the-pose-becomes-a-drawing",
        made && rest.zip(bent).is_some_and(|(a, b)| b[1] < a[1] - 40.0),
        "on another frame the arm's drawing is bent upwards as the bones were",
    );

    j.act("play the two poses with Return, then stop");
    j.key(Key::Enter);
    j.h.run_steps(10);
    let playing = j.ed().is_some_and(|ed| ed.timeline.player.is_some());
    j.look("playing");
    j.key(Key::Enter);
    j.expect("the-poses-play", playing, "Return plays the poses");

    j.act("export the rig for Godot: File > Export > Godot…, a rig, the folder game, Export");
    j.menu("File", "Export", "Godot…");
    j.look("godot-window");
    let offered = j.click("a rig");
    j.retype("Godot folder", "game");
    j.click("Export");
    let project = j.folder.join("game");
    let folder = project.clone();
    let written = j.wait("the rig", |_| files_in(&folder).iter().any(|f| f.ends_with(".tscn")));
    j.look("rig-exported");
    let files = files_in(&project);
    let scene = files
        .iter()
        .find(|f| f.ends_with(".tscn"))
        .and_then(|f| std::fs::read_to_string(project.join(f)).ok())
        .unwrap_or_default();
    j.note(format!(
        "files in game: {files:?}; the scene has {} Bone2D and {} Polygon2D nodes; it said: {}",
        scene.matches("type=\"Bone2D\"").count(),
        scene.matches("type=\"Polygon2D\"").count(),
        j.heard().replace(&j.folder.display().to_string(), "")
    ));
    j.expect(
        "the-rig-exports-with-its-bones",
        offered && written && scene.matches("type=\"Bone2D\"").count() == 2 && scene.contains("Skeleton2D"),
        "the export writes a scene with a Skeleton2D and the two bones",
    );

    j.finish(&[]);
}

fn maps_on(j: &Journey) -> bool {
    j.ed().is_some_and(|ed| ed.scene.bundle.doc.maps.is_some())
}

fn shows(j: &Journey) -> String {
    j.ed()
        .and_then(|ed| ed.maps.show)
        .map_or_else(|| "the picture".to_string(), |show| format!("{show:?}"))
}

fn lights(j: &Journey) -> usize {
    j.ed()
        .and_then(|ed| ed.scene.bundle.doc.maps.as_ref().map(|m| m.lights.len()))
        .unwrap_or(0)
}

#[test]
fn someone_lights_a_drawing_for_a_game() {
    let mut j = Journey::begin("light-maps", WINDOW, Some("lantern.klad"));

    j.act("open the lantern and bring up the light maps: Window > Light maps");
    j.look("opened");
    if j.ed().is_none() {
        return j.finish(&[]);
    }
    j.click("Window");
    let asked = j.click("Light maps");
    j.look("light-maps-panel");
    let title = j.hint_in_sight("fold Light maps to its title");
    let boxed = j.in_sight("light maps");
    j.note(format!(
        "the panel's title is in sight: {title}; its light maps box is in sight: {boxed}; it said: {}",
        j.heard()
    ));
    j.expect(
        "the-light-maps-panel-opens-with-its-switch-in-sight",
        asked && title && boxed,
        "Window > Light maps opens the panel with the box that turns the maps on in sight",
    );

    j.act("turn them on: the light maps box");
    if !boxed {
        j.scroll_to("light maps");
    }
    j.click("light maps");
    j.look("maps-on");
    j.note(format!("light maps are on: {}; it said: {}", maps_on(&j), j.heard()));
    j.expect(
        "the-box-turns-the-maps-on",
        maps_on(&j),
        "the box turns the light maps on",
    );

    j.act("look at the maps in turn with M, back to the picture");
    let seen: Vec<String> = (0..7)
        .map(|_| {
            j.key(Key::M);
            j.settle();
            shows(&j)
        })
        .collect();
    j.look("after-m");
    j.note(format!("M showed in turn: {seen:?}"));
    j.expect(
        "m-walks-the-maps-and-returns-to-the-picture",
        seen.first().is_some_and(|s| s.contains("Normal")) && seen.last().is_some_and(|s| s == "the picture"),
        "M shows the normal map first and the picture again after the last map",
    );

    j.act("see it lit: the lit button of the panel, then hold L and move the pointer over the lantern");
    let reach = j.in_sight("lit") || j.scroll_to("lit");
    let lit = reach && j.click("lit");
    j.settle();
    j.look("lit");
    j.note(format!("the canvas shows {}; it said: {}", shows(&j), j.heard()));
    j.hold(Key::L, true);
    j.hover_with(300.0, 300.0, Modifiers::NONE);
    j.h.run_steps(6);
    j.settle();
    j.look("lit-by-the-pointer");
    j.hover_with(700.0, 600.0, Modifiers::NONE);
    j.h.run_steps(6);
    j.settle();
    j.look("lit-from-the-other-side");
    j.hold(Key::L, false);
    j.expect(
        "the-lit-button-shows-the-picture-lit",
        lit && shows(&j).contains("Lit"),
        "the lit button shows the picture lit",
    );

    j.act("a light that stays: the panel's add a point light button");
    let reach = j.in_sight("add a point light") || j.scroll_to("add a point light");
    j.look("looking-for-the-lights");
    j.expect(
        "the-preview-lights-are-in-reach",
        reach,
        "the panel's lights can be brought in sight with the timeline open",
    );
    if !reach {
        j.note("the lights lie under the timeline; closing it with Window > Timeline");
        j.click("Window");
        j.click("Timeline");
        j.scroll_to("add a point light");
    }
    let added = j.click("add a point light");
    j.settle();
    j.look("point-light");
    j.note(format!("the drawing has {} lights; it said: {}", lights(&j), j.heard()));
    j.expect(
        "a-point-light-is-added",
        added && lights(&j) == 1,
        "the button adds a point light to the drawing",
    );
    if !reach {
        j.click("Window");
        j.click("Timeline");
    }

    j.act("raise the flame: a map layer from the Layers panel's map button, a white stroke over the flame");
    let before = names(&j);
    let reach = j.hint_in_sight("new map layer") || j.scroll_to_hinted("new map layer");
    let made = reach && j.press_hinted("new map layer");
    j.look("map-layer");
    let fresh = names(&j).into_iter().find(|n| !before.contains(n)).unwrap_or_default();
    j.note(format!(
        "layers {:?}; the new layer is {fresh:?}, a {:?} map; it said: {}",
        names(&j),
        layer_of(&j, &fresh).and_then(|l| l.map),
        j.heard()
    ));
    j.expect(
        "the-map-button-makes-a-height-layer",
        made && layer_of(&j, &fresh).is_some_and(|l| l.map.is_some()),
        "the map button makes a height layer above the layer in hand",
    );
    j.key(Key::B);
    j.drag(&[(500.0, 380.0), (512.0, 420.0), (520.0, 460.0)]);
    j.settle();
    j.look("height-stroke");
    j.note(format!(
        "the map layer holds {} marks; the brush colour is {}; it said: {}",
        count_on(&j, &fresh),
        j.ed().map_or(String::new(), |ed| crate::color::hex(ed.color)),
        j.heard()
    ));
    j.expect(
        "a-stroke-lands-on-the-map-layer",
        count_on(&j, &fresh) == 1,
        "the stroke is on the map layer",
    );

    j.act("have klad check the maps: the check the maps box");
    let reach = j.in_sight("check the maps") || j.scroll_to("check the maps");
    let checked = reach && j.click("check the maps");
    j.settle();
    j.look("maps-checked");
    j.note(format!("it said: {}", j.heard()));
    j.expect(
        "check-the-maps-answers",
        checked && !j.heard().is_empty(),
        "check the maps says what it found",
    );

    j.act("export the picture with its normal map: File > Export > Image…, the normal box, Export");
    j.menu("File", "Export", "Image… cmd E");
    j.look("export-window");
    let offered = j.click("normal");
    j.click("Export");
    let folder = j.folder.clone();
    let written = j.wait("the picture and its map", |_| pngs(&folder).len() >= 2);
    j.look("exported");
    let made: Vec<String> = pngs(&j.folder)
        .iter()
        .map(|p| {
            format!(
                "{} {:?}",
                p.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default(),
                png_size(p)
            )
        })
        .collect();
    j.note(format!(
        "pictures written: {made:?}; it said: {}",
        j.heard().replace(&j.folder.display().to_string(), "")
    ));
    j.expect(
        "the-normal-map-exports-beside-the-picture",
        offered && written && made.len() == 2,
        "the export writes the picture and its normal map, the same size",
    );
    j.press_hinted(&format!("close {} Export", crate::theme::icon::EXPORT));

    j.act("and for Godot with a rim light: File > Export > Godot…, the folder game/art, the rim shader box, Export");
    j.menu("File", "Export", "Godot…");
    j.retype("Godot folder", "game/art");
    let shader = j.click("rim shader");
    j.look("godot-ready");
    j.click("Export");
    let project = j.folder.join("game").join("art");
    let folder = project.clone();
    let written = j.wait("the scene", |ed| {
        ed.jobs.is_empty() && files_in(&folder).iter().any(|f| f.ends_with(".tscn"))
    });
    j.look("godot-exported");
    let files = files_in(&project);
    let text: String = files
        .iter()
        .filter(|f| f.ends_with(".tscn") || f.ends_with(".tres"))
        .filter_map(|f| std::fs::read_to_string(project.join(f)).ok())
        .collect();
    j.note(format!(
        "files in game/art: {files:?}; a normal map is wired in: {}; it said: {}",
        text.contains("normal_texture"),
        j.heard().replace(&j.folder.display().to_string(), "")
    ));
    j.expect(
        "the-godot-export-carries-the-maps-and-the-shader",
        shader && written && text.contains("normal_texture") && files.iter().any(|f| f.ends_with(".gdshader")),
        "the Godot export wires the normal map in and writes the rim shader beside it",
    );
    let root = j.folder.clone();
    if let Some(said) = godot(&mut j, &root.join("game"), "lantern") {
        j.expect(
            "godot-loads-the-lit-scene",
            said.contains("ROOT "),
            "Godot loads the scene",
        );
    }

    j.finish(&[]);
}
