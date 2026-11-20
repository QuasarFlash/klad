use klad_client::{Klad, Value};
use rmcp::handler::server::{router::tool::ToolRouter, wrapper::Parameters};
use rmcp::model::{
    CallToolResult, ContentBlock, GetPromptRequestParams, GetPromptResponse, GetPromptResult, Implementation,
    ListPromptsResult, ListResourcesResult, PaginatedRequestParams, Prompt, PromptMessage, ReadResourceRequestParams,
    ReadResourceResponse, ReadResourceResult, Resource, ResourceContents, Role, ServerCapabilities, ServerConfig,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt, schemars, tool, tool_handler, tool_router};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub const SKILL: &str = include_str!("../../../docs/skill/SKILL.md");
pub const SKILL_URI: &str = "klad://skill";
const LONGEST: f32 = 1024.0;

pub const REFUSED: [&str; 12] = [
    "open", "watch", "install", "stress", "scribble", "bench", "demo", "report", "updates", "skill", "samples", "mcp",
];

#[derive(Clone, Debug)]
pub struct Config {
    pub program: PathBuf,
    pub version: String,
    pub roots: Vec<PathBuf>,
    pub app: Option<PathBuf>,
}

#[derive(Clone)]
pub struct Server {
    config: Config,
    klad: Klad,
    tool_router: ToolRouter<Self>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct LookArgs {
    #[schemars(description = "the drawing, a .klad folder: a path, or a name in the drawings folder")]
    pub drawing: String,
    #[schemars(description = "the frame to show; 0 when left out")]
    pub frame: Option<u32>,
    #[schemars(
        description = "the picture's size as a part of the canvas (1 is real size); left out, the longest side fits 1024 px"
    )]
    pub scale: Option<f32>,
    #[schemars(description = "only this part of the canvas: x, y, width and height in canvas px")]
    pub region: Option<[f32; 4]>,
    #[schemars(description = "drawn on top, comma separated: ids, bbox, grid, onion, parts or arcs")]
    pub overlay: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum What {
    Layers,
    Strokes,
    Selection,
    Notes,
    Document,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ReadArgs {
    #[schemars(description = "the drawing, a .klad folder")]
    pub drawing: String,
    #[schemars(
        description = "layers: the layer tree at a frame; strokes: one layer's strokes and fills; selection: what the person selected in the editor; notes: the notes they pinned for you; document: the whole doc.json"
    )]
    pub what: What,
    #[schemars(description = "with strokes: the layer, by id or name")]
    pub layer: Option<String>,
    #[schemars(description = "with layers and strokes: the frame; 0 when left out")]
    pub frame: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct EditArgs {
    #[schemars(description = "the drawing, a .klad folder")]
    pub drawing: String,
    #[schemars(
        description = "JSON ops as klad apply takes them, one object each, for example {\"op\":\"stroke\",\"layer\":\"l1\",\"path\":\"M10 10 L200 120\",\"size\":8}; they land as one step the person can undo"
    )]
    pub ops: Vec<Value>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RunArgs {
    #[schemars(
        description = "a klad command, such as export, info, frames, undo or palette (the klad guide lists them)"
    )]
    pub command: String,
    #[schemars(description = "the drawing it works on, when it takes one")]
    pub drawing: Option<String>,
    #[schemars(description = "the rest of the command line, one argument each, for example [\"--png\", \"out.png\"]")]
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct NewArgs {
    #[schemars(description = "where the new drawing goes: a path ending in .klad, or a name for the drawings folder")]
    pub drawing: String,
    #[schemars(description = "the canvas width in px")]
    pub width: Option<u32>,
    #[schemars(description = "the canvas height in px")]
    pub height: Option<u32>,
    #[schemars(description = "the paper colour, such as #f4efe6; left out, the canvas is see-through")]
    pub background: Option<String>,
    #[schemars(description = "a pixel art drawing of this size, such as 64x64, instead of width and height")]
    pub pixel: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct NoteArgs {
    #[schemars(description = "the drawing, a .klad folder")]
    pub drawing: String,
    #[schemars(description = "the note's id, from read with what = notes")]
    pub note: u32,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct OpenArgs {
    #[schemars(description = "the drawing to show in klad's editor")]
    pub drawing: String,
}

fn failed(text: impl Into<String>) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(text)])
}

fn said(text: impl Into<String>) -> CallToolResult {
    CallToolResult::success(vec![ContentBlock::text(text)])
}

fn tilde(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => PathBuf::from(home).join(rest),
        _ => PathBuf::from(path),
    }
}

fn settled(path: &Path) -> Option<PathBuf> {
    path.canonicalize().ok().or_else(|| {
        let name = path.file_name()?;
        path.parent()
            .map(|p| if p.as_os_str().is_empty() { Path::new(".") } else { p })
            .and_then(|p| p.canonicalize().ok())
            .map(|p| p.join(name))
    })
}

fn listed(roots: &[PathBuf]) -> String {
    roots
        .iter()
        .map(|r| r.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    bytes
        .chunks(3)
        .flat_map(|chunk| {
            let n = chunk
                .iter()
                .enumerate()
                .fold(0u32, |n, (i, b)| n | (*b as u32) << (16 - 8 * i));
            (0..4).map(move |k| match k <= chunk.len() {
                true => ALPHABET[(n >> (18 - 6 * k) & 63) as usize] as char,
                false => '=',
            })
        })
        .collect()
}

impl Server {
    pub fn new(config: Config) -> Self {
        Self {
            klad: Klad::at(&config.program),
            config,
            tool_router: Self::tool_router(),
        }
    }

    pub fn place(&self, drawing: &str) -> Result<PathBuf, String> {
        let given = tilde(drawing);
        let path = match given.is_absolute() {
            true => given,
            false => match self.config.roots.first() {
                Some(root) => root.join(given),
                None => given,
            },
        };
        let path = settled(&path).ok_or_else(|| format!("{drawing}: its folder does not exist"))?;
        let inside = self
            .config
            .roots
            .iter()
            .filter_map(|r| r.canonicalize().ok())
            .any(|r| path.starts_with(&r));
        match inside {
            true => Ok(path),
            false => Err(format!(
                "{} is outside the folders klad works in ({}). The person can add its folder in klad: File > Connect an agent app.",
                path.display(),
                listed(&self.config.roots)
            )),
        }
    }

    fn existing(&self, drawing: &str) -> Result<PathBuf, String> {
        let path = self.place(drawing)?;
        let named = match drawing.ends_with(".klad") {
            true => None,
            false => self.place(&format!("{drawing}.klad")).ok(),
        };
        std::iter::once(path.clone())
            .chain(named)
            .find(|p| p.join("doc.json").is_file())
            .ok_or_else(|| {
                format!(
                    "{} is not a klad drawing (no doc.json in it); new makes one",
                    path.display()
                )
            })
    }

    async fn blocking<T: Send + 'static>(
        &self,
        work: impl FnOnce(Klad) -> Result<T, String> + Send + 'static,
    ) -> Result<T, String> {
        let klad = self.klad.clone();
        tokio::task::spawn_blocking(move || work(klad))
            .await
            .map_err(|e| format!("klad stopped: {e}"))?
    }

    fn instructions(&self) -> String {
        format!(
            "klad is a 2D painting and animation app. You paint and edit .klad drawings that the person may have open in klad's editor at the same time: your changes show there, and they can undo each one. \
             Read the klad guide first (the prompt or the resource named klad): it holds the craft and every op. \
             look shows a drawing, read gives its layers, strokes, selection and notes, edit paints and changes it with JSON ops, run does any other klad command (export, frames, undo...), new makes a drawing and open shows one in klad. \
             Notes the person pinned on the canvas are requests for you: read them, do them, then note_done. \
             Drawings live in: {}.",
            listed(&self.config.roots)
        )
    }
}

#[tool_router]
impl Server {
    #[tool(
        description = "See a drawing: renders a frame of a .klad drawing to a picture, optionally a region of it with ids, boxes or the grid drawn on top."
    )]
    async fn look(&self, Parameters(args): Parameters<LookArgs>) -> CallToolResult {
        let path = match self.existing(&args.drawing) {
            Ok(p) => p,
            Err(e) => return failed(e),
        };
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let out = std::env::temp_dir().join(format!(
            "klad-mcp-{}-{}.png",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let frame = args.frame.unwrap_or(0);
        let shown = out.clone();
        let picture = self
            .blocking(move |klad| {
                let drawing = klad.drawing(&path);
                let canvas = drawing.document().map_err(|e| e.to_string())?;
                let side = |k: &str| canvas["canvas"][k].as_f64().unwrap_or(0.0) as f32;
                let longest = match args.region {
                    Some([_, _, w, h]) => w.max(h),
                    None => side("width").max(side("height")),
                };
                let scale = args.scale.or_else(|| (longest > LONGEST).then(|| LONGEST / longest));
                let mut line: Vec<String> = vec!["--frame".into(), frame.to_string(), "--out".into()];
                line.push(out.display().to_string());
                line.extend(scale.map(|s| ["--scale".into(), s.to_string()]).into_iter().flatten());
                line.extend(
                    args.region
                        .map(|[x, y, w, h]| ["--region".into(), format!("{x},{y},{w},{h}")])
                        .into_iter()
                        .flatten(),
                );
                line.extend(args.overlay.map(|o| ["--overlay".into(), o]).into_iter().flatten());
                drawing.command("render", &line).map_err(|e| e.to_string())?;
                let bytes = std::fs::read(&out).map_err(|e| format!("reading {}: {e}", out.display()))?;
                let _ = std::fs::remove_file(&out);
                Ok(bytes)
            })
            .await;
        match picture {
            Ok(bytes) => CallToolResult::success(vec![
                ContentBlock::image(base64(&bytes), "image/png"),
                ContentBlock::text(format!("{}, frame {frame}", args.drawing)),
            ]),
            Err(e) => {
                let _ = std::fs::remove_file(&shown);
                failed(e)
            }
        }
    }

    #[tool(
        description = "Read a drawing as JSON: its layers at a frame, a layer's strokes and fills, what the person selected in the editor, the notes they pinned for you, or the whole document."
    )]
    async fn read(&self, Parameters(args): Parameters<ReadArgs>) -> CallToolResult {
        let path = match self.existing(&args.drawing) {
            Ok(p) => p,
            Err(e) => return failed(e),
        };
        let frame = args.frame.unwrap_or(0);
        let read = self
            .blocking(move |klad| {
                let drawing = klad.drawing(&path);
                let value = match args.what {
                    What::Layers => drawing.ls(frame),
                    What::Strokes => match &args.layer {
                        Some(layer) => drawing.strokes(layer, frame),
                        None => return Err("strokes needs a layer: read with what = layers lists them".into()),
                    },
                    What::Selection => drawing.selection(),
                    What::Notes => drawing.notes(),
                    What::Document => drawing.document(),
                };
                value
                    .map_err(|e| e.to_string())
                    .map(|v| serde_json::to_string_pretty(&v).unwrap_or_default())
            })
            .await;
        match read {
            Ok(text) => said(text),
            Err(e) => failed(e),
        }
    }

    #[tool(
        description = "Paint and change a drawing with JSON ops (stroke, fill, add, set, rm, transform and the rest the klad guide lists). They land in one step the person can undo in the editor; the ids of new things come back."
    )]
    async fn edit(&self, Parameters(args): Parameters<EditArgs>) -> CallToolResult {
        let path = match self.existing(&args.drawing) {
            Ok(p) => p,
            Err(e) => return failed(e),
        };
        if args.ops.is_empty() {
            return failed(
                "no ops: give at least one, such as {\"op\":\"stroke\",\"layer\":\"l1\",\"path\":\"M10 10 L200 120\"}",
            );
        }
        let done = self
            .blocking(move |klad| klad.drawing(&path).apply(args.ops).map_err(|e| e.to_string()))
            .await;
        match done {
            Ok(lines) if lines.is_empty() => said("done"),
            Ok(lines) => said(lines.join("\n")),
            Err(e) => failed(e),
        }
    }

    #[tool(
        description = "Run any other klad command on a drawing, such as export, info, frames, palette, undo or redo, with its arguments as a list. Commands that leave the drawing (open, watch, install and the like) are refused: use open to show a drawing."
    )]
    async fn run(&self, Parameters(args): Parameters<RunArgs>) -> CallToolResult {
        let command = args.command.trim().trim_start_matches("klad ").to_string();
        if REFUSED.contains(&command.as_str()) {
            return failed(match command.as_str() {
                "open" => "open is a tool of its own: call open with the drawing".to_string(),
                other => format!("klad {other} is not run from an agent app"),
            });
        }
        let path = match args
            .drawing
            .as_deref()
            .map(|d| self.existing(d).or_else(|_| self.place(d)))
            .transpose()
        {
            Ok(p) => p,
            Err(e) => return failed(e),
        };
        let line: Vec<String> = std::iter::once(command)
            .chain(path.map(|p| p.display().to_string()))
            .chain(args.args)
            .collect();
        match self
            .blocking(move |klad| klad.run(&line).map_err(|e| e.to_string()))
            .await
        {
            Ok(text) if text.trim().is_empty() => said("done"),
            Ok(text) => said(text),
            Err(e) => failed(e),
        }
    }

    #[tool(
        name = "new",
        description = "Make a new drawing: a canvas of a size with an optional paper colour, or a pixel art drawing (pixel = 64x64). Its path ends in .klad."
    )]
    async fn make(&self, Parameters(args): Parameters<NewArgs>) -> CallToolResult {
        let named = match args.drawing.ends_with(".klad") {
            true => args.drawing.clone(),
            false => format!("{}.klad", args.drawing),
        };
        let path = match self.place(&named) {
            Ok(p) => p,
            Err(e) => return failed(e),
        };
        if path.exists() {
            return failed(format!("{} is there already: pick another name", path.display()));
        }
        let mut line: Vec<String> = vec!["new".into(), path.display().to_string()];
        line.extend(
            args.width
                .map(|w| ["--width".into(), w.to_string()])
                .into_iter()
                .flatten(),
        );
        line.extend(
            args.height
                .map(|h| ["--height".into(), h.to_string()])
                .into_iter()
                .flatten(),
        );
        line.extend(
            args.background
                .map(|b| ["--background".into(), b])
                .into_iter()
                .flatten(),
        );
        line.extend(args.pixel.map(|p| ["--pixel".into(), p]).into_iter().flatten());
        match self
            .blocking(move |klad| klad.run(&line).map_err(|e| e.to_string()))
            .await
        {
            Ok(text) => said(format!("{}\n{}", path.display(), text.trim())),
            Err(e) => failed(e),
        }
    }

    #[tool(description = "Mark a note the person pinned on the canvas as done, once you have done what it asks.")]
    async fn note_done(&self, Parameters(args): Parameters<NoteArgs>) -> CallToolResult {
        let path = match self.existing(&args.drawing) {
            Ok(p) => p,
            Err(e) => return failed(e),
        };
        let note = args.note.to_string();
        let done = self
            .blocking(move |klad| {
                klad.drawing(&path)
                    .command("notes", ["--done", note.as_str()])
                    .map_err(|e| e.to_string())
            })
            .await;
        match done {
            Ok(text) if text.trim().is_empty() => said(format!("note {} is done", args.note)),
            Ok(text) => said(text),
            Err(e) => failed(e),
        }
    }

    #[tool(
        description = "Show a drawing in klad's editor, so the person sees it; klad starts if it is not running. The person keeps drawing there while you edit."
    )]
    async fn open(&self, Parameters(args): Parameters<OpenArgs>) -> CallToolResult {
        let path = match self.existing(&args.drawing) {
            Ok(p) => p,
            Err(e) => return failed(e),
        };
        let launched = match &self.config.app {
            Some(app) => std::process::Command::new("open").arg("-a").arg(app).arg(&path).spawn(),
            None => std::process::Command::new(&self.config.program)
                .arg("open")
                .arg(&path)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn(),
        };
        match launched {
            Ok(_) => said(format!("{} is open in klad", path.display())),
            Err(e) => failed(format!("klad could not be started: {e}")),
        }
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_prompts()
                .enable_resources()
                .build(),
        )
        .with_server_info(Implementation::new("klad", self.config.version.clone()))
        .with_instructions(self.instructions())
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, ErrorData> {
        Ok(ListPromptsResult::with_all_items(vec![Prompt::new(
            "klad",
            Some("How to paint, edit and animate with klad: the whole guide"),
            None,
        )]))
    }

    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResponse, ErrorData> {
        match request.name.as_str() {
            "klad" => Ok(GetPromptResult::new(vec![PromptMessage::new_text(Role::User, SKILL)]).into()),
            other => Err(ErrorData::invalid_params(
                format!("no prompt {other}: klad has one, named klad"),
                None,
            )),
        }
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        Ok(ListResourcesResult::with_all_items(vec![
            Resource::new(SKILL_URI, "klad")
                .with_title("The klad guide")
                .with_description("How to paint, edit and animate with klad, and every op")
                .with_mime_type("text/markdown"),
        ]))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        match request.uri.as_str() {
            SKILL_URI => Ok(ReadResourceResult::new(vec![ResourceContents::text(SKILL, SKILL_URI)]).into()),
            other => Err(ErrorData::resource_not_found(
                format!("no resource {other}: klad has {SKILL_URI}"),
                None,
            )),
        }
    }
}

pub fn serve(config: Config) -> Result<(), Box<dyn std::error::Error>> {
    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    runtime.block_on(async move {
        let service = Server::new(config).serve(rmcp::transport::stdio()).await?;
        service.waiting().await?;
        Ok::<(), Box<dyn std::error::Error>>(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_standard_alphabet_and_padding() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(&[0xff, 0xee, 0x01]), "/+4B");
    }

    #[test]
    fn drawings_stay_inside_the_folders_klad_was_given() {
        let root = std::env::temp_dir().join(format!("klad-mcp-roots-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("art")).unwrap();
        let server = Server::new(Config {
            program: PathBuf::from("klad"),
            version: "test".into(),
            roots: vec![root.clone()],
            app: None,
        });
        let inside = server.place("art/cat.klad").unwrap();
        assert!(
            inside.ends_with("art/cat.klad"),
            "a name lands in the first folder: {inside:?}"
        );
        assert!(server.place(&root.join("dog.klad").display().to_string()).is_ok());
        let outside = server.place("/etc/passwd").unwrap_err();
        assert!(outside.contains("outside the folders"), "{outside}");
        let sneaky = server.place("art/../../escape.klad").unwrap_err();
        assert!(sneaky.contains("outside the folders"), "{sneaky}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
