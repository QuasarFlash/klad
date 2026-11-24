use std::sync::OnceLock;

pub const STAND_IN: &str = "KLAD_MEMORY_GB";
const PIXEL: u64 = 8;
const TIGHT: f64 = 0.5;
const TOO_BIG: f64 = 0.9;

fn read_total() -> Option<u64> {
    if let Some(gb) = std::env::var(STAND_IN).ok().and_then(|v| v.parse::<f64>().ok()) {
        return Some((gb * 1_073_741_824.0) as u64);
    }
    if cfg!(target_os = "linux") {
        let text = std::fs::read_to_string("/proc/meminfo").ok()?;
        let kb: u64 = text
            .lines()
            .find(|l| l.starts_with("MemTotal:"))?
            .split_whitespace()
            .nth(1)?
            .parse()
            .ok()?;
        return Some(kb * 1024);
    }
    if cfg!(target_os = "macos") {
        let out = std::process::Command::new("sysctl")
            .args(["-n", "hw.memsize"])
            .output()
            .ok()?;
        return String::from_utf8_lossy(&out.stdout).trim().parse().ok();
    }
    if cfg!(target_os = "windows") {
        let out = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory",
            ])
            .output()
            .ok()?;
        return String::from_utf8_lossy(&out.stdout).trim().parse().ok();
    }
    None
}

pub fn total() -> Option<u64> {
    if std::env::var_os(STAND_IN).is_some() {
        return read_total();
    }
    static TOTAL: OnceLock<Option<u64>> = OnceLock::new();
    *TOTAL.get_or_init(read_total)
}

pub fn needed(width: u32, height: u32) -> u64 {
    let px = width as u64 * height as u64;
    let layer = px * PIXEL;
    layer * 3 + layer * 4 / 3
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    Fine,
    Tight { need: u64, have: u64 },
    TooBig { need: u64, have: u64 },
}

pub fn fit(width: u32, height: u32) -> Fit {
    fit_in(width, height, total())
}

pub fn fit_in(width: u32, height: u32, have: Option<u64>) -> Fit {
    let need = needed(width, height);
    match have {
        None => Fit::Fine,
        Some(have) if need as f64 > have as f64 * TOO_BIG => Fit::TooBig { need, have },
        Some(have) if need as f64 > have as f64 * TIGHT => Fit::Tight { need, have },
        Some(_) => Fit::Fine,
    }
}

pub fn gb(bytes: u64) -> String {
    let g = bytes as f64 / 1_073_741_824.0;
    match g < 10.0 {
        true => format!("{g:.1} GB"),
        false => format!("{g:.0} GB"),
    }
}

impl Fit {
    pub fn words(&self, width: u32, height: u32) -> Option<String> {
        match self {
            Fit::Fine => None,
            Fit::Tight { need, have } => Some(format!(
                "a {width}x{height} canvas needs about {} of memory to paint on, and this machine has {}: it may slow down as the machine swaps; close other apps, or make it smaller",
                gb(*need),
                gb(*have)
            )),
            Fit::TooBig { need, have } => Some(format!(
                "a {width}x{height} canvas needs about {} of memory to paint on, and this machine has {}: make it smaller (half the width and height needs a quarter)",
                gb(*need),
                gb(*have)
            )),
        }
    }
}

pub fn note_path() -> Option<std::path::PathBuf> {
    let base = match cfg!(target_os = "macos") {
        true => std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join("Library/Logs/klad")),
        false => std::env::var_os("XDG_STATE_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/state")))
            .or_else(|| std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from))
            .map(|d| d.join("klad")),
    }?;
    Some(base.join("out-of-memory.txt"))
}

pub fn last_time() -> Option<String> {
    let path = note_path()?;
    let text = std::fs::read_to_string(&path).ok()?;
    let line = text.lines().rev().find(|l| !l.trim().is_empty())?.trim().to_string();
    let _ = std::fs::OpenOptions::new().write(true).truncate(true).open(&path);
    Some(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_canvas_is_weighed_against_the_machines_memory() {
        assert_eq!(needed(4096, 4096), 4096 * 4096 * 8 * 3 + 4096 * 4096 * 8 * 4 / 3);
        let gig = 1u64 << 30;
        assert_eq!(fit_in(4096, 4096, Some(16 * gig)), Fit::Fine);
        assert!(matches!(fit_in(16384, 16384, Some(8 * gig)), Fit::TooBig { .. }));
        assert!(matches!(fit_in(16384, 16384, Some(16 * gig)), Fit::Tight { .. }));
        assert!(matches!(fit_in(8192, 8192, Some(4 * gig)), Fit::Tight { .. }));
        assert_eq!(fit_in(16384, 16384, None), Fit::Fine, "unknown memory refuses nothing");
        let said = fit_in(16384, 16384, Some(8 * gig)).words(16384, 16384).unwrap();
        assert!(said.contains("8.7 GB") && said.contains("8.0 GB"), "{said}");
        assert_eq!(gb(3 * gig / 2), "1.5 GB");
    }
}
