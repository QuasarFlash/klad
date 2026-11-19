use crate::overlay::node_world;
use crate::vector::{node_matrix, shape_path};
use anyhow::{Result, anyhow, bail};
use klad_core::doc::{Document, Loc, Node, Shape};
use klad_core::ops::{Op, Place};
use serde_json::{Value, json};
use skia_safe::{Matrix, Path, PathOp};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Combine {
    Unite,
    Subtract,
    Intersect,
    Exclude,
}

impl Combine {
    pub fn parse(word: &str) -> Result<Combine> {
        Ok(match word {
            "unite" | "union" => Combine::Unite,
            "subtract" | "minus" => Combine::Subtract,
            "intersect" => Combine::Intersect,
            "exclude" | "xor" => Combine::Exclude,
            other => bail!("{other:?}: combine with unite, subtract, intersect or exclude"),
        })
    }

    pub fn word(self) -> &'static str {
        match self {
            Combine::Unite => "united",
            Combine::Subtract => "subtracted",
            Combine::Intersect => "intersected",
            Combine::Exclude => "excluded",
        }
    }
}

fn place_of(doc: &Document, id: &str) -> Option<(Vec<usize>, usize, Vec<usize>)> {
    match doc.locate(id)? {
        Loc::Node(layer, cel, nodes) => Some((layer, cel, nodes)),
        _ => None,
    }
}

fn world(doc: &Document, id: &str) -> Result<(Node, Path, Matrix)> {
    let (node, parent) = node_world(doc, id)?.ok_or_else(|| anyhow!("no shape {id}"))?;
    let path = shape_path(&node.shape)?.ok_or_else(|| {
        anyhow!(
            "{id} is a {}: unite, subtract and intersect take rectangles, ellipses, paths and text",
            node.type_name()
        )
    })?;
    let m = Matrix::concat(&parent, &node_matrix(node)?);
    Ok((node.clone(), path.with_transform(&m), parent))
}

fn styled(node: &Node, d: String) -> Value {
    let mut value = json!({"type": "path", "d": d});
    if let Some(fill) = &node.fill {
        value["fill"] = serde_json::to_value(fill).unwrap_or(Value::Null);
    }
    if let Some(stroke) = &node.stroke {
        value["stroke"] = serde_json::to_value(stroke).unwrap_or(Value::Null);
    }
    if node.opacity < 1.0 {
        value["opacity"] = json!(node.opacity);
    }
    if !node.name.is_empty() {
        value["name"] = json!(node.name);
    }
    value
}

pub fn combine(doc: &Document, ids: &[String], how: Combine) -> Result<Vec<Op>> {
    if ids.len() < 2 {
        bail!("give two shapes or more to combine");
    }
    let mut placed: Vec<(Vec<usize>, usize, Vec<usize>, String)> = ids
        .iter()
        .map(|id| {
            place_of(doc, id)
                .map(|(l, c, n)| (l, c, n, id.clone()))
                .ok_or_else(|| anyhow!("{id} is not a shape on a vector layer"))
        })
        .collect::<Result<_>>()?;
    if placed.iter().any(|(l, c, _, _)| (l, c) != (&placed[0].0, &placed[0].1)) {
        bail!("the shapes are on different layers or drawings: combine shapes of one drawing of one vector layer");
    }
    placed.sort_by(|a, b| a.2.cmp(&b.2));
    let order: Vec<String> = placed.iter().map(|(_, _, _, id)| id.clone()).collect();
    let shapes: Vec<(Node, Path, Matrix)> = order.iter().map(|id| world(doc, id)).collect::<Result<_>>()?;
    let (back, first, parent) = shapes[0].clone();
    let op = |a: &Path, b: &Path, op: PathOp| a.op(b, op).ok_or_else(|| anyhow!("the shapes could not be combined"));
    let result = match how {
        Combine::Unite => shapes[1..]
            .iter()
            .try_fold(first, |acc, (_, p, _)| op(&acc, p, PathOp::Union))?,
        Combine::Intersect => shapes[1..]
            .iter()
            .try_fold(first, |acc, (_, p, _)| op(&acc, p, PathOp::Intersect))?,
        Combine::Exclude => shapes[1..]
            .iter()
            .try_fold(first, |acc, (_, p, _)| op(&acc, p, PathOp::XOR))?,
        Combine::Subtract => shapes[1..]
            .iter()
            .try_fold(first, |acc, (_, p, _)| op(&acc, p, PathOp::Difference))?,
    };
    if result.is_empty() || result.compute_tight_bounds().is_empty() {
        bail!(
            "nothing is left once they are {}: the shapes do not overlap that way",
            how.word()
        );
    }
    let local = match parent.invert() {
        Some(inverse) => result.with_transform(&inverse),
        None => result,
    };
    let node = styled(&back, local.to_svg());
    Ok(vec![
        Op::AddNode {
            node,
            place: Place {
                above: Some(back.id.clone()),
                ..Place::default()
            },
        },
        Op::Remove { ids: order },
    ])
}

pub fn text_to_path(doc: &Document, id: &str) -> Result<Option<Vec<Op>>> {
    let Some((node, _)) = node_world(doc, id)? else {
        return Ok(None);
    };
    let Shape::Text(text) = &node.shape else {
        return Ok(None);
    };
    let d = crate::text::text_path(text)?.to_svg();
    let mut value = styled(node, d);
    if !node.transform.is_identity() {
        let mut t = node.transform;
        if t.pivot.is_none() {
            let local = crate::vector::local_bounds(node)?;
            t.pivot = local.map(|r| [r.center_x(), r.center_y()]);
        }
        value["transform"] = serde_json::to_value(t)?;
    }
    if value["name"].is_null() {
        value["name"] = json!(text.text.lines().next().unwrap_or("text"));
    }
    Ok(Some(vec![
        Op::AddNode {
            node: value,
            place: Place {
                above: Some(id.to_string()),
                ..Place::default()
            },
        },
        Op::Remove {
            ids: vec![id.to_string()],
        },
    ]))
}
