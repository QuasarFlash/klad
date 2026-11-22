use crate::bundle::write_atomic;
use crate::doc::Document;
use crate::here;
use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub const FOLDER: &str = "QuickLook";
pub const PREVIEW: &str = "Preview.png";
pub const THUMBNAIL: &str = "Thumbnail.png";
pub const PREVIEW_SIDE: f32 = 1024.0;
pub const THUMBNAIL_SIDE: f32 = 512.0;
pub const EXPLORER: &str = "folder.jpg";
pub const IGNORED: [&str; 2] = ["QuickLook/", "folder.jpg"];

fn stamp(root: &Path) -> PathBuf {
    root.join(".cache").join("preview")
}

pub fn frame(doc: &Document) -> u32 {
    doc.timeline.clips.first().map_or(0, |clip| clip.from)
}

pub fn stale(root: &Path) -> bool {
    let made = fs::read_to_string(stamp(root))
        .ok()
        .and_then(|text| text.trim().parse::<u64>().ok());
    made != Some(here::fingerprint(root)) || !root.join(FOLDER).join(PREVIEW).is_file()
}

pub fn made(root: &Path, fingerprint: u64) -> Result<()> {
    ignore(root)?;
    write_atomic(&stamp(root), fingerprint.to_string().as_bytes())?;
    let _ = fs::File::open(root).and_then(|folder| folder.set_modified(SystemTime::now()));
    Ok(())
}

pub fn ignore(root: &Path) -> Result<()> {
    let path = root.join(".gitignore");
    let text = fs::read_to_string(&path).unwrap_or_default();
    let missing: Vec<&str> = IGNORED
        .into_iter()
        .filter(|ignored| !text.lines().any(|line| line.trim() == *ignored))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    let mut text = text;
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    missing.iter().for_each(|line| {
        text.push_str(line);
        text.push('\n');
    });
    write_atomic(&path, text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::Bundle;
    use crate::doc::Clip;

    fn drawing(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("klad-preview-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        Bundle::create(&dir, Document::new(64, 48)).unwrap();
        dir
    }

    #[test]
    fn a_preview_is_stale_until_made_and_again_after_the_drawing_changes() {
        let dir = drawing("stale");
        assert!(stale(&dir), "a new drawing has no preview");
        fs::create_dir_all(dir.join(FOLDER)).unwrap();
        fs::write(dir.join(FOLDER).join(PREVIEW), b"png").unwrap();
        made(&dir, here::fingerprint(&dir)).unwrap();
        assert!(!stale(&dir), "made from the drawing as it is");
        let mut bundle = Bundle::open(&dir).unwrap();
        bundle.doc.canvas.width = 80;
        bundle.save().unwrap();
        assert!(stale(&dir), "a saved change makes it stale");
        made(&dir, here::fingerprint(&dir)).unwrap();
        fs::remove_file(dir.join(FOLDER).join(PREVIEW)).unwrap();
        assert!(stale(&dir), "a missing picture is stale too");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_preview_stays_out_of_git_in_new_and_older_drawings() {
        let dir = drawing("ignored");
        let lines = |dir: &Path| {
            fs::read_to_string(dir.join(".gitignore"))
                .unwrap()
                .lines()
                .map(str::to_string)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            lines(&dir),
            [".cache/", "QuickLook/", "folder.jpg"],
            "a new drawing ignores them"
        );
        fs::write(dir.join(".gitignore"), ".cache/\nmine").unwrap();
        ignore(&dir).unwrap();
        ignore(&dir).unwrap();
        assert_eq!(
            lines(&dir),
            [".cache/", "mine", "QuickLook/", "folder.jpg"],
            "added once, after the lines kept"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_preview_shows_the_first_clip_or_else_frame_zero() {
        let mut doc = Document::new(64, 48);
        assert_eq!(frame(&doc), 0);
        doc.timeline.frames = 24;
        doc.timeline.clips = vec![
            Clip {
                name: "walk".into(),
                from: 6,
                to: 13,
                looping: true,
                fps: None,
            },
            Clip {
                name: "idle".into(),
                from: 0,
                to: 5,
                looping: true,
                fps: None,
            },
        ];
        assert_eq!(frame(&doc), 6, "the first clip, as the timeline lists them");
    }
}
