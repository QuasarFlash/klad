use crate::bundle::Bundle;
use crate::doc::{Blend, Kind, Layer, Loc};
use crate::ops::{CopySpec, CopyTarget, Op, Place};
use crate::stroke::Mark;
use crate::timing::{CelAction, CelSpec};
use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub struct Merged {
    pub ops: Vec<Op>,
    pub into: String,
    pub count: usize,
    pub baked: Vec<String>,
}

fn strip(value: &mut Value) {
    if let Some(map) = value.as_object_mut() {
        map.remove("id");
        if let Some(children) = map.get_mut("children").and_then(Value::as_array_mut) {
            children.iter_mut().for_each(strip);
        }
    }
}

fn plain(layer: &Layer, what: &str) -> Result<()> {
    let name = &layer.name;
    if !layer.visible {
        bail!("{name} is hidden: show it first, or leave it out of the merge");
    }
    if layer.blend != Blend::Normal {
        bail!(
            "{name} mixes with what is under it ({}): set it to normal first, or use Merge visible, which makes one picture",
            serde_json::to_value(layer.blend)
                .ok()
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_default()
        );
    }
    if layer.mask.is_some() {
        bail!("{name} has a mask: apply or remove it first, or use Merge visible, which makes one picture");
    }
    if layer.clip {
        bail!("{name} is clipped to the layer below {what}: unclip it first, or use Merge visible");
    }
    if !layer.effects.is_empty() {
        bail!("{name} has effects: they would be lost; use Merge visible, which makes one picture");
    }
    if !layer.keys.is_empty() {
        bail!("{name} has keys that animate it: merging would lose them");
    }
    Ok(())
}

fn modifiers(bundle: &Bundle, layer: &Layer, marks: &[Mark]) -> Result<()> {
    let loose = marks.iter().filter_map(Mark::as_stroke).any(|s| {
        s.on.is_empty()
            && bundle
                .brush(&s.brush)
                .ok()
                .flatten()
                .or_else(|| crate::brushes::library(&s.brush))
                .is_some_and(|def| crate::attach::modifier(&def).is_some())
    });
    match loose {
        true => bail!(
            "{} has eraser, smudge or blender strokes that are attached to nothing: merged, they would work on the layer below too. Detach or delete them first, or use Merge visible",
            layer.name
        ),
        false => Ok(()),
    }
}

pub fn merge(bundle: &Bundle, keys: &[String]) -> Result<Merged> {
    let doc = &bundle.doc;
    let mut layers: Vec<(Vec<usize>, &Layer)> = keys
        .iter()
        .map(|key| {
            let layer = doc.find_layer(key)?;
            match doc.locate(&layer.id) {
                Some(Loc::Layer(path)) => Ok((path, layer)),
                _ => bail!("{key} is not a layer"),
            }
        })
        .collect::<Result<_>>()?;
    layers.sort_by(|a, b| a.0.cmp(&b.0));
    layers.dedup_by(|a, b| a.1.id == b.1.id);
    if layers.len() < 2 {
        bail!("give two layers or more to merge");
    }
    let parent = &layers[0].0[..layers[0].0.len() - 1];
    if layers.iter().any(|(p, _)| &p[..p.len() - 1] != parent) {
        bail!("the layers are in different groups: move them into one group first, or use Merge visible");
    }
    let (_, target) = layers[0];
    let kind = target.kind();
    if !matches!(kind, Kind::Paint | Kind::Vector) {
        bail!(
            "{} is a {} layer: merge takes paint, pixel and vector layers",
            target.name,
            kind.name()
        );
    }
    if let Some((_, other)) = layers
        .iter()
        .find(|(_, l)| l.kind() != kind || l.pixel() != target.pixel())
    {
        let word = |l: &Layer| match (l.kind(), l.pixel()) {
            (Kind::Paint, true) => "pixel".to_string(),
            (k, _) => k.name().to_string(),
        };
        bail!(
            "{} is a {} layer and {} a {} layer: layers merge with their own kind, so the strokes, pixels or shapes stay editable. Merge visible makes one picture of different kinds",
            other.name,
            word(other),
            target.name,
            word(target)
        );
    }
    plain(target, "it")?;
    if target.opacity < 1.0 {
        bail!(
            "{} is the bottom layer and is see-through ({:.0}%): set it to 100% first, so what merges into it keeps its look",
            target.name,
            target.opacity * 100.0
        );
    }
    let uppers: Vec<&Layer> = layers[1..].iter().map(|(_, l)| *l).collect();
    uppers.iter().try_for_each(|l| plain(l, "it"))?;
    if let Some(other) = uppers
        .iter()
        .find(|l| l.rig != target.rig || l.depth != target.depth || l.z != target.z)
    {
        bail!(
            "{} moves differently from {} (its bones, depth or parallax): merged, it would move with {}",
            other.name,
            target.name,
            target.name
        );
    }
    let frames = doc.timeline.frames;
    let keys: BTreeSet<u32> = layers
        .iter()
        .flat_map(|(_, l)| l.cels().iter().map(|c| c.frame))
        .filter(|f| *f < frames)
        .collect();
    let mut ops = Vec::new();
    let mut baked = Vec::new();
    match kind {
        Kind::Paint => uppers.iter().try_for_each(|upper| {
            if upper.opacity < 1.0 {
                baked.push(upper.name.clone());
            }
            keys.iter().try_for_each(|&k| {
                let Some(cel) = upper.content_at(k) else {
                    return Ok::<_, anyhow::Error>(());
                };
                let marks = bundle.read_marks(&upper.id, &cel.id)?;
                modifiers(bundle, upper, &marks)?;
                if marks.is_empty() {
                    return Ok(());
                }
                let held = marks
                    .into_iter()
                    .map(|mut m| {
                        match &mut m {
                            Mark::Stroke(s) => s.opacity *= upper.opacity,
                            Mark::Fill(f) => f.opacity *= upper.opacity,
                        }
                        (cel.id.clone(), m)
                    })
                    .collect();
                ops.push(Op::Copy(CopySpec {
                    ids: Vec::new(),
                    held,
                    to: CopyTarget::Frame {
                        frame: k,
                        layer: Some(target.id.clone()),
                    },
                    transform: None,
                    cut: None,
                }));
                Ok(())
            })
        })?,
        _ => {
            let mut started: BTreeSet<u32> = target.cels().iter().map(|c| c.frame).collect();
            uppers.iter().try_for_each(|upper| {
                keys.iter().try_for_each(|&k| {
                    let Some(cel) = upper.content_at(k).filter(|c| !c.nodes.is_empty()) else {
                        return Ok::<_, anyhow::Error>(());
                    };
                    if !started.contains(&k) {
                        ops.push(match target.cel_at(k) {
                            Some(held) => Op::Cel(CelSpec {
                                action: CelAction::Dup { cel: held.id.clone() },
                                id: None,
                                frame: Some(k),
                                hold: None,
                                insert: false,
                            }),
                            None => Op::AddCel {
                                cel: json!({"frame": k}),
                                layer: target.id.clone(),
                            },
                        });
                        started.insert(k);
                    }
                    let mut nodes: Vec<Value> = cel
                        .nodes
                        .iter()
                        .map(|n| serde_json::to_value(n).map_err(|e| anyhow!("{e}")))
                        .collect::<Result<_>>()?;
                    nodes.iter_mut().for_each(strip);
                    let place = Place {
                        into: Some(target.id.clone()),
                        frame: Some(k),
                        ..Place::default()
                    };
                    match upper.opacity < 1.0 {
                        true => ops.push(Op::AddNode {
                            node: json!({"type": "group", "name": upper.name, "opacity": upper.opacity, "children": nodes}),
                            place,
                        }),
                        false => ops.extend(nodes.into_iter().map(|node| Op::AddNode {
                            node,
                            place: place.clone(),
                        })),
                    }
                    Ok(())
                })
            })?
        }
    }
    ops.push(Op::Remove {
        ids: uppers.iter().map(|l| l.id.clone()).collect(),
    });
    Ok(Merged {
        ops,
        into: target.name.clone(),
        count: layers.len(),
        baked,
    })
}

pub fn below(bundle: &Bundle, key: &str) -> Result<String> {
    let doc = &bundle.doc;
    let layer = doc.find_layer(key)?;
    let Some(Loc::Layer(mut path)) = doc.locate(&layer.id) else {
        bail!("{key} is not a layer");
    };
    let index = path.pop().unwrap_or_default();
    if index == 0 {
        bail!(
            "{} is the bottom layer{}: there is nothing under it to merge into",
            layer.name,
            if path.is_empty() { "" } else { " of its group" }
        );
    }
    let siblings = match path.is_empty() {
        true => &doc.layers,
        false => doc.layer_at(&path).children(),
    };
    Ok(siblings[index - 1].id.clone())
}

pub fn ungroup(doc: &crate::doc::Document, key: &str) -> Result<(Vec<Op>, Vec<String>)> {
    let group = doc.find_layer(key)?;
    if group.kind() != Kind::Group {
        bail!("{} is a {} layer, not a group", group.name, group.kind().name());
    }
    let mut lost = Vec::new();
    if group.blend != Blend::Normal {
        lost.push("its blend mode");
    }
    if group.mask.is_some() {
        lost.push("its mask");
    }
    if !group.effects.is_empty() {
        lost.push("its effects");
    }
    let moves = group.children().iter().map(|child| Op::Move {
        id: child.id.clone(),
        place: Place {
            below: Some(group.id.clone()),
            ..Place::default()
        },
    });
    let sets = group.children().iter().filter_map(|child| {
        let mut props = serde_json::Map::new();
        if group.opacity < 1.0 {
            props.insert("opacity".into(), json!(child.opacity * group.opacity));
        }
        if !group.visible && child.visible {
            props.insert("visible".into(), json!(false));
        }
        (!props.is_empty()).then(|| Op::Set {
            id: child.id.clone(),
            props,
        })
    });
    let ops = moves
        .chain(sets)
        .chain(std::iter::once(Op::Remove {
            ids: vec![group.id.clone()],
        }))
        .collect();
    Ok((ops, lost.into_iter().map(str::to_string).collect()))
}
