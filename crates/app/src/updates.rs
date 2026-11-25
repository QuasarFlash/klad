use serde::Deserialize;
use std::time::{SystemTime, UNIX_EPOCH};

pub const FEED: &str = "https://api.github.com/repos/klad-app/klad/releases/latest";
const DAY: u64 = 24 * 60 * 60;

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: String,
    pub url: String,
    pub notes: String,
}

#[derive(Deserialize)]
struct Feed {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    body: Option<String>,
}

pub fn parse(text: &str) -> Option<Release> {
    if text.trim_start().starts_with('<') {
        return appcast(text);
    }
    let feed: Feed = serde_json::from_str(text).ok()?;
    Some(Release {
        version: feed.tag_name.trim_start_matches('v').to_string(),
        url: feed.html_url,
        notes: feed.body.unwrap_or_default(),
    })
}

fn between<'a>(text: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let start = text.find(open)? + open.len();
    let end = text[start..].find(close)? + start;
    Some(&text[start..end])
}

fn unescape(text: &str) -> String {
    [
        ("&lt;", "<"),
        ("&gt;", ">"),
        ("&quot;", "\""),
        ("&#39;", "'"),
        ("&apos;", "'"),
        ("&amp;", "&"),
    ]
    .iter()
    .fold(text.to_string(), |t, (from, to)| t.replace(from, to))
}

fn plain(html: &str) -> String {
    let (kept, _) = html
        .chars()
        .fold((String::new(), false), |(mut kept, tag), c| match (c, tag) {
            ('<', _) => (kept, true),
            ('>', true) => {
                kept.push('\n');
                (kept, false)
            }
            (_, true) => (kept, true),
            (c, false) => {
                kept.push(c);
                (kept, false)
            }
        });
    unescape(&kept)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn appcast(xml: &str) -> Option<Release> {
    xml.split("<item>")
        .skip(1)
        .filter_map(|item| {
            let version = between(item, "<sparkle:shortVersionString>", "</sparkle:shortVersionString>")?;
            let url = between(item, "<enclosure url=\"", "\"")?;
            let notes = between(item, "<description><![CDATA[", "]]></description>").unwrap_or_default();
            Some(Release {
                version: version.trim().to_string(),
                url: unescape(url),
                notes: plain(notes),
            })
        })
        .max_by(|a, b| parts(&a.version).cmp(&parts(&b.version)))
}

fn parts(version: &str) -> Vec<u64> {
    version
        .trim_start_matches('v')
        .split(['.', '-', '+', ' '])
        .take(3)
        .map(|p| p.parse().unwrap_or(0))
        .collect()
}

pub fn newer(current: &str, latest: &str) -> bool {
    parts(latest) > parts(current)
}

pub fn due(enabled: bool, last: u64, now: u64) -> bool {
    enabled && now.saturating_sub(last) >= DAY
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

pub type Checked = Result<Option<Release>, String>;

fn answer(code: &str, body: &str) -> Result<String, String> {
    match code.trim() {
        "200" => Ok(body.to_string()),
        "404" => Err("no release of klad is published yet".to_string()),
        other => Err(format!("the releases feed answered {other}")),
    }
}

fn fetch(feed: &str) -> Result<String, String> {
    if !feed.starts_with("http") {
        return std::fs::read_to_string(feed).map_err(|e| format!("reading {feed}: {e}"));
    }
    let out = std::process::Command::new("curl")
        .args([
            "-sSL",
            "--max-time",
            "10",
            "-H",
            "Accept: application/vnd.github+json",
            "-w",
            "\n%{http_code}",
            feed,
        ])
        .output()
        .map_err(|e| format!("could not run curl: {e}"))?;
    if !out.status.success() {
        return Err("could not reach the releases feed (no network?)".to_string());
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let (body, code) = text.rsplit_once('\n').unwrap_or(("", &text));
    answer(code, body)
}

pub fn check(feed: &str, current: &str) -> Checked {
    let release = parse(&fetch(feed)?).ok_or_else(|| "the releases feed did not make sense".to_string())?;
    Ok(Some(release).filter(|r| newer(current, &r.version)))
}

pub fn summary(release: &Release) -> String {
    let first = release
        .notes
        .lines()
        .map(|l| l.trim().trim_start_matches(['-', '*', '#']).trim())
        .find(|l| !l.is_empty());
    match first {
        Some(line) => format!("klad {} is out: {line}", release.version),
        None => format!("klad {} is out", release.version),
    }
}

pub fn outcome(checked: &Checked) -> String {
    match checked {
        Ok(Some(release)) => summary(release),
        Ok(None) => format!("klad {} is the newest version", env!("CARGO_PKG_VERSION")),
        Err(e) => format!("could not check: {e}"),
    }
}

pub fn spawn(feed: String) -> std::sync::mpsc::Receiver<Checked> {
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("klad update check".into())
        .spawn(move || {
            let _ = send.send(check(&feed, env!("CARGO_PKG_VERSION")));
        })
        .ok();
    receive
}

pub fn feed() -> String {
    std::env::var("KLAD_UPDATE_FEED").unwrap_or_else(|_| feed_for(option_env!("KLAD_SITE"), env!("CARGO_PKG_VERSION")))
}

fn feed_for(site: Option<&str>, version: &str) -> String {
    match site.map(|s| s.trim_end_matches('/')).filter(|s| !s.is_empty()) {
        Some(site) => format!("{site}/api/appcast/{}", parts(version).first().copied().unwrap_or(0)),
        None => FEED.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_newer_release_in_the_feed_is_found_once_a_day_and_never_when_off() {
        let dir = std::env::temp_dir().join(format!("klad-updates-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let feed = dir.join("latest.json");
        let write = |tag: &str| {
            std::fs::write(
                &feed,
                format!(
                    r#"{{"tag_name":"{tag}","html_url":"https://github.com/klad-app/klad/releases/tag/{tag}","body":"\n- Paint over: brushwork from any picture\n- more"}}"#
                ),
            )
            .unwrap()
        };
        write("v0.2.0");
        let found = check(feed.to_str().unwrap(), "0.1.0").unwrap().unwrap();
        assert_eq!(found.version, "0.2.0");
        assert_eq!(
            summary(&found),
            "klad 0.2.0 is out: Paint over: brushwork from any picture"
        );
        write("v0.1.0");
        assert_eq!(check(feed.to_str().unwrap(), "0.1.0"), Ok(None));
        assert_eq!(
            outcome(&Ok(None)),
            format!("klad {} is the newest version", env!("CARGO_PKG_VERSION"))
        );
        let missing = check(dir.join("missing.json").to_str().unwrap(), "0.1.0");
        assert!(
            outcome(&missing).starts_with("could not check: reading "),
            "{missing:?}"
        );
        std::fs::write(&feed, "<html>").unwrap();
        assert_eq!(
            check(feed.to_str().unwrap(), "0.1.0"),
            Err("the releases feed did not make sense".to_string())
        );
        assert_eq!(
            answer("404", ""),
            Err("no release of klad is published yet".to_string())
        );
        assert_eq!(answer("503", ""), Err("the releases feed answered 503".to_string()));
        assert_eq!(answer("200", "{}"), Ok("{}".to_string()));
        assert!(newer("0.9.3", "0.10.0") && !newer("1.2.0", "1.1.9") && newer("0.1.0", "v0.1.1"));
        assert!(due(true, 0, DAY) && !due(true, DAY, DAY + 60) && !due(false, 0, 10 * DAY));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_bought_build_asks_the_sites_appcast_for_its_major_and_reads_the_newest_item() {
        assert_eq!(
            feed_for(Some("https://klad-dev.atelier-schaaf.be/"), "0.5.0"),
            "https://klad-dev.atelier-schaaf.be/api/appcast/0"
        );
        assert_eq!(
            feed_for(Some("https://klad.studio"), "1.2.3"),
            "https://klad.studio/api/appcast/1"
        );
        assert_eq!(feed_for(None, "1.2.3"), FEED, "a build from source asks GitHub");
        assert_eq!(feed_for(Some(""), "1.2.3"), FEED);
        let dir = std::env::temp_dir().join(format!("klad-appcast-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let feed = dir.join("appcast.xml");
        std::fs::write(
            &feed,
            r#"<?xml version="1.0" encoding="utf-8"?>
<rss version="2.0" xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle">
<channel><title>klad 0.x</title>
<item><title>klad 0.5.0</title><sparkle:version>0.5.0</sparkle:version><sparkle:shortVersionString>0.5.0</sparkle:shortVersionString><description><![CDATA[<p>The first test.</p>]]></description><enclosure url="https://klad-dev.atelier-schaaf.be/api/updates/download?key=0.5.0%2Fklad-0.5.0-macos.dmg&amp;v=1" length="10" type="application/octet-stream" sparkle:edSignature="x"/></item>
<item><title>klad 0.5.1</title><sparkle:version>0.5.1</sparkle:version><sparkle:shortVersionString>0.5.1</sparkle:shortVersionString><description><![CDATA[<p>Updates from inside klad &amp; more.</p><ul><li>Install and restart</li></ul>]]></description><enclosure url="https://klad-dev.atelier-schaaf.be/api/updates/download?key=0.5.1%2Fklad-0.5.1-macos.dmg" length="10" type="application/octet-stream" sparkle:edSignature="y"/></item>
</channel></rss>"#,
        )
        .unwrap();
        let found = check(feed.to_str().unwrap(), "0.5.0").unwrap().unwrap();
        assert_eq!(found.version, "0.5.1");
        assert_eq!(
            found.url,
            "https://klad-dev.atelier-schaaf.be/api/updates/download?key=0.5.1%2Fklad-0.5.1-macos.dmg"
        );
        assert_eq!(found.notes, "Updates from inside klad & more.\nInstall and restart");
        assert_eq!(summary(&found), "klad 0.5.1 is out: Updates from inside klad & more.");
        assert_eq!(check(feed.to_str().unwrap(), "0.5.1"), Ok(None));
        assert_eq!(
            parse("<rss><channel></channel></rss>"),
            None,
            "an appcast with no release yet"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_check_runs_beside_the_ui() {
        let dir = std::env::temp_dir().join(format!("klad-updates-bg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let feed = dir.join("latest.json");
        std::fs::write(&feed, r#"{"tag_name":"v99.0.0","html_url":"https://example.org"}"#).unwrap();
        let checking = spawn(feed.display().to_string());
        let found = checking.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(found.unwrap().map(|r| r.version), Some("99.0.0".to_string()));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
