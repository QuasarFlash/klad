use super::ui_tests::{activate, canvas_at, drag, editor, harness, layer_named, marks_on, said, sample};
use super::{Editor, Focus, Picked, Tool};
use crate::commands::Cmd;
use egui::{Key, Modifiers, vec2};
use egui_kittest::Harness;
use klad_core::doc::Node;
use klad_core::ops::parse_lines;
use klad_core::stroke::Mark;

fn nodes(h: &Harness<'static, Editor>, layer: &str) -> Vec<Node> {
    layer_named(h, layer).cels()[0].nodes.clone()
}

fn ids(nodes: &[Node]) -> Vec<String> {
    nodes.iter().map(|n| n.id.clone()).collect()
}

fn chord(h: &mut Harness<'static, Editor>, m: Modifiers, key: Key) {
    h.key_press_modifiers(m, key);
    h.run_steps(3);
}

fn hold(h: &mut Harness<'static, Editor>, m: Modifiers) {
    h.event(egui::Event::ModifiersChanged(m));
    h.run_steps(1);
}

type Ends = ((f32, f32), (f32, f32), usize);

fn ends(h: &Harness<'static, Editor>) -> Vec<Ends> {
    h.state()
        .scene
        .active_marks()
        .iter()
        .filter_map(Mark::as_stroke)
        .map(|s| {
            (
                s.samples.first().map(|p| p.pos()).unwrap_or_default(),
                s.samples.last().map(|p| p.pos()).unwrap_or_default(),
                s.samples.len(),
            )
        })
        .collect()
}

fn near(a: (f32, f32), b: (f32, f32)) -> bool {
    (a.0 - b.0).abs() < 1.5 && (a.1 - b.1).abs() < 1.5
}

fn click(h: &mut Harness<'static, Editor>, at: egui::Pos2) {
    h.hover_at(at);
    h.run_steps(1);
    h.drag_at(at);
    h.run_steps(1);
    h.drop_at(at);
    h.run_steps(2);
}

fn add_rects(h: &mut Harness<'static, Editor>) -> Vec<String> {
    let layer = layer_named(h, "shapes").id.clone();
    let ops = parse_lines(&format!(
        r##"{{"op":"add","node":{{"type":"rect","x":60,"y":40,"w":120,"h":80,"fill":"#3040a0"}},"in":"{layer}","frame":0}}
{{"op":"add","node":{{"type":"rect","x":100,"y":60,"w":120,"h":80,"fill":"#30a040"}},"in":"{layer}","frame":0}}"##
    ))
    .unwrap();
    h.state_mut().apply(ops).unwrap();
    h.run_steps(2);
    ids(&nodes(h, "shapes"))
}

#[test]
fn cmd_x_cuts_strokes_shapes_and_cells_each_in_one_undo_step() {
    let path = sample("cut");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    activate(&mut h, "lines");
    assert_eq!(marks_on(&h, &layer_named(&h, "lines").clone()), 2);
    h.key_press(Key::S);
    h.run_steps(2);
    chord(&mut h, Modifiers::COMMAND, Key::A);
    chord(&mut h, Modifiers::COMMAND, Key::X);
    assert_eq!(marks_on(&h, &layer_named(&h, "lines").clone()), 0, "{}", said(&h));
    assert!(said(&h).contains("cut 2 marks"), "{}", said(&h));
    chord(&mut h, Modifiers::COMMAND, Key::Z);
    assert_eq!(
        marks_on(&h, &layer_named(&h, "lines").clone()),
        2,
        "one cmd Z takes the cut back"
    );
    activate(&mut h, "colour");
    chord(&mut h, Modifiers::COMMAND, Key::V);
    assert_eq!(marks_on(&h, &layer_named(&h, "colour").clone()), 2, "{}", said(&h));

    activate(&mut h, "shapes");
    let id = nodes(&h, "shapes")[0].id.clone();
    h.state_mut().tool = Tool::Select;
    h.state_mut().selection = Some(id);
    h.run_steps(2);
    chord(&mut h, Modifiers::COMMAND, Key::X);
    assert!(nodes(&h, "shapes").is_empty(), "{}", said(&h));
    assert!(said(&h).contains("cut one shape"), "{}", said(&h));
    chord(&mut h, Modifiers::COMMAND, Key::V);
    assert_eq!(nodes(&h, "shapes").len(), 1, "the cut shape pastes back: {}", said(&h));

    activate(&mut h, "lines");
    h.state_mut().focus = Focus::Timeline;
    assert_eq!(
        h.state().for_focus(Cmd::Cut),
        Cmd::CutCells,
        "cmd X in the timeline cuts cells"
    );
    let cels = |h: &Harness<'static, Editor>| layer_named(h, "lines").cels().len();
    assert_eq!(cels(&h), 1);
    chord(&mut h, Modifiers::COMMAND, Key::X);
    assert_eq!(cels(&h), 0, "{}", said(&h));
    assert!(said(&h).contains("cut 1 drawing"), "{}", said(&h));
    assert!(h.state().timeline.cells.is_some());
    chord(&mut h, Modifiers::COMMAND, Key::Z);
    assert_eq!(cels(&h), 1, "one cmd Z brings the drawing back");
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn shapes_and_strokes_come_to_the_front_and_go_back_with_the_bracket_keys() {
    let path = sample("arrange");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    activate(&mut h, "shapes");
    let start = add_rects(&mut h);
    assert_eq!(start.len(), 3);
    h.state_mut().tool = Tool::Select;
    h.state_mut().selection = Some(start[0].clone());
    h.run_steps(2);
    chord(&mut h, Modifiers::COMMAND | Modifiers::SHIFT, Key::CloseBracket);
    assert_eq!(
        ids(&nodes(&h, "shapes")),
        [start[1].clone(), start[2].clone(), start[0].clone()],
        "{}",
        said(&h)
    );
    assert!(said(&h).contains("brought the shape to the front"), "{}", said(&h));
    chord(&mut h, Modifiers::COMMAND, Key::CloseBracket);
    assert!(said(&h).contains("already in front"), "{}", said(&h));
    chord(&mut h, Modifiers::COMMAND, Key::OpenBracket);
    assert_eq!(
        ids(&nodes(&h, "shapes")),
        [start[1].clone(), start[0].clone(), start[2].clone()]
    );
    chord(&mut h, Modifiers::COMMAND | Modifiers::SHIFT, Key::OpenBracket);
    assert_eq!(ids(&nodes(&h, "shapes")), start);
    chord(&mut h, Modifiers::COMMAND, Key::Z);
    assert_eq!(
        ids(&nodes(&h, "shapes")),
        [start[1].clone(), start[0].clone(), start[2].clone()],
        "each change of place is one undo step"
    );

    activate(&mut h, "lines");
    h.state_mut().tool = Tool::Strokes;
    let cel = h.state().scene.active_cel().unwrap();
    let marks: Vec<u32> = h.state().scene.active_marks().iter().map(Mark::id).collect();
    h.state_mut().picked = Some(Picked::whole(cel.clone(), vec![marks[0]]));
    h.run_steps(2);
    assert!(h.state().can_run(Cmd::BringForward).is_none());
    chord(&mut h, Modifiers::COMMAND, Key::CloseBracket);
    let now: Vec<u32> = h.state().scene.active_marks().iter().map(Mark::id).collect();
    assert_eq!(now, [marks[1], marks[0]], "{}", said(&h));
    assert!(said(&h).contains("brought the mark forward one place"), "{}", said(&h));
    h.state_mut().picked = None;
    h.state_mut().selection = None;
    h.run_steps(1);
    assert!(
        h.state()
            .can_run(Cmd::SendToBack)
            .is_some_and(|why| why.contains("select strokes (S) or shapes (V) first"))
    );
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn shift_paints_straight_lines_and_the_brush_paints_rectangles_and_ellipses() {
    let path = sample("straight");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    activate(&mut h, "colour");
    let (a, b) = (canvas_at(&h, 100.0, 100.0), canvas_at(&h, 200.0, 150.0));
    drag(&mut h, a, b);
    h.run_steps(2);
    let first = ends(&h);
    assert_eq!(first.len(), 1, "{}", said(&h));
    hold(&mut h, Modifiers::SHIFT);
    let at = canvas_at(&h, 300.0, 150.0);
    click(&mut h, at);
    hold(&mut h, Modifiers::NONE);
    let after = ends(&h);
    assert_eq!(after.len(), 2, "{}", said(&h));
    assert!(
        near(after[1].0, first[0].1),
        "a shift click starts at the end of the last stroke: {after:?}"
    );
    assert!(near(after[1].1, (300.0, 150.0)), "{after:?}");
    assert!(after[1].2 > 20, "the straight line is sampled along its length");

    hold(&mut h, Modifiers::SHIFT);
    let (c, d) = (canvas_at(&h, 100.0, 300.0), canvas_at(&h, 300.0, 312.0));
    drag(&mut h, c, d);
    hold(&mut h, Modifiers::NONE);
    let level = ends(&h);
    assert_eq!(level.len(), 3);
    assert!(near(level[2].0, (100.0, 300.0)), "{level:?}");
    assert!(
        (level[2].1.1 - 300.0).abs() < 1.5,
        "shift keeps the stroke level: {level:?}"
    );

    h.state_mut().brush_figure = super::straight::Figure::Rect;
    let (e, f) = (canvas_at(&h, 320.0, 200.0), canvas_at(&h, 420.0, 280.0));
    drag(&mut h, e, f);
    let rect = h
        .state()
        .scene
        .active_marks()
        .last()
        .and_then(Mark::as_stroke)
        .map(|s| s.samples.iter().map(|p| p.pos()).collect::<Vec<_>>())
        .unwrap();
    let (lo, hi) = rect
        .iter()
        .fold(((f32::MAX, f32::MAX), (f32::MIN, f32::MIN)), |(lo, hi), p| {
            ((lo.0.min(p.0), lo.1.min(p.1)), (hi.0.max(p.0), hi.1.max(p.1)))
        });
    assert!(near(lo, (320.0, 200.0)) && near(hi, (420.0, 280.0)), "{lo:?} {hi:?}");
    assert!(near(rect[0], *rect.last().unwrap()), "a rectangle closes");
    h.state_mut().brush_figure = super::straight::Figure::Ellipse;
    let count = ends(&h).len();
    let (g, k) = (canvas_at(&h, 100.0, 200.0), canvas_at(&h, 160.0, 240.0));
    drag(&mut h, g, k);
    assert_eq!(ends(&h).len(), count + 1, "{}", said(&h));
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn a_shape_moves_level_with_shift_scales_from_its_far_corner_and_takes_typed_numbers() {
    let path = sample("modifiers");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    activate(&mut h, "shapes");
    let id = nodes(&h, "shapes")[0].id.clone();
    h.state_mut().tool = Tool::Select;
    let inside = canvas_at(&h, 120.0, 30.0);
    click(&mut h, inside);
    h.run_steps(3);
    assert_eq!(h.state().selection, Some(id.clone()), "{}", said(&h));
    let corners = |h: &Harness<'static, Editor>| h.state().handles().map(|(_, c)| c).unwrap();
    let before = corners(&h);
    assert!(near((before[0].x, before[0].y), (20.0, 20.0)), "{before:?}");
    let (from, to) = (canvas_at(&h, 140.0, 100.0), canvas_at(&h, 200.0, 140.0));
    drag(&mut h, from, to);
    h.run_steps(3);
    let grown = corners(&h);
    assert!(
        near((grown[0].x, grown[0].y), (20.0, 20.0)),
        "a corner scales from the opposite corner, which stays: {grown:?}"
    );
    assert!(grown[2].x > 180.0, "{grown:?}");
    chord(&mut h, Modifiers::COMMAND, Key::Z);
    hold(&mut h, Modifiers::SHIFT);
    let (from, to) = (canvas_at(&h, 120.0, 30.0), canvas_at(&h, 220.0, 45.0));
    drag(&mut h, from, to);
    hold(&mut h, Modifiers::NONE);
    h.run_steps(3);
    let moved = corners(&h);
    assert!(
        near((moved[0].x, moved[0].y), (120.0, 20.0)),
        "shift keeps the move level: {moved:?} {}",
        said(&h)
    );

    h.state_mut().apply_typed(0, 300.0).unwrap();
    h.run_steps(2);
    assert!(
        near((corners(&h)[0].x, corners(&h)[0].y), (300.0, 20.0)),
        "{:?}",
        corners(&h)
    );
    h.state_mut().apply_typed(2, 240.0).unwrap();
    h.run_steps(2);
    let wide = corners(&h);
    assert!(
        (wide[1].x - wide[0].x - 240.0).abs() < 1.0 && (wide[3].y - wide[0].y - 80.0).abs() < 1.0,
        "typing a width stretches the width only: {wide:?}"
    );
    h.state_mut().apply_typed(4, 30.0).unwrap();
    h.run_steps(2);
    let node = nodes(&h, "shapes").into_iter().find(|n| n.id == id).unwrap();
    assert!((node.transform.rotate - 30.0).abs() < 0.01, "{}", node.transform.rotate);

    activate(&mut h, "lines");
    h.state_mut().tool = Tool::Strokes;
    h.state_mut().selection = None;
    let cel = h.state().scene.active_cel().unwrap();
    let marks: Vec<u32> = h.state().scene.active_marks().iter().map(Mark::id).collect();
    h.state_mut().picked = Some(Picked::whole(cel, marks));
    h.run_steps(2);
    h.state_mut().apply_typed(1, 10.0).unwrap();
    h.run_steps(2);
    let top = corners(&h).iter().map(|c| c.y).fold(f32::MAX, f32::min);
    assert!((top - 10.0).abs() < 1.0, "typing y moves the strokes: {top}");
    let _ = std::fs::remove_dir_all(&path);
}

fn picked_ids(h: &Harness<'static, Editor>) -> Vec<u32> {
    let mut ids = h.state().picked.as_ref().map(|p| p.ids.clone()).unwrap_or_default();
    ids.sort_unstable();
    ids
}

#[test]
fn an_ellipse_a_wand_and_alt_select_and_take_away_strokes() {
    use super::marquee::Marquee;
    let path = sample("marquee");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    activate(&mut h, "lines");
    h.key_press(Key::S);
    h.run_steps(2);
    let layer = layer_named(&h, "lines").id.clone();
    let short = parse_lines(&format!(
        r#"{{"op":"stroke","layer":"{layer}","path":"M200 330 L260 330","size":6}}"#
    ))
    .unwrap();
    h.state_mut().apply(short).unwrap();
    h.run_steps(2);
    let marks: Vec<u32> = h.state().scene.active_marks().iter().map(Mark::id).collect();
    assert_eq!(marks.len(), 3);

    h.state_mut().marquee = Marquee::Ellipse;
    let (a, b) = (canvas_at(&h, 180.0, 312.0), canvas_at(&h, 280.0, 348.0));
    drag(&mut h, a, b);
    h.run_steps(2);
    assert_eq!(
        picked_ids(&h),
        [marks[2]],
        "the ellipse takes the stroke wholly inside it: {}",
        said(&h)
    );

    h.state_mut().marquee = Marquee::Box;
    chord(&mut h, Modifiers::COMMAND, Key::A);
    assert_eq!(picked_ids(&h).len(), 3);
    hold(&mut h, Modifiers::ALT);
    let (a, b) = (canvas_at(&h, 310.0, 210.0), canvas_at(&h, 30.0, 30.0));
    drag(&mut h, a, b);
    hold(&mut h, Modifiers::NONE);
    assert_eq!(
        picked_ids(&h),
        [marks[1], marks[2]],
        "alt takes the boxed stroke away: {}",
        said(&h)
    );

    chord(&mut h, Modifiers::NONE, Key::Escape);
    h.state_mut().marquee = Marquee::Wand;
    let empty = canvas_at(&h, 480.0, 360.0);
    click(&mut h, empty);
    assert!(
        said(&h).contains("the wand takes an area of one colour"),
        "{}",
        said(&h)
    );
    let on = canvas_at(&h, 170.0, 120.0);
    click(&mut h, on);
    assert!(h.state().picked.is_some(), "{}", said(&h));
    assert!(
        picked_ids(&h).contains(&marks[0]),
        "the wand takes the stroke it was clicked on: {:?}",
        picked_ids(&h)
    );
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn a_picture_from_another_app_pastes_on_a_new_layer_and_copies_go_out_as_pictures() {
    let path = sample("pictures");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    activate(&mut h, "colour");
    let layers = h.state().scene.bundle.doc.all_layers().len();
    let red = egui::ColorImage::new([20, 10], vec![egui::Color32::RED; 200]);
    h.event(egui::Event::PasteImage(std::sync::Arc::new(red)));
    h.run_steps(3);
    assert_eq!(
        h.state().scene.bundle.doc.all_layers().len(),
        layers + 1,
        "{}",
        said(&h)
    );
    assert!(said(&h).contains("pasted a 20 by 10 px picture"), "{}", said(&h));
    assert_eq!(h.state().tool, Tool::Select);
    assert!(
        h.state().selection.is_some(),
        "the picture is selected so it moves at once"
    );

    activate(&mut h, "lines");
    h.key_press(Key::S);
    h.run_steps(2);
    chord(&mut h, Modifiers::COMMAND, Key::A);
    chord(&mut h, Modifiers::COMMAND, Key::C);
    let own = h.state().own_picture.expect("cmd C puts a picture on the clipboard");
    assert!(
        own.0 > 200 && own.1 > 150,
        "the picture is the selection's box: {own:?}"
    );
    let ours = h.state().selection_picture().unwrap();
    let image = egui::ColorImage::from_rgba_unmultiplied([ours.width as usize, ours.height as usize], &ours.rgba);
    activate(&mut h, "colour");
    let before = marks_on(&h, &layer_named(&h, "colour").clone());
    let all = h.state().scene.bundle.doc.all_layers().len();
    h.event(egui::Event::PasteImage(std::sync::Arc::new(image)));
    h.run_steps(3);
    assert_eq!(
        (
            marks_on(&h, &layer_named(&h, "colour").clone()),
            h.state().scene.bundle.doc.all_layers().len()
        ),
        (before + 2, all),
        "klad's own picture pastes back as the strokes it was copied from: {}",
        said(&h)
    );

    chord(&mut h, Modifiers::NONE, Key::Escape);
    h.state_mut().picked = None;
    h.state_mut().selection = None;
    h.state_mut().invoke(Cmd::CopyMerged).unwrap();
    h.run_steps(2);
    assert_eq!(
        h.state().own_picture.map(|o| (o.0, o.1)),
        Some((512, 384)),
        "{}",
        said(&h)
    );
    assert!(said(&h).contains("copied the whole picture"), "{}", said(&h));
    let png = path.join("photo.png");
    std::fs::write(&png, klad_render::clipboard::png(&ours)).unwrap();
    assert!(crate::start::foreign(&png), "File > Open takes pictures");
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn the_wand_on_a_pixel_layer_takes_the_joined_pixels_of_one_colour_and_leaves_the_hole() {
    use super::marquee::Marquee;
    let path = super::ui_tests::pixel_sample("wand");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    h.key_press(Key::S);
    h.run_steps(2);
    h.state_mut().marquee = Marquee::Wand;
    let at = canvas_at(&h, 4.5, 4.5);
    click(&mut h, at);
    let marks: Vec<u32> = h.state().scene.active_marks().iter().map(Mark::id).collect();
    assert_eq!(marks.len(), 2);
    assert_eq!(
        picked_ids(&h),
        [marks[0]],
        "the ring of dark pixels, and not the red pixel in its hole: {}",
        said(&h)
    );
    let red = canvas_at(&h, 5.5, 5.5);
    click(&mut h, red);
    assert_eq!(picked_ids(&h), [marks[1]]);
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn shapes_take_another_look_combine_into_one_path_and_text_becomes_paths() {
    let path = sample("shape-tools");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    activate(&mut h, "shapes");
    let ids = add_rects(&mut h);
    h.state_mut().tool = Tool::Select;
    h.state_mut().selection = Some(ids[0].clone());
    h.run_steps(2);
    h.state_mut().invoke(Cmd::CopyLook).unwrap();
    assert!(said(&h).contains("copied the look"), "{}", said(&h));
    h.state_mut().selection = Some(ids[1].clone());
    h.run_steps(1);
    h.state_mut().invoke(Cmd::PasteLook).unwrap();
    h.run_steps(2);
    let now = nodes(&h, "shapes");
    assert_eq!(now[1].fill, now[0].fill, "{}", said(&h));

    let widths = |h: &Harness<'static, Editor>| {
        nodes(h, "shapes")
            .iter()
            .map(|n| n.stroke.as_ref().map(|s| s.width))
            .collect::<Vec<_>>()
    };
    let set = |h: &mut Harness<'static, Editor>, id: &str, props: serde_json::Value| {
        let props = props.as_object().unwrap().clone();
        h.state_mut()
            .apply(vec![klad_core::ops::Op::Set {
                id: id.to_string(),
                props,
            }])
            .unwrap();
        h.run_steps(1);
    };
    set(
        &mut h,
        &ids[2],
        serde_json::json!({"stroke.width": 6.0, "stroke.paint": "#102030"}),
    );
    assert_eq!(
        widths(&h)[2],
        Some(6.0),
        "an outline width on a shape without one gives it an outline"
    );
    set(
        &mut h,
        &ids[2],
        serde_json::json!({"stroke.cap": "square", "stroke.join": "bevel"}),
    );
    set(&mut h, &ids[2], serde_json::json!({"stroke": null}));
    assert_eq!(widths(&h)[2], None, "width 0 takes the outline off");

    h.state_mut().selection = Some(ids[1].clone());
    h.state_mut().group = vec![ids[1].clone(), ids[2].clone()];
    h.run_steps(1);
    assert!(
        h.state().can_run(Cmd::Unite).is_none(),
        "{:?}",
        h.state().can_run(Cmd::Unite)
    );
    h.state_mut().invoke(Cmd::Unite).unwrap();
    h.run_steps(2);
    let after = nodes(&h, "shapes");
    assert_eq!(after.len(), 2, "two rectangles became one path: {}", said(&h));
    let made = after
        .iter()
        .find(|n| matches!(n.shape, klad_core::doc::Shape::Path { .. }))
        .unwrap();
    assert_eq!(h.state().selection, Some(made.id.clone()));
    assert!(said(&h).contains("united 2 shapes into one path"), "{}", said(&h));

    h.state_mut().group = vec![ids[0].clone(), made.id.clone()];
    h.state_mut().selection = Some(ids[0].clone());
    h.run_steps(1);
    h.state_mut().invoke(Cmd::SubtractShapes).unwrap();
    h.run_steps(2);
    assert_eq!(nodes(&h, "shapes").len(), 1, "{}", said(&h));
    h.state_mut().group.clear();
    h.run_steps(1);
    assert!(
        h.state()
            .can_run(Cmd::Intersect)
            .is_some_and(|w| w.contains("two shapes or more"))
    );

    let root = h.state().scene.bundle.root.clone();
    klad_render::fonts::bring(&root, "Inter")
        .unwrap_or_else(|e| panic!("{e}; klad's fonts folder: {:?}", klad_render::fonts::shipped_dir()));
    let layer = layer_named(&h, "shapes").id.clone();
    let text = parse_lines(&format!(
        r##"{{"op":"add","node":{{"id":"words","type":"text","text":"Ab","font":"Inter.ttf","x":40,"y":200,"size":60,"fill":"#202020"}},"in":"{layer}","frame":0}}"##
    ))
    .unwrap();
    h.state_mut().apply(text).unwrap();
    h.state_mut().selection = Some("words".into());
    h.run_steps(2);
    assert!(h.state().can_run(Cmd::MakePath).is_none());
    h.state_mut().invoke(Cmd::MakePath).unwrap();
    h.run_steps(2);
    let shapes = nodes(&h, "shapes");
    assert!(shapes.iter().all(|n| n.id != "words"));
    assert!(
        shapes
            .iter()
            .any(|n| matches!(&n.shape, klad_core::doc::Shape::Path { d } if d.len() > 40)),
        "the letters are a path: {}",
        said(&h)
    );

    h.state_mut().invoke(Cmd::ExportSvg).unwrap();
    h.run_steps(2);
    assert!(h.state().windows.export);
    let form = h.state().export.clone();
    assert_eq!(form.only.len(), 1, "with a shape selected the SVG holds it alone");
    let args = super::export::args(&root, &root, 0, &form);
    assert!(
        args.contains(&"--svg".to_string()) && args.contains(&"--only".to_string()),
        "{args:?}"
    );
    let _ = std::fs::remove_dir_all(&path);
}

fn right_click_row(h: &mut Harness<'static, Editor>, name: &str) {
    use egui_kittest::kittest::{NodeT, Queryable};
    let canvas = h.state().canvas_rect;
    let found = h
        .query_all_by_label(name)
        .filter(|n| n.accesskit_node().role() == egui::accesskit::Role::Button)
        .find(|n| !canvas.contains(n.rect().center()))
        .expect("a row of that name");
    found.click_secondary();
    h.run_steps(4);
}

fn shown(h: &Harness<'static, Editor>, label: &str) -> bool {
    use egui_kittest::kittest::Queryable;
    h.query_all_by_label_contains(label).next().is_some()
}

#[test]
fn a_right_click_on_a_layer_opens_its_menu_and_layers_merge_move_and_step_from_the_keyboard() {
    let path = sample("layer-menu");
    let mut h = harness(editor(&path), vec2(1440.0, 1600.0));
    activate(&mut h, "lines");
    right_click_row(&mut h, "colour");
    assert_eq!(
        h.state().scene.active_layer().map(|l| l.name.clone()).as_deref(),
        Some("colour")
    );
    assert!(
        shown(&h, "Merge down") && shown(&h, "Rename") && shown(&h, "Make it a map layer"),
        "the paint layer's menu"
    );
    assert!(!shown(&h, "Ungroup"), "ungroup is for groups");
    h.key_press(Key::Escape);
    h.run_steps(2);
    right_click_row(&mut h, "shapes");
    assert!(
        shown(&h, "Merge down") && !shown(&h, "Lock alpha") && !shown(&h, "Make it a colour layer"),
        "the vector layer's menu"
    );
    h.key_press(Key::Escape);
    h.run_steps(2);

    activate(&mut h, "colour");
    chord(&mut h, Modifiers::ALT, Key::CloseBracket);
    assert_eq!(
        h.state().scene.active_layer().map(|l| l.name.clone()).as_deref(),
        Some("lines"),
        "alt ] goes to the layer above"
    );
    chord(&mut h, Modifiers::ALT, Key::OpenBracket);
    assert_eq!(
        h.state().scene.active_layer().map(|l| l.name.clone()).as_deref(),
        Some("colour")
    );
    let order = |h: &Harness<'static, Editor>| -> Vec<String> {
        h.state()
            .scene
            .bundle
            .doc
            .layers
            .iter()
            .map(|l| l.name.clone())
            .collect()
    };
    let before = order(&h);
    chord(&mut h, Modifiers::ALT | Modifiers::COMMAND, Key::CloseBracket);
    let after = order(&h);
    let at = |list: &[String], name: &str| list.iter().position(|n| n == name).unwrap();
    assert_eq!(
        at(&after, "colour"),
        at(&before, "colour") + 1,
        "alt cmd ] moves the layer up: {after:?}"
    );
    chord(&mut h, Modifiers::ALT | Modifiers::COMMAND, Key::OpenBracket);
    assert_eq!(order(&h), before);

    activate(&mut h, "lines");
    let marks = marks_on(&h, &layer_named(&h, "lines").clone()) + marks_on(&h, &layer_named(&h, "colour").clone());
    h.state_mut().invoke(Cmd::MergeDown).unwrap();
    h.run_steps(3);
    assert!(said(&h).contains("merged 2 layers into colour"), "{}", said(&h));
    assert!(h.state().scene.bundle.doc.find_layer("lines").is_err());
    assert_eq!(marks_on(&h, &layer_named(&h, "colour").clone()), marks);
    chord(&mut h, Modifiers::COMMAND, Key::Z);
    assert!(
        h.state().scene.bundle.doc.find_layer("lines").is_ok(),
        "one cmd Z takes the merge back"
    );

    activate(&mut h, "rim");
    let shade = layer_named(&h, "shade").id.clone();
    h.state_mut().also = vec![shade];
    h.run_steps(1);
    h.state_mut().invoke(Cmd::ToggleVisible).unwrap();
    h.run_steps(2);
    assert!(
        !layer_named(&h, "rim").visible && !layer_named(&h, "shade").visible,
        "hide acts on every selected layer"
    );
    h.state_mut().invoke(Cmd::GroupLayer).unwrap();
    h.run_steps(2);
    let group = h.state().scene.active_layer().unwrap().clone();
    assert_eq!(group.children().len(), 2, "group takes every selected layer");
    right_click_row(&mut h, "group");
    assert!(shown(&h, "Ungroup"));
    h.key_press(Key::Escape);
    h.run_steps(2);
    h.state_mut().invoke(Cmd::Ungroup).unwrap();
    h.run_steps(2);
    assert!(h.state().scene.bundle.doc.find_layer("group").is_err(), "{}", said(&h));
    assert!(!layer_named(&h, "rim").id.is_empty());
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn the_canvas_turns_crops_to_a_drag_and_takes_a_size_and_a_paper_from_its_window() {
    use egui_kittest::kittest::{NodeT, Queryable};
    let path = sample("canvas");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    let size = |h: &Harness<'static, Editor>| h.state().scene.size();
    assert_eq!(size(&h), (512, 384));
    h.state_mut().invoke(Cmd::CanvasRight).unwrap();
    h.run_steps(4);
    assert_eq!(size(&h), (384, 512), "{}", said(&h));
    assert!(said(&h).contains("turned the drawing right"), "{}", said(&h));
    chord(&mut h, Modifiers::COMMAND, Key::Z);
    h.run_steps(4);
    assert_eq!(size(&h), (512, 384), "one cmd Z turns it back");

    h.key_press(Key::C);
    h.run_steps(2);
    assert!(h.state().cropping.is_some(), "C starts the crop");
    let (a, b) = (canvas_at(&h, 50.0, 40.0), canvas_at(&h, 350.0, 240.0));
    drag(&mut h, a, b);
    h.key_press(Key::Enter);
    h.run_steps(4);
    let (w, hh) = size(&h);
    assert!(
        (w as i32 - 300).abs() <= 2 && (hh as i32 - 200).abs() <= 2,
        "cropped to the drag: {w}x{hh} {}",
        said(&h)
    );
    assert!(h.state().cropping.is_none());

    h.state_mut().invoke(Cmd::Trim).unwrap();
    h.run_steps(4);
    assert!(said(&h).contains("trimmed to what is drawn"), "{}", said(&h));

    h.state_mut().invoke(Cmd::CanvasWindow).unwrap();
    h.run_steps(3);
    h.state_mut().canvas_form.width = 400;
    h.state_mut().canvas_form.height = 300;
    h.run_steps(1);
    h.get_by_label("change the canvas size").click();
    h.run_steps(4);
    assert_eq!(size(&h), (400, 300), "{}", said(&h));
    let paper = h
        .query_all_by_label_contains("use #")
        .next()
        .map(|n| n.accesskit_node().label().unwrap_or_default());
    assert!(paper.is_some());
    h.get_by_label(&paper.unwrap()).click();
    h.run_steps(3);
    assert!(h.state().scene.bundle.doc.canvas.background.is_some(), "{}", said(&h));
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn shapes_snap_to_the_grid_guides_and_other_shapes_and_guides_come_out_of_the_rulers() {
    let path = sample("snap");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    activate(&mut h, "shapes");
    chord(&mut h, Modifiers::COMMAND, Key::Quote);
    chord(&mut h, Modifiers::COMMAND | Modifiers::SHIFT, Key::Quote);
    let g = h.state().scene.bundle.doc.guides.clone();
    assert!(
        g.show_grid && g.snap && g.grid.is_some(),
        "cmd ' shows the grid and shift cmd ' snaps: {g:?}"
    );
    h.state_mut()
        .timeline_apply(vec![klad_core::ops::Op::Grid(klad_core::guides::GridEdit {
            every: Some([32.0, 32.0]),
            ..Default::default()
        })])
        .unwrap();
    h.key_press(Key::U);
    h.run_steps(2);
    let (a, b) = (canvas_at(&h, 225.0, 255.0), canvas_at(&h, 289.5, 319.0));
    drag(&mut h, a, b);
    h.run_steps(3);
    let made = nodes(&h, "shapes").last().cloned().unwrap();
    match made.shape {
        klad_core::doc::Shape::Rect { x, y, w, h: hh, .. } => {
            assert_eq!(
                (x, y, w, hh),
                (224.0, 256.0, 64.0, 64.0),
                "the rectangle's corners snapped to the grid"
            )
        }
        other => panic!("{other:?}"),
    }

    hold(&mut h, Modifiers::COMMAND);
    let (a, b) = (canvas_at(&h, 353.0, 33.0), canvas_at(&h, 410.0, 70.0));
    drag(&mut h, a, b);
    hold(&mut h, Modifiers::NONE);
    let loose = nodes(&h, "shapes").last().cloned().unwrap();
    assert!(
        matches!(loose.shape, klad_core::doc::Shape::Rect { x, .. } if (x - 353.0).abs() < 1.0),
        "cmd leaves the snap: {:?}",
        loose.shape
    );

    h.state_mut().invoke(Cmd::Rulers).unwrap();
    h.run_steps(2);
    let rect = h.state().canvas_rect;
    let (from, to) = (
        egui::pos2(rect.center().x, rect.top() + 6.0),
        canvas_at(&h, 300.0, 150.0),
    );
    drag(&mut h, from, to);
    h.run_steps(2);
    let ys = h.state().scene.bundle.doc.guides.y.clone();
    assert_eq!(ys.len(), 1, "a guide dragged out of the top ruler: {}", said(&h));
    assert!((ys[0] - 150.0).abs() <= 1.0, "{ys:?}");
    h.state_mut().tool = Tool::Select;
    h.run_steps(1);
    let on = canvas_at(&h, 40.0, ys[0]);
    let rect = h.state().canvas_rect;
    let back = egui::pos2(rect.center().x, rect.top() + 6.0);
    drag(&mut h, on, back);
    h.run_steps(2);
    assert!(
        h.state().scene.bundle.doc.guides.y.is_empty(),
        "dragged back onto the ruler, it goes: {}",
        said(&h)
    );

    h.state_mut()
        .timeline_apply(vec![klad_core::ops::Op::Grid(klad_core::guides::GridEdit {
            show: Some(false),
            ..Default::default()
        })])
        .unwrap();
    h.run_steps(1);
    let first = nodes(&h, "shapes")[0].id.clone();
    h.state_mut().selection = Some(first.clone());
    h.run_steps(2);
    let start = canvas_at(&h, 80.0, 60.0);
    let (_, corners) = h.state().handles().unwrap();
    let left = corners.iter().map(|c| c.x).fold(f32::MAX, f32::min);
    let other = nodes(&h, "shapes")[1].id.clone();
    let edge = klad_render::overlay::node_world_box(&h.state().scene.bundle.doc, &other)
        .unwrap()
        .unwrap()[0];
    let end = canvas_at(&h, 80.0 + (edge - left) - 2.0, 60.0);
    drag(&mut h, start, end);
    h.run_steps(3);
    let (_, corners) = h.state().handles().unwrap();
    let now = corners.iter().map(|c| c.x).fold(f32::MAX, f32::min);
    assert!(
        (now - edge).abs() < 0.25,
        "the moved shape's left edge snapped to the other shape's at {edge}: {now}"
    );
    let _ = std::fs::remove_dir_all(&path);
}

fn theme_icon_plus() -> &'static str {
    crate::theme::icon::PLUS
}

fn last_points(h: &Harness<'static, Editor>) -> Vec<(f32, f32)> {
    h.state()
        .scene
        .active_marks()
        .iter()
        .rev()
        .find_map(Mark::as_stroke)
        .map(|s| s.samples.iter().map(|p| p.pos()).collect())
        .unwrap_or_default()
}

#[test]
fn the_brush_follows_perspective_and_a_ruler_and_the_guides_move_in_their_window() {
    use klad_core::assist::Assist;
    use klad_core::guides::AssistEdit;
    let path = sample("assists");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    activate(&mut h, "colour");
    h.state_mut()
        .timeline_apply(vec![klad_core::ops::Op::Assists(AssistEdit {
            add: vec![Assist::Perspective {
                points: vec![[-500.0, 100.0], [900.0, 100.0]],
            }],
            on: Some(true),
            ..Default::default()
        })])
        .unwrap();
    h.run_steps(2);
    assert_eq!(h.state().is_on(Cmd::FollowGuides), Some(true));
    let (a, b) = (canvas_at(&h, 300.0, 300.0), canvas_at(&h, 146.0, 256.0));
    drag(&mut h, a, b);
    h.run_steps(2);
    let points = last_points(&h);
    assert!(points.len() > 3, "{}", said(&h));
    points.iter().for_each(|(x, y)| {
        let cross = (x - 300.0) * (100.0 - 300.0) - (y - 300.0) * (-500.0 - 300.0);
        let off = cross.abs() / (800.0f32).hypot(200.0);
        assert!(
            off < 1.0,
            "the stroke runs to the left vanishing point: ({x}, {y}) is {off} px off"
        );
    });
    let (c, d) = (canvas_at(&h, 200.0, 300.0), canvas_at(&h, 204.0, 200.0));
    drag(&mut h, c, d);
    h.run_steps(2);
    let up = last_points(&h);
    assert!(
        up.iter().all(|(x, _)| (x - 200.0).abs() < 0.6),
        "uprights stay upright: {up:?}"
    );
    h.state_mut().invoke(Cmd::FollowGuides).unwrap();
    h.run_steps(2);
    assert_eq!(h.state().is_on(Cmd::FollowGuides), Some(false));
    let (e, f) = (canvas_at(&h, 400.0, 300.0), canvas_at(&h, 440.0, 220.0));
    drag(&mut h, e, f);
    h.run_steps(2);
    let free = last_points(&h);
    assert!(
        near(*free.last().unwrap(), (440.0, 220.0)),
        "with following off the stroke is free: {free:?}"
    );
    h.state_mut()
        .timeline_apply(vec![klad_core::ops::Op::Assists(AssistEdit {
            clear: true,
            add: vec![Assist::Ruler {
                from: [100.0, 350.0],
                to: [400.0, 350.0],
            }],
            on: Some(true),
            ..Default::default()
        })])
        .unwrap();
    h.run_steps(2);
    let (g, k) = (canvas_at(&h, 110.0, 356.0), canvas_at(&h, 300.0, 362.0));
    drag(&mut h, g, k);
    h.run_steps(2);
    let along = last_points(&h);
    assert!(
        along.iter().all(|(_, y)| (y - 350.0).abs() < 0.6),
        "a stroke near the ruler runs on it: {along:?}"
    );
    h.state_mut().invoke(Cmd::AssistsWindow).unwrap();
    h.run_steps(3);
    assert!(shown(&h, "2-point perspective"), "the drawing guides window is open");
    let marks = h.state().scene.active_marks().len();
    let (m, n) = (canvas_at(&h, 400.0, 350.0), canvas_at(&h, 400.0, 300.0));
    drag(&mut h, m, n);
    h.run_steps(2);
    assert_eq!(
        h.state().scene.active_marks().len(),
        marks,
        "dragging a guide's point paints nothing"
    );
    match &h.state().scene.bundle.doc.guides.assists[0] {
        Assist::Ruler { to, .. } => assert!((to[1] - 300.0).abs() < 1.0, "the ruler's end moved: {to:?}"),
        other => panic!("{other:?}"),
    }
    h.state_mut().invoke(Cmd::Undo).unwrap();
    h.run_steps(2);
    match &h.state().scene.bundle.doc.guides.assists[0] {
        Assist::Ruler { to, .. } => assert!((to[1] - 350.0).abs() < 1.0, "moving a guide is one undo step: {to:?}"),
        other => panic!("{other:?}"),
    }
    h.state_mut().invoke(Cmd::AssistsWindow).unwrap();
    h.run_steps(2);
    assert!(!h.state().windows.assists, "the window is closed");
    h.key_press(Key::V);
    h.run_steps(2);
    let (m, n) = (canvas_at(&h, 400.0, 350.0), canvas_at(&h, 400.0, 320.0));
    drag(&mut h, m, n);
    h.run_steps(2);
    match &h.state().scene.bundle.doc.guides.assists[0] {
        Assist::Ruler { to, .. } => assert!(
            (to[1] - 320.0).abs() < 1.0,
            "with V a guide's point drags with the window closed: {to:?}"
        ),
        other => panic!("{other:?}"),
    }
}

#[test]
fn brushes_saved_to_yours_show_in_another_drawing_which_keeps_its_own_copy() {
    let root = std::env::temp_dir().join(format!("draw-ui-yours-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let lib = klad_render::library::Library::at(root.join("brushes"));
    let path = sample("yours-a");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    h.state_mut().yours = super::yours::Yours::at(Some(lib.clone()));
    let mut def = klad_core::brushes::library("ink").unwrap();
    def.name = "wet ink".into();
    def.spacing = 0.35;
    h.state().scene.bundle.embed_brush("wet-ink", &def).unwrap();
    h.state_mut().embedded.insert("wet-ink".into(), def.clone());
    h.state_mut().brushes.insert("wet-ink", def.clone());
    h.state_mut().save_to_yours("wet-ink").unwrap();
    assert!(said(&h).contains("your brushes"), "{}", said(&h));
    assert!(
        h.state_mut().save_to_yours("ink").is_err(),
        "klad's own brushes are in every drawing already"
    );
    assert_eq!(lib.list().len(), 1);

    let other = sample("yours-b");
    let mut g = harness(editor(&other), vec2(1440.0, 1000.0));
    g.state_mut().yours = super::yours::Yours::at(Some(lib.clone()));
    g.state_mut().tool = Tool::Brush;
    if g.state().is_on(Cmd::BrushPane) != Some(true) {
        g.state_mut().invoke(Cmd::BrushPane).unwrap();
    }
    g.run_steps(3);
    assert!(shown(&g, "your brushes"), "the brush tiles show your brushes");
    {
        use egui_kittest::kittest::{NodeT, Queryable};
        g.query_all_by_label("wet ink")
            .find(|n| n.accesskit_node().role() == egui::accesskit::Role::Button)
            .expect("a tile for the saved brush")
            .scroll_to_me();
        g.run_steps(4);
        g.query_all_by_label("wet ink")
            .find(|n| n.accesskit_node().role() == egui::accesskit::Role::Button)
            .expect("a tile for the saved brush")
            .click();
    }
    g.run_steps(3);
    assert_eq!(g.state().brush, "wet-ink", "{}", said(&g));
    assert_eq!(
        g.state().scene.bundle.brush("wet-ink").unwrap().unwrap().spacing,
        0.35,
        "picking it gives the drawing its own copy"
    );
    activate(&mut g, "colour");
    let (a, b) = (canvas_at(&g, 100.0, 100.0), canvas_at(&g, 200.0, 150.0));
    drag(&mut g, a, b);
    g.run_steps(2);
    let drawn = g
        .state()
        .scene
        .active_marks()
        .iter()
        .filter_map(Mark::as_stroke)
        .any(|s| s.brush == "wet-ink");
    assert!(drawn, "{}", said(&g));
    lib.forget("wet-ink").unwrap();
    let reopened = klad_core::bundle::Bundle::open(&other).unwrap();
    assert_eq!(
        reopened.brush("wet-ink").unwrap().unwrap().spacing,
        0.35,
        "the drawing renders where your brushes lack it"
    );

    h.state_mut().tool = Tool::Brush;
    if h.state().is_on(Cmd::BrushPane) != Some(true) {
        h.state_mut().invoke(Cmd::BrushPane).unwrap();
    }
    h.run_steps(3);
    {
        use egui_kittest::kittest::{NodeT, Queryable};
        let stale = |h: &Harness<'static, Editor>| {
            h.query_all_by_label("wet ink")
                .filter(|n| n.accesskit_node().role() == egui::accesskit::Role::Button)
                .count()
        };
        let last = stale(&h) - 1;
        h.query_all_by_label("wet ink")
            .filter(|n| n.accesskit_node().role() == egui::accesskit::Role::Button)
            .nth(last)
            .expect("the first drawing still lists the brush it saved")
            .scroll_to_me();
        h.run_steps(4);
        let last = stale(&h) - 1;
        h.query_all_by_label("wet ink")
            .filter(|n| n.accesskit_node().role() == egui::accesskit::Role::Button)
            .nth(last)
            .expect("the first drawing still lists the brush it saved")
            .click();
    }
    h.run_steps(3);
    assert!(
        !said(&h).contains("no brush"),
        "a brush forgotten in another drawing is no error here: {}",
        said(&h)
    );
    assert_eq!(h.state().brush, "wet-ink", "this drawing's own copy is picked");
    assert!(
        h.state().yours.saved.is_empty(),
        "and its list of your brushes is read again"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_export_window_writes_jpeg_webp_tiff_pdf_and_video_and_the_selection_alone() {
    use super::export::{Picture, What};
    let path = sample("formats");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    activate(&mut h, "colour");
    let (a, b) = (canvas_at(&h, 100.0, 100.0), canvas_at(&h, 200.0, 150.0));
    drag(&mut h, a, b);
    h.run_steps(2);
    let (cel, id) = {
        let ed = h.state();
        let cel = ed.scene.active_cel().unwrap().to_string();
        let id = ed
            .scene
            .active_marks()
            .iter()
            .rev()
            .find_map(Mark::as_stroke)
            .unwrap()
            .id;
        (cel, id)
    };
    h.state_mut().picked = Some(Picked {
        cel: cel.clone(),
        ids: vec![id],
        cut: None,
        more: Vec::new(),
    });
    h.state_mut().invoke(Cmd::Export).unwrap();
    h.run_steps(3);
    assert_eq!(
        h.state().export.only,
        [format!("{cel}:s{id}")],
        "a stroke selection exports alone"
    );
    assert!(shown(&h, "only the selection"));
    {
        use egui_kittest::kittest::Queryable;
        h.get_by_label("JPEG").click();
    }
    h.run_steps(2);
    assert_eq!(h.state().export.picture, Picture::Jpeg);
    assert!(shown(&h, "quality"), "a JPEG takes a quality");
    let root = h.state().scene.bundle.root.clone();
    let args = super::export::args(&root, &root, 0, &h.state().export).join(" ");
    assert!(
        args.contains("--format jpeg") && args.contains(&format!("--only {cel}:s{id}")),
        "{args}"
    );
    h.state_mut().invoke(Cmd::ExportPdf).unwrap();
    h.run_steps(8);
    assert_eq!(h.state().export.what, What::Pdf);
    {
        use egui_kittest::kittest::Queryable;
        h.get_by_label("every frame").click();
    }
    h.run_steps(2);
    assert!(
        shown(&h, "pixels per inch"),
        "the PDF on the drawing's size takes a resolution"
    );
    let args = super::export::args(&root, &root, 0, &h.state().export).join(" ");
    assert!(args.contains("--format pdf --pages all"), "{args}");
    h.state_mut().invoke(Cmd::ExportVideo).unwrap();
    h.run_steps(3);
    let form = h.state().export.clone();
    assert_eq!((form.what, form.quality), (What::Video, 80.0));
    let args = super::export::args(&root, &root, 0, &form).join(" ");
    assert!(args.contains("--format mp4 --animated all"), "{args}");
    h.state_mut().invoke(Cmd::ExportGif).unwrap();
    h.run_steps(3);
    {
        use egui_kittest::kittest::Queryable;
        h.get_by_label("WebP").click();
    }
    h.run_steps(2);
    let args = super::export::args(&root, &root, 0, &h.state().export).join(" ");
    assert!(args.contains("--format webp --animated"), "{args}");
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn frames_come_in_from_a_sheet_cut_in_the_import_frames_window_with_a_preview() {
    use super::framesin::Way;
    let path = sample("frames-in");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    let sheet = path.parent().unwrap().join("frames-in-sheet.png");
    let picture = klad_render::pixelise::Picture {
        width: 20,
        height: 8,
        rgba: (0..20 * 8)
            .flat_map(|i| {
                if (i % 20) / 5 == 2 {
                    [0, 0, 0, 0]
                } else {
                    [200, 40, 40, 255]
                }
            })
            .collect(),
    };
    std::fs::write(&sheet, klad_render::clipboard::png(&picture)).unwrap();
    h.state_mut().invoke(Cmd::ImportFrames).unwrap();
    h.run_steps(3);
    assert!(h.state().windows.frames);
    h.state_mut().frames_in.path = sheet.display().to_string();
    h.state_mut().frames_in.way = Way::Grid;
    h.state_mut().frames_in.grid = [4, 1];
    h.run_steps(4);
    assert!(
        shown(&h, "3 frames of 5 x 8 px"),
        "the preview counts the cells that are not empty"
    );
    let root = h.state().scene.bundle.root.clone();
    let args = h.state().frames_in.args(&root).join(" ");
    assert_eq!(
        args,
        format!("import {} --grid 4x1 --to {}", sheet.display(), root.display())
    );
    h.state_mut().frames_in.way = Way::Files;
    assert!(
        h.state_mut().import_frames_for_test().is_err(),
        "a picture per frame wants a folder"
    );
    let _ = std::fs::remove_file(&sheet);
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn strokes_copied_in_one_drawing_paste_into_another_with_their_brush() {
    let (pa, pb) = (sample("across-a"), sample("across-b"));
    let mut a = harness(editor(&pa), vec2(1440.0, 1000.0));
    let mut b = harness(editor(&pb), vec2(1440.0, 1000.0));
    let mut def = klad_core::brushes::library("ink").unwrap();
    def.name = "wet ink".into();
    def.spacing = 0.3;
    a.state().scene.bundle.embed_brush("wet-ink", &def).unwrap();
    a.state_mut().embedded.insert("wet-ink".into(), def.clone());
    a.state_mut().brushes.insert("wet-ink", def.clone());
    activate(&mut a, "colour");
    a.state_mut().pick("wet-ink");
    let (p, q) = (canvas_at(&a, 100.0, 100.0), canvas_at(&a, 220.0, 160.0));
    drag(&mut a, p, q);
    a.run_steps(2);
    let (cel, id) = {
        let ed = a.state();
        let s = ed
            .scene
            .active_marks()
            .iter()
            .rev()
            .find_map(Mark::as_stroke)
            .unwrap()
            .clone();
        assert_eq!(s.brush, "wet-ink", "{}", said(&a));
        (ed.scene.active_cel().unwrap().to_string(), s.id)
    };
    a.state_mut().picked = Some(Picked {
        cel,
        ids: vec![id],
        cut: None,
        more: Vec::new(),
    });
    a.state_mut().tool = Tool::Strokes;
    a.state_mut().invoke(Cmd::Copy).unwrap();
    a.run_steps(2);
    a.state().hand_over(b.state_mut());
    activate(&mut b, "colour");
    let before = b.state().scene.active_marks().len();
    b.state_mut().invoke(Cmd::Paste).unwrap();
    b.run_steps(2);
    let after = b.state().scene.active_marks().to_vec();
    assert_eq!(after.len(), before + 1, "{}", said(&b));
    assert!(after.iter().filter_map(Mark::as_stroke).any(|s| s.brush == "wet-ink"));
    assert_eq!(
        b.state().scene.bundle.brush("wet-ink").unwrap().unwrap().spacing,
        0.3,
        "the other drawing gets its own copy of the brush"
    );
    let _ = std::fs::remove_dir_all(&pa);
    let _ = std::fs::remove_dir_all(&pb);
}

#[test]
fn a_drawing_is_saved_as_a_copy_renamed_duplicated_and_reverted_to_how_it_opened() {
    let path = sample("files");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    let home = path.parent().unwrap().to_path_buf();
    let stem = path.file_stem().unwrap().to_string_lossy().to_string();
    let copy = h.state_mut().save_copy("kept copy", false).unwrap();
    assert_eq!(copy, home.join("kept copy.klad"));
    assert!(
        copy.join("doc.json").is_file() && h.state().wants.is_none(),
        "staying here: {}",
        said(&h)
    );
    let second = h.state_mut().save_copy("go copy", true).unwrap();
    assert!(
        matches!(&h.state().wants, Some(super::Want::Path(p)) if *p == second),
        "going on in the copy"
    );
    h.state_mut().wants = None;
    assert!(
        h.state_mut().save_copy("kept copy", false).is_err(),
        "never over another drawing"
    );
    let duplicate = h.state_mut().duplicate_drawing().unwrap();
    assert_eq!(duplicate, home.join(format!("{stem} copy.klad")));
    activate(&mut h, "colour");
    let marks = h.state().scene.active_marks().len();
    let (p, q) = (canvas_at(&h, 100.0, 100.0), canvas_at(&h, 220.0, 160.0));
    drag(&mut h, p, q);
    h.run_steps(2);
    assert_eq!(h.state().scene.active_marks().len(), marks + 1);
    h.state_mut().invoke(Cmd::RevertDrawing).unwrap();
    h.run_steps(3);
    assert_eq!(
        h.state().scene.active_marks().len(),
        marks,
        "back to how it was opened: {}",
        said(&h)
    );
    h.state_mut().invoke(Cmd::Undo).unwrap();
    h.run_steps(3);
    assert_eq!(
        h.state().scene.active_marks().len(),
        marks + 1,
        "the revert is one undo step"
    );
    let renamed = h.state_mut().rename_drawing("renamed here").unwrap();
    assert_eq!(renamed, home.join("renamed here.klad"));
    assert_eq!(
        h.state().scene.bundle.root,
        renamed,
        "klad goes on in the renamed drawing"
    );
    assert!(!path.exists());
    h.state_mut().invoke(Cmd::SaveCopy).unwrap();
    h.run_steps(3);
    assert!(shown(&h, "Save and go on in the copy"), "shift cmd S asks for a name");
    [copy, second, duplicate, renamed].iter().for_each(|p| {
        let _ = std::fs::remove_dir_all(p);
    });
}

#[test]
fn the_weight_brush_adds_takes_away_with_strength_and_reads_the_weight_under_the_pointer() {
    use klad_core::rig::Mode as W;
    let path = sample("weights");
    {
        let mut bundle = klad_core::bundle::Bundle::open(&path).unwrap();
        let colour = bundle.doc.find_layer("colour").unwrap().id.clone();
        let ops = parse_lines(&format!(
            r#"{{"op":"add","layer":{{"id":"bones","kind":"bones","name":"bones"}}}}
{{"op":"bone","layer":"bones","add":"thigh","head":[150,60],"tail":[150,180]}}
{{"op":"bone","layer":"bones","add":"shin","parent":"thigh","head":[150,180],"tail":[150,330]}}
{{"op":"attach","layer":"{colour}","skeleton":"bones"}}"#
        ))
        .unwrap();
        bundle.apply(&ops).unwrap();
    }
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    h.state_mut().tool = Tool::Bone;
    h.state_mut().bones.mode = super::bones::Mode::Weights;
    h.state_mut().bones.selected = Some("shin".into());
    h.run_steps(3);
    assert!(
        shown(&h, "weight") && shown(&h, theme_icon_plus()),
        "the bar has the modes and the weight"
    );
    h.key_press_modifiers(Modifiers::SHIFT, Key::Num2);
    h.run_steps(2);
    assert_eq!(h.state().bones.wmode, W::Add, "shift 2 adds weight");
    h.state_mut().bones.flow = 0.5;
    h.state_mut().bones.strength = 1.0;
    let (a, b) = (canvas_at(&h, 200.0, 250.0), canvas_at(&h, 204.0, 252.0));
    drag(&mut h, a, b);
    h.run_steps(2);
    let skeleton = h.state().scene.bundle.doc.find_layer("bones").unwrap().id.clone();
    let painted = |h: &Harness<'static, Editor>| {
        h.state()
            .scene
            .bundle
            .read_weights(&skeleton)
            .unwrap()
            .value_at("shin", [200.0, 250.0])
    };
    let added = painted(&h);
    assert!(
        added > 0.3 && added < 0.95,
        "half strength adds part of the weight: {added}"
    );
    h.state_mut().invoke(Cmd::WeightRemove).unwrap();
    h.state_mut().bones.flow = 1.0;
    h.state_mut().bones.strength = 0.2;
    drag(&mut h, a, b);
    h.run_steps(2);
    let removed = painted(&h);
    assert!(
        removed < added - 0.1,
        "remove takes weight away: {added} then {removed}"
    );
    h.hover_at(a);
    h.run_steps(2);
    let (under, used) = h.state_mut().weight_under(vec2(200.0, 250.0)).unwrap();
    assert!(
        (under - removed).abs() < 0.02 && used > 0.0,
        "the readout has the weight under the pointer"
    );
    h.state_mut().invoke(Cmd::PickWeight).unwrap();
    assert!(
        (h.state().bones.strength - removed).abs() < 0.02,
        "alt I picks it up into the weight: {}",
        said(&h)
    );
    h.key_press_modifiers(Modifiers::SHIFT, Key::Num1);
    h.run_steps(2);
    assert_eq!(h.state().bones.wmode, W::Paint);
    h.state_mut().tool = Tool::Brush;
    assert!(
        h.state().can_run(Cmd::WeightAdd).is_some(),
        "the weight keys are for the bone tool's weights"
    );
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn a_new_layer_made_inside_a_hidden_group_goes_above_the_group_where_it_shows() {
    let path = sample("hidden-group");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    activate(&mut h, "colour");
    h.state_mut().invoke(Cmd::GroupLayer).unwrap();
    h.run_steps(2);
    let group = {
        let doc = &h.state().scene.bundle.doc;
        doc.layers
            .iter()
            .find(|l| l.children().iter().any(|c| c.name == "colour"))
            .unwrap()
            .id
            .clone()
    };
    h.state_mut()
        .timeline_apply(vec![klad_core::ops::Op::Set {
            id: group.clone(),
            props: serde_json::from_str(r#"{"visible":false}"#).unwrap(),
        }])
        .unwrap();
    activate(&mut h, "colour");
    h.state_mut().invoke(Cmd::NewPaintLayer).unwrap();
    h.run_steps(2);
    let doc = &h.state().scene.bundle.doc;
    let made = h.state().scene.active_layer().unwrap().id.clone();
    assert!(
        doc.layers.iter().any(|l| l.id == made),
        "the new layer is not inside the hidden group"
    );
    let at = |id: &str| doc.layers.iter().position(|l| l.id == id).unwrap();
    assert_eq!(at(&made), at(&group) + 1, "it sits just above the group");
    let (a, b) = (canvas_at(&h, 100.0, 100.0), canvas_at(&h, 200.0, 150.0));
    let before = h.state().scene.total_marks();
    drag(&mut h, a, b);
    h.run_steps(2);
    assert!(
        h.state().scene.total_marks() > before,
        "and a stroke lands on it: {}",
        said(&h)
    );
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn stepping_past_the_last_layer_and_pasting_a_look_onto_its_own_shape_say_why() {
    let path = sample("edges-said");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    activate(&mut h, "shapes");
    let ids = add_rects(&mut h);
    h.state_mut().tool = Tool::Select;
    h.state_mut().selection = Some(ids[0].clone());
    h.run_steps(2);
    h.state_mut().invoke(Cmd::CopyLook).unwrap();
    h.state_mut().invoke(Cmd::PasteLook).unwrap();
    h.run_steps(1);
    assert!(said(&h).contains("the look came from"), "{}", said(&h));
    let layers = h.state().scene.bundle.doc.layers.len();
    (0..=layers).for_each(|_| h.state_mut().invoke(Cmd::LayerAbove).unwrap());
    h.run_steps(1);
    assert!(said(&h).contains("this is the top layer"), "{}", said(&h));
    (0..=layers * 3).for_each(|_| h.state_mut().invoke(Cmd::LayerBelow).unwrap());
    h.run_steps(1);
    assert!(said(&h).contains("this is the bottom layer"), "{}", said(&h));
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn select_all_frames_with_no_layer_left_says_why() {
    let path = sample("frames-no-layer");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    let layers = h.state().scene.bundle.doc.layers.len();
    (0..layers).for_each(|_| h.state_mut().invoke(Cmd::DeleteLayer).unwrap());
    h.run_steps(2);
    h.state_mut().invoke(Cmd::SelectAllFrames).unwrap();
    h.run_steps(1);
    assert!(h.state().timeline.span.is_none());
    assert!(said(&h).contains("no frames to select"), "{}", said(&h));
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn select_all_leaves_out_the_eraser_riding_on_a_stroke_and_delete_takes_both() {
    let path = sample("select-all-eraser");
    let mut h = harness(editor(&path), vec2(1440.0, 900.0));
    activate(&mut h, "lines");
    h.state_mut()
        .apply(
            parse_lines(r#"{"op":"stroke","layer":"l6","path":"M120 60 L180 140","size":24,"brush":"eraser"}"#)
                .unwrap(),
        )
        .unwrap();
    h.run_steps(3);
    let marks = h.state().scene.active_marks().to_vec();
    let eraser = marks.last().and_then(Mark::as_stroke).unwrap().clone();
    assert!(!eraser.on.is_empty(), "the eraser rides on the line it crosses");
    h.state_mut().tool = Tool::Strokes;
    chord(&mut h, Modifiers::COMMAND, Key::A);
    let picked = picked_ids(&h);
    assert!(
        !picked.contains(&eraser.id),
        "select all leaves the riding eraser out: {picked:?}"
    );
    assert_eq!(picked.len(), marks.len() - 1, "and picks every stroke: {picked:?}");
    chord(&mut h, Modifiers::NONE, Key::Backspace);
    assert!(!said(&h).contains("does not exist"), "{}", said(&h));
    assert!(
        h.state().scene.active_marks().is_empty(),
        "delete takes the strokes and the eraser on them: {}",
        said(&h)
    );
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn duplicate_has_its_own_icon_beside_copy() {
    assert_ne!(Cmd::Copy.spec().icon, Cmd::DuplicateInPlace.spec().icon);
    assert_eq!(Cmd::DuplicateInPlace.spec().icon, Cmd::DuplicateLayer.spec().icon);
    assert_eq!(Cmd::DuplicateInPlace.spec().icon, crate::theme::icon::DUPLICATE);
}

#[test]
fn the_brush_menu_and_the_numbers_menu_stay_open_while_they_are_used() {
    use egui::accesskit::Role;
    use egui_kittest::kittest::Queryable;
    let path = sample("held-menus");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    activate(&mut h, "lines");
    h.state_mut().tool = Tool::Brush;
    h.run_steps(2);
    let more: &'static str = Box::leak(format!("{} more", crate::theme::icon::MORE).into_boxed_str());
    h.get_by_role_and_label(Role::Button, more).click();
    h.run_steps(2);
    h.get_by_label("rectangle").click();
    h.run_steps(2);
    assert_eq!(
        h.state().brush_figure,
        super::straight::Figure::Rect,
        "draws: rectangle"
    );
    assert!(
        h.query_all_by_label_contains("lightest size").next().is_some(),
        "the menu stays open after the pick"
    );
    h.get_by_label("free").click();
    h.run_steps(2);
    assert_eq!(h.state().brush_figure, super::straight::Figure::Free);
    h.key_press(Key::Escape);
    h.run_steps(2);

    activate(&mut h, "shapes");
    h.state_mut().tool = Tool::Select;
    let inside = canvas_at(&h, 120.0, 30.0);
    click(&mut h, inside);
    h.run_steps(3);
    assert!(h.state().selection.is_some(), "{}", said(&h));
    let numbers: &'static str = Box::leak(format!("{} numbers", crate::theme::icon::TYPED).into_boxed_str());
    h.get_by_role_and_label(Role::Button, numbers).click();
    h.run_steps(2);
    h.get_by_role_and_label(Role::TextInput, "x").click();
    h.run_steps(2);
    assert!(
        h.query_by_role_and_label(Role::TextInput, "x").is_some(),
        "a click in a field leaves the numbers open"
    );
    h.key_press(Key::End);
    (0..6).for_each(|_| h.key_press(Key::Backspace));
    h.get_by_role_and_label(Role::TextInput, "x").type_text("100");
    h.run_steps(1);
    h.key_press(Key::Enter);
    h.run_steps(4);
    let corners = h.state().handles().map(|(_, c)| c).unwrap();
    assert!(
        (corners[0].x - 100.0).abs() < 1.0,
        "x typed in the numbers moves the shape's left edge: {corners:?} {}",
        said(&h)
    );
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn with_all_layers_the_wand_and_select_all_reach_every_layer() {
    use super::marquee::Marquee;
    let path = sample("all-layers");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    activate(&mut h, "colour");
    let colour = layer_named(&h, "colour").id.clone();
    h.state_mut()
        .apply(
            parse_lines(&format!(
                r#"{{"op":"stroke","layer":"{colour}","path":"M380 330 L460 330","size":10}}"#
            ))
            .unwrap(),
        )
        .unwrap();
    h.run_steps(3);
    h.key_press(Key::S);
    h.run_steps(2);
    h.state_mut().settings.pick_all_layers = true;
    h.state_mut().marquee = Marquee::Wand;
    let on_lines = canvas_at(&h, 170.0, 120.0);
    click(&mut h, on_lines);
    assert_eq!(
        h.state().scene.active_layer().unwrap().name,
        "lines",
        "the wand goes to the layer painted under the click: {}",
        said(&h)
    );
    assert!(!picked_ids(&h).is_empty(), "and selects there: {}", said(&h));

    chord(&mut h, Modifiers::NONE, Key::Escape);
    activate(&mut h, "shapes");
    h.state_mut().tool = Tool::Select;
    h.run_steps(2);
    chord(&mut h, Modifiers::COMMAND, Key::A);
    let picked = h.state().picked.clone().expect("cmd A picks strokes");
    let layers: Vec<String> = std::iter::once(picked.cel.clone())
        .chain(picked.more.iter().map(|p| p.cel.clone()))
        .collect();
    let cel_of = |h: &Harness<'static, Editor>, name: &str| layer_named(h, name).cels()[0].id.clone();
    assert!(
        layers.contains(&cel_of(&h, "lines")) && layers.contains(&cel_of(&h, "colour")),
        "cmd A with all layers takes the strokes of every paint layer: {layers:?} {}",
        said(&h)
    );

    chord(&mut h, Modifiers::NONE, Key::Escape);
    h.state_mut().settings.pick_all_layers = false;
    activate(&mut h, "lines");
    chord(&mut h, Modifiers::COMMAND, Key::A);
    let picked = h.state().picked.clone().unwrap();
    assert!(
        picked.more.is_empty(),
        "with this layer, only the active layer's strokes"
    );
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn the_crop_tool_is_in_the_strip_with_handles_a_box_that_moves_and_its_bar() {
    use egui::accesskit::Role;
    use egui_kittest::kittest::Queryable;
    let path = sample("crop-tool");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    let size = |h: &Harness<'static, Editor>| h.state().scene.size();
    let span = |h: &Harness<'static, Editor>| {
        let c = h.state().cropping.unwrap();
        (c.from, c.to)
    };
    h.get_by_role_and_label(Role::Button, Cmd::CropTool.spec().icon).click();
    h.run_steps(2);
    assert!(h.state().cropping.is_some(), "the strip's crop button starts the crop");
    assert_eq!(h.state().is_on(Cmd::CropTool), Some(true));
    assert_eq!(
        h.state().is_on(Cmd::Brush),
        Some(false),
        "only the crop is lit in the strip"
    );
    assert!(
        h.query_by_role_and_label(Role::SpinButton, "w").is_some(),
        "the bar has the crop's size"
    );

    let (a, b) = (canvas_at(&h, 512.0, 192.0), canvas_at(&h, 412.0, 192.0));
    drag(&mut h, a, b);
    let (lo, hi) = span(&h);
    assert!(
        (hi.x - 412.0).abs() < 2.0 && lo.x.abs() < 0.5 && (hi.y - 384.0).abs() < 0.5,
        "the right handle moves the right edge only: {lo:?} {hi:?}"
    );

    hold(&mut h, Modifiers::SHIFT);
    let (a, b) = (canvas_at(&h, 0.0, 0.0), canvas_at(&h, 100.0, 10.0));
    drag(&mut h, a, b);
    hold(&mut h, Modifiers::NONE);
    let (lo, hi) = span(&h);
    let ratio = (hi.x - lo.x) / (hi.y - lo.y);
    assert!(
        (ratio - 412.0 / 384.0).abs() < 0.02 && lo.x > 5.0 && lo.y > 5.0,
        "shift on a corner keeps the box's shape: {lo:?} {hi:?}"
    );

    let middle = (lo + hi) / 2.0;
    let (a, b) = (
        canvas_at(&h, middle.x, middle.y),
        canvas_at(&h, middle.x - 4.0, middle.y),
    );
    drag(&mut h, a, b);
    let (moved, _) = span(&h);
    assert!(
        (moved.x - (lo.x - 4.0)).abs() < 1.5 && (moved.y - lo.y).abs() < 1.0,
        "a drag inside moves the box: {moved:?} from {lo:?}"
    );

    let (_, hi) = span(&h);
    let expect = ((hi.x - moved.x).round() as i32, (hi.y - moved.y).round() as i32);
    h.get_by_label("crop to the box").click();
    h.run_steps(4);
    let (w, hh) = size(&h);
    assert!(
        (w as i32 - expect.0).abs() <= 2 && (hh as i32 - expect.1).abs() <= 2,
        "the bar's crop button keeps the box: {w}x{hh}, the box was {expect:?} {}",
        said(&h)
    );
    assert!(h.state().cropping.is_none());
    chord(&mut h, Modifiers::COMMAND, Key::Z);
    h.run_steps(4);
    assert_eq!(size(&h), (512, 384), "cmd Z takes the crop back");

    let cel = layer_named(&h, "lines").cels()[0].id.clone();
    let bounds = |h: &Harness<'static, Editor>| h.state().scene.marks_of(&cel)[0].bounds().unwrap();
    let first = bounds(&h);
    h.key_press(Key::C);
    h.run_steps(2);
    let (a, b) = (canvas_at(&h, 512.0, 192.0), canvas_at(&h, 600.0, 192.0));
    drag(&mut h, a, b);
    let (a, b) = (canvas_at(&h, 0.0, 192.0), canvas_at(&h, -50.0, 192.0));
    drag(&mut h, a, b);
    h.key_press(Key::Enter);
    h.run_steps(4);
    let (w, hh) = size(&h);
    assert!(
        (w as i32 - 650).abs() <= 2 && hh == 384,
        "past the edges the crop grows the canvas, as Canvas size does: {w}x{hh} {}",
        said(&h)
    );
    let moved = bounds(&h);
    assert!(
        (moved[0] - first[0] - 50.0).abs() < 1.5,
        "and the drawing moves with the left edge: {moved:?} from {first:?}"
    );
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn the_connect_window_puts_klad_in_an_agent_apps_settings_and_takes_it_out() {
    use egui_kittest::kittest::Queryable;
    let path = sample("connect");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    let home = std::env::temp_dir().join(format!("draw-ui-connect-home-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    let claude = home.join("Library/Application Support/Claude");
    std::fs::create_dir_all(&claude).unwrap();
    let program = home.join("klad");
    std::fs::write(&program, "").unwrap();
    h.state_mut().connect.places = Some(klad_mcp::Places {
        home: home.clone(),
        root: home.join("computer"),
        os: klad_mcp::Os::Mac,
    });
    h.state_mut().connect.program = program.clone();
    h.state_mut().invoke(Cmd::ConnectAgent).unwrap();
    h.run_steps(3);
    assert_eq!(h.state().is_on(Cmd::ConnectAgent), Some(true));
    assert!(
        h.query_by_label("Claude Desktop").is_some(),
        "the app on this computer has a card"
    );
    assert!(
        h.query_by_label_contains("Also works with Codex").is_some(),
        "the apps that are not here are named in one line"
    );
    assert!(h.query_by_label("Codex").is_none(), "and get no card of their own");
    h.get_by_label("Connect").click();
    h.run_steps(3);
    let config = std::fs::read_to_string(claude.join("claude_desktop_config.json")).unwrap();
    let written: serde_json::Value = serde_json::from_str(&config).unwrap();
    assert_eq!(
        written["mcpServers"]["klad"]["command"].as_str(),
        Some(program.display().to_string().as_str()),
        "{config}"
    );
    assert!(
        h.query_by_label_contains("Connected. Quit Claude Desktop").is_some()
            && h.query_by_label_contains("Then ask Claude Desktop for a drawing")
                .is_some(),
        "it says what to do next"
    );
    h.get_by_label_contains("Copy").click();
    h.run_steps(2);
    assert!(
        h.query_by_label_contains("Copied").is_some(),
        "the example request is copied"
    );
    h.get_by_label("Disconnect").click();
    h.run_steps(3);
    let config = std::fs::read_to_string(claude.join("claude_desktop_config.json")).unwrap();
    assert!(!config.contains("\"klad\""), "{config}");
    assert!(h.query_by_label("Connect").is_some(), "and it can connect again");
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&path);
}
