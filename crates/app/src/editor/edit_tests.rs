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
