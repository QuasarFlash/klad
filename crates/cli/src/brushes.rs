use anyhow::{Result, anyhow};
use klad_core::brushes::{self, BrushDef, Curve, TipDef};
use klad_core::bundle::Bundle;
use serde_json::json;
use std::path::Path;

struct Listed {
    id: String,
    source: &'static str,
    def: BrushDef,
}

fn listed(bundle: Option<&Bundle>) -> Result<Vec<Listed>> {
    let embedded = bundle.map(Bundle::brushes).transpose()?.unwrap_or_default();
    let in_bundle = embedded.iter().map(|(id, def)| Listed {
        id: id.clone(),
        source: match brushes::library(id) {
            Some(lib) if &lib == def => "bundle",
            Some(_) => "bundle, differs from library",
            None => "bundle only",
        },
        def: def.clone(),
    });
    let library = brushes::library_ids()
        .into_iter()
        .filter(|id| !embedded.contains_key(*id))
        .filter_map(|id| {
            brushes::library(id).map(|def| Listed {
                id: id.to_string(),
                source: "library",
                def,
            })
        });
    Ok(in_bundle.chain(library).collect())
}

fn curve(c: Option<Curve>) -> String {
    c.map_or("-".into(), |c| {
        let gamma = if c.gamma == 1.0 {
            String::new()
        } else {
            format!(" ^{}", c.gamma)
        };
        format!("{}..{}{gamma}", c.min, c.max)
    })
}

fn tip(def: &BrushDef) -> String {
    let main = match &def.tip {
        TipDef::Round { hardness } => format!("round {hardness}"),
        TipDef::Chalk { seed } => format!("chalk {seed}"),
        TipDef::Image { image, more } => [image].into_iter().chain(more).cloned().collect::<Vec<_>>().join("+"),
    };
    let extras: Vec<&str> = [
        (def.dual.is_some(), "dual"),
        (def.grain.is_some(), "texture"),
        (def.color.is_some(), "colour"),
        (matches!(def.paint, Some(brushes::PaintDef::Wet { .. })), "wet"),
        (matches!(def.paint, Some(brushes::PaintDef::Smudge { .. })), "smudge"),
        (matches!(def.paint, Some(brushes::PaintDef::Blend { .. })), "blend"),
        (def.body > 0.0, "body"),
        (def.feather > 0.0, "feather"),
        (def.fringe > 0.0, "fringe"),
    ]
    .iter()
    .filter(|(on, _)| *on)
    .map(|(_, word)| *word)
    .collect();
    match extras.is_empty() {
        true => main,
        false => format!("{main} +{}", extras.join("+")),
    }
}

pub fn set(bundle: &Path, id: &str, file: &Path) -> Result<()> {
    let bundle = Bundle::open(bundle)?;
    let text = match file.to_str() {
        Some("-") => std::io::read_to_string(std::io::stdin())?,
        _ => std::fs::read_to_string(file)?,
    };
    let def = BrushDef::parse(&text)?;
    let set = klad_brush::BrushSet::for_bundle(&bundle)?;
    let missing: Vec<&str> = def
        .images()
        .into_iter()
        .filter(|name| !set.pictures().contains_key(*name))
        .collect();
    if !missing.is_empty() {
        anyhow::bail!(
            "brush {id} uses pictures not in the bundle: {} (add them with klad brushes {} --picture <name> <file.png>)",
            missing.join(", "),
            bundle.root.display()
        );
    }
    match bundle.brush(id)? {
        Some(known) if known == def => println!("brush {id} is already in the bundle as given"),
        Some(_) => anyhow::bail!(
            "brush {id} is in use with other settings; strokes keep their brush, so give the new one another id"
        ),
        None => {
            bundle.embed_brush(id, &def)?;
            println!("brush {id} added to the bundle");
        }
    }
    Ok(())
}

pub fn picture(bundle: &Path, name: &str, file: &Path) -> Result<()> {
    let bundle = Bundle::open(bundle)?;
    brushes::valid_image(name)?;
    let bytes = std::fs::read(file)?;
    let picture = klad_brush::images::decode(&bytes)?;
    let path = bundle.root.join("brushes").join("images").join(format!("{name}.png"));
    if path.exists() {
        match std::fs::read(&path)? == bytes {
            true => println!("picture {name} is already in the bundle"),
            false => anyhow::bail!(
                "picture {name} is in the bundle already with other pixels; brushes using it would change, so give it another name"
            ),
        }
        return Ok(());
    }
    std::fs::create_dir_all(path.parent().expect("a folder"))?;
    klad_core::bundle::write_atomic(&path, &bytes)?;
    println!(
        "picture {name} ({}x{}) added to the bundle's brushes/images",
        picture.width, picture.height
    );
    Ok(())
}

pub fn list(bundle: Option<&Path>, show: Option<&str>, as_json: bool) -> Result<()> {
    let opened = bundle.map(Bundle::open).transpose()?;
    let all = listed(opened.as_ref())?;
    if let Some(id) = show {
        let found = all.iter().find(|l| l.id == id).ok_or_else(|| {
            let ids: Vec<&str> = all.iter().map(|l| l.id.as_str()).collect();
            anyhow!("no brush {id:?}: brushes are {}", ids.join(", "))
        })?;
        print!("{}", found.def.to_json()?);
        return Ok(());
    }
    if as_json {
        let list: Vec<serde_json::Value> = all
            .iter()
            .map(|l| json!({ "id": l.id, "source": l.source, "brush": l.def }))
            .collect();
        println!("{}", serde_json::Value::Array(list));
        return Ok(());
    }
    println!(
        "{:<16} {:<12} {:<11} {:>7} {:<16} {:>5} {:>8} {:>7} {:>8}  source",
        "id", "name", "tip", "spacing", "size by pressure", "flow", "taper", "stab ms", "smooth"
    );
    all.iter().for_each(|l| {
        let d = &l.def;
        let erase = if d.blend == brushes::BrushBlend::Erase {
            " (erases)"
        } else {
            ""
        };
        println!(
            "{:<16} {:<12} {:<11} {:>7} {:<16} {:>5} {:>8} {:>7} {:>8}  {}{erase}",
            l.id,
            d.name,
            tip(d),
            d.spacing,
            curve(d.dynamics.size.pressure),
            d.flow,
            format!("{},{}", d.taper[0], d.taper[1]),
            d.stabilizer,
            d.smoothing,
            l.source
        );
    });
    Ok(())
}

#[derive(Default)]
pub struct Mine {
    pub list: bool,
    pub save: Option<String>,
    pub save_as: Option<String>,
    pub take: Option<String>,
    pub forget: Option<String>,
    pub rename: Option<Vec<String>>,
    pub group: Option<Vec<String>>,
    pub shift: Option<Vec<String>>,
    pub export: Option<std::path::PathBuf>,
    pub import: Option<std::path::PathBuf>,
}

impl Mine {
    pub fn any(&self) -> bool {
        self.list
            || self.save.is_some()
            || self.take.is_some()
            || self.forget.is_some()
            || self.rename.is_some()
            || self.group.is_some()
            || self.shift.is_some()
            || self.export.is_some()
            || self.import.is_some()
    }
}

pub fn mine(bundle: Option<&Path>, m: Mine, as_json: bool) -> Result<()> {
    let lib = klad_render::library::Library::home()?;
    let need = || bundle.ok_or_else(|| anyhow!("--save and --take need a bundle: klad brushes art.klad --save my-ink"));
    if let Some(id) = &m.save {
        let opened = Bundle::open(need()?)?;
        let def = opened.brush(id)?.ok_or_else(|| {
            anyhow!(
                "the drawing has no brush {id} of its own (klad brushes {} lists them)",
                opened.root.display()
            )
        })?;
        let to = m.save_as.as_deref().unwrap_or(id);
        lib.save(&opened, to, &def)?;
        println!("saved {to} in your brushes ({})", lib.dir.display());
    }
    if let Some(id) = &m.take {
        let opened = Bundle::open(need()?)?;
        let (_, fresh) = lib.take(&opened, id)?;
        match fresh {
            true => println!("the drawing has its own copy of {id} now"),
            false => println!("the drawing has a brush {id} already and keeps it"),
        }
    }
    if let Some(id) = &m.forget {
        lib.forget(id)?;
        println!("{id} is out of your brushes; drawings that use it keep their copy");
    }
    if let Some(v) = &m.rename {
        lib.rename(&v[0], &v[1])?;
    }
    if let Some(v) = &m.group {
        lib.group(&v[0], &v[1])?;
    }
    if let Some(v) = &m.shift {
        let by: i32 = v[1]
            .parse()
            .map_err(|_| anyhow!("--move takes a brush and a number of places, like -1 or 2"))?;
        lib.shift(&v[0], by)?;
    }
    if let Some(file) = &m.import {
        let added = lib.import(file)?;
        println!("{} brushes added to your brushes: {}", added.len(), added.join(", "));
    }
    if let Some(file) = &m.export {
        let n = lib.export(file)?;
        println!("{n} brushes written to {}", file.display());
    }
    if m.list {
        let saved = lib.list();
        if as_json {
            let list: Vec<serde_json::Value> = saved
                .iter()
                .map(|s| json!({ "id": s.id, "group": s.group, "brush": s.def }))
                .collect();
            println!("{}", serde_json::Value::Array(list));
            return Ok(());
        }
        if saved.is_empty() {
            println!(
                "no brushes of your own yet ({}): klad brushes art.klad --save <brush>",
                lib.dir.display()
            );
        }
        saved.iter().for_each(|s| {
            let group = match s.group.is_empty() {
                true => String::new(),
                false => format!("  [{}]", s.group),
            };
            println!("{:<16} {:<16} {:<11}{group}", s.id, s.def.name, tip(&s.def));
        });
    }
    Ok(())
}
