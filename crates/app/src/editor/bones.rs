use super::{Editor, Tool};
use crate::jobs::{Spec, Tag};
use crate::messages::Tone;
use crate::theme::{self, Hint, icon};
use anyhow::{Context, Result};
use egui::{Color32, Key, Modifiers, Pos2, RichText, Vec2};
use klad_core::doc::Kind;
use klad_core::ops::{BendSpec, BoneSpec, IkSpec, Op, PoseSpec, PosedCel, WeightsSpec};
use klad_core::rig::{
    Bend, Bone, CELL, Ik, Pose, Reach, Weights, applied, bent_against, limb_of, moved, near_degrees, painted, subtree,
    target_for_end, tips, weights_for,
};
use std::collections::BTreeMap;

pub(crate) const PREVIEW: &str =
    "Bones are a preview: posing may still change in later versions. Drawings keep their rigs when it does.";
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Mode {
    #[default]
    Draw,
    Weights,
    Pose,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Grip {
    Head,
    Tail,
    Whole,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Dragging {
    New(Option<String>),
    Bone(String, Grip),
}

#[derive(Clone, Debug)]
pub(super) struct Drag {
    pub from: Vec2,
    pub to: Vec2,
    pub what: Dragging,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Chain {
    pub owner: String,
    pub parent: Option<String>,
    pub head: [f32; 2],
    pub joined: bool,
    pub drawn: Vec<String>,
    pub undone: Vec<String>,
    pub to: Vec2,
    pub pressed: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Posing {
    Turn(String),
    Reach(String, Vec2),
    Root(Vec2),
}

#[derive(Clone)]
pub(super) struct Live {
    pub key: (u64, u32, String, Pose),
    pub ops: std::sync::Arc<Vec<Op>>,
    pub cels: std::sync::Arc<Vec<String>>,
}

type Rests = (u64, String, Weights, BTreeMap<String, Vec<klad_core::stroke::Mark>>);

pub(super) struct BoneTool {
    pub mode: Mode,
    pub selected: Option<String>,
    pub drag: Option<Drag>,
    pub chain: Option<Chain>,
    pub dabs: Vec<[f32; 4]>,
    pub radius: f32,
    pub strength: f32,
    pub wmode: klad_core::rig::Mode,
    pub flow: f32,
    pub hardness: f32,
    pub pressure: bool,
    pub pens: Vec<f32>,
    pub pen: f32,
    pub stroke_mode: klad_core::rig::Mode,
    pub all: bool,
    pub read: Option<(u64, Weights)>,
    pub pose: BTreeMap<String, f32>,
    pub root: [f32; 2],
    pub reach: BTreeMap<String, Reach>,
    pub loaded: Option<(u32, String, Option<Pose>)>,
    pub live: Option<Live>,
    rests: Option<Rests>,
    pub sent: bool,
    pub posing: Option<Posing>,
    pub name: String,
    pub heat: Option<(String, u64, egui::TextureHandle)>,
}

impl Default for BoneTool {
    fn default() -> Self {
        Self {
            mode: Mode::Draw,
            selected: None,
            drag: None,
            chain: None,
            dabs: Vec::new(),
            radius: 24.0,
            strength: 1.0,
            wmode: klad_core::rig::Mode::Paint,
            flow: 1.0,
            hardness: 0.0,
            pressure: true,
            pens: Vec::new(),
            pen: 1.0,
            stroke_mode: klad_core::rig::Mode::Paint,
            all: false,
            read: None,
            pose: BTreeMap::new(),
            root: [0.0, 0.0],
            reach: BTreeMap::new(),
            loaded: None,
            live: None,
            rests: None,
            sent: false,
            posing: None,
            name: String::new(),
            heat: None,
        }
    }
}

fn heat(v: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(240, (60.0 + 160.0 * v) as u8, 40, (v * 150.0) as u8)
}

fn bone_colour(k: usize) -> [f32; 3] {
    const COLOURS: [[f32; 3]; 8] = [
        [230.0, 80.0, 70.0],
        [70.0, 150.0, 230.0],
        [90.0, 200.0, 110.0],
        [240.0, 190.0, 60.0],
        [180.0, 100.0, 220.0],
        [60.0, 200.0, 200.0],
        [240.0, 130.0, 50.0],
        [200.0, 200.0, 200.0],
    ];
    COLOURS[k % COLOURS.len()]
}

fn weight_legend(ui: &mut egui::Ui) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(64.0, theme::CONTROL), egui::Sense::hover());
    let bar = egui::Rect::from_min_size(rect.min + egui::vec2(0.0, 4.0), egui::vec2(64.0, 8.0));
    (0..16).for_each(|i| {
        let x0 = bar.min.x + bar.width() * i as f32 / 16.0;
        let cell = egui::Rect::from_min_size(
            egui::pos2(x0, bar.min.y),
            egui::vec2(bar.width() / 16.0 + 0.5, bar.height()),
        );
        ui.painter().rect_filled(cell, 0.0, heat(i as f32 / 15.0));
    });
    let font = theme::regular(theme::SMALL);
    ui.painter().text(
        egui::pos2(bar.min.x, bar.max.y + 1.0),
        egui::Align2::LEFT_TOP,
        "0",
        font.clone(),
        theme::WEAK,
    );
    ui.painter().text(
        egui::pos2(bar.max.x, bar.max.y + 1.0),
        egui::Align2::RIGHT_TOP,
        "1",
        font,
        theme::WEAK,
    );
    response.hint("the overlay's colours: no weight to all of it; it shows the weight used, after every bone's share");
}

const NEAR: f32 = 10.0;
const CLICK: f32 = 5.0;
const JOINED: f32 = 0.5;
const TARGET: Color32 = Color32::from_rgb(255, 120, 50);

type Later = Box<dyn FnOnce(&mut Editor) -> Result<()>>;

fn near_segment(p: Vec2, a: [f32; 2], b: [f32; 2]) -> f32 {
    let (a, b) = (Vec2::new(a[0], a[1]), Vec2::new(b[0], b[1]));
    let d = b - a;
    let t = ((p - a).dot(d) / d.length_sq().max(1e-6)).clamp(0.0, 1.0);
    (a + d * t - p).length()
}

fn at(p: [f32; 2]) -> Vec2 {
    Vec2::new(p[0], p[1])
}

const NOTHING_FOLLOWS: &str =
    "no layer follows these bones yet: attach one with the bone button on its row in the Layers panel";

pub(super) fn pose_args(bundle: &std::path::Path, owner: &str, frame: u32, pose: &Pose) -> Vec<String> {
    let mut args = vec![
        "pose".to_string(),
        bundle.to_string_lossy().to_string(),
        owner.to_string(),
        "--from".into(),
        frame.to_string(),
        "--to".into(),
        frame.to_string(),
    ];
    pose.angles
        .iter()
        .filter(|(_, v)| v.abs() > 1e-3)
        .for_each(|(k, v)| args.extend(["--bone".to_string(), format!("{k}={v:.2}")]));
    if pose.root != [0.0, 0.0] {
        args.extend(["--root".to_string(), format!("{:.2},{:.2}", pose.root[0], pose.root[1])]);
    }
    pose.ik.iter().for_each(|(k, r)| {
        args.extend([
            "--target".to_string(),
            format!("{k}={:.2},{:.2}", r.target[0], r.target[1]),
        ]);
        if r.mix != 1.0 {
            args.extend(["--mix".to_string(), format!("{k}={:.3}", r.mix)]);
        }
    });
    args
}

impl Editor {
    pub(super) fn rig_owner(&self) -> Option<String> {
        let doc = &self.scene.bundle.doc;
        let active = doc.locate_path(&self.scene.active);
        active
            .filter(|l| l.kind() == Kind::Bones)
            .or_else(|| active.and_then(|l| klad_core::rig::linked(doc, l)))
            .or_else(|| klad_core::rig::skeletons(doc).into_iter().next())
            .map(|l| l.id.clone())
    }

    pub(super) fn new_skeleton(&mut self) -> Result<String> {
        let taken: Vec<String> = klad_core::rig::skeletons(&self.scene.bundle.doc)
            .iter()
            .map(|s| s.name.clone())
            .collect();
        let name = std::iter::once("bones".to_string())
            .chain((2..).map(|n| format!("bones {n}")))
            .find(|n| !taken.contains(n))
            .unwrap_or_default();
        self.add_layer_value(serde_json::json!({"kind": "bones", "name": name}))?;
        self.rig_owner().context("the bones layer was not made")
    }

    fn rig_bones(&self) -> Vec<Bone> {
        self.rig_owner()
            .and_then(|id| self.scene.bundle.doc.layer(&id).map(|l| l.bones.clone()))
            .unwrap_or_default()
    }

    fn rig_ik(&self) -> Vec<Ik> {
        self.rig_owner()
            .and_then(|id| self.scene.bundle.doc.layer(&id).map(|l| l.ik.clone()))
            .unwrap_or_default()
    }

    fn shown_pose(&self) -> Pose {
        match self.bones.mode {
            Mode::Pose => Pose {
                rest: String::new(),
                angles: self.bones.pose.clone(),
                root: self.bones.root,
                ik: self.bones.reach.clone(),
            },
            _ => Pose::default(),
        }
    }

    pub(super) fn live_pose(&mut self) -> Option<Live> {
        if self.tool != Tool::Bone || self.bones.mode != Mode::Pose {
            self.bones.live = None;
            return None;
        }
        let owner = self.rig_owner()?;
        let shown = self.shown_pose();
        let stored = self.stored_pose(&owner).unwrap_or_default();
        let same = |a: &BTreeMap<String, f32>, b: &BTreeMap<String, f32>| {
            a.keys()
                .chain(b.keys())
                .all(|k| (a.get(k).copied().unwrap_or(0.0) - b.get(k).copied().unwrap_or(0.0)).abs() < 1e-3)
        };
        if same(&shown.angles, &stored.angles) && shown.root == stored.root && shown.ik == stored.ik {
            self.bones.live = None;
            return None;
        }
        let doc = &self.scene.bundle.doc;
        let key = (doc.rev, self.scene.frame, owner.clone(), shown.clone());
        if let Some(live) = self.bones.live.as_ref().filter(|l| l.key == key) {
            return Some(live.clone());
        }
        let skeleton = doc.layer(&owner)?.clone();
        let canvas = (doc.canvas.width, doc.canvas.height);
        if self
            .bones
            .rests
            .as_ref()
            .is_none_or(|(r, o, _, _)| (*r, o) != (doc.rev, &owner))
        {
            let weights = self
                .scene
                .bundle
                .read_weights(&owner)
                .ok()
                .filter(|w| !w.maps.is_empty())
                .unwrap_or_else(|| Weights::auto(&skeleton.bones, canvas));
            self.bones.rests = Some((doc.rev, owner.clone(), weights, BTreeMap::new()));
        }
        let frame = self.scene.frame;
        let wanted: Vec<(String, String)> = painted(doc, &skeleton.id)
            .iter()
            .filter_map(|layer| {
                let cel = layer.content_at(frame)?;
                Some((
                    layer.id.clone(),
                    cel.pose.as_ref().map_or_else(|| cel.id.clone(), |p| p.rest.clone()),
                ))
            })
            .collect();
        let (scene, rests) = (&self.scene, &mut self.bones.rests);
        let (_, _, weights, marks) = rests.as_mut()?;
        wanted.iter().for_each(|(layer, rest)| {
            if !marks.contains_key(rest) {
                let read = match scene.marks_of(rest) {
                    [] => scene.bundle.read_marks(layer, rest).unwrap_or_default(),
                    loaded => loaded.to_vec(),
                };
                marks.insert(rest.clone(), read);
            }
        });
        let (weights, rests) = (&*weights, &*marks);
        let made: Vec<PosedCel> = painted(&self.scene.bundle.doc, &skeleton.id)
            .into_iter()
            .filter_map(|layer| {
                let cel = layer.content_at(frame)?;
                let rest = cel.pose.as_ref().map_or_else(|| cel.id.clone(), |p| p.rest.clone());
                let marks = rests.get(&rest)?;
                let pose = Pose {
                    rest: rest.clone(),
                    ..shown.clone()
                };
                let marks = klad_core::rig::Rig::new(&skeleton.bones, &skeleton.ik, weights, &pose)
                    .following(layer.follows())
                    .pose(marks, canvas);
                Some(PosedCel {
                    layer: layer.id.clone(),
                    cel: cel.id.clone(),
                    frame: cel.frame,
                    hold: cel.hold,
                    pose,
                    marks,
                    fresh: false,
                })
            })
            .collect();
        if made.is_empty() {
            return None;
        }
        let cels = made.iter().map(|c| c.cel.clone()).collect();
        let ops = vec![Op::Pose(PoseSpec {
            layer: owner,
            from: frame,
            to: frame,
            bones: shown.angles.clone(),
            root: shown.root,
            ik: shown.ik.clone(),
            hold: None,
            resolved: Some(made),
        })];
        let live = Live {
            key,
            ops: std::sync::Arc::new(ops),
            cels: std::sync::Arc::new(cels),
        };
        self.bones.live = Some(live.clone());
        Some(live)
    }

    fn bends(&self) -> bool {
        self.rig_owner().is_none_or(|o| {
            painted(&self.scene.bundle.doc, &o)
                .iter()
                .any(|l| l.follows().is_none())
        })
    }

    fn stored_pose(&self, owner: &str) -> Option<Pose> {
        painted(&self.scene.bundle.doc, owner)
            .iter()
            .find_map(|l| l.content_at(self.scene.frame).and_then(|c| c.pose.clone()))
    }

    pub(super) fn bone_keys(&mut self, ui: &mut egui::Ui) {
        self.tend_chain();
        self.tend_pose();
        self.tend_bone_pick();
        if self.bones.chain.is_none() || self.keys_busy {
            return;
        }
        let (enter, escape) = ui.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        if enter || escape {
            self.end_chain();
        }
    }

    pub(super) fn end_chain(&mut self) {
        if self.bones.chain.take().is_some() {
            self.say(Tone::Info, "chain ended");
        }
    }

    fn tend_chain(&mut self) {
        let Some(mut chain) = self.bones.chain.take() else {
            return;
        };
        if self.tool != Tool::Bone || self.bones.mode != Mode::Draw || self.rig_owner().as_deref() != Some(&chain.owner)
        {
            return;
        }
        let bones = self.rig_bones();
        let exists = |n: &str| bones.iter().any(|b| b.name == n);
        if chain.joined
            && chain.parent.as_deref().is_some_and(|p| !exists(p))
            && let Some(renamed) = bones
                .iter()
                .find(|b| (at(b.tail) - at(chain.head)).length() <= JOINED && !chain.drawn.contains(&b.name))
        {
            let (was, now) = (chain.parent.clone().unwrap_or_default(), renamed.name.clone());
            chain
                .drawn
                .iter_mut()
                .filter(|n| **n == was)
                .for_each(|n| *n = now.clone());
            chain.parent = Some(now);
        }
        if chain.parent.as_deref().is_some_and(|p| !exists(p)) {
            while chain.drawn.last().is_some_and(|n| !exists(n)) {
                chain.undone.extend(chain.drawn.pop());
            }
            let Some(last) = chain.drawn.last() else {
                return;
            };
            chain.parent = Some(last.clone());
            chain.joined = true;
        }
        while let Some(back) = chain.undone.last().cloned()
            && bones
                .iter()
                .any(|b| b.name == back && b.parent == chain.parent && b.parent.is_some())
        {
            chain.undone.pop();
            chain.drawn.push(back.clone());
            chain.parent = Some(back);
            chain.joined = true;
        }
        if chain.joined
            && let Some(p) = chain.parent.as_ref().and_then(|p| bones.iter().find(|b| &b.name == p))
        {
            chain.head = p.tail;
        }
        self.bones.chain = Some(chain);
    }

    fn tend_bone_pick(&mut self) {
        let Some(picked) = self.bones.selected.clone() else {
            return;
        };
        let bones = self.rig_bones();
        if bones.iter().any(|b| b.name == picked) {
            return;
        }
        let next = self
            .bones
            .chain
            .as_ref()
            .and_then(|c| c.parent.clone())
            .filter(|p| bones.iter().any(|b| &b.name == p));
        self.bones.name = next.clone().unwrap_or_default();
        self.bones.selected = next;
        self.bones.heat = None;
    }

    fn tend_pose(&mut self) {
        if self.bones.posing.is_some() {
            return;
        }
        let Some(owner) = self.rig_owner() else {
            return;
        };
        let stored = self.stored_pose(&owner);
        let key = (self.scene.frame, owner, stored.clone());
        if self.bones.loaded.as_ref() == Some(&key) {
            return;
        }
        if self.bones.sent
            && self
                .bones
                .loaded
                .as_ref()
                .is_some_and(|(f, o, _)| (*f, o) == (key.0, &key.1))
        {
            self.bones.sent = false;
            self.bones.loaded = Some(key);
            return;
        }
        self.bones.sent = false;
        let pose = stored.unwrap_or_default();
        self.bones.pose = pose.angles;
        self.bones.root = pose.root;
        self.bones.reach = pose.ik;
        self.bones.posing = None;
        self.bones.loaded = Some(key);
    }

    fn add_bone(&mut self, parent: Option<String>, head: [f32; 2], tail: [f32; 2]) -> Result<Bone> {
        let owner = match self.rig_owner() {
            Some(o) => o,
            None => self.new_skeleton()?,
        };
        let taken = self.rig_bones();
        let name = (1..)
            .map(|n| format!("bone {n}"))
            .find(|n| !taken.iter().any(|b| &b.name == n))
            .unwrap_or_default();
        self.apply(vec![Op::Bone(BoneSpec {
            layer: owner,
            add: Some(name.clone()),
            parent: parent.clone(),
            head: Some(head),
            tail: Some(tail),
            ..BoneSpec::default()
        })])?;
        self.pick_bone(Some(name.clone()));
        Ok(Bone {
            name,
            parent,
            head,
            tail,
        })
    }

    fn chain_on(&mut self, drawn: Vec<String>, made: &Bone) {
        let Some(owner) = self.rig_owner() else {
            return;
        };
        let bones = self.rig_bones();
        let sibling = self.modifiers.shift;
        let joined = |b: &Bone| {
            b.parent
                .as_ref()
                .and_then(|p| bones.iter().find(|o| &o.name == p))
                .is_some_and(|p| (at(p.tail) - at(b.head)).length() <= JOINED)
        };
        self.bones.chain = Some(Chain {
            owner,
            parent: match sibling {
                true => made.parent.clone(),
                false => Some(made.name.clone()),
            },
            head: if sibling { made.head } else { made.tail },
            joined: !sibling || joined(made),
            drawn,
            undone: Vec::new(),
            to: at(made.tail),
            pressed: false,
        });
    }

    fn chain_up(&mut self) -> Result<()> {
        self.tend_chain();
        let Some(chain) = self.bones.chain.clone() else {
            return Ok(());
        };
        if !chain.pressed {
            return Ok(());
        }
        if let Some(c) = self.bones.chain.as_mut() {
            c.pressed = false;
        }
        if (chain.to - at(chain.head)).length() * self.zoom() <= NEAR {
            self.end_chain();
            return Ok(());
        }
        let made = self.add_bone(chain.parent.clone(), chain.head, [chain.to.x, chain.to.y])?;
        let mut drawn = chain.drawn;
        drawn.push(made.name.clone());
        self.chain_on(drawn, &made);
        Ok(())
    }

    fn limb_of(&self, name: &str) -> Option<String> {
        let bones = self.rig_bones();
        self.rig_ik()
            .into_iter()
            .filter(|k| self.bones.reach.get(&k.bone).is_some_and(|r| r.mix > 0.0))
            .find(|k| limb_of(&bones, k).iter().any(|b| b == name))
            .map(|k| k.bone)
    }

    fn ik_on(&mut self, end: &str) -> Option<[f32; 2]> {
        let (bones, ik) = (self.rig_bones(), self.rig_ik());
        let shown = self.shown_pose();
        let target = target_for_end(&bones, &ik, &shown, end)?;
        if bent_against(&bones, &ik, &shown, end) {
            let side = match ik.iter().find(|k| k.bone == end).map(|k| k.bend) {
                Some(Bend::Counterclockwise) => "counterclockwise",
                _ => "clockwise",
            };
            self.say(
                Tone::Info,
                format!(
                    "{end}'s IK bends {side}, so the joint turns over to that side; flip it in draw mode's IK menu"
                ),
            );
        }
        self.bones.reach.insert(end.to_string(), Reach { target, mix: 1.0 });
        Some(target)
    }

    fn neighbour_turns(&self, end: &str) -> BTreeMap<String, f32> {
        let (bones, ik) = (self.rig_bones(), self.rig_ik());
        let Some(owner) = self.rig_owner().and_then(|o| self.scene.bundle.doc.layer(&o).cloned()) else {
            return BTreeMap::new();
        };
        let frame = self.scene.frame;
        let here = self.stored_pose(&owner.id).map(|p| p.rest);
        let limb: Vec<String> = ik
            .iter()
            .find(|k| k.bone == end)
            .map(|k| limb_of(&bones, k))
            .unwrap_or_default();
        painted(&self.scene.bundle.doc, &owner.id)
            .first()
            .and_then(|l| {
                l.cels()
                    .iter()
                    .filter(|c| c.frame < frame)
                    .filter_map(|c| c.pose.as_ref())
                    .rfind(|p| here.as_ref().is_none_or(|r| &p.rest == r))
                    .cloned()
            })
            .map(|p| applied(&bones, &ik, &p))
            .unwrap_or_default()
            .into_iter()
            .filter(|(b, _)| limb.contains(b))
            .collect()
    }

    fn ik_off(&mut self, end: &str) {
        let (bones, ik) = (self.rig_bones(), self.rig_ik());
        let solved = applied(&bones, &ik, &self.shown_pose());
        let before = self.neighbour_turns(end);
        if let Some(k) = ik.iter().find(|k| k.bone == end) {
            limb_of(&bones, k).into_iter().for_each(|b| {
                if let Some(v) = solved.get(&b) {
                    let to = before
                        .get(&b)
                        .or_else(|| self.bones.pose.get(&b))
                        .copied()
                        .unwrap_or(0.0);
                    self.bones.pose.insert(b, near_degrees(*v, to));
                }
            });
        }
        self.bones.reach.remove(end);
    }

    fn zoom(&self) -> f32 {
        self.view.map_or(1.0, |v| v.zoom)
    }

    pub(super) fn pick_bone(&mut self, name: Option<String>) {
        self.bones.name = name.clone().unwrap_or_default();
        self.bones.selected = name;
        self.bones.heat = None;
    }

    fn grip_at(&self, c: Vec2) -> Option<(String, String, Grip)> {
        let bones = self.rig_bones();
        let reach = NEAR / self.zoom();
        let at = |p: [f32; 2]| (Vec2::new(p[0], p[1]) - c).length();
        let picked = self.bones.selected.as_deref();
        let joint = |b: &Bone| {
            b.parent
                .as_ref()
                .and_then(|p| bones.iter().find(|o| &o.name == p))
                .filter(|p| (p.tail[0] - b.head[0]).hypot(p.tail[1] - b.head[1]) <= JOINED)
                .map(|p| p.name.clone())
        };
        bones
            .iter()
            .flat_map(|b| {
                [
                    (Grip::Tail, at(b.tail)),
                    (Grip::Head, at(b.head)),
                    (Grip::Whole, near_segment(c, b.head, b.tail)),
                ]
                .into_iter()
                .filter(move |(_, d)| *d <= reach)
                .map(move |(g, d)| ((g == Grip::Whole, picked != Some(b.name.as_str()), d), b, g))
            })
            .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(_, b, g)| match (g, joint(b)) {
                (Grip::Head, Some(parent)) => (b.name.clone(), parent, Grip::Tail),
                _ => (b.name.clone(), b.name.clone(), g),
            })
    }

    fn drag_spec(&self, drag: &Drag) -> Option<BoneSpec> {
        let Dragging::Bone(name, grip) = &drag.what else {
            return None;
        };
        let bone = self.rig_bones().into_iter().find(|b| &b.name == name)?;
        let d = drag.to - drag.from;
        let by = |p: [f32; 2]| [p[0] + d.x, p[1] + d.y];
        Some(BoneSpec {
            layer: self.rig_owner()?,
            moved: Some(name.clone()),
            head: (*grip == Grip::Head).then(|| by(bone.head)),
            tail: (*grip == Grip::Tail).then(|| by(bone.tail)),
            by: (*grip == Grip::Whole).then_some([d.x, d.y]),
            ..BoneSpec::default()
        })
    }

    fn drawn_bones(&self) -> Vec<Bone> {
        let bones = self.rig_bones();
        self.bones
            .drag
            .as_ref()
            .filter(|d| (d.to - d.from).length() * self.zoom() >= CLICK)
            .and_then(|d| self.drag_spec(d))
            .and_then(|s| moved(&bones, s.moved.as_deref()?, s.head, s.tail, s.by).ok())
            .unwrap_or(bones)
    }

    fn bone_at(&self, c: Vec2) -> Option<String> {
        let bones = self.rig_bones();
        let zoom = self.zoom();
        let shown = tips(&bones, &self.rig_ik(), &self.shown_pose());
        bones
            .iter()
            .zip(shown)
            .map(|(b, (h, t))| (b.name.clone(), near_segment(c, h, t)))
            .filter(|(_, d)| *d <= NEAR / zoom)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(n, _)| n)
    }

    pub(super) fn bone_down(&mut self, c: Vec2) {
        match self.bones.mode {
            Mode::Draw if self.bones.chain.is_some() => {
                if let Some(chain) = self.bones.chain.as_mut() {
                    chain.pressed = true;
                    chain.to = c;
                }
            }
            Mode::Draw if self.modifiers.alt => {
                let bones = self.rig_bones();
                let reach = NEAR / self.zoom();
                let from_tail = bones
                    .iter()
                    .find(|b| (at(b.tail) - c).length() <= reach)
                    .map(|b| (b.name.clone(), at(b.tail)));
                let (parent, from) = match from_tail {
                    Some((name, tail)) => (Some(name), tail),
                    None => (self.bone_at(c), c),
                };
                self.bones.drag = Some(Drag {
                    from,
                    to: c,
                    what: Dragging::New(parent),
                });
            }
            Mode::Draw => {
                let what = match self.grip_at(c) {
                    Some((picked, moving, grip)) => {
                        if self.bones.selected.as_ref() != Some(&picked) {
                            self.pick_bone(Some(picked));
                        }
                        Dragging::Bone(moving, grip)
                    }
                    None => Dragging::New(None),
                };
                self.bones.drag = Some(Drag { from: c, to: c, what });
            }
            Mode::Weights if self.modifiers.command => {
                let found = self.bone_at(c);
                if found.is_some() {
                    self.pick_bone(found);
                }
            }
            Mode::Weights => {
                self.bones.dabs.clear();
                self.bone_dab(c);
            }
            Mode::Pose => self.pose_down(c),
        }
    }

    fn pose_down(&mut self, c: Vec2) {
        if self.nothing_follows() {
            self.say(Tone::Refused, NOTHING_FOLLOWS);
        }
        let (bones, ik) = (self.rig_bones(), self.rig_ik());
        let shown = tips(&bones, &ik, &self.shown_pose());
        let reach = NEAR / self.zoom();
        let alt = self.modifiers.alt;
        let handle = ik
            .iter()
            .filter_map(|k| {
                let i = bones.iter().position(|b| b.name == k.bone)?;
                let tip = shown[i].1;
                let target = self.bones.reach.get(&k.bone).map(|r| r.target);
                let d = (at(tip) - c)
                    .length()
                    .min(target.map_or(f32::MAX, |t| (at(t) - c).length()));
                (d <= reach).then(|| (d, k.bone.clone(), target.unwrap_or(tip)))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0));
        if !alt && let Some((_, end, target)) = handle {
            let target = match self.bones.reach.get(&end) {
                Some(r) => r.target,
                None => self.ik_on(&end).unwrap_or(target),
            };
            self.bones.posing = Some(Posing::Reach(end.clone(), at(target) - c));
            self.pick_bone(Some(end));
            return;
        }
        let root = bones
            .iter()
            .zip(&shown)
            .filter(|(b, _)| b.parent.is_none())
            .map(|(b, (h, _))| ((at(*h) - c).length(), b.name.clone()))
            .filter(|(d, _)| *d <= reach)
            .min_by(|a, b| a.0.total_cmp(&b.0));
        if !alt && let Some((_, name)) = root {
            self.bones.posing = Some(Posing::Root(c));
            self.pick_bone(Some(name));
            return;
        }
        let Some(name) = self.bone_at(c) else {
            self.bones.posing = None;
            return;
        };
        self.pick_bone(Some(name.clone()));
        self.bones.posing = match self.limb_of(&name) {
            Some(end) if !alt => {
                let target = self.bones.reach[&end].target;
                Some(Posing::Reach(end, at(target) - c))
            }
            Some(end) => {
                self.ik_off(&end);
                Some(Posing::Turn(name))
            }
            None => Some(Posing::Turn(name)),
        };
    }

    pub(super) fn weight_mode_now(&self) -> klad_core::rig::Mode {
        use klad_core::rig::Mode as W;
        match (self.bones.wmode, self.modifiers.alt) {
            (W::Add, true) => W::Remove,
            (W::Remove, true) => W::Add,
            (mode, _) => mode,
        }
    }

    fn bone_dab(&mut self, c: Vec2) {
        if self.bones.dabs.is_empty() {
            self.bones.stroke_mode = self.weight_mode_now();
            self.bones.pens.clear();
        }
        let value = match (self.bones.stroke_mode, self.modifiers.alt) {
            (klad_core::rig::Mode::Paint, true) => 0.0,
            _ => self.bones.strength,
        };
        if self
            .bones
            .dabs
            .last()
            .is_none_or(|d| (Vec2::new(d[0], d[1]) - c).length() > self.bones.radius * 0.25)
        {
            self.bones.dabs.push([c.x, c.y, self.bones.radius, value]);
            self.bones.pens.push(self.bones.pen);
        }
    }

    pub(super) fn weights_read(&mut self) -> Option<Weights> {
        let rev = self.scene.bundle.doc.rev;
        if let Some((r, w)) = &self.bones.read
            && *r == rev
        {
            return Some(w.clone());
        }
        let owner = self.rig_owner()?;
        let weights: Weights = self.scene.bundle.read_weights(&owner).ok()?;
        self.bones.read = Some((rev, weights.clone()));
        Some(weights)
    }

    pub(super) fn weight_under(&mut self, p: Vec2) -> Option<(f32, f32)> {
        let bone = self.bones.selected.clone()?;
        let bones = self.rig_bones();
        let i = bones.iter().position(|b| b.name == bone)?;
        let weights = self.weights_read()?;
        Some((weights.value_at(&bone, [p.x, p.y]), weights.at(&bones, [p.x, p.y])[i]))
    }

    pub(super) fn pointer_canvas(&self) -> Option<Vec2> {
        let (view, p) = (self.view?, self.pointer?);
        let rect = self.canvas_rect;
        rect.contains(p).then(|| view.to_canvas(p - rect.min, rect.size()))
    }

    pub(super) fn weight_mode(&mut self, mode: klad_core::rig::Mode) {
        self.bones.wmode = mode;
        self.say(
            crate::messages::Tone::Info,
            format!(
                "the weight brush will {}",
                match mode {
                    klad_core::rig::Mode::Paint => "paint toward the weight",
                    klad_core::rig::Mode::Add => "add weight (alt takes it away)",
                    klad_core::rig::Mode::Remove => "take weight away (alt adds it)",
                    klad_core::rig::Mode::Smooth => "smooth the weights under it",
                }
            ),
        );
    }

    pub(super) fn pick_weight(&mut self) {
        let Some(p) = self.pointer_canvas() else {
            self.say(
                crate::messages::Tone::Info,
                "point at the canvas to pick up the weight there",
            );
            return;
        };
        match self.weight_under(p) {
            Some((painted, _)) => {
                self.bones.strength = (painted * 100.0).round() / 100.0;
                self.say(
                    crate::messages::Tone::Done,
                    format!("the weight is {:.2} now, as under the pointer", self.bones.strength),
                );
            }
            None => self.say(crate::messages::Tone::Info, "pick a bone first (cmd click it)"),
        }
    }

    pub(super) fn bone_move(&mut self, c: Vec2) {
        match self.bones.mode {
            Mode::Draw => {
                if let Some(chain) = self.bones.chain.as_mut() {
                    chain.to = c;
                }
                if let Some(drag) = self.bones.drag.as_mut() {
                    drag.to = c;
                }
            }
            Mode::Weights => {
                if !self.bones.dabs.is_empty() {
                    self.bone_dab(c);
                }
            }
            Mode::Pose => match self.bones.posing.clone() {
                Some(Posing::Turn(name)) => {
                    let bones = self.rig_bones();
                    let Some(i) = bones.iter().position(|b| b.name == name) else {
                        return;
                    };
                    let shown = tips(&bones, &self.rig_ik(), &self.shown_pose());
                    let (head, tail) = shown[i];
                    let now = (tail[1] - head[1]).atan2(tail[0] - head[0]);
                    let want = (c.y - head[1]).atan2(c.x - head[0]);
                    let mut turn = (want - now).to_degrees();
                    turn = (turn + 540.0).rem_euclid(360.0) - 180.0;
                    *self.bones.pose.entry(name).or_insert(0.0) += turn;
                }
                Some(Posing::Reach(end, offset)) => {
                    if let Some(r) = self.bones.reach.get_mut(&end) {
                        r.target = [c.x + offset.x, c.y + offset.y];
                    }
                }
                Some(Posing::Root(last)) => {
                    self.bones.root = [self.bones.root[0] + c.x - last.x, self.bones.root[1] + c.y - last.y];
                    self.bones.posing = Some(Posing::Root(c));
                }
                None => {}
            },
        }
    }

    fn flush_name(&mut self) -> Result<()> {
        let Some(picked) = self.bones.selected.clone() else {
            return Ok(());
        };
        let to = self.bones.name.trim().to_string();
        let bones = self.rig_bones();
        if to.is_empty()
            || to == picked
            || !bones.iter().any(|b| b.name == picked)
            || bones.iter().any(|b| b.name == to)
        {
            return Ok(());
        }
        self.bone_edit(|s| {
            s.rename = Some(picked.clone());
            s.to_name = Some(to.clone());
        })?;
        if let Some(Drag {
            what: Dragging::Bone(n, _) | Dragging::New(Some(n)),
            ..
        }) = self.bones.drag.as_mut()
            && *n == picked
        {
            *n = to.clone();
        }
        self.bones.selected = Some(to);
        Ok(())
    }

    pub(super) fn bone_up(&mut self) -> Result<()> {
        if self.bones.mode == Mode::Draw {
            self.flush_name()?;
        }
        match self.bones.mode {
            Mode::Draw if self.bones.chain.is_some() => self.chain_up()?,
            Mode::Draw => {
                let Some(drag) = self.bones.drag.take() else {
                    return Ok(());
                };
                let short = (drag.to - drag.from).length() * self.zoom() < CLICK;
                let parent = match (&drag.what, short) {
                    (Dragging::Bone(..), true) => return Ok(()),
                    (Dragging::Bone(..), false) => {
                        let spec = self.drag_spec(&drag).context("that bone is gone")?;
                        return self.apply(vec![Op::Bone(spec)]);
                    }
                    (Dragging::New(_), true) => {
                        self.pick_bone(self.bone_at(drag.to));
                        return Ok(());
                    }
                    (Dragging::New(parent), false) => parent.clone(),
                };
                let made = self.add_bone(parent, [drag.from.x, drag.from.y], [drag.to.x, drag.to.y])?;
                self.chain_on(vec![made.name.clone()], &made);
            }
            Mode::Weights => {
                let dabs = std::mem::take(&mut self.bones.dabs);
                if dabs.is_empty() {
                    return Ok(());
                }
                let bone = self
                    .bones
                    .selected
                    .clone()
                    .context("pick a bone to paint its weights: cmd click it, or pick it in the skeleton list")?;
                let pens = std::mem::take(&mut self.bones.pens);
                self.paint_weights(WeightsSpec {
                    layer: self.rig_owner().context("no skeleton here")?,
                    frame: self.scene.frame,
                    bone: Some(bone),
                    set: dabs,
                    mode: self.bones.stroke_mode,
                    strength: self.bones.flow,
                    hardness: self.bones.hardness,
                    pressures: if self.bones.pressure { pens } else { Vec::new() },
                    ..WeightsSpec::default()
                })?;
            }
            Mode::Pose => self.bones.posing = None,
        }
        Ok(())
    }

    fn paint_weights(&mut self, mut spec: WeightsSpec) -> Result<()> {
        let bundle = &self.scene.bundle;
        let made = weights_for(bundle, &bundle.doc, &spec, &BTreeMap::new())?;
        spec.resolved = Some(made);
        self.apply(vec![Op::Weights(spec)])?;
        self.bones.heat = None;
        Ok(())
    }

    pub(super) fn bone_edit(&mut self, edit: impl FnOnce(&mut BoneSpec)) -> Result<()> {
        let mut spec = BoneSpec {
            layer: self.rig_owner().context("no skeleton here")?,
            ..BoneSpec::default()
        };
        edit(&mut spec);
        self.apply(vec![Op::Bone(spec)])
    }

    fn nothing_follows(&self) -> bool {
        self.rig_owner()
            .is_some_and(|o| klad_core::rig::painted(&self.scene.bundle.doc, &o).is_empty())
    }

    fn pose_target(&self, owner: &str, frame: u32) -> std::result::Result<u32, String> {
        let doc = &self.scene.bundle.doc;
        let layers = klad_core::rig::painted(doc, owner);
        let on_rest = layers
            .iter()
            .any(|l| l.cels().iter().any(|c| c.frame == frame && c.pose.is_none()));
        if !on_rest {
            return Ok(frame);
        }
        let next = frame + 1;
        let taken = layers.iter().find(|l| {
            l.cels().iter().any(|c| {
                c.frame == next
                    && c.pose
                        .as_ref()
                        .is_none_or(|p| l.content_at(frame).is_none_or(|r| r.id != p.rest))
            })
        });
        match taken {
            Some(layer) => Err(format!(
                "this frame holds the drawing the bones are rigged to, and frame {next} has another drawing on {}: go to an empty frame to keep the pose",
                layer.name
            )),
            None => Ok(next),
        }
    }

    fn pose_drawing(&mut self) -> Result<()> {
        let owner = self.rig_owner().context("no skeleton here")?;
        if self.nothing_follows() {
            self.say(Tone::Refused, NOTHING_FOLLOWS);
            return Ok(());
        }
        let pose = self.shown_pose();
        if pose.angles.values().all(|v| v.abs() < 1e-3) && pose.root == [0.0, 0.0] && pose.ik.is_empty() {
            self.say(Tone::Refused, "turn a bone first: drag a bone's end in pose mode");
            return Ok(());
        }
        let frame = self.scene.frame;
        let to = match self.pose_target(&owner, frame) {
            Ok(to) => to,
            Err(why) => {
                self.say(Tone::Refused, why);
                return Ok(());
            }
        };
        if to >= self.scene.bundle.doc.timeline.frames {
            self.timeline_apply(vec![Op::Timeline {
                fps: None,
                frames: Some(to + 1),
                origin: None,
            }])?;
        }
        let mut args = pose_args(&self.scene.bundle.root, &owner, frame, &pose);
        if to != frame {
            if let Some(at) = args.iter().position(|a| a == "--to") {
                args[at + 1] = to.to_string();
            }
            self.say(
                Tone::Info,
                format!("frame {frame} holds the drawing the bones are rigged to, so the pose goes on frame {to}"),
            );
        }
        self.start_job(Spec {
            tag: Tag::Edit,
            name: format!("the pose on frame {to}"),
            exe: PathBuf::new(),
            args,
            env: Vec::new(),
            reruns: 3,
            summary: |_, _| String::new(),
            failure: |_, err| super::tweens::failure(err),
        });
        self.bones.sent = true;
        if to != frame {
            self.go_to(to)?;
        }
        Ok(())
    }

    fn ik_spec(&self, edit: impl FnOnce(&mut IkSpec)) -> Option<IkSpec> {
        let mut spec = IkSpec {
            layer: self.rig_owner()?,
            ..IkSpec::default()
        };
        edit(&mut spec);
        Some(spec)
    }

    fn ik_menu(&mut self, ui: &mut egui::Ui, picked: &str) -> Option<Later> {
        let (bones, ik) = (self.rig_bones(), self.rig_ik());
        let parent = bones.iter().find(|b| b.name == picked)?.parent.clone();
        let mut chosen: Option<IkSpec> = None;
        let name = picked.to_string();
        match ik.iter().find(|k| k.bone == picked) {
            Some(k) => {
                let label = match (k.chain, &parent) {
                    (2, Some(_)) => "IK: limb".to_string(),
                    _ => "IK: aim".to_string(),
                };
                ui.menu_button(label, |ui| {
                    if k.chain == 2 {
                        [
                            (Bend::Clockwise, BendSpec::Clockwise, "bends clockwise"),
                            (
                                Bend::Counterclockwise,
                                BendSpec::Counterclockwise,
                                "bends counterclockwise",
                            ),
                        ]
                        .into_iter()
                        .for_each(|(bend, spec, word)| {
                            if ui
                                .selectable_label(k.bend == bend, word)
                                .hint("which way the joint between the two bones bends on screen")
                                .clicked()
                                && k.bend != bend
                            {
                                chosen = self.ik_spec(|s| {
                                    s.set = Some(name.clone());
                                    s.bend = Some(spec);
                                });
                                ui.close();
                            }
                        });
                        let mut soft = k.softness;
                        let r = ui
                            .add(egui::Slider::new(&mut soft, 0.0..=100.0).text("softness"))
                            .hint("px: the limb slows this far before full reach, so it does not snap straight");
                        if (r.drag_stopped() || (r.changed() && !r.dragged())) && soft != k.softness {
                            chosen = self.ik_spec(|s| {
                                s.set = Some(name.clone());
                                s.softness = Some(soft);
                            });
                        }
                        if ui
                            .button("make it an aim")
                            .hint("turn this bone alone to point at the target")
                            .clicked()
                        {
                            chosen = self.ik_spec(|s| {
                                s.set = Some(name.clone());
                                s.chain = Some(1);
                            });
                            ui.close();
                        }
                    } else if let Some(p) = &parent
                        && ui
                            .button(format!("make it a limb with {p}"))
                            .hint("turn this bone and its parent so its end reaches the target")
                            .clicked()
                    {
                        chosen = self.ik_spec(|s| {
                            s.set = Some(name.clone());
                            s.chain = Some(2);
                        });
                        ui.close();
                    }
                    if ui.button("remove IK").hint("poses keep their look, as turns").clicked() {
                        chosen = self.ik_spec(|s| s.rm = Some(name.clone()));
                        ui.close();
                    }
                })
                .response
                .hint(match (k.chain, &parent) {
                    (2, Some(p)) => {
                        format!("this bone's IK, a limb with {p}: in pose mode, drag its end and the limb follows")
                    }
                    _ => "this bone's IK, an aim: in pose mode, drag its end and the bone points there".to_string(),
                });
            }
            None => {
                ui.menu_button("add IK", |ui| {
                    if let Some(p) = &parent
                        && ui
                            .button(format!("limb: {picked} and {p}"))
                            .hint(
                                "in pose mode, drag this bone's end and both bones turn to reach it, bending as drawn",
                            )
                            .clicked()
                    {
                        chosen = self.ik_spec(|s| {
                            s.add = Some(name.clone());
                            s.chain = Some(2);
                        });
                        ui.close();
                    }
                    if ui
                        .button(format!("aim: {picked}"))
                        .hint("in pose mode, drag this bone's end and it turns to point there")
                        .clicked()
                    {
                        chosen = self.ik_spec(|s| {
                            s.add = Some(name.clone());
                            s.chain = Some(1);
                        });
                        ui.close();
                    }
                })
                .response
                .hint("IK: pose this bone by where its end goes instead of by turning it");
            }
        }
        chosen.map(|spec| -> Later { Box::new(move |e| e.apply(vec![Op::Ik(spec)])) })
    }

    pub(super) fn bone_options(&mut self, ui: &mut egui::Ui) -> Result<()> {
        let mut run: Option<Later> = None;
        [
            (
                Mode::Draw,
                "draw",
                "drag on empty canvas to draw a chain of bones; drag a bone's end or middle to move it; alt drag from a bone's end for a child",
            ),
            (
                Mode::Weights,
                "weights",
                "paint the picked bone's weight; alt removes it; cmd click a bone to pick it",
            ),
            (
                Mode::Pose,
                "pose",
                "drag a bone's end to turn it, its children follow; drag an IK limb's end and the limb reaches for it",
            ),
        ]
        .iter()
        .for_each(|(mode, label, hint)| {
            if ui
                .selectable_label(self.bones.mode == *mode, *label)
                .hint(*hint)
                .clicked()
            {
                self.bones.mode = *mode;
                self.bones.posing = None;
            }
        });
        ui.label(RichText::new("preview").small().color(theme::WEAK))
            .hint(PREVIEW);
        if let Some(later) = self.skeleton_list(ui) {
            run = Some(later);
        }
        ui.separator();
        match self.bones.mode {
            Mode::Draw => {
                if let Some(picked) = self.bones.selected.clone() {
                    let label = ui.label("name");
                    let r = ui
                        .add(egui::TextEdit::singleline(&mut self.bones.name).desired_width(110.0))
                        .labelled_by(label.id)
                        .hint(match self.bones.chain {
                            Some(_) => "the picked bone's name; Return renames it",
                            None => "the picked bone's name; Return renames it. Drag its ends or middle to move it; alt drag from its end starts a chain from there; a drag on empty canvas starts a new chain",
                        });
                    if r.lost_focus() && self.bones.name.trim() != picked && !self.bones.name.trim().is_empty() {
                        let to = self.bones.name.trim().to_string();
                        run = Some(Box::new(move |e| {
                            e.bone_edit(|s| {
                                s.rename = Some(picked);
                                s.to_name = Some(to.clone());
                            })?;
                            e.pick_bone(Some(to));
                            Ok(())
                        }));
                    } else if let Some(later) = self.parent_menu(ui, &picked) {
                        run = Some(later);
                    } else if let Some(later) = self.ik_menu(ui, &picked) {
                        run = Some(later);
                    } else if let Some(later) = self.attach_menu(ui, &picked) {
                        run = Some(later);
                    } else if ui
                        .button(theme::icon::TRASH)
                        .hint("remove the picked bone and its children")
                        .clicked()
                    {
                        run = Some(Box::new(move |e| {
                            e.end_chain();
                            e.bone_edit(|s| s.rm = Some(picked))?;
                            e.pick_bone(None);
                            Ok(())
                        }));
                    }
                }
                let said = match (&self.bones.chain, &self.bones.selected) {
                    (Some(_), _) => Some((
                        "click: the next bone; Esc: done",
                        "each click places the next bone of the chain; shift as you let go starts the next one from the same joint; Esc, Enter, a right click or a click on the chain's open end ends it",
                    )),
                    (None, Some(_)) => None,
                    (None, None) => Some((
                        "drag to draw a chain of bones",
                        "a drag draws a bone and the chain carries on from its end: each click adds the next; click a bone to pick it, then its layers menu attaches the layers that move with it",
                    )),
                };
                if let Some((hint, more)) = said {
                    let room = ui.available_size_before_wrap().x;
                    if room > 48.0 {
                        ui.add_sized(
                            [room, ui.spacing().interact_size.y],
                            egui::Label::new(RichText::new(hint).color(theme::WEAK)).truncate(),
                        )
                        .hint(more);
                    }
                }
            }
            Mode::Weights if !self.bends() => {
                super::options::note(ui, RichText::new("every layer follows a bone: nothing bends by weights").color(theme::WEAK))
                    .hint("weights are for a layer drawn in one piece across a joint; detach one (draw mode, the picked bone's layers menu, or the bone in its layers row) to paint its weights");
            }
            Mode::Weights => {
                use klad_core::rig::Mode as W;
                [
                    (
                        W::Paint,
                        icon::BRUSH,
                        "paint toward the weight (shift 1)",
                        crate::commands::Cmd::WeightPaint,
                    ),
                    (
                        W::Add,
                        icon::PLUS,
                        "add weight; alt takes it away (shift 2)",
                        crate::commands::Cmd::WeightAdd,
                    ),
                    (
                        W::Remove,
                        icon::MINUS,
                        "take weight away; alt adds it (shift 3)",
                        crate::commands::Cmd::WeightRemove,
                    ),
                    (
                        W::Smooth,
                        icon::DROP,
                        "smooth the weights under the brush (shift 4)",
                        crate::commands::Cmd::WeightSmooth,
                    ),
                ]
                .into_iter()
                .for_each(|(mode, glyph, hint, cmd)| {
                    let button = egui::Button::selectable(self.bones.wmode == mode, glyph)
                        .min_size(egui::vec2(theme::CONTROL, theme::CONTROL));
                    if ui.add(button).hint(format!("{}: {hint}", mode.name())).clicked() {
                        self.queued.push(cmd);
                    }
                });
                ui.spacing_mut().slider_width = 48.0;
                super::options::whole(ui, "weights size", |ui| {
                    crate::widgets::labelled_slider(
                        ui,
                        "size",
                        egui::Slider::new(&mut self.bones.radius, 4.0..=200.0).logarithmic(true),
                    )
                })
                .hint("the weight brush's radius in px");
                super::options::whole(ui, "weights weight", |ui| {
                    crate::widgets::labelled_slider(
                        ui,
                        "weight",
                        egui::Slider::new(&mut self.bones.strength, 0.0..=1.0).fixed_decimals(2),
                    )
                })
                .hint(
                    "the weight a dab paints toward, or adds and takes away (alt I picks it up from under the pointer)",
                );
                theme::held_menu(ui, icon::MORE, |ui| {
                    crate::widgets::labelled_slider(
                        ui,
                        "strength",
                        egui::Slider::new(&mut self.bones.flow, 0.0..=1.0).fixed_decimals(2),
                    )
                    .hint("how far one dab goes toward its weight: 0.1 builds it up over strokes, 1 sets it at once; the pen's pressure scales it");
                    crate::widgets::labelled_slider(
                        ui,
                        "hardness",
                        egui::Slider::new(&mut self.bones.hardness, 0.0..=1.0).fixed_decimals(2),
                    )
                    .hint("0: a soft edge all the way in; 1: full strength to the edge");
                    ui.checkbox(&mut self.bones.pressure, "pressure sets the strength")
                        .hint("off: every dab at full strength, whatever the pen");
                    weight_legend(ui);
                    if ui
                        .checkbox(&mut self.bones.all, "show every bone's weight")
                        .hint("each bone in its own colour; off shows the picked bone's alone")
                        .changed()
                    {
                        self.bones.heat = None;
                    }
                    ui.separator();
                    if ui
                        .button("automatic")
                        .hint("automatic weights: start again from the bones, by distance")
                        .clicked()
                    {
                        run = Some(Box::new(|e| {
                            e.paint_weights(WeightsSpec {
                                layer: e.rig_owner().context("no skeleton here")?,
                                frame: e.scene.frame,
                                auto: true,
                                ..WeightsSpec::default()
                            })
                        }));
                        ui.close();
                    }
                    if ui
                        .button("smooth all")
                        .hint("blur every bone's whole weight map a little")
                        .clicked()
                    {
                        run = Some(Box::new(|e| {
                            e.paint_weights(WeightsSpec {
                                layer: e.rig_owner().context("no skeleton here")?,
                                frame: e.scene.frame,
                                smooth: true,
                                ..WeightsSpec::default()
                            })
                        }));
                        ui.close();
                    }
                })
                .response
                .hint("more: strength, hardness, pressure, the legend, every bone's colours, automatic weights and smooth all");
                if self.bones.selected.is_none() {
                    ui.label(RichText::new("pick a bone: cmd click it, or pick it in the list").color(theme::WEAK));
                }
            }
            Mode::Pose => {
                if ui
                    .button("make this pose a drawing")
                    .hint("bends the drawing here (or its rest drawing) into this pose, as klad pose")
                    .clicked()
                {
                    run = Some(Box::new(|e| e.pose_drawing()));
                }
                if ui
                    .button("back to rest")
                    .hint("forget the turns, the move and the targets")
                    .clicked()
                {
                    self.bones.pose.clear();
                    self.bones.root = [0.0, 0.0];
                    self.bones.reach.clear();
                }
                let (bones, ik) = (self.rig_bones(), self.rig_ik());
                let held = self
                    .bones
                    .selected
                    .as_ref()
                    .and_then(|p| ik.iter().find(|k| limb_of(&bones, k).contains(p)))
                    .map(|k| k.bone.clone());
                let how = "drag a bone's end to turn it; drag the end of an IK limb and the limb reaches for it; drag a root bone's head to move the whole while IK ends stay; alt drag turns a bone by hand, taking its limb off IK";
                match held {
                    Some(end) => {
                        let mut on = self.bones.reach.contains_key(&end);
                        if ui
                            .checkbox(&mut on, "IK")
                            .hint(format!(
                                "on: {end}'s limb reaches for its target; off: turn it by hand, as it is now. {how}"
                            ))
                            .changed()
                        {
                            match on {
                                true => {
                                    self.ik_on(&end);
                                }
                                false => self.ik_off(&end),
                            }
                        }
                        if let Some(r) = self.bones.reach.get_mut(&end) {
                            ui.spacing_mut().slider_width = 56.0;
                            crate::widgets::labelled_slider(ui, "mix", egui::Slider::new(&mut r.mix, 0.0..=1.0))
                                .hint("0: the limb's own turns; 1: it reaches the target");
                        }
                    }
                    None => {
                        ui.label(RichText::new("drag an end; alt: turn by hand").color(theme::WEAK))
                            .hint(how);
                    }
                }
            }
        }
        if let Some(f) = run {
            f(self)?;
        }
        Ok(())
    }

    fn attach_menu(&mut self, ui: &mut egui::Ui, picked: &str) -> Option<Later> {
        let owner = self.rig_owner()?;
        let doc = &self.scene.bundle.doc;
        let masks: Vec<&str> = doc
            .all_layers()
            .iter()
            .filter_map(|l| l.mask.as_deref().map(|m| m.id.as_str()))
            .collect();
        let layers: Vec<(String, String, Option<klad_core::rig::Link>)> = doc
            .all_layers()
            .iter()
            .filter(|l| l.kind() == Kind::Paint && !masks.contains(&l.id.as_str()))
            .map(|l| (l.id.clone(), l.name.clone(), l.rig.clone()))
            .collect();
        let mine = |link: &Option<klad_core::rig::Link>| {
            link.as_ref()
                .is_some_and(|k| k.skeleton == owner && k.bone.as_deref() == Some(picked))
        };
        let on = layers.iter().filter(|l| mine(&l.2)).count();
        let mut chosen = None;
        theme::held_menu(ui, format!("layers: {on}"), |ui| {
            layers.iter().for_each(|(id, name, link)| {
                let mut checked = mine(link);
                let label = match link.as_ref().filter(|_| !checked) {
                    Some(klad_core::rig::Link { bone: Some(b), .. }) => format!("{name} (follows {b})"),
                    Some(_) => format!("{name} (bends)"),
                    None => name.clone(),
                };
                if ui.checkbox(&mut checked, label).changed() {
                    chosen = Some((id.clone(), checked));
                }
            });
        })
        .response
        .hint(format!(
            "the layers that move with {picked} alone when posed; a layer's bone button in the layers panel can also make it bend with the skeleton"
        ));
        let (skeleton, bone) = (owner.clone(), picked.to_string());
        chosen.map(|(layer, on)| -> Later {
            Box::new(move |e| {
                e.apply(vec![Op::Attach(klad_core::ops::AttachSpec {
                    layer,
                    skeleton: on.then_some(skeleton),
                    bone: on.then_some(bone),
                })])
            })
        })
    }

    fn skeleton_list(&mut self, ui: &mut egui::Ui) -> Option<Later> {
        let bones = self.rig_bones();
        let doc = &self.scene.bundle.doc;
        if bones.is_empty() {
            match self.rig_owner().and_then(|o| doc.layer(&o)) {
                Some(skeleton) => ui
                    .label(RichText::new(format!("{} has no bones yet: drag on the canvas", skeleton.name)).color(theme::WEAK)),
                None => ui
                    .label(RichText::new("no bones layer yet: drawing a bone makes one").color(theme::WEAK))
                    .hint("a bones layer holds a skeleton and draws nothing; every paint layer can follow one of its bones, or bend with it"),
            };
            return None;
        }
        let owner = self
            .rig_owner()
            .and_then(|o| doc.layer(&o).map(|l| l.name.clone()))
            .unwrap_or_default();
        let picked = self
            .bones
            .selected
            .clone()
            .filter(|p| bones.iter().any(|b| &b.name == p));
        let depth = |b: &Bone| {
            std::iter::successors(b.parent.clone(), |p| {
                bones.iter().find(|o| &o.name == p).and_then(|o| o.parent.clone())
            })
            .count()
        };
        let mut chosen = None;
        ui.menu_button(
            format!("{} {}", theme::icon::BONE, picked.as_deref().unwrap_or("pick a bone")),
            |ui| {
                ui.label(
                    RichText::new(format!("the bones of {owner}"))
                        .color(theme::WEAK)
                        .small(),
                );
                bones.iter().for_each(|b| {
                    ui.horizontal(|ui| {
                        ui.add_space(14.0 * depth(b) as f32);
                        if ui
                            .selectable_label(picked.as_ref() == Some(&b.name), &b.name)
                            .hint(match &b.parent {
                                Some(p) => format!("pick {}, a child of {p}", b.name),
                                None => format!("pick {}, a root bone", b.name),
                            })
                            .clicked()
                        {
                            chosen = Some(b.name.clone());
                            ui.close();
                        }
                    });
                });
            },
        )
        .response
        .hint(format!("the skeleton of {owner}, each bone under its parent: pick one"));
        chosen.map(|name| -> Later {
            Box::new(move |e| {
                e.end_chain();
                e.pick_bone(Some(name));
                Ok(())
            })
        })
    }

    fn parent_menu(&mut self, ui: &mut egui::Ui, picked: &str) -> Option<Later> {
        let bones = self.rig_bones();
        let current = bones.iter().find(|b| b.name == picked)?.parent.clone();
        let below = subtree(&bones, picked);
        let mut chosen: Option<Option<String>> = None;
        let label = ui.label("parent");
        egui::ComboBox::from_id_salt("bones.parent")
            .width(90.0)
            .selected_text(current.as_deref().unwrap_or("none"))
            .show_ui(ui, |ui| {
                std::iter::once(None)
                    .chain(
                        bones
                            .iter()
                            .filter(|b| !below.contains(&b.name))
                            .map(|b| Some(b.name.clone())),
                    )
                    .for_each(|p| {
                        if ui
                            .selectable_label(p == current, p.as_deref().unwrap_or("none"))
                            .clicked()
                            && p != current
                        {
                            chosen = Some(p);
                        }
                    });
            })
            .response
            .labelled_by(label.id)
            .hint("the bone this one hangs from; none makes it a root");
        let name = picked.to_string();
        chosen.map(|parent| -> Later {
            Box::new(move |e| {
                e.bone_edit(|s| {
                    s.reparent = Some(name);
                    s.parent = parent;
                })
            })
        })
    }

    fn heat_texture(&mut self, bone: &str) -> Option<egui::TextureHandle> {
        let rev = self.scene.bundle.doc.rev;
        let key = match self.bones.all {
            true => "*".to_string(),
            false => bone.to_string(),
        };
        if let Some((b, r, t)) = &self.bones.heat
            && *b == key
            && *r == rev
        {
            return Some(t.clone());
        }
        let bones = self.rig_bones();
        let i = bones.iter().position(|b| b.name == bone)?;
        let weights = self.weights_read()?;
        let (w, h) = (weights.width as usize, weights.height as usize);
        let all = self.bones.all;
        let pixels: Vec<Color32> = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .map(|(x, y)| {
                let p = [(x as f32 + 0.5) * CELL as f32, (y as f32 + 0.5) * CELL as f32];
                let used = weights.at(&bones, p);
                match all {
                    false => heat(used[i]),
                    true => {
                        let [r, g, b] = used.iter().enumerate().fold([0.0f32; 3], |acc, (k, v)| {
                            let c = bone_colour(k);
                            [acc[0] + c[0] * v, acc[1] + c[1] * v, acc[2] + c[2] * v]
                        });
                        Color32::from_rgba_unmultiplied(r as u8, g as u8, b as u8, 140)
                    }
                }
            })
            .collect();
        let image = egui::ColorImage::new([w, h], pixels);
        let texture = self
            .ctx
            .load_texture("bone weights", image, egui::TextureOptions::LINEAR);
        self.bones.heat = Some((key, rev, texture.clone()));
        Some(texture)
    }

    pub(super) fn decorate_bones(
        &mut self,
        painter: &egui::Painter,
        place: impl Fn(Vec2) -> Pos2,
        hover: Option<Vec2>,
    ) {
        if self.tool != super::Tool::Bone {
            return;
        }
        if self.bones.mode == Mode::Weights
            && let Some(bone) = self.bones.selected.clone()
            && let Some(texture) = self.heat_texture(&bone)
        {
            let canvas = self.canvas_size();
            let rect = egui::Rect::from_two_pos(place(Vec2::ZERO), place(canvas));
            painter.image(
                texture.id(),
                rect,
                egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        if self.bones.mode == Mode::Weights
            && let Some(p) = hover
            && let Some((painted, used)) = self.weight_under(p)
        {
            let at = place(p) + egui::vec2(16.0, -18.0);
            let text = match (painted - used).abs() < 0.005 {
                true => format!("{used:.2}"),
                false => format!("{used:.2} (painted {painted:.2})"),
            };
            let font = theme::regular(theme::BODY);
            let galley = painter.layout_no_wrap(text, font, Color32::WHITE);
            let back = egui::Rect::from_min_size(at, galley.size()).expand(3.0);
            painter.rect_filled(back, 3.0, Color32::from_black_alpha(170));
            painter.galley(at, galley, Color32::WHITE);
        }
        let bones = self.drawn_bones();
        let ik = self.rig_ik();
        let pose = self.shown_pose();
        let shown = tips(&bones, &ik, &pose);
        let editing = self.bones.mode == Mode::Draw;
        let held: Vec<String> = ik
            .iter()
            .filter(|k| pose.ik.get(&k.bone).is_some_and(|r| r.mix > 0.0))
            .flat_map(|k| limb_of(&bones, k))
            .collect();
        bones.iter().zip(&shown).for_each(|(b, (h, t))| {
            let (a, z) = (place(at(*h)), place(at(*t)));
            let picked = self.bones.selected.as_ref() == Some(&b.name);
            let colour = if picked {
                theme::MARK
            } else {
                Color32::from_rgb(120, 200, 240)
            };
            painter.line_segment([a, z], egui::Stroke::new(6.0, Color32::from_black_alpha(150)));
            painter.line_segment([a, z], egui::Stroke::new(3.0, colour));
            if held.contains(&b.name) {
                painter.line_segment([a, z], egui::Stroke::new(1.2, Color32::from_black_alpha(200)));
            }
            painter.circle_filled(a, 4.5, colour);
            painter.circle_stroke(z, 4.0, egui::Stroke::new(1.5, colour));
            if ik.iter().any(|k| k.bone == b.name) {
                painter.rect_stroke(
                    egui::Rect::from_center_size(z, egui::vec2(11.0, 11.0)),
                    1.0,
                    egui::Stroke::new(1.5, TARGET),
                    egui::StrokeKind::Middle,
                );
            }
            if picked && editing {
                [a, z].into_iter().for_each(|p| {
                    painter.circle_filled(p, 6.5, Color32::WHITE);
                    painter.circle_stroke(p, 6.5, egui::Stroke::new(2.0, theme::MARK));
                });
            }
            painter.text(
                a + (z - a) * 0.5 + egui::vec2(6.0, -6.0),
                egui::Align2::LEFT_BOTTOM,
                &b.name,
                theme::regular(theme::SMALL),
                colour,
            );
        });
        if let Some(Drag {
            from,
            to,
            what: Dragging::New(_),
        }) = &self.bones.drag
        {
            painter.line_segment([place(*from), place(*to)], egui::Stroke::new(2.0, theme::MARK));
        }
        if editing && let Some(chain) = &self.bones.chain {
            let (a, z) = (place(at(chain.head)), place(chain.to));
            painter.extend(egui::Shape::dashed_line(
                &[a, z],
                egui::Stroke::new(2.0, theme::MARK),
                6.0,
                4.0,
            ));
            painter.circle_stroke(a, 6.5, egui::Stroke::new(1.5, theme::MARK));
        }
        if self.bones.mode == Mode::Pose {
            ik.iter().for_each(|k| {
                let (Some(r), Some(i)) = (pose.ik.get(&k.bone), bones.iter().position(|b| b.name == k.bone)) else {
                    return;
                };
                let (end, target) = (place(at(shown[i].1)), place(at(r.target)));
                if (end - target).length() > 2.0 {
                    painter.extend(egui::Shape::dashed_line(
                        &[end, target],
                        egui::Stroke::new(1.5, TARGET),
                        4.0,
                        3.0,
                    ));
                }
                let stroke = egui::Stroke::new(2.0, TARGET);
                painter.circle_stroke(target, 7.0, egui::Stroke::new(4.0, Color32::from_black_alpha(150)));
                painter.circle_stroke(target, 7.0, stroke);
                painter.line_segment([target - egui::vec2(11.0, 0.0), target + egui::vec2(11.0, 0.0)], stroke);
                painter.line_segment([target - egui::vec2(0.0, 11.0), target + egui::vec2(0.0, 11.0)], stroke);
            });
        }
        if editing
            && self.bones.drag.is_none()
            && self.bones.chain.is_none()
            && let Some(p) = hover
        {
            let cursor = match (self.modifiers.alt, self.grip_at(p)) {
                (true, _) => None,
                (false, Some((_, _, Grip::Whole))) => Some(egui::CursorIcon::Move),
                (false, Some(_)) => Some(egui::CursorIcon::Grab),
                (false, None) => None,
            };
            if let Some(c) = cursor {
                self.ctx.set_cursor_icon(c);
            }
        }
        if self.bones.mode == Mode::Weights
            && let Some(p) = hover
        {
            let r = self.bones.radius * self.view.map_or(1.0, |v| v.zoom);
            painter.circle_stroke(place(p), r, egui::Stroke::new(1.0, theme::MARK));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pose_from_the_editor_is_the_cli_pose() {
        let mut pose = Pose {
            angles: BTreeMap::from([("fore".to_string(), -45.0), ("upper".to_string(), 0.0)]),
            ..Pose::default()
        };
        assert_eq!(
            pose_args(std::path::Path::new("/w/a.klad"), "l1", 4, &pose).join(" "),
            "pose /w/a.klad l1 --from 4 --to 4 --bone fore=-45.00"
        );
        pose.root = [-3.0, 6.5];
        pose.ik.insert(
            "shin".into(),
            Reach {
                target: [410.0, 742.25],
                mix: 1.0,
            },
        );
        pose.ik.insert(
            "hand".into(),
            Reach {
                target: [-5.0, 20.0],
                mix: 0.25,
            },
        );
        assert_eq!(
            pose_args(std::path::Path::new("/w/a.klad"), "l1", 4, &pose).join(" "),
            "pose /w/a.klad l1 --from 4 --to 4 --bone fore=-45.00 --root -3.00,6.50 --target hand=-5.00,20.00 --mix hand=0.250 --target shin=410.00,742.25"
        );
    }
}
