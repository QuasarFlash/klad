use super::Editor;
use super::layers::{LayerRow, Where};
use crate::commands::Cmd;
use crate::messages::Tone;
use anyhow::Result;
use klad_core::doc::{Kind, Loc};
use klad_core::ops::{Op, Place};
use serde_json::json;

const MENU: [Option<Cmd>; 25] = [
    Some(Cmd::RenameLayer),
    Some(Cmd::DuplicateLayer),
    Some(Cmd::DeleteLayer),
    None,
    Some(Cmd::NewPaintLayer),
    Some(Cmd::NewPixelLayer),
    Some(Cmd::NewVectorLayer),
    Some(Cmd::NewGroupLayer),
    Some(Cmd::GroupLayer),
    Some(Cmd::Ungroup),
    None,
    Some(Cmd::MergeDown),
    Some(Cmd::MergeSelected),
    None,
    Some(Cmd::ToggleVisible),
    Some(Cmd::SoloLayer),
    Some(Cmd::ToggleLocked),
    Some(Cmd::ToggleLockAlpha),
    Some(Cmd::ToggleClip),
    Some(Cmd::AddMask),
    None,
    Some(Cmd::InspectorPane),
    Some(Cmd::SelectAll),
    Some(Cmd::Paste),
    Some(Cmd::ExportLayer),
];

pub(super) fn menu_for(kind: Kind, pixel: bool) -> Vec<Option<Cmd>> {
    let extra: Vec<Cmd> = match (kind, pixel) {
        (Kind::Paint, true) => vec![Cmd::PixelOutline, Cmd::Pixelize],
        (Kind::Paint, false) => vec![Cmd::MakeMapLayer, Cmd::MakeColourLayer, Cmd::PaintOver],
        (Kind::Vector, _) => vec![Cmd::MakeMapLayer],
        _ => Vec::new(),
    };
    let mut items: Vec<Option<Cmd>> = MENU
        .iter()
        .copied()
        .filter(|c| match (c, kind) {
            (Some(Cmd::Ungroup), k) => k == Kind::Group,
            (Some(Cmd::ToggleLockAlpha), k) => k == Kind::Paint,
            (Some(Cmd::SelectAll | Cmd::Paste), k) => matches!(k, Kind::Paint | Kind::Vector),
            _ => true,
        })
        .collect();
    if !extra.is_empty() {
        items.push(None);
        items.extend(extra.into_iter().map(Some));
    }
    items
}

impl Editor {
    pub(super) fn layer_menu(&mut self, ui: &mut egui::Ui, row: &LayerRow) {
        ui.label(egui::RichText::new(&row.name).strong());
        menu_for(row.kind, row.pixel).into_iter().for_each(|item| match item {
            Some(cmd) => self.menu_entry(ui, cmd),
            None => {
                ui.separator();
            }
        });
    }

    pub(super) fn chosen_layers(&self) -> Vec<String> {
        let active: Vec<String> = self.scene.active_id().into_iter().collect();
        let more: Vec<String> = self.also.iter().filter(|id| !active.contains(id)).cloned().collect();
        active.into_iter().chain(more).collect()
    }

    fn named(&self, ids: &[String]) -> String {
        match ids {
            [one] => self.scene.bundle.doc.layer(one).map_or(one.clone(), |l| l.name.clone()),
            many => format!("{} layers", many.len()),
        }
    }

    pub(super) fn merge_layers(&mut self, ids: Vec<String>) -> Result<()> {
        let merged = match klad_core::merge::merge(&self.scene.bundle, &ids) {
            Ok(merged) => merged,
            Err(e) => {
                self.say(Tone::Refused, format!("{e:#}"));
                return Ok(());
            }
        };
        let into = merged.into.clone();
        if self.timeline_apply(merged.ops)?.is_none() {
            return Ok(());
        }
        self.also.clear();
        if let Some(Loc::Layer(path)) = self
            .scene
            .bundle
            .doc
            .find_layer(&into)
            .ok()
            .and_then(|l| self.scene.bundle.doc.locate(&l.id))
        {
            self.activate(path)?;
        }
        let baked = match merged.baked.is_empty() {
            true => String::new(),
            false => format!(
                "; the opacity of {} went onto each of its strokes",
                merged.baked.join(", ")
            ),
        };
        self.say(
            Tone::Done,
            format!(
                "merged {} layers into {into}{baked} (cmd Z takes it back)",
                merged.count
            ),
        );
        Ok(())
    }

    pub(super) fn merge_down(&mut self) -> Result<()> {
        let Some(id) = self.scene.active_id() else {
            return Ok(());
        };
        match klad_core::merge::below(&self.scene.bundle, &id) {
            Ok(under) => self.merge_layers(vec![under, id]),
            Err(e) => {
                self.say(Tone::Refused, format!("{e:#}"));
                Ok(())
            }
        }
    }

    pub(super) fn merge_visible(&mut self) -> Result<()> {
        let doc = &self.scene.bundle.doc;
        let ids: Vec<String> = doc
            .all_layers()
            .iter()
            .filter(|l| l.visible && !l.reference && matches!(l.kind(), Kind::Paint | Kind::Vector))
            .map(|l| l.id.clone())
            .collect();
        if klad_core::merge::merge(&self.scene.bundle, &ids).is_ok() {
            return self.merge_layers(ids);
        }
        let bundle = self.scene.bundle.root.to_string_lossy().to_string();
        self.start_job(crate::jobs::Spec {
            tag: crate::jobs::Tag::Edit,
            name: "merge the visible layers".to_string(),
            exe: std::path::PathBuf::new(),
            args: vec!["merge".to_string(), bundle, "--visible".to_string()],
            env: Vec::new(),
            reruns: 1,
            summary: |_, err| match err.contains("one picture") {
                true => {
                    "the visible layers are one picture now, on a layer named merged (cmd Z takes it back)".to_string()
                }
                false => "merged the visible layers (cmd Z takes it back)".to_string(),
            },
            failure: |_, err| super::tweens::failure(err),
        });
        Ok(())
    }

    pub(super) fn ungroup(&mut self) -> Result<()> {
        let Some(id) = self.scene.active_id() else {
            return Ok(());
        };
        let (ops, lost) = match klad_core::merge::ungroup(&self.scene.bundle.doc, &id) {
            Ok(done) => done,
            Err(e) => {
                self.say(Tone::Refused, format!("{e:#}"));
                return Ok(());
            }
        };
        let name = self.named(std::slice::from_ref(&id));
        let first = self
            .scene
            .active_layer()
            .and_then(|l| l.children().last())
            .map(|c| c.id.clone());
        if self.timeline_apply(ops)?.is_none() {
            return Ok(());
        }
        if let Some(Loc::Layer(path)) = first.and_then(|c| self.scene.bundle.doc.locate(&c)) {
            self.activate(path)?;
        }
        let lost = match lost.is_empty() {
            true => String::new(),
            false => format!("; its {} went with it", lost.join(" and ")),
        };
        self.say(Tone::Done, format!("took {name} apart{lost} (cmd Z puts it back)"));
        Ok(())
    }

    pub(super) fn move_layer(&mut self, up: bool) -> Result<()> {
        let Some(id) = self.scene.active_id().filter(|_| !self.scene.active_is_mask()) else {
            return Ok(());
        };
        let doc = &self.scene.bundle.doc;
        let Some(Loc::Layer(mut path)) = doc.locate(&id) else {
            return Ok(());
        };
        let index = path.pop().unwrap_or_default();
        let siblings = match path.is_empty() {
            true => &doc.layers,
            false => doc.layer_at(&path).children(),
        };
        let other = match up {
            true => siblings.get(index + 1),
            false => index.checked_sub(1).and_then(|i| siblings.get(i)),
        };
        let Some(other) = other.map(|l| l.id.clone()) else {
            self.say(
                Tone::Refused,
                match up {
                    true => "it is the top layer of its group already",
                    false => "it is the bottom layer of its group already",
                },
            );
            return Ok(());
        };
        let place = match up {
            true => Place {
                above: Some(other),
                ..Place::default()
            },
            false => Place {
                below: Some(other),
                ..Place::default()
            },
        };
        if self.timeline_apply(vec![Op::Move { id: id.clone(), place }])?.is_some()
            && let Some(Loc::Layer(path)) = self.scene.bundle.doc.locate(&id)
        {
            self.activate(path)?;
        }
        Ok(())
    }

    pub(super) fn step_layer(&mut self, up: bool) -> Result<()> {
        let rows: Vec<Vec<usize>> = self
            .layer_rows(Where::Panel)
            .into_iter()
            .filter(|r| !r.mask)
            .map(|r| r.path)
            .collect();
        let Some(at) = rows.iter().position(|p| *p == self.scene.active) else {
            match rows.first().cloned() {
                Some(first) => self.activate(first)?,
                None => self.say(
                    Tone::Refused,
                    "there are no layers yet: add one with + in the layers panel",
                ),
            }
            return Ok(());
        };
        let next = match up {
            true => at.checked_sub(1),
            false => (at + 1 < rows.len()).then_some(at + 1),
        };
        match next.and_then(|i| rows.get(i).cloned()) {
            Some(path) => {
                self.also.clear();
                self.activate(path)?;
            }
            None => self.say(
                Tone::Info,
                match (rows.len(), up) {
                    (1, _) => "this is the only layer",
                    (_, true) => "this is the top layer",
                    (_, false) => "this is the bottom layer",
                },
            ),
        }
        Ok(())
    }

    pub(super) fn chosen_set(&mut self, key: &str, on: bool) -> Result<bool> {
        let ids = self.chosen_layers();
        if ids.len() < 2 {
            return Ok(false);
        }
        let ops = ids
            .iter()
            .map(|id| Op::Set {
                id: id.clone(),
                props: serde_json::Map::from_iter([(key.to_string(), json!(on))]),
            })
            .collect();
        self.timeline_apply(ops)?;
        Ok(true)
    }

    pub(super) fn delete_chosen(&mut self) -> Result<bool> {
        let ids = self.chosen_layers();
        if ids.len() < 2 {
            return Ok(false);
        }
        let name = self.named(&ids);
        self.also.clear();
        if self.timeline_apply(vec![Op::Remove { ids }])?.is_some() {
            if let Some(path) = self
                .layer_rows(Where::Panel)
                .into_iter()
                .find(|r| !r.mask)
                .map(|r| r.path)
            {
                self.activate(path)?;
            }
            self.say(Tone::Done, format!("deleted {name} (cmd Z brings them back)"));
        }
        Ok(true)
    }

    pub(super) fn duplicate_chosen(&mut self) -> Result<bool> {
        let ids = self.chosen_layers();
        if ids.len() < 2 {
            return Ok(false);
        }
        let name = self.named(&ids);
        let ops = ids.into_iter().map(|layer| Op::Duplicate { layer }).collect();
        if self.timeline_apply(ops)?.is_some() {
            self.say(Tone::Done, format!("duplicated {name}"));
        }
        Ok(true)
    }

    pub(super) fn group_chosen(&mut self) -> Result<bool> {
        let ids = self.chosen_layers();
        if ids.len() < 2 {
            return Ok(false);
        }
        let doc = &self.scene.bundle.doc;
        let mut placed: Vec<(Vec<usize>, String)> = ids
            .iter()
            .filter_map(|id| match doc.locate(id) {
                Some(Loc::Layer(path)) => Some((path, id.clone())),
                _ => None,
            })
            .collect();
        placed.sort();
        let Some((_, top)) = placed.last().cloned() else {
            return Ok(true);
        };
        let mut taken = doc.ids();
        let group = (0..)
            .map(|n| format!("group{n}"))
            .find(|id| !taken.contains(id))
            .unwrap_or_default();
        taken.insert(group.clone());
        let ops = std::iter::once(Op::AddLayer {
            layer: json!({"kind": "group", "id": group, "name": "group"}),
            place: Place {
                above: Some(top),
                ..Place::default()
            },
        })
        .chain(placed.iter().map(|(_, id)| Op::Move {
            id: id.clone(),
            place: Place {
                into: Some(group.clone()),
                ..Place::default()
            },
        }))
        .collect();
        self.also.clear();
        if self.timeline_apply(ops)?.is_some()
            && let Some(Loc::Layer(path)) = self.scene.bundle.doc.locate(&group)
        {
            self.activate(path)?;
            self.say(Tone::Done, format!("grouped {} layers", placed.len()));
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_kind_of_layer_gets_the_items_that_fit_it() {
        let has = |kind: Kind, pixel: bool, cmd: Cmd| menu_for(kind, pixel).contains(&Some(cmd));
        assert!(has(Kind::Paint, true, Cmd::PixelOutline) && has(Kind::Paint, true, Cmd::Pixelize));
        assert!(!has(Kind::Paint, false, Cmd::PixelOutline) && has(Kind::Paint, false, Cmd::MakeMapLayer));
        assert!(has(Kind::Paint, false, Cmd::ToggleLockAlpha) && !has(Kind::Vector, false, Cmd::ToggleLockAlpha));
        assert!(has(Kind::Group, false, Cmd::Ungroup) && !has(Kind::Paint, false, Cmd::Ungroup));
        assert!(!has(Kind::Bones, false, Cmd::SelectAll) && !has(Kind::Group, false, Cmd::Paste));
        [Kind::Paint, Kind::Vector, Kind::Group, Kind::Bones, Kind::Adjust]
            .iter()
            .for_each(|k| {
                assert!(
                    has(*k, false, Cmd::RenameLayer)
                        && has(*k, false, Cmd::MergeDown)
                        && has(*k, false, Cmd::DeleteLayer)
                );
            });
    }
}
