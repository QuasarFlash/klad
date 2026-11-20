use egui::Vec2;
use serde::{Deserialize, Serialize};

pub const STRIP: f32 = 44.0;
pub const STATUS: f32 = 28.0;
pub const TITLE: f32 = 30.0;
const SHRINK_FIRST: [Pane; 8] = [
    Pane::Brush,
    Pane::Preview,
    Pane::History,
    Pane::Notes,
    Pane::Maps,
    Pane::Inspector,
    Pane::Tool,
    Pane::Layers,
];
const FOLD_FIRST: [Pane; 7] = [
    Pane::Brush,
    Pane::Preview,
    Pane::History,
    Pane::Notes,
    Pane::Maps,
    Pane::Inspector,
    Pane::Tool,
];
pub const DOCK_MIN: f32 = 260.0;
pub const DOCK_DEFAULT: f32 = 330.0;
pub const TIMELINE_MIN: f32 = 150.0;
pub const TIMELINE_DEFAULT: f32 = 330.0;
pub const TIMELINE_FOLDED: f32 = 64.0;
pub const CANVAS_MIN: Vec2 = Vec2::new(320.0, 200.0);
pub const LAYER_ROWS_KEPT: usize = 3;
pub const USABLE: f32 = 60.0;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fit {
    pub content: Vec<(Pane, f32)>,
    pub layers_least: f32,
    pub chosen: Option<Pane>,
}

impl Fit {
    fn content(&self, pane: Pane) -> f32 {
        self.content
            .iter()
            .find(|(p, _)| *p == pane)
            .map_or(f32::INFINITY, |(_, h)| *h)
    }

    fn floor(&self, pane: Pane) -> f32 {
        match pane {
            p if Some(p) == self.chosen => f32::INFINITY,
            Pane::Layers => pane.min_body().max(self.layers_least),
            _ => pane.min_body(),
        }
    }

    fn kept(&self, pane: Pane) -> f32 {
        match pane {
            p if Some(p) == self.chosen => f32::INFINITY,
            Pane::Layers => self.layers_least,
            _ => 0.0,
        }
    }

    fn held(&self, pane: Pane) -> f32 {
        match pane {
            p if Some(p) == self.chosen => pane.min_body(),
            Pane::Layers => self.layers_least,
            _ => 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Pane {
    Tool,
    Brush,
    Layers,
    Inspector,
    Notes,
    History,
    Preview,
    Maps,
}

impl Pane {
    pub fn title(self) -> &'static str {
        match self {
            Pane::Tool => "Colour",
            Pane::Brush => "Brush",
            Pane::Layers => "Layers",
            Pane::Inspector => "Layer settings",
            Pane::Notes => "Notes for the agent",
            Pane::History => "History",
            Pane::Preview => "Real size",
            Pane::Maps => "Light maps",
        }
    }

    pub fn min_body(self) -> f32 {
        match self {
            Pane::Tool => 230.0,
            Pane::Brush => 80.0,
            Pane::Layers => 130.0,
            Pane::Inspector => 110.0,
            Pane::Notes => 60.0,
            Pane::History => 60.0,
            Pane::Preview => 110.0,
            Pane::Maps => 280.0,
        }
    }

    fn default_body(self) -> f32 {
        match self {
            Pane::Tool => 260.0,
            Pane::Brush => 150.0,
            Pane::Layers => 240.0,
            Pane::Inspector => 220.0,
            Pane::Notes => 90.0,
            Pane::History => 160.0,
            Pane::Preview => 170.0,
            Pane::Maps => 300.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PaneState {
    pub pane: Pane,
    pub open: bool,
    pub collapsed: bool,
    pub body: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Layout {
    pub strip: bool,
    pub dock: bool,
    pub dock_width: f32,
    pub timeline: bool,
    pub timeline_height: f32,
    pub timeline_folded: bool,
    pub numbers: bool,
    pub panes: Vec<PaneState>,
    #[serde(skip)]
    pub hidden: bool,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            strip: true,
            dock: true,
            dock_width: DOCK_DEFAULT,
            timeline: true,
            timeline_height: TIMELINE_DEFAULT,
            timeline_folded: false,
            numbers: false,
            panes: [
                (Pane::Tool, true, false),
                (Pane::Preview, false, false),
                (Pane::Brush, true, false),
                (Pane::Layers, true, false),
                (Pane::Inspector, true, false),
                (Pane::Maps, false, false),
                (Pane::Notes, true, true),
                (Pane::History, true, true),
            ]
            .into_iter()
            .map(|(pane, open, collapsed)| PaneState {
                pane,
                open,
                collapsed,
                body: pane.default_body(),
            })
            .collect(),
            hidden: false,
        }
    }
}

impl Layout {
    pub fn repaired(mut self) -> Self {
        let known: Vec<Pane> = self.panes.iter().map(|p| p.pane).collect();
        self.panes.dedup_by_key(|p| p.pane);
        let order = Layout::default().panes;
        order
            .iter()
            .enumerate()
            .filter(|(_, p)| !known.contains(&p.pane))
            .for_each(|(i, p)| {
                let after = i
                    .checked_sub(1)
                    .and_then(|k| self.panes.iter().position(|q| q.pane == order[k].pane));
                match after {
                    Some(k) => self.panes.insert(k + 1, p.clone()),
                    None => self.panes.push(p.clone()),
                }
            });
        self.panes
            .iter_mut()
            .for_each(|p| p.body = p.body.max(p.pane.min_body()));
        self
    }

    pub fn shows_strip(&self) -> bool {
        self.strip && !self.hidden
    }

    pub fn shows_dock(&self) -> bool {
        self.dock && !self.hidden && self.panes.iter().any(|p| p.open)
    }

    pub fn shows_timeline(&self) -> bool {
        self.timeline && !self.hidden
    }

    pub fn pane(&self, pane: Pane) -> Option<&PaneState> {
        self.panes.iter().find(|p| p.pane == pane)
    }

    pub fn pane_mut(&mut self, pane: Pane) -> Option<&mut PaneState> {
        self.panes.iter_mut().find(|p| p.pane == pane)
    }

    pub fn is_open(&self, pane: Pane) -> bool {
        self.pane(pane).is_some_and(|p| p.open)
    }

    pub fn shows(&self, pane: Pane) -> bool {
        self.dock && !self.hidden && self.pane(pane).is_some_and(|p| p.open && !p.collapsed)
    }

    pub fn toggle(&mut self, pane: Pane) {
        let show = !self.shows(pane);
        if let Some(p) = self.pane_mut(pane) {
            p.open = show;
            if show {
                p.collapsed = false;
            }
        }
        if show {
            self.dock = true;
            self.hidden = false;
        }
    }

    fn needed(&self) -> f32 {
        self.panes
            .iter()
            .filter(|p| p.open)
            .map(|p| TITLE + if p.collapsed { 0.0 } else { p.pane.min_body() })
            .sum()
    }

    pub fn make_room(&mut self, keep: Pane, room: f32) -> Vec<Pane> {
        if room <= 0.0 {
            return Vec::new();
        }
        FOLD_FIRST
            .iter()
            .filter(|p| **p != keep)
            .fold(Vec::new(), |mut folded, pane| {
                let fits = self.needed() <= room;
                if let Some(p) = self.pane_mut(*pane).filter(|p| !fits && p.open && !p.collapsed) {
                    p.collapsed = true;
                    folded.push(*pane);
                }
                folded
            })
    }

    fn cramped(&self, available: f32, fit: &Fit) -> bool {
        self.shared(available, fit).iter().any(|(pane, size)| {
            *pane != Pane::Layers
                && Some(*pane) != fit.chosen
                && size.is_some_and(|size| {
                    let want = self.pane(*pane).map_or(0.0, |p| p.body).min(fit.content(*pane));
                    size + 0.5 < USABLE.min(want)
                })
        })
    }

    pub fn folded_for_room(&self, available: f32, fit: &Fit) -> Vec<Pane> {
        let mut trial = self.clone();
        FOLD_FIRST
            .iter()
            .filter(|p| Some(**p) != fit.chosen)
            .fold(Vec::new(), |mut folded, pane| {
                let cramped = trial.cramped(available, fit);
                if let Some(p) = trial.pane_mut(*pane).filter(|p| cramped && p.open && !p.collapsed) {
                    p.collapsed = true;
                    folded.push(*pane);
                }
                folded
            })
    }

    pub fn dock_range(window: Vec2) -> (f32, f32) {
        (DOCK_MIN, (window.x * 0.45).max(DOCK_MIN))
    }

    pub fn timeline_range(window: Vec2) -> (f32, f32) {
        (TIMELINE_MIN, (window.y * 0.6).max(TIMELINE_MIN))
    }

    pub fn clamp(&mut self, window: Vec2) {
        let (lo, hi) = Self::dock_range(window);
        self.dock_width = self.dock_width.clamp(lo, hi);
    }

    pub fn timeline_size(&self, window: Vec2, scale: f32, rows: f32) -> (f32, f32, f32) {
        let (lo, hi) = Self::timeline_range(window);
        let room = (window.y - STATUS - CANVAS_MIN.y).max(lo);
        let hi = hi.min(room).min(rows.max(lo));
        let height = (self.timeline_height / scale.max(1.0)).clamp(lo, hi);
        (height, lo, hi)
    }

    pub fn keep_timeline_height(&mut self, shown: f32, scale: f32) {
        self.timeline_height = shown * scale.max(1.0);
    }

    pub fn bodies(&self, available: f32, fit: &Fit) -> Vec<(Pane, Option<f32>)> {
        let mut trial = self.clone();
        self.folded_for_room(available, fit).iter().for_each(|pane| {
            if let Some(p) = trial.pane_mut(*pane) {
                p.collapsed = true;
            }
        });
        trial.shared(available, fit)
    }

    fn shared(&self, available: f32, fit: &Fit) -> Vec<(Pane, Option<f32>)> {
        let open: Vec<&PaneState> = self.panes.iter().filter(|p| p.open).collect();
        let expanded: Vec<&PaneState> = open.iter().copied().filter(|p| !p.collapsed).collect();
        let room = (available - open.len() as f32 * TITLE).max(0.0);
        let need: Vec<f32> = expanded.iter().map(|p| fit.content(p.pane)).collect();
        let want: Vec<f32> = expanded.iter().zip(&need).map(|(p, n)| p.body.min(*n)).collect();
        let floor: Vec<f32> = expanded
            .iter()
            .zip(&want)
            .map(|(p, w)| fit.floor(p.pane).min(*w))
            .collect();
        let kept: Vec<f32> = expanded
            .iter()
            .zip(&floor)
            .map(|(p, f)| fit.kept(p.pane).min(*f))
            .collect();
        let held: Vec<f32> = expanded
            .iter()
            .zip(&kept)
            .map(|(p, k)| fit.held(p.pane).min(*k))
            .collect();
        let sum = |v: &[f32]| v.iter().sum::<f32>();
        let order: Vec<usize> = SHRINK_FIRST
            .iter()
            .filter(|p| Some(**p) != fit.chosen)
            .chain(fit.chosen.iter())
            .filter_map(|p| expanded.iter().position(|e| e.pane == *p))
            .collect();
        let none = vec![0.0; expanded.len()];
        let mut sizes = want.clone();
        let mut over = sum(&want) - room;
        [&floor, &kept, &held, &none].into_iter().for_each(|least| {
            order.iter().for_each(|i| {
                let cut = (sizes[*i] - least[*i]).max(0.0).min(over.max(0.0));
                sizes[*i] -= cut;
                over -= cut;
            });
        });
        let mut spare = (-over).max(0.0);
        order.iter().rev().for_each(|i| {
            let more = match need[*i].is_finite() {
                true => (need[*i] - sizes[*i]).max(0.0).min(spare),
                false => 0.0,
            };
            sizes[*i] += more;
            spare -= more;
        });
        if let Some(last) = sizes.last_mut() {
            *last += spare;
        }
        open.iter()
            .map(|p| {
                let size = expanded.iter().position(|e| e.pane == p.pane).map(|i| sizes[i]);
                (p.pane, size)
            })
            .collect()
    }

    pub fn resize_pane(&mut self, pane: Pane, by: f32) {
        if let Some(p) = self.pane_mut(pane) {
            p.body = (p.body + by).max(p.pane.min_body());
        }
    }
}

pub fn take_ctrl_tabs(raw: &mut egui::RawInput) -> i32 {
    let mut by = 0;
    raw.events.retain(|e| match e {
        egui::Event::Key {
            key: egui::Key::Tab,
            pressed,
            modifiers,
            ..
        } if modifiers.ctrl && !modifiers.alt => {
            if *pressed {
                by += if modifiers.shift { -1 } else { 1 };
            }
            false
        }
        _ => true,
    });
    by
}

pub fn take_tabs(raw: &mut egui::RawInput) -> Vec<bool> {
    let mut shifts = Vec::new();
    raw.events.retain(|e| match e {
        egui::Event::Key {
            key: egui::Key::Tab,
            pressed,
            modifiers,
            ..
        } if !modifiers.alt && !modifiers.command => {
            if *pressed {
                shifts.push(modifiers.shift);
            }
            false
        }
        _ => true,
    });
    shifts
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::vec2;

    fn used(bodies: &[(Pane, Option<f32>)]) -> f32 {
        bodies.iter().map(|(_, b)| b.unwrap_or(0.0) + TITLE).sum()
    }

    fn body(bodies: &[(Pane, Option<f32>)], pane: Pane) -> f32 {
        bodies.iter().find(|(p, _)| *p == pane).and_then(|(_, b)| *b).unwrap()
    }

    #[test]
    fn panes_fit_the_dock_and_the_last_one_takes_what_is_left() {
        let layout = Layout::default();
        let none = Fit::default();
        let bodies = layout.bodies(1200.0, &none);
        assert_eq!(bodies.len(), 6);
        assert!((used(&bodies) - 1200.0).abs() < 0.01);
        assert_eq!(bodies[4], (Pane::Notes, None));
        assert!(bodies[3].1.unwrap() > Pane::Inspector.default_body());
        let small = layout.bodies(800.0, &none);
        small.iter().filter_map(|(p, b)| b.map(|b| (p, b))).for_each(|(p, b)| {
            assert!(b >= p.min_body() - 0.01, "{p:?} {b}");
        });
        assert!((used(&small) - 800.0).abs() < 0.01);
        assert_eq!(
            body(&small, Pane::Brush),
            Pane::Brush.min_body(),
            "Brush gives way first"
        );
        assert!(
            body(&small, Pane::Layers) > Pane::Layers.min_body(),
            "Layers last: {small:?}"
        );
    }

    #[test]
    fn a_short_dock_shares_its_height_and_layers_keeps_three_rows() {
        let layout = Layout::default();
        let fit = Fit {
            layers_least: 180.0,
            ..Fit::default()
        };
        [606.0, 420.0, 300.0, 200.0].into_iter().for_each(|dock| {
            let bodies = layout.shared(dock, &fit);
            assert!(
                (used(&bodies) - dock).abs() < 0.01,
                "nothing runs past a dock of {dock}"
            );
            bodies
                .iter()
                .for_each(|(p, b)| assert!(b.is_none_or(|b| b >= 0.0), "{p:?}"));
            let rows = 180.0_f32.min(dock - 6.0 * TITLE);
            assert!(body(&bodies, Pane::Layers) >= rows - 0.01, "{dock}: {bodies:?}");
        });
        let bodies = layout.shared(606.0, &fit);
        assert!(body(&bodies, Pane::Tool) > body(&bodies, Pane::Inspector), "{bodies:?}");
        assert!(
            body(&bodies, Pane::Inspector) > body(&bodies, Pane::Brush),
            "{bodies:?}"
        );
        assert!(body(&bodies, Pane::Inspector) < Pane::Inspector.min_body());
    }

    #[test]
    fn a_dock_too_short_for_every_pane_shows_the_ones_that_would_be_slivers_folded() {
        let fit = Fit {
            layers_least: 169.0,
            ..Fit::default()
        };
        let layout = Layout::default();
        assert!(layout.folded_for_room(900.0, &fit).is_empty());
        assert_eq!(
            layout.folded_for_room(370.0, &fit),
            vec![Pane::Brush, Pane::Inspector, Pane::Tool]
        );
        let bodies = layout.bodies(370.0, &fit);
        assert!((used(&bodies) - 370.0).abs() < 0.01);
        assert!(body(&bodies, Pane::Layers) >= 169.0);
        assert!(
            bodies
                .iter()
                .filter(|(p, _)| *p != Pane::Layers)
                .all(|(_, b)| b.is_none())
        );
        assert_eq!(
            layout,
            Layout::default(),
            "folding for room is not kept in the settings"
        );
        let chose_brush = Fit {
            chosen: Some(Pane::Brush),
            ..fit.clone()
        };
        assert!(!layout.folded_for_room(450.0, &chose_brush).contains(&Pane::Brush));
        layout
            .bodies(450.0, &chose_brush)
            .iter()
            .filter(|(p, _)| *p != Pane::Layers)
            .for_each(|(p, b)| assert!(b.is_none_or(|b| b >= USABLE - 0.5), "{p:?} {b:?}"));
    }

    #[test]
    fn a_pane_the_person_opened_gets_its_room_before_the_rows_of_layers() {
        let layout = Layout::default();
        let fit = Fit {
            layers_least: 165.0,
            chosen: Some(Pane::Tool),
            ..Fit::default()
        };
        let bodies = layout.bodies(345.0, &fit);
        assert!((used(&bodies) - 345.0).abs() < 0.01);
        assert_eq!(body(&bodies, Pane::Tool), 165.0, "{bodies:?}");
        let middling = layout.bodies(180.0 + 165.0 + 230.0 + 20.0, &fit);
        assert_eq!(
            body(&middling, Pane::Layers),
            165.0,
            "three rows before the rest: {middling:?}"
        );
        assert_eq!(body(&middling, Pane::Tool), 230.0 + 20.0, "{middling:?}");
        let roomier = layout.bodies(560.0, &fit);
        assert_eq!(body(&roomier, Pane::Tool), Pane::Tool.min_body(), "{roomier:?}");
        assert_eq!(body(&roomier, Pane::Layers), 150.0, "{roomier:?}");
    }

    #[test]
    fn a_pane_takes_no_more_than_its_content_and_the_room_goes_to_the_one_that_needs_it() {
        let layout = Layout::default();
        let fit = Fit {
            content: vec![
                (Pane::Tool, 420.0),
                (Pane::Brush, 60.0),
                (Pane::Layers, 110.0),
                (Pane::Inspector, 90.0),
            ],
            layers_least: 110.0,
            chosen: None,
        };
        let bodies = layout.bodies(900.0, &fit);
        assert_eq!(body(&bodies, Pane::Brush), 60.0);
        let greedy = Fit {
            content: vec![(Pane::Brush, 1000.0), (Pane::Inspector, 700.0)],
            ..fit.clone()
        };
        let tall = layout.bodies(1200.0, &greedy);
        assert!(
            body(&tall, Pane::Inspector) > body(&tall, Pane::Brush),
            "Brush is the last to get spare room: {tall:?}"
        );
        assert_eq!(body(&bodies, Pane::Layers), 110.0);
        assert_eq!(body(&bodies, Pane::Tool), 420.0, "{bodies:?}");
        assert!((used(&bodies) - 900.0).abs() < 0.01);
        let tight = layout.shared(600.0, &fit);
        assert!((used(&tight) - 600.0).abs() < 0.01);
        assert!(body(&tight, Pane::Brush) <= 60.0 && body(&tight, Pane::Layers) >= 110.0 - 0.01);
    }

    #[test]
    fn closing_collapsing_and_toggling_panes() {
        let mut layout = Layout::default();
        layout.toggle(Pane::Layers);
        assert!(!layout.is_open(Pane::Layers));
        assert_eq!(layout.bodies(900.0, &Fit::default()).len(), 5);
        assert!(!layout.shows(Pane::Notes), "a folded pane does not show");
        layout.toggle(Pane::Notes);
        assert!(layout.is_open(Pane::Notes) && !layout.pane(Pane::Notes).unwrap().collapsed);
        assert!(layout.shows(Pane::Notes));
        layout.toggle(Pane::Notes);
        assert!(!layout.is_open(Pane::Notes));
        layout.toggle(Pane::Notes);
        layout.hidden = true;
        layout.toggle(Pane::Notes);
        assert!(
            layout.is_open(Pane::Notes) && !layout.hidden,
            "a hidden pane is shown, not closed"
        );
        layout.dock = false;
        layout.toggle(Pane::Tool);
        assert!(layout.dock && layout.is_open(Pane::Tool));
        layout.panes.iter_mut().for_each(|p| p.open = false);
        assert!(!layout.shows_dock());
        layout.resize_pane(Pane::Tool, -1000.0);
        assert_eq!(layout.pane(Pane::Tool).unwrap().body, Pane::Tool.min_body());
    }

    #[test]
    fn a_pane_opened_in_a_full_dock_folds_others_until_it_fits() {
        let mut layout = Layout::default();
        layout.toggle(Pane::Maps);
        assert_eq!(
            layout.make_room(Pane::Maps, 720.0),
            vec![Pane::Brush, Pane::Inspector, Pane::Tool]
        );
        let collapsed = |l: &Layout, p: Pane| l.pane(p).unwrap().collapsed;
        assert!(!collapsed(&layout, Pane::Maps) && !collapsed(&layout, Pane::Layers));
        assert!(layout.needed() <= 720.0);
        let mut taller = Layout::default();
        taller.toggle(Pane::Maps);
        assert_eq!(
            taller.make_room(Pane::Maps, 860.0),
            vec![Pane::Brush, Pane::Inspector],
            "folds only what it must"
        );
        assert!(!collapsed(&taller, Pane::Tool));
        let mut roomy = Layout::default();
        roomy.toggle(Pane::Maps);
        assert!(roomy.make_room(Pane::Maps, 2000.0).is_empty());
        assert!(
            roomy.make_room(Pane::Maps, 0.0).is_empty(),
            "before the dock is measured"
        );
    }

    #[test]
    fn sizes_clamp_to_the_window() {
        let mut layout = Layout {
            dock_width: 2000.0,
            timeline_height: 2000.0,
            ..Layout::default()
        };
        layout.clamp(vec2(1280.0, 800.0));
        assert_eq!(layout.dock_width, 1280.0 * 0.45);
        assert_eq!(layout.timeline_height, 2000.0, "the setting is kept");
        let rows = f32::INFINITY;
        assert_eq!(layout.timeline_size(vec2(1280.0, 800.0), 1.0, rows).0, 800.0 * 0.6);
        assert_eq!(
            layout.timeline_size(vec2(1280.0, 500.0), 1.0, rows).0,
            500.0 - STATUS - CANVAS_MIN.y
        );
        layout.dock_width = 10.0;
        layout.timeline_height = 10.0;
        layout.clamp(vec2(1280.0, 800.0));
        assert_eq!(layout.dock_width, DOCK_MIN);
        assert_eq!(layout.timeline_size(vec2(1280.0, 800.0), 1.0, rows).0, TIMELINE_MIN);
        layout.clamp(vec2(300.0, 260.0));
        assert_eq!(layout.dock_width, DOCK_MIN);
        assert_eq!(layout.timeline_size(vec2(300.0, 260.0), 1.0, rows).0, TIMELINE_MIN);
        let tab = Layout {
            hidden: true,
            ..Layout::default()
        };
        assert!(!tab.shows_dock() && !tab.shows_strip() && !tab.shows_timeline());
    }

    #[test]
    fn the_timeline_is_no_taller_than_its_rows_and_keeps_its_size_on_screen_at_a_larger_interface() {
        let mut layout = Layout::default();
        let window = vec2(1440.0, 900.0);
        assert_eq!(layout.timeline_size(window, 1.0, 1000.0).0, TIMELINE_DEFAULT);
        assert_eq!(layout.timeline_size(window, 1.0, 210.0).0, 210.0);
        assert_eq!(layout.timeline_size(window, 1.0, 40.0).0, TIMELINE_MIN);
        assert_eq!(layout.timeline_size(window, 0.75, 1000.0).0, TIMELINE_DEFAULT);
        let (shown, _, _) = layout.timeline_size(window / 1.5, 1.5, 1000.0);
        assert_eq!(shown, TIMELINE_DEFAULT / 1.5);
        layout.keep_timeline_height(shown, 1.5);
        assert_eq!(layout.timeline_height, TIMELINE_DEFAULT);
    }

    #[test]
    fn the_layout_survives_a_restart_and_old_settings_gain_new_panes() {
        let mut layout = Layout {
            dock_width: 410.0,
            ..Layout::default()
        };
        layout.toggle(Pane::History);
        layout.toggle(Pane::History);
        layout.hidden = true;
        let text = serde_json::to_string(&layout).unwrap();
        let back: Layout = serde_json::from_str(&text).unwrap();
        assert_eq!(back.dock_width, 410.0);
        assert!(!back.hidden && !back.is_open(Pane::History));
        let old = serde_json::from_str::<Layout>(
            r#"{"dockWidth":300,"panes":[{"pane":"layers","open":true,"collapsed":false,"body":5}]}"#,
        )
        .unwrap()
        .repaired();
        assert_eq!(old.panes.len(), 8);
        assert!(
            !old.is_open(Pane::Preview) && !old.is_open(Pane::Maps),
            "a pane that is new to old settings keeps its default"
        );
        assert_eq!(old.panes[0].pane, Pane::Layers);
        assert_eq!(old.panes[0].body, Pane::Layers.min_body());
    }

    #[test]
    fn tab_is_taken_before_egui_moves_focus_with_it() {
        let key = |key, shift, pressed| egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers {
                shift,
                ..Default::default()
            },
        };
        let mut raw = egui::RawInput {
            events: vec![
                key(egui::Key::Tab, false, true),
                key(egui::Key::Tab, false, false),
                key(egui::Key::K, false, true),
                key(egui::Key::Tab, true, true),
            ],
            ..Default::default()
        };
        assert_eq!(take_tabs(&mut raw), [false, true]);
        assert_eq!(raw.events.len(), 1);
    }
}
