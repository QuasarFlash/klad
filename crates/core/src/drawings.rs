use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};

const CACHE: &str = ".cache";

pub fn with_extension(path: &Path) -> PathBuf {
    match path.extension().is_some_and(|e| e == "klad") {
        true => path.to_path_buf(),
        false => {
            let mut name = path.as_os_str().to_owned();
            name.push(".klad");
            PathBuf::from(name)
        }
    }
}

fn check_drawing(from: &Path) -> Result<()> {
    if !from.join("doc.json").is_file() {
        bail!("{} is not a klad drawing (it has no doc.json)", from.display());
    }
    Ok(())
}

fn check_free(to: &Path) -> Result<()> {
    if to.exists() {
        bail!("{} is there already: pick another name", to.display());
    }
    if to.file_stem().is_none_or(|s| s.to_string_lossy().trim().is_empty()) {
        bail!("give the drawing a name");
    }
    Ok(())
}

fn copy_tree(from: &Path, to: &Path) -> Result<u64> {
    fs::create_dir_all(to).with_context(|| format!("making {}", to.display()))?;
    fs::read_dir(from)?.flatten().try_fold(0u64, |total, entry| {
        let path = entry.path();
        let target = to.join(entry.file_name());
        let kind = entry.file_type()?;
        Ok(total
            + match (kind.is_dir(), kind.is_symlink()) {
                (_, true) => 0,
                (true, false) => copy_tree(&path, &target)?,
                (false, false) => fs::copy(&path, &target).with_context(|| format!("copying {}", path.display()))?,
            })
    })
}

pub fn copy(from: &Path, to: &Path) -> Result<PathBuf> {
    check_drawing(from)?;
    let to = with_extension(to);
    check_free(&to)?;
    let made = (|| -> Result<()> {
        fs::create_dir_all(&to)?;
        fs::read_dir(from)?
            .flatten()
            .filter(|e| e.file_name() != CACHE)
            .try_for_each(|entry| {
                let target = to.join(entry.file_name());
                match entry.file_type()?.is_dir() {
                    true => copy_tree(&entry.path(), &target).map(|_| ()),
                    false => fs::copy(entry.path(), &target).map(|_| ()).map_err(Into::into),
                }
            })
    })();
    if let Err(e) = made {
        let _ = fs::remove_dir_all(&to);
        return Err(e.context(format!("copying {} to {}", from.display(), to.display())));
    }
    Ok(to)
}

pub fn rename(from: &Path, to: &Path) -> Result<PathBuf> {
    check_drawing(from)?;
    let to = match to.parent().is_none_or(|p| p.as_os_str().is_empty()) {
        true => from.with_file_name(with_extension(to)),
        false => with_extension(to),
    };
    check_free(&to)?;
    fs::rename(from, &to).with_context(|| format!("renaming {} to {}", from.display(), to.display()))?;
    Ok(to)
}

pub fn copy_name(from: &Path) -> PathBuf {
    let stem = from
        .file_stem()
        .map_or("drawing".into(), |s| s.to_string_lossy().to_string());
    let dir = from.parent().map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    (1..)
        .map(|n| match n {
            1 => dir.join(format!("{stem} copy.klad")),
            n => dir.join(format!("{stem} copy {n}.klad")),
        })
        .find(|p| !p.exists())
        .unwrap_or_else(|| dir.join(format!("{stem} copy.klad")))
}

pub fn renamed(root: &Path, doc: &crate::doc::Document) -> Option<PathBuf> {
    if root.exists() {
        return None;
    }
    let beside = root.parent().filter(|p| !p.as_os_str().is_empty())?;
    let tag = &doc.here.tag;
    let same_doc = |p: &Path| {
        fs::read_to_string(p.join("doc.json"))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .and_then(|v| crate::migrate::migrate(v).ok())
            .and_then(|v| serde_json::from_value::<crate::doc::Document>(v).ok())
            .is_some_and(|d| d == *doc)
    };
    let found: Vec<PathBuf> = fs::read_dir(beside)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("doc.json").is_file())
        .filter(|p| crate::here::read(p).is_some_and(|s| s.tag == *tag))
        .filter(|p| !tag.is_empty() || same_doc(p))
        .collect();
    match found.as_slice() {
        [one] => Some(one.clone()),
        _ => None,
    }
}

fn unique(dir: &Path, name: &std::ffi::OsStr) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let stem = Path::new(name)
        .file_stem()
        .map_or(String::new(), |s| s.to_string_lossy().to_string());
    let ext = Path::new(name)
        .extension()
        .map_or(String::new(), |e| format!(".{}", e.to_string_lossy()));
    (2..)
        .map(|n| dir.join(format!("{stem} {n}{ext}")))
        .find(|p| !p.exists())
        .unwrap_or(first)
}

pub fn trash(path: &Path) -> Result<PathBuf> {
    trash_to(path, std::env::var_os("KLAD_TRASH").map(PathBuf::from))
}

pub fn trash_to(path: &Path, stand_in: Option<PathBuf>) -> Result<PathBuf> {
    check_drawing(path)?;
    let name = path.file_name().map(|n| n.to_owned()).unwrap_or_default();
    if let Some(dir) = stand_in {
        fs::create_dir_all(&dir)?;
        let to = unique(&dir, &name);
        fs::rename(path, &to)?;
        return Ok(to);
    }
    if cfg!(target_os = "macos") {
        let home = std::env::var_os("HOME").map(PathBuf::from).context("HOME is not set")?;
        let to = unique(&home.join(".Trash"), &name);
        return fs::rename(path, &to)
            .map(|_| to)
            .with_context(|| format!("{} could not go to the Trash: move it there in Finder", path.display()));
    }
    if cfg!(target_os = "windows") {
        let full = fs::canonicalize(path)?;
        let script = format!(
            "Add-Type -AssemblyName Microsoft.VisualBasic; [Microsoft.VisualBasic.FileIO.FileSystem]::DeleteDirectory('{}', 'OnlyErrorDialogs', 'SendToRecycleBin')",
            full.display().to_string().replace('\'', "''")
        );
        let out = std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", &script])
            .output()
            .context("the recycle bin could not be reached")?;
        if !out.status.success() || path.exists() {
            bail!(
                "{} could not go to the recycle bin: delete it in Explorer",
                path.display()
            );
        }
        return Ok(PathBuf::new());
    }
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .context("HOME is not set")?;
    let (files, info) = (data.join("Trash/files"), data.join("Trash/info"));
    fs::create_dir_all(&files)?;
    fs::create_dir_all(&info)?;
    let to = unique(&files, &name);
    let full = fs::canonicalize(path)?;
    let when = {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()) as i64;
        let (days, rest) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
        let (y, m, d) = civil(days);
        format!(
            "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}",
            rest / 3600,
            rest / 60 % 60,
            rest % 60
        )
    };
    let entry = info.join(format!(
        "{}.trashinfo",
        to.file_name().unwrap_or_default().to_string_lossy()
    ));
    fs::write(
        &entry,
        format!("[Trash Info]\nPath={}\nDeletionDate={when}\n", full.display()),
    )?;
    if let Err(e) = fs::rename(path, &to) {
        let _ = fs::remove_file(&entry);
        return Err(anyhow::Error::from(e).context(format!("{} could not go to the Trash", path.display())));
    }
    Ok(to)
}

const OPENED: &str = "opened";

fn opened_dir(root: &Path) -> PathBuf {
    root.join(CACHE).join(OPENED)
}

fn drawing_files(root: &Path) -> Vec<PathBuf> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        entries.flatten().for_each(|e| {
            let path = e.path();
            let rel = path.strip_prefix(base).map(Path::to_path_buf).unwrap_or_default();
            if rel.as_os_str() == CACHE || rel.as_os_str() == "doc.json" {
                return;
            }
            match e.file_type().map(|t| t.is_dir()) {
                Ok(true) => walk(base, &path, out),
                Ok(false) => out.push(rel),
                Err(_) => {}
            }
        });
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

pub fn keep_opened(root: &Path) -> Result<()> {
    let dir = opened_dir(root);
    if dir.exists() {
        fs::remove_dir_all(&dir)?;
    }
    fs::create_dir_all(&dir)?;
    std::iter::once(PathBuf::from("doc.json"))
        .chain(drawing_files(root))
        .try_for_each(|rel| {
            let (from, to) = (root.join(&rel), dir.join(&rel));
            if let Some(parent) = to.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::hard_link(&from, &to).or_else(|_| fs::copy(&from, &to).map(|_| ()))?;
            Ok::<_, anyhow::Error>(())
        })
}

pub fn has_opened(root: &Path) -> bool {
    opened_dir(root).join("doc.json").is_file()
}

pub fn revert(bundle: &mut crate::bundle::Bundle, author: &str) -> Result<bool> {
    let root = bundle.root.clone();
    let snap = opened_dir(&root);
    if !snap.join("doc.json").is_file() {
        bail!("there is no copy of how the drawing was when it was opened");
    }
    let text = fs::read_to_string(snap.join("doc.json"))?;
    let value = crate::migrate::migrate(serde_json::from_str(&text)?)?;
    let mut opened: crate::doc::Document = serde_json::from_value(value)?;
    opened.here = bundle.doc.here.clone();
    opened.rev = bundle.doc.rev;
    opened.next_id = opened.next_id.max(bundle.doc.next_id);
    let (now, then) = (drawing_files(&root), drawing_files(&snap));
    let same_files = now == then
        && now
            .iter()
            .all(|rel| fs::read(root.join(rel)).ok() == fs::read(snap.join(rel)).ok());
    if same_files && opened == bundle.doc {
        return Ok(false);
    }
    let touched: Vec<PathBuf> = now
        .iter()
        .chain(then.iter().filter(|rel| !now.contains(rel)))
        .map(|rel| root.join(rel))
        .collect();
    crate::history::checkpoint(
        bundle,
        &opened,
        author,
        "back to how it was when it was opened",
        &touched,
    )?;
    then.iter()
        .try_for_each(|rel| crate::bundle::write_atomic(&root.join(rel), &fs::read(snap.join(rel))?))?;
    now.iter()
        .filter(|rel| !then.contains(rel))
        .try_for_each(|rel| fs::remove_file(root.join(rel)))?;
    bundle.doc = opened;
    bundle.save()?;
    bundle.stamp(author)?;
    Ok(true)
}

fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::Bundle;
    use crate::doc::Document;

    #[test]
    fn drawings_are_copied_without_their_cache_renamed_found_by_tag_and_trashed() {
        let dir = std::env::temp_dir().join(format!("klad-drawings-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a.klad");
        let bundle = Bundle::create(&a, Document::new(16, 16)).unwrap();
        fs::create_dir_all(a.join(".cache")).unwrap();
        fs::write(a.join(".cache/junk"), b"x").unwrap();
        fs::create_dir_all(a.join("strokes/l1")).unwrap();
        fs::write(a.join("strokes/l1/c1.strokes"), b"strokes").unwrap();
        let tag = bundle.doc.here.tag.clone();
        let b = copy(&a, &dir.join("b")).unwrap();
        assert_eq!(b, dir.join("b.klad"));
        assert!(b.join("doc.json").is_file() && b.join("strokes/l1/c1.strokes").is_file());
        assert!(!b.join(".cache").exists(), "the copy gets its own identity when opened");
        assert!(copy(&a, &b).is_err(), "never over a drawing");
        assert_eq!(copy_name(&a), dir.join("a copy.klad"));
        let c = rename(&a, Path::new("c")).unwrap();
        assert_eq!(c, dir.join("c.klad"));
        assert!(!a.exists());
        assert!(tag.is_empty(), "the first copy has no tag");
        assert_eq!(
            renamed(&a, &bundle.doc),
            Some(c.clone()),
            "a renamed drawing is found by its document"
        );
        assert_eq!(
            renamed(&c, &bundle.doc),
            None,
            "a drawing that is there was not renamed"
        );
        let mut tagged = bundle.doc.clone();
        tagged.here.tag = "qz".into();
        crate::here::write(
            &c,
            &crate::here::State {
                tag: "qz".into(),
                ..Default::default()
            },
        )
        .unwrap();
        tagged.canvas.background = Some("#ffffff".into());
        assert_eq!(
            renamed(&a, &tagged),
            Some(c.clone()),
            "or by its copy's tag once it has changed"
        );
        let gone = trash_to(&b, Some(dir.join("trash"))).unwrap();
        assert!(!b.exists() && gone.join("doc.json").is_file());
        let mut c_bundle = Bundle::open(&c).unwrap();
        keep_opened(&c).unwrap();
        assert!(has_opened(&c));
        assert!(!revert(&mut c_bundle, "test").unwrap(), "nothing to go back to yet");
        crate::bundle::write_atomic(&c.join("strokes/l1/c1.strokes"), b"changed").unwrap();
        fs::write(c.join("strokes/l1/c9.strokes"), b"new").unwrap();
        c_bundle.doc.canvas.background = Some("#123456".into());
        c_bundle.save().unwrap();
        assert!(revert(&mut c_bundle, "test").unwrap());
        assert_eq!(fs::read(c.join("strokes/l1/c1.strokes")).unwrap(), b"strokes");
        assert!(!c.join("strokes/l1/c9.strokes").exists());
        assert_eq!(Bundle::open(&c).unwrap().doc.canvas.background, None);
        crate::history::step(&mut c_bundle, crate::history::Stack::Undo).unwrap();
        assert_eq!(
            fs::read(c.join("strokes/l1/c1.strokes")).unwrap(),
            b"changed",
            "the revert is one undo step"
        );
        assert!(c.join("strokes/l1/c9.strokes").exists());
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(20_412), (2025, 11, 20));
        let _ = fs::remove_dir_all(&dir);
    }
}
