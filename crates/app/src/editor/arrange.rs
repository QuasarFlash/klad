use super::Editor;
use super::selection::Selected;
use crate::commands::Cmd;
use crate::messages::Tone;
use anyhow::Result;
use klad_core::doc::{Document, Item, Node};
use klad_core::markedit::{Order, address, order_marks};
use klad_core::ops::{self, Applied, Op};

pub(super) const ARRANGE: [Cmd; 4] = [Cmd::BringToFront, Cmd::BringForward, Cmd::SendBackward, Cmd::SendToBack];

pub(super) fn order_of(cmd: Cmd) -> Option<Order> {
    match cmd {
        Cmd::BringToFront => Some(Order::Front),
        Cmd::BringForward => Some(Order::Forward),
        Cmd::SendBackward => Some(Order::Backward),
        Cmd::SendToBack => Some(Order::Back),
        _ => None,
    }
}

fn walk(nodes: &[Node], out: &mut Vec<String>) {
    nodes.iter().for_each(|n| {
        out.push(n.id.clone());
        walk(n.children(), out);
    });
}

fn node_order(doc: &Document, ids: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    ids.iter().for_each(|id| {
        if let Some(Item::Node(_, cel, _)) = doc.item(id)
            && !seen.contains(&cel.id)
        {
            seen.push(cel.id.clone());
            walk(&cel.nodes, &mut out);
        }
    });
    out
}

fn said(to: Order, what: &str) -> String {
    match to {
        Order::Front => format!("brought {what} to the front"),
        Order::Forward => format!("brought {what} forward one place"),
        Order::Backward => format!("sent {what} back one place"),
        Order::Back => format!("sent {what} to the back"),
    }
}

fn already(to: Order) -> &'static str {
    match to {
        Order::Front | Order::Forward => "already in front of everything it overlaps on its layer",
        Order::Back | Order::Backward => "already behind everything it overlaps on its layer",
    }
}

fn counted(n: usize, one: &str) -> String {
    match n {
        1 => format!("the {one}"),
        n => format!("the {n} {one}s"),
    }
}

impl Editor {
    pub(super) fn can_arrange(&self) -> Option<String> {
        match self.selected() {
            Some(Selected::Strokes | Selected::Shape(_)) => {
                self.busy().then(|| "finish the stroke or drag first".to_string())
            }
            _ => Some("select strokes (S) or shapes (V) first".to_string()),
        }
    }

    fn picked_addresses(&self) -> Vec<(String, Vec<u32>)> {
        self.picked
            .as_ref()
            .map(|p| {
                std::iter::once((p.cel.clone(), p.ids.clone()))
                    .chain(p.more.iter().map(|m| (m.cel.clone(), m.ids.clone())))
                    .filter(|(_, ids)| !ids.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(super) fn arrange(&mut self, to: Order) -> Result<()> {
        let (ids, moves, what) = match self.on_marks() {
            true => {
                let parts = self.picked_addresses();
                let moves = parts.iter().any(|(cel, ids)| {
                    let mut marks = self.scene.marks_of(cel).to_vec();
                    order_marks(&mut marks, ids, to)
                });
                let count = parts.iter().map(|(_, ids)| ids.len()).sum();
                let ids: Vec<String> = parts
                    .iter()
                    .flat_map(|(cel, ids)| ids.iter().map(move |id| address(cel, *id)))
                    .collect();
                (ids, moves, counted(count, "mark"))
            }
            false => {
                let ids = self.shapes();
                let doc = &self.scene.bundle.doc;
                let mut trial = doc.clone();
                let op = Op::Order { ids: ids.clone(), to };
                let moves = ops::apply(&mut trial, &op, &mut Applied::default()).is_ok()
                    && node_order(&trial, &ids) != node_order(doc, &ids);
                let what = counted(ids.len(), "shape");
                (ids, moves, what)
            }
        };
        if ids.is_empty() {
            return Ok(());
        }
        if !moves {
            self.say(
                Tone::Refused,
                format!("{}: {}", what.trim_start_matches("the "), already(to)),
            );
            return Ok(());
        }
        self.apply(vec![Op::Order { ids, to }])?;
        self.say(Tone::Done, format!("{} (cmd Z puts it back)", said(to, &what)));
        Ok(())
    }
}
