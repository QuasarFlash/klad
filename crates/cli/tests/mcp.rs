use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

struct Session {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    next: u64,
}

impl Session {
    fn start(home: &Path, root: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_klad"))
            .args(["mcp", "--root"])
            .arg(root)
            .env("HOME", home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        let mut session = Self {
            child,
            input,
            output,
            next: 0,
        };
        let hello = session.ask(
            "initialize",
            json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test", "version": "1"}}),
        );
        assert_eq!(hello["serverInfo"]["name"], "klad", "{hello}");
        assert!(
            hello["instructions"].as_str().unwrap_or_default().contains("note_done"),
            "the instructions name the tools: {hello}"
        );
        session.tell("notifications/initialized");
        session
    }

    fn tell(&mut self, method: &str) {
        writeln!(self.input, "{}", json!({"jsonrpc": "2.0", "method": method})).unwrap();
    }

    fn ask(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let id = self.next;
        writeln!(
            self.input,
            "{}",
            json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
        )
        .unwrap();
        loop {
            let mut line = String::new();
            assert!(
                self.output.read_line(&mut line).unwrap() > 0,
                "klad mcp stopped answering"
            );
            let message: Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("{e}: {line}"));
            if message["id"] == json!(id) {
                assert!(message.get("error").is_none(), "{method}: {message}");
                return message["result"].clone();
            }
        }
    }

    fn call(&mut self, tool: &str, arguments: Value) -> (bool, Vec<Value>) {
        let result = self.ask("tools/call", json!({"name": tool, "arguments": arguments}));
        let failed = result["isError"].as_bool().unwrap_or(false);
        (failed, result["content"].as_array().cloned().unwrap_or_default())
    }

    fn text(&mut self, tool: &str, arguments: Value) -> String {
        let (failed, content) = self.call(tool, arguments.clone());
        let text = content
            .iter()
            .filter_map(|c| c["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!failed, "{tool} {arguments}: {text}");
        text
    }

    fn refused(&mut self, tool: &str, arguments: Value) -> String {
        let (failed, content) = self.call(tool, arguments.clone());
        let text = content[0]["text"].as_str().unwrap_or_default().to_string();
        assert!(failed, "{tool} {arguments} should be refused: {text}");
        text
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("klad-mcp-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn an_agent_app_paints_reads_looks_and_marks_a_note_done_through_every_tool() {
    let dir = scratch("tools");
    let (home, root) = (dir.join("home"), dir.join("drawings"));
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&root).unwrap();
    let mut s = Session::start(&home, &root);

    let tools = s.ask("tools/list", json!({}));
    let mut names: Vec<&str> = tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    names.sort_unstable();
    assert_eq!(names, ["edit", "look", "new", "note_done", "open", "read", "run"]);
    assert!(
        tools["tools"]
            .as_array()
            .unwrap()
            .iter()
            .all(|t| t["inputSchema"]["properties"].is_object()),
        "every tool says what it takes"
    );

    let made = s.text(
        "new",
        json!({"drawing": "cat", "width": 200, "height": 120, "background": "#ffffff"}),
    );
    let cat = root.canonicalize().unwrap().join("cat.klad");
    assert!(made.contains("cat.klad") && cat.join("doc.json").is_file(), "{made}");
    let again = s.refused("new", json!({"drawing": "cat"}));
    assert!(again.contains("there already"), "{again}");

    let layers: Value = serde_json::from_str(&s.text("read", json!({"drawing": "cat", "what": "layers"}))).unwrap();
    let layer = layers["layers"]
        .as_array()
        .and_then(|l| l.iter().find(|l| l["kind"] == "paint"))
        .and_then(|l| l["id"].as_str())
        .unwrap_or_else(|| panic!("a paint layer: {layers}"))
        .to_string();
    let ids = s.text(
        "edit",
        json!({"drawing": "cat", "ops": [{"op": "stroke", "layer": layer, "path": "M20 20 L180 100", "size": 10, "color": "#cc3322"}]}),
    );
    assert!(!ids.trim().is_empty(), "the edit says what it made: {ids}");
    let strokes = s.text("read", json!({"drawing": "cat", "what": "strokes", "layer": layer}));
    assert!(strokes.contains("\"brush\""), "the stroke is there: {strokes}");
    let lacking = s.refused("read", json!({"drawing": "cat", "what": "strokes"}));
    assert!(lacking.contains("needs a layer"), "{lacking}");

    let (failed, picture) = s.call("look", json!({"drawing": "cat", "overlay": "ids"}));
    assert!(!failed, "{picture:?}");
    assert_eq!(picture[0]["type"], "image");
    assert_eq!(picture[0]["mimeType"], "image/png");
    assert!(
        picture[0]["data"].as_str().unwrap().starts_with("iVBORw0KGgo"),
        "a PNG picture comes back"
    );

    let info = s.text("run", json!({"command": "info", "drawing": "cat"}));
    assert!(info.contains("200x120"), "{info}");
    let opened = s.refused("run", json!({"command": "open", "drawing": "cat"}));
    assert!(opened.contains("tool of its own"), "{opened}");
    let watching = s.refused("run", json!({"command": "watch", "drawing": "cat"}));
    assert!(watching.contains("not run from an agent app"), "{watching}");

    std::fs::create_dir_all(cat.join(".cache")).unwrap();
    std::fs::write(
        cat.join(".cache/notes.json"),
        r#"[{"id":1,"x":40,"y":40,"frame":0,"text":"make the line thicker","author":"human","atMs":0,"done":false}]"#,
    )
    .unwrap();
    let notes = s.text("read", json!({"drawing": "cat", "what": "notes"}));
    assert!(notes.contains("make the line thicker"), "{notes}");
    s.text("note_done", json!({"drawing": "cat", "note": 1}));
    let left = s.text("read", json!({"drawing": "cat", "what": "notes"}));
    assert!(
        !left.contains("make the line thicker"),
        "the done note leaves the open ones: {left}"
    );

    let elsewhere = dir.join("elsewhere.klad");
    let outside = s.refused("look", json!({"drawing": elsewhere.display().to_string()}));
    assert!(outside.contains("outside the folders klad works in"), "{outside}");
    let made_outside = s.refused("new", json!({"drawing": elsewhere.display().to_string()}));
    assert!(made_outside.contains("outside the folders"), "{made_outside}");
    let missing = s.refused("open", json!({"drawing": "dog"}));
    assert!(missing.contains("not a klad drawing"), "{missing}");

    let prompts = s.ask("prompts/list", json!({}));
    assert_eq!(prompts["prompts"][0]["name"], "klad");
    let guide = s.ask("prompts/get", json!({"name": "klad"}));
    let text = guide["messages"][0]["content"]["text"].as_str().unwrap_or_default();
    assert!(text.contains("klad apply"), "the prompt is the klad guide");
    let resource = s.ask("resources/read", json!({"uri": "klad://skill"}));
    assert_eq!(resource["contents"][0]["text"].as_str().unwrap_or_default(), text);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn klad_mcp_installs_itself_in_an_agent_apps_settings_and_takes_itself_out() {
    let dir = scratch("install");
    let run = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_klad"))
            .arg("mcp")
            .args(args)
            .env("HOME", &dir)
            .output()
            .unwrap();
        (
            out.status.success(),
            String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr),
        )
    };
    let (ok, status) = run(&["--status"]);
    assert!(
        ok && status.contains("Codex") && status.contains("not on this computer"),
        "{status}"
    );
    let (ok, said) = run(&["--install", "codex"]);
    assert!(ok && said.contains("Codex's settings"), "{said}");
    let config = std::fs::read_to_string(dir.join(".codex/config.toml")).unwrap();
    assert!(
        config.contains("[mcp_servers.klad]") && config.contains("args = [\"mcp\"]"),
        "{config}"
    );
    let (_, status) = run(&["--status"]);
    assert!(
        status
            .lines()
            .any(|l| l.starts_with("Codex") && l.contains("connected")),
        "{status}"
    );
    let (ok, printed) = run(&["--print", "claude-desktop"]);
    assert!(
        ok && printed.contains("\"mcpServers\"") && printed.contains("goes in"),
        "{printed}"
    );
    let (ok, said) = run(&["--uninstall", "codex"]);
    assert!(ok && said.contains("out of Codex's settings"), "{said}");
    assert!(
        !std::fs::read_to_string(dir.join(".codex/config.toml"))
            .unwrap()
            .contains("klad")
    );
    let (ok, said) = run(&["--install", "photoshop"]);
    assert!(!ok && said.contains("claude-desktop, codex"), "{said}");
    let _ = std::fs::remove_dir_all(&dir);
}
