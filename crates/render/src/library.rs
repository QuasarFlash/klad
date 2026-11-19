use anyhow::{Context, Result, anyhow, bail};
use klad_core::brushes::BrushDef;
use klad_core::bundle::{Bundle, write_atomic};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const STAND_IN: &str = "KLAD_LIBRARY";
const IMAGES: &str = "images";
const ORDER: &str = "order.json";
const SET: &str = "klad-brushes/1";

pub fn home_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os(STAND_IN) {
        return Some(PathBuf::from(dir));
    }
    if cfg!(target_os = "windows") {
        return std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("klad").join("brushes"));
    }
    let home = PathBuf::from(std::env::var_os("HOME")?);
    let data = match cfg!(target_os = "macos") {
        true => home.join("Library/Application Support"),
        false => std::env::var_os("XDG_DATA_HOME").map_or(home.join(".local/share"), PathBuf::from),
    };
    Some(data.join("klad").join("brushes"))
}

pub fn valid(id: &str) -> Result<()> {
    match !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        true => Ok(()),
        false => bail!("{id:?} cannot be a brush's id: use letters, digits, - and _"),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Saved {
    pub id: String,
    pub def: BrushDef,
    pub group: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Order {
    ids: Vec<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    groups: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize)]
struct Set {
    format: String,
    brushes: Vec<SetBrush>,
    #[serde(default)]
    images: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize)]
struct SetBrush {
    id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    group: String,
    brush: serde_json::Value,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Library {
    pub dir: PathBuf,
}

impl Library {
    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn home() -> Result<Self> {
        home_dir()
            .map(Self::at)
            .ok_or_else(|| anyhow!("there is no folder for your brushes: HOME is not set"))
    }

    fn images(&self) -> PathBuf {
        self.dir.join(IMAGES)
    }

    fn order(&self) -> Order {
        std::fs::read(self.dir.join(ORDER))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    fn write_order(&self, order: &Order) -> Result<()> {
        write_atomic(&self.dir.join(ORDER), serde_json::to_string_pretty(order)?.as_bytes())
    }

    fn file(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }

    pub fn list(&self) -> Vec<Saved> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut found: BTreeMap<String, BrushDef> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "json"))
            .filter_map(|p| {
                let id = p.file_stem()?.to_str()?.to_string();
                let def = BrushDef::parse(&std::fs::read_to_string(&p).ok()?).ok()?;
                (id != "order").then_some((id, def))
            })
            .collect();
        let order = self.order();
        let known: Vec<String> = order.ids.iter().filter(|id| found.contains_key(*id)).cloned().collect();
        let rest: Vec<String> = found.keys().filter(|id| !known.contains(id)).cloned().collect();
        known
            .into_iter()
            .chain(rest)
            .filter_map(|id| {
                let def = found.remove(&id)?;
                let group = order.groups.get(&id).cloned().unwrap_or_default();
                Some(Saved { id, def, group })
            })
            .collect()
    }

    pub fn get(&self, id: &str) -> Option<Saved> {
        self.list().into_iter().find(|s| s.id == id)
    }

    fn must(&self, id: &str) -> Result<Saved> {
        self.get(id)
            .ok_or_else(|| anyhow!("no brush {id} in your brushes (klad brushes --library lists them)"))
    }

    pub fn save(&self, bundle: &Bundle, id: &str, def: &BrushDef) -> Result<()> {
        valid(id)?;
        if klad_core::brushes::library(id).is_some() {
            bail!("{id} is one of klad's own brushes, in every drawing already; save your version under another name");
        }
        std::fs::create_dir_all(self.images()).with_context(|| format!("making {}", self.dir.display()))?;
        def.images().iter().try_for_each(|name| {
            match std::fs::read(bundle.root.join("brushes").join(IMAGES).join(format!("{name}.png"))) {
                Ok(bytes) => write_atomic(&self.images().join(format!("{name}.png")), &bytes),
                Err(_) => Ok(()),
            }
        })?;
        write_atomic(&self.file(id), def.to_json()?.as_bytes())?;
        let mut order = self.order();
        if !order.ids.iter().any(|i| i == id) {
            order.ids.push(id.to_string());
        }
        self.write_order(&order)
    }

    pub fn take(&self, bundle: &Bundle, id: &str) -> Result<(BrushDef, bool)> {
        let saved = self.must(id)?;
        saved.def.images().iter().try_for_each(|name| {
            let to = bundle.root.join("brushes").join(IMAGES).join(format!("{name}.png"));
            match (to.exists(), std::fs::read(self.images().join(format!("{name}.png")))) {
                (false, Ok(bytes)) => write_atomic(&to, &bytes),
                _ => Ok(()),
            }
        })?;
        let fresh = bundle.embed_brush(id, &saved.def)?;
        let kept = bundle.brush(id)?.unwrap_or(saved.def);
        Ok((kept, fresh))
    }

    pub fn forget(&self, id: &str) -> Result<()> {
        self.must(id)?;
        std::fs::remove_file(self.file(id))?;
        let mut order = self.order();
        order.ids.retain(|i| i != id);
        order.groups.remove(id);
        self.write_order(&order)
    }

    pub fn rename(&self, id: &str, name: &str) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            bail!("a brush needs a name");
        }
        let mut def = self.must(id)?.def;
        def.name = name.to_string();
        write_atomic(&self.file(id), def.to_json()?.as_bytes())
    }

    pub fn group(&self, id: &str, group: &str) -> Result<()> {
        self.must(id)?;
        let mut order = self.order();
        match group.trim() {
            "" => order.groups.remove(id),
            g => order.groups.insert(id.to_string(), g.to_string()),
        };
        self.write_order(&order)
    }

    pub fn shift(&self, id: &str, by: i32) -> Result<()> {
        let mut ids: Vec<String> = self.list().into_iter().map(|s| s.id).collect();
        let Some(at) = ids.iter().position(|i| i == id) else {
            bail!("no brush {id} in your brushes");
        };
        let to = (at as i64 + by as i64).clamp(0, ids.len() as i64 - 1) as usize;
        let moved = ids.remove(at);
        ids.insert(to, moved);
        let mut order = self.order();
        order.ids = ids;
        self.write_order(&order)
    }

    pub fn export(&self, file: &Path) -> Result<usize> {
        let saved = self.list();
        let images = saved
            .iter()
            .flat_map(|s| s.def.images().into_iter().map(str::to_string).collect::<Vec<_>>())
            .filter_map(|name| {
                let bytes = std::fs::read(self.images().join(format!("{name}.png"))).ok()?;
                Some((name, crate::svg::base64(&bytes)))
            })
            .collect();
        let brushes = saved
            .iter()
            .map(|s| {
                Ok(SetBrush {
                    id: s.id.clone(),
                    group: s.group.clone(),
                    brush: serde_json::from_str(&s.def.to_json()?)?,
                })
            })
            .collect::<Result<_>>()?;
        let set = Set {
            format: SET.into(),
            brushes,
            images,
        };
        write_atomic(file, serde_json::to_string_pretty(&set)?.as_bytes())?;
        Ok(saved.len())
    }

    pub fn import(&self, file: &Path) -> Result<Vec<String>> {
        let bytes = std::fs::read(file).with_context(|| format!("reading {}", file.display()))?;
        let set: Set = serde_json::from_slice(&bytes)
            .map_err(|e| anyhow!("{} is not a set of klad brushes: {e}", file.display()))?;
        if set.format != SET {
            bail!(
                "{} is a set of brushes in another format ({})",
                file.display(),
                set.format
            );
        }
        let parsed: Vec<(SetBrush, BrushDef)> = set
            .brushes
            .into_iter()
            .map(|b| {
                valid(&b.id)?;
                let def = BrushDef::parse(&b.brush.to_string()).with_context(|| format!("brush {}", b.id))?;
                Ok((b, def))
            })
            .collect::<Result<_>>()?;
        std::fs::create_dir_all(self.images())?;
        set.images.iter().try_for_each(|(name, data)| {
            klad_core::brushes::valid_image(name)?;
            let bytes = crate::svgin::unbase64(data);
            if bytes.is_empty() {
                bail!("picture {name} in the set is broken");
            }
            write_atomic(&self.images().join(format!("{name}.png")), &bytes)
        })?;
        let mut order = self.order();
        let added = parsed
            .into_iter()
            .map(|(b, def)| {
                write_atomic(&self.file(&b.id), def.to_json()?.as_bytes())?;
                if !order.ids.contains(&b.id) {
                    order.ids.push(b.id.clone());
                }
                if !b.group.is_empty() {
                    order.groups.insert(b.id.clone(), b.group.clone());
                }
                Ok(b.id)
            })
            .collect::<Result<Vec<_>>>()?;
        self.write_order(&order)?;
        Ok(added)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn your_brushes_are_saved_ordered_grouped_shared_and_taken_into_drawings() {
        let root = std::env::temp_dir().join(format!("klad-library-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let lib = Library::at(root.join("mine"));
        assert!(lib.list().is_empty());
        let a = Bundle::create(root.join("a.klad"), klad_core::doc::Document::new(64, 64)).unwrap();
        let mut def = klad_core::brushes::library("ink").unwrap();
        def.name = "wet ink".into();
        def.spacing = 0.3;
        assert!(lib.save(&a, "ink", &def).is_err());
        lib.save(&a, "wet-ink", &def).unwrap();
        lib.save(&a, "dry", &klad_core::brushes::library("pencil").unwrap())
            .unwrap();
        assert_eq!(
            lib.list().iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            ["wet-ink", "dry"]
        );
        lib.shift("dry", -1).unwrap();
        lib.group("dry", "sketch").unwrap();
        lib.rename("wet-ink", "wetter ink").unwrap();
        let list = lib.list();
        assert_eq!(list[0].id, "dry");
        assert_eq!(list[0].group, "sketch");
        assert_eq!(list[1].def.name, "wetter ink");
        let b = Bundle::create(root.join("b.klad"), klad_core::doc::Document::new(64, 64)).unwrap();
        let (taken, fresh) = lib.take(&b, "wet-ink").unwrap();
        assert!(fresh && taken.spacing == 0.3);
        assert_eq!(b.brush("wet-ink").unwrap().unwrap().spacing, 0.3);
        lib.export(&root.join("set.json")).unwrap();
        let other = Library::at(root.join("theirs"));
        assert_eq!(other.import(&root.join("set.json")).unwrap(), ["dry", "wet-ink"]);
        assert_eq!(other.get("dry").unwrap().group, "sketch");
        lib.forget("wet-ink").unwrap();
        assert!(lib.get("wet-ink").is_none());
        assert_eq!(
            b.brush("wet-ink").unwrap().unwrap().spacing,
            0.3,
            "the drawing keeps its own copy"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
