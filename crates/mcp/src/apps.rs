use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

pub const NAME: &str = "klad";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Mac,
    Linux,
    Windows,
}

impl Os {
    pub fn here() -> Self {
        match std::env::consts::OS {
            "macos" => Os::Mac,
            "windows" => Os::Windows,
            _ => Os::Linux,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum App {
    ClaudeDesktop,
    Codex,
    ClaudeCode,
    Cursor,
    VsCode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    McpServers,
    Typed,
    VsCode,
    Codex,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub command: PathBuf,
    pub args: Vec<String>,
}

impl Entry {
    pub fn serving(program: &Path) -> Self {
        Self {
            command: program.to_path_buf(),
            args: vec!["mcp".into()],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Missing,
    Found,
    Connected(Entry),
    Moved(Entry),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Places {
    pub home: PathBuf,
    pub os: Os,
}

impl Places {
    pub fn here() -> Option<Self> {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)?;
        Some(Self { home, os: Os::here() })
    }

    fn support(&self) -> PathBuf {
        match self.os {
            Os::Mac => self.home.join("Library/Application Support"),
            Os::Windows => self.home.join("AppData/Roaming"),
            Os::Linux => self.home.join(".config"),
        }
    }
}

impl App {
    pub const ALL: [App; 5] = [
        App::ClaudeDesktop,
        App::Codex,
        App::ClaudeCode,
        App::Cursor,
        App::VsCode,
    ];

    pub fn name(self) -> &'static str {
        match self {
            App::ClaudeDesktop => "Claude Desktop",
            App::Codex => "Codex",
            App::ClaudeCode => "Claude Code",
            App::Cursor => "Cursor",
            App::VsCode => "VS Code",
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            App::ClaudeDesktop => "claude-desktop",
            App::Codex => "codex",
            App::ClaudeCode => "claude-code",
            App::Cursor => "cursor",
            App::VsCode => "vscode",
        }
    }

    pub fn parse(word: &str) -> Option<App> {
        App::ALL.into_iter().find(|a| a.word() == word)
    }

    pub fn words() -> String {
        App::ALL.map(App::word).join(", ")
    }

    pub fn next_step(self) -> &'static str {
        match self {
            App::ClaudeDesktop => "quit Claude Desktop (cmd Q) and open it again",
            App::Codex => "open a new Codex chat (restart the Codex app if it was running)",
            App::ClaudeCode => "start a new Claude Code session",
            App::Cursor => "reload Cursor's window or restart Cursor",
            App::VsCode => "reload VS Code's window, then start klad under MCP Servers",
        }
    }

    fn shape(self) -> Shape {
        match self {
            App::Codex => Shape::Codex,
            App::VsCode => Shape::VsCode,
            App::ClaudeCode => Shape::Typed,
            App::ClaudeDesktop | App::Cursor => Shape::McpServers,
        }
    }

    pub fn file(self, places: &Places) -> PathBuf {
        match self {
            App::ClaudeDesktop => places.support().join("Claude/claude_desktop_config.json"),
            App::Codex => places.home.join(".codex/config.toml"),
            App::ClaudeCode => places.home.join(".claude.json"),
            App::Cursor => places.home.join(".cursor/mcp.json"),
            App::VsCode => places.support().join("Code/User/mcp.json"),
        }
    }

    fn bundles(self) -> &'static [&'static str] {
        match self {
            App::ClaudeDesktop => &["Claude.app"],
            App::Codex => &["Codex.app"],
            App::ClaudeCode => &[],
            App::Cursor => &["Cursor.app"],
            App::VsCode => &["Visual Studio Code.app"],
        }
    }

    fn signs(self, places: &Places) -> Vec<PathBuf> {
        let folder = match self {
            App::ClaudeDesktop => places.support().join("Claude"),
            App::Codex => places.home.join(".codex"),
            App::ClaudeCode => places.home.join(".claude"),
            App::Cursor => places.home.join(".cursor"),
            App::VsCode => places.support().join("Code"),
        };
        let apps = match places.os {
            Os::Mac => self
                .bundles()
                .iter()
                .flat_map(|b| {
                    [
                        Path::new("/Applications").join(b),
                        places.home.join("Applications").join(b),
                    ]
                })
                .collect(),
            _ => Vec::new(),
        };
        std::iter::once(folder)
            .chain(std::iter::once(self.file(places)))
            .chain(apps)
            .collect()
    }

    pub fn status(self, places: &Places) -> Status {
        match read_entry(self, &self.file(places)) {
            Some(entry) if entry.command.exists() => Status::Connected(entry),
            Some(entry) => Status::Moved(entry),
            None if self.signs(places).iter().any(|p| p.exists()) => Status::Found,
            None => Status::Missing,
        }
    }

    pub fn snippet(self, entry: &Entry) -> String {
        match self.shape() {
            Shape::Codex => {
                let mut doc = toml_edit::DocumentMut::new();
                set_toml(&mut doc, Some(entry));
                doc.to_string()
            }
            shape => {
                let mut root = Map::new();
                set_json(&mut root, shape, Some(entry));
                serde_json::to_string_pretty(&Value::Object(root)).unwrap_or_default()
            }
        }
    }

    pub fn connect(self, places: &Places, entry: &Entry) -> Result<PathBuf, String> {
        write(self, places, Some(entry))
    }

    pub fn disconnect(self, places: &Places) -> Result<bool, String> {
        match read_entry(self, &self.file(places)) {
            Some(_) => write(self, places, None).map(|_| true),
            None => Ok(false),
        }
    }
}

fn server_json(shape: Shape, entry: &Entry) -> Value {
    let args: Vec<Value> = entry.args.iter().map(|a| json!(a)).collect();
    match shape {
        Shape::McpServers => json!({ "command": entry.command, "args": args }),
        _ => json!({ "type": "stdio", "command": entry.command, "args": args }),
    }
}

fn table(shape: Shape) -> &'static str {
    match shape {
        Shape::VsCode => "servers",
        _ => "mcpServers",
    }
}

fn set_json(root: &mut Map<String, Value>, shape: Shape, entry: Option<&Entry>) {
    let key = table(shape);
    let servers = root.entry(key).or_insert_with(|| json!({}));
    if !servers.is_object() {
        *servers = json!({});
    }
    let Some(servers) = servers.as_object_mut() else {
        return;
    };
    match entry {
        Some(entry) => {
            servers.insert(NAME.into(), server_json(shape, entry));
        }
        None => {
            servers.remove(NAME);
        }
    }
    if servers.is_empty() && entry.is_none() {
        root.remove(key);
    }
}

fn set_toml(doc: &mut toml_edit::DocumentMut, entry: Option<&Entry>) {
    use toml_edit::{Array, Item, Table, value};
    let servers = doc
        .entry("mcp_servers")
        .or_insert_with(|| {
            let mut t = Table::new();
            t.set_implicit(true);
            Item::Table(t)
        })
        .as_table_mut();
    let Some(servers) = servers else {
        return;
    };
    match entry {
        Some(entry) => {
            let mut klad = Table::new();
            klad.insert("command", value(entry.command.display().to_string()));
            klad.insert("args", value(entry.args.iter().map(String::as_str).collect::<Array>()));
            servers.insert(NAME, Item::Table(klad));
        }
        None => {
            servers.remove(NAME);
        }
    }
}

fn read_entry(app: App, file: &Path) -> Option<Entry> {
    let text = std::fs::read_to_string(file).ok()?;
    let found = match app.shape() {
        Shape::Codex => {
            let doc: toml_edit::DocumentMut = text.parse().ok()?;
            let klad = doc.get("mcp_servers")?.get(NAME)?;
            let command = klad.get("command")?.as_str()?.to_string();
            let args = klad
                .get("args")
                .and_then(|a| a.as_array())
                .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();
            (command, args)
        }
        shape => {
            let root: Value = serde_json::from_str(&text).ok()?;
            let klad = root.get(table(shape))?.get(NAME)?;
            let command = klad.get("command")?.as_str()?.to_string();
            let args = klad
                .get("args")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();
            (command, args)
        }
    };
    Some(Entry {
        command: PathBuf::from(found.0),
        args: found.1,
    })
}

fn write(app: App, places: &Places, entry: Option<&Entry>) -> Result<PathBuf, String> {
    let file = app.file(places);
    let before = match std::fs::read_to_string(&file) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("reading {}: {e}", file.display())),
    };
    let after = match app.shape() {
        Shape::Codex => {
            let mut doc: toml_edit::DocumentMut = before.as_deref().unwrap_or("").parse().map_err(|e| {
                format!(
                    "{}'s settings ({}) do not read as TOML, so klad leaves them alone: {e}",
                    app.name(),
                    file.display()
                )
            })?;
            set_toml(&mut doc, entry);
            doc.to_string()
        }
        shape => {
            let mut root = match before.as_deref().map(str::trim) {
                None | Some("") => Map::new(),
                Some(text) => match serde_json::from_str::<Value>(text) {
                    Ok(Value::Object(map)) => map,
                    _ => {
                        return Err(format!(
                            "{}'s settings ({}) do not read as JSON, so klad leaves them alone",
                            app.name(),
                            file.display()
                        ));
                    }
                },
            };
            set_json(&mut root, shape, entry);
            serde_json::to_string_pretty(&Value::Object(root)).map_err(|e| e.to_string())? + "\n"
        }
    };
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("making {}: {e}", parent.display()))?;
    }
    if let Some(before) = &before {
        let backup = file.with_file_name(format!(
            "{}.before-klad",
            file.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()
        ));
        std::fs::write(&backup, before).map_err(|e| format!("writing {}: {e}", backup.display()))?;
    }
    let partial = file.with_extension("klad-partial");
    std::fs::write(&partial, after).map_err(|e| format!("writing {}: {e}", partial.display()))?;
    std::fs::rename(&partial, &file).map_err(|e| format!("writing {}: {e}", file.display()))?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home(name: &str) -> Places {
        let home = std::env::temp_dir().join(format!("klad-mcp-home-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        Places { home, os: Os::Mac }
    }

    fn klad(places: &Places) -> Entry {
        let program = places.home.join("klad.app/Contents/MacOS/klad");
        std::fs::create_dir_all(program.parent().unwrap()).unwrap();
        std::fs::write(&program, "").unwrap();
        Entry::serving(&program)
    }

    #[test]
    fn claude_desktop_keeps_its_other_servers_and_gets_a_backup() {
        let places = home("claude");
        let file = App::ClaudeDesktop.file(&places);
        assert!(file.ends_with("Library/Application Support/Claude/claude_desktop_config.json"));
        assert_eq!(App::ClaudeDesktop.status(&places), Status::Missing);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(
            &file,
            r#"{"mcpServers":{"files":{"command":"npx","args":["files"]}},"theme":"dark"}"#,
        )
        .unwrap();
        assert_eq!(App::ClaudeDesktop.status(&places), Status::Found);
        let entry = klad(&places);
        App::ClaudeDesktop.connect(&places, &entry).unwrap();
        let written: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(
            written["mcpServers"]["files"]["command"], "npx",
            "the other server stays"
        );
        assert_eq!(written["theme"], "dark", "and every other setting");
        assert_eq!(written["mcpServers"]["klad"]["args"], json!(["mcp"]));
        assert!(file.with_file_name("claude_desktop_config.json.before-klad").exists());
        assert_eq!(App::ClaudeDesktop.status(&places), Status::Connected(entry.clone()));
        std::fs::remove_file(&entry.command).unwrap();
        assert!(
            matches!(App::ClaudeDesktop.status(&places), Status::Moved(_)),
            "klad.app was moved"
        );
        assert!(App::ClaudeDesktop.disconnect(&places).unwrap());
        let left: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert!(left["mcpServers"].get("klad").is_none());
        assert_eq!(left["mcpServers"]["files"]["command"], "npx");
        assert!(
            !App::ClaudeDesktop.disconnect(&places).unwrap(),
            "nothing to take out twice"
        );
        let _ = std::fs::remove_dir_all(&places.home);
    }

    #[test]
    fn codex_gets_a_toml_table_and_its_comments_stay() {
        let places = home("codex");
        let file = App::Codex.file(&places);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(
            &file,
            "# my settings\nmodel = \"gpt-5\"\n\n[mcp_servers.docs]\ncommand = \"docs\"\n",
        )
        .unwrap();
        let entry = klad(&places);
        App::Codex.connect(&places, &entry).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.starts_with("# my settings\nmodel = \"gpt-5\""), "{text}");
        assert!(
            text.contains("[mcp_servers.docs]") && text.contains("[mcp_servers.klad]"),
            "{text}"
        );
        assert!(text.contains("args = [\"mcp\"]"), "{text}");
        assert_eq!(App::Codex.status(&places), Status::Connected(entry));
        App::Codex.disconnect(&places).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(
            !text.contains("mcp_servers.klad") && text.contains("[mcp_servers.docs]"),
            "{text}"
        );
        let _ = std::fs::remove_dir_all(&places.home);
    }

    #[test]
    fn vs_code_cursor_and_claude_code_take_their_own_shapes_and_bad_files_are_left_alone() {
        let places = home("others");
        let entry = klad(&places);
        App::VsCode.connect(&places, &entry).unwrap();
        let vs: Value = serde_json::from_str(&std::fs::read_to_string(App::VsCode.file(&places)).unwrap()).unwrap();
        assert_eq!(vs["servers"]["klad"]["type"], "stdio");
        App::Cursor.connect(&places, &entry).unwrap();
        assert!(App::Cursor.file(&places).ends_with(".cursor/mcp.json"));
        App::ClaudeCode.connect(&places, &entry).unwrap();
        let code: Value =
            serde_json::from_str(&std::fs::read_to_string(App::ClaudeCode.file(&places)).unwrap()).unwrap();
        assert_eq!(code["mcpServers"]["klad"]["type"], "stdio");
        std::fs::write(App::Cursor.file(&places), "{ not json").unwrap();
        let refused = App::Cursor.connect(&places, &entry).unwrap_err();
        assert!(refused.contains("leaves them alone"), "{refused}");
        assert_eq!(
            std::fs::read_to_string(App::Cursor.file(&places)).unwrap(),
            "{ not json",
            "a file klad cannot read is not touched"
        );
        assert!(App::Codex.snippet(&entry).contains("[mcp_servers.klad]"));
        assert!(App::ClaudeDesktop.snippet(&entry).contains("\"mcpServers\""));
        assert_eq!(App::parse("claude-desktop"), Some(App::ClaudeDesktop));
        let _ = std::fs::remove_dir_all(&places.home);
    }
}
