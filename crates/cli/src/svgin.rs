use crate::agent;
use anyhow::{Context, Result, bail};
use klad_core::bundle::{Blank, Bundle};
use klad_core::ops::{Op, Place};
use serde_json::json;
use std::path::{Path, PathBuf};

pub fn is_svg(file: &Path) -> bool {
    file.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("svg"))
}

fn fonts(root: &Path, node: &mut serde_json::Value) -> Result<()> {
    if node["type"] == "text" {
        let family = node["font"].as_str().unwrap_or_default().to_string();
        let name = match klad_render::fonts::find(&family) {
            Some(_) if !family.is_empty() => family,
            _ => "Inter".to_string(),
        };
        let chosen = klad_render::fonts::bring(root, &name)?;
        node["font"] = json!(chosen.file);
        if let Some(w) = chosen.weight.filter(|_| node.get("weight").is_none()) {
            node["weight"] = json!(w);
        }
    }
    if let Some(children) = node.get_mut("children").and_then(serde_json::Value::as_array_mut) {
        children.iter_mut().try_for_each(|c| fonts(root, c))?;
    }
    Ok(())
}

fn ops(read: &klad_render::svgin::Read) -> Vec<Op> {
    read.layers
        .iter()
        .map(|(name, nodes)| Op::AddLayer {
            layer: json!({"kind": "vector", "name": name, "cels": [{"frame": 0, "nodes": nodes}]}),
            place: Place::default(),
        })
        .collect()
}

fn notes(read: &klad_render::svgin::Read) {
    let mut skipped = read.skipped.clone();
    skipped.sort();
    skipped.dedup();
    if !skipped.is_empty() {
        eprintln!("note: left out what klad has no shape for: {}", skipped.join(", "));
    }
}

fn parsed(file: &Path, root: &Path) -> Result<klad_render::svgin::Read> {
    let text = std::fs::read_to_string(file).with_context(|| format!("reading {}", file.display()))?;
    let near = file.parent().unwrap_or(Path::new("."));
    klad_render::svgin::read(&text, near, &mut |bytes| klad_render::assets::import_image(root, bytes))
}

pub fn import(file: &Path, out: Option<PathBuf>) -> Result<PathBuf> {
    let target = out.unwrap_or_else(|| file.with_extension("klad"));
    if target.exists() {
        bail!(
            "{} is there already: give the drawing another name with -o, or add the SVG to it with --into",
            target.display()
        );
    }
    let probe = std::env::temp_dir().join(format!("klad-svg-{}", std::process::id()));
    let size = parsed(file, &probe);
    let _ = std::fs::remove_dir_all(&probe);
    let size = size?;
    let mut bundle = Bundle::blank(
        &target,
        &Blank {
            width: size.width.round().max(1.0) as u32,
            height: size.height.round().max(1.0) as u32,
            layers: 0,
            ..Blank::default()
        },
        "cli",
    )?;
    let mut read = parsed(file, &target)?;
    read.layers
        .iter_mut()
        .flat_map(|(_, nodes)| nodes.iter_mut())
        .try_for_each(|n| fonts(&target, n))?;
    let applied = bundle.apply(&ops(&read)).and_then(|_| bundle.stamp("cli"));
    if let Err(e) = applied {
        let _ = std::fs::remove_dir_all(&target);
        return Err(e.context(format!("bringing in {}", file.display())));
    }
    eprintln!(
        "{}x{}, {} layer{} of shapes",
        bundle.doc.canvas.width,
        bundle.doc.canvas.height,
        read.layers.len(),
        if read.layers.len() == 1 { "" } else { "s" }
    );
    notes(&read);
    Ok(target)
}

pub fn into(file: &Path, bundle: &Path, output: &agent::Output) -> Result<()> {
    let mut read = parsed(file, bundle)?;
    read.layers
        .iter_mut()
        .flat_map(|(_, nodes)| nodes.iter_mut())
        .try_for_each(|n| fonts(bundle, n))?;
    agent::edit(bundle, &ops(&read), output)?;
    notes(&read);
    Ok(())
}
