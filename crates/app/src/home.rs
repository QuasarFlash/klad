use crate::commands::{Bindings, Cmd};
use crate::editor::{Editor, Want};
use crate::gpu::CanvasGpu;
use crate::menus::{self, Picked};
use crate::messages::Tone;
use crate::picker;
use crate::settings::{self, Settings};
use crate::start::{self, Section, Start};
use crate::theme;
use crate::updates;
use anyhow::{Result, anyhow};
use egui::RichText;
use egui_wgpu::wgpu;
use std::path::{Path, PathBuf};

const WIDTH: f32 = 760.0;

pub fn run(path: Option<PathBuf>) -> Result<()> {
    crate::opened::listen();
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        crate::report::log(&format!("the editor stopped: {}", crate::report::panic_text(info)));
        previous(info);
    }));
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(start::title(path.as_deref()))
            .with_inner_size([1600.0, 1000.0]),
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: egui_wgpu::WgpuConfiguration {
            surface: egui_wgpu::SurfaceConfig {
                present_mode: wgpu::PresentMode::AutoVsync,
                desired_maximum_frame_latency: Some(1),
            },
            ..Default::default()
        },
        ..Default::default()
    };
    eframe::run_native(
        "klad",
        options,
        Box::new(move |cc| Ok(Box::new(Home::new(cc, path.as_deref())?))),
    )
    .map_err(|e| anyhow!("{e}"))
}

pub(crate) struct Home {
    pub(crate) editor: Option<Box<Editor>>,
    pub(crate) parked: Vec<Box<Editor>>,
    pub(crate) at: usize,
    pub(crate) start: Option<Start>,
    pub(crate) settings: Settings,
    pub(crate) settings_path: Option<PathBuf>,
    gpu: Option<egui_wgpu::RenderState>,
    themed: bool,
    pub(crate) update: Option<crate::updates::Release>,
    checking: Option<std::sync::mpsc::Receiver<updates::Checked>>,
    native_menus: bool,
    menus_built: Option<u64>,
    menus_shown: Option<std::time::Instant>,
    cycling: i32,
}

fn used_twice(e: &anyhow::Error) -> bool {
    e.downcast_ref::<klad_core::repair::UsedTwice>().is_some()
}

impl Home {
    fn new(cc: &eframe::CreationContext, path: Option<&Path>) -> Result<Self> {
        let gpu = cc
            .wgpu_render_state
            .clone()
            .ok_or_else(|| anyhow!("the wgpu renderer is not available"))?;
        let settings_path = settings::default_path();
        let settings = settings_path.as_deref().map(Settings::load).unwrap_or_default();
        let mut home = Self::with(settings, settings_path, Some(gpu));
        home.native_menus = true;
        if updates::due(
            home.settings.check_updates,
            home.settings.updates_checked,
            updates::now(),
        ) {
            home.checking = Some(updates::spawn(updates::feed()));
        }
        home.begin(&cc.egui_ctx, path);
        Ok(home)
    }

    pub(crate) fn with(
        settings: Settings,
        settings_path: Option<PathBuf>,
        gpu: Option<egui_wgpu::RenderState>,
    ) -> Self {
        Self {
            editor: None,
            parked: Vec::new(),
            at: 0,
            start: None,
            settings,
            settings_path,
            gpu,
            themed: false,
            update: None,
            checking: None,
            native_menus: false,
            menus_built: None,
            menus_shown: None,
            cycling: 0,
        }
    }

    pub(crate) fn begin(&mut self, ctx: &egui::Context, path: Option<&Path>) {
        let Some(path) = path else {
            self.start = Some(Start::new(&self.settings, Section::New));
            return;
        };
        if let Err(e) = self.open(ctx, path) {
            let mut start = Start::new(&self.settings, Section::Open);
            start.path = path.display().to_string();
            start.error = Some(open_words(path, &e));
            start.repair = used_twice(&e).then(|| path.to_path_buf());
            self.start = Some(start);
        }
    }

    pub(crate) fn open(&mut self, ctx: &egui::Context, path: &Path) -> Result<()> {
        let (made, told) = start::brought_in(path)?;
        let path = made.as_path();
        let same = |e: &Editor| crate::start::same_drawing(&e.scene.bundle.root, path);
        let open_already = match self.parked.iter().position(|e| same(e)) {
            Some(found) => {
                let slot = if found < self.at { found } else { found + 1 };
                self.show_slot(ctx, slot);
                true
            }
            None => self.editor.as_deref().is_some_and(same),
        };
        if open_already {
            if let (Some(words), Some(editor)) = (told, self.editor.as_mut()) {
                editor.say(Tone::Done, words);
            }
            self.start = None;
            return Ok(());
        }
        let mut settings = self
            .editor
            .as_ref()
            .map_or_else(|| self.settings.clone(), |e| e.settings.clone());
        let mut editor = Editor::open(ctx.clone(), path)?;
        if let Some(words) = self.start.as_mut().and_then(|s| s.repaired.take()) {
            editor.say(Tone::Done, words);
        }
        if let Some(words) = told {
            editor.say(Tone::Done, words);
        }
        settings.opened(path);
        settings.offer_real_size(editor.scene.bundle.doc.pixel_art());
        editor.settings = settings.clone();
        editor.settings_path = self.settings_path.clone();
        editor.settings_dirty = true;
        if let Some(gpu) = &self.gpu {
            gpu.renderer.write().callback_resources.insert(CanvasGpu::new(
                &gpu.device,
                gpu.target_format,
                editor.scene.size(),
            ));
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(start::title(Some(path))));
        self.settings = settings;
        let mut editor = Box::new(editor);
        if let Some(old) = self.editor.take() {
            old.hand_over(&mut editor);
            self.parked.insert(self.at.min(self.parked.len()), old);
            self.at = (self.at + 1).min(self.parked.len());
        }
        self.editor = Some(editor);
        self.start = None;
        Ok(())
    }

    pub(crate) fn drawings(&self) -> Vec<PathBuf> {
        let mut all: Vec<PathBuf> = self.parked.iter().map(|e| e.scene.bundle.root.clone()).collect();
        if let Some(editor) = &self.editor {
            all.insert(self.at.min(all.len()), editor.scene.bundle.root.clone());
        }
        all
    }

    pub(crate) fn show_slot(&mut self, ctx: &egui::Context, slot: usize) {
        let count = self.parked.len() + usize::from(self.editor.is_some());
        if slot == self.at || slot >= count || self.editor.is_none() {
            return;
        }
        let index = if slot < self.at { slot } else { slot - 1 };
        let mut next = self.parked.remove(index);
        if let Some(old) = self.editor.take() {
            old.hand_over(&mut next);
            old.hand_settings(&mut next);
            let back = if slot < self.at { self.at - 1 } else { self.at };
            self.parked.insert(back.min(self.parked.len()), old);
        }
        self.at = slot;
        self.reshow(ctx, next);
    }

    fn reshow(&mut self, ctx: &egui::Context, mut editor: Box<Editor>) {
        if let Some(gpu) = &self.gpu {
            gpu.renderer.write().callback_resources.insert(CanvasGpu::new(
                &gpu.device,
                gpu.target_format,
                editor.scene.size(),
            ));
        }
        editor.reshown();
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(start::title(Some(
            &editor.scene.bundle.root,
        ))));
        self.settings = editor.settings.clone();
        self.editor = Some(editor);
    }

    fn cycle(&mut self, ctx: &egui::Context, by: i32) {
        let count = self.parked.len() + usize::from(self.editor.is_some());
        if count < 2 {
            return;
        }
        let slot = (self.at as i64 + by as i64).rem_euclid(count as i64) as usize;
        self.show_slot(ctx, slot);
    }

    fn drawing_tabs(&mut self, ui: &mut egui::Ui) -> Option<(usize, bool)> {
        if self.parked.is_empty() || self.editor.is_none() {
            return None;
        }
        let drawings = self.drawings();
        let mut picked = None;
        egui::Panel::top("drawings").resizable(false).show(ui, |ui| {
            ui.horizontal(|ui| {
                drawings.iter().enumerate().for_each(|(slot, path)| {
                    let name = path
                        .file_stem()
                        .map_or_else(|| path.display().to_string(), |s| s.to_string_lossy().to_string());
                    let shown = slot == self.at;
                    if ui
                        .add(egui::Button::selectable(shown, name).min_size(egui::vec2(0.0, theme::TARGET)))
                        .on_hover_text(format!("{} (ctrl Tab goes to the next drawing)", path.display()))
                        .clicked()
                    {
                        picked = Some((slot, false));
                    }
                    if ui
                        .add(egui::Button::new(theme::icon::CLOSE).min_size(egui::vec2(theme::TARGET, theme::TARGET)))
                        .on_hover_text("close this drawing (it stays saved)")
                        .clicked()
                    {
                        picked = Some((slot, true));
                    }
                    ui.separator();
                });
            });
        });
        picked
    }

    fn close_slot(&mut self, ctx: &egui::Context, slot: usize) {
        if slot == self.at {
            self.close(ctx);
            return;
        }
        let index = if slot < self.at { slot } else { slot - 1 };
        if index < self.parked.len() {
            let parked = self.parked.remove(index);
            self.settings = self
                .editor
                .as_ref()
                .map_or_else(|| parked.settings.clone(), |e| e.settings.clone());
            if slot < self.at {
                self.at -= 1;
            }
        }
    }

    fn checked(&mut self) {
        let Some(result) = self.checking.as_ref().and_then(|c| c.try_recv().ok()) else {
            return;
        };
        self.checking = None;
        let now = updates::now();
        self.settings.updates_checked = now;
        match self.editor.as_mut() {
            Some(editor) => {
                editor.settings.updates_checked = now;
                editor.settings_dirty = true;
            }
            None => {
                if let Some(path) = &self.settings_path
                    && let Err(e) = self.settings.save(path)
                    && let Some(start) = self.start.as_mut()
                {
                    start.error = Some(format!(
                        "your settings could not be saved ({e:#}); they apply until klad closes"
                    ));
                }
            }
        }
        if let Ok(Some(release)) = result {
            if let Some(editor) = self.editor.as_mut() {
                editor.say(Tone::Info, updates::summary(&release));
            }
            self.update = Some(release);
        }
    }

    fn tend_menus(&mut self, ctx: &egui::Context) {
        if !self.native_menus {
            return;
        }
        let settings = self.editor.as_ref().map_or(&self.settings, |e| &e.settings);
        let built = menus::signature(&settings.keys, &settings.documents);
        if self.menus_built != Some(built) {
            menus::install(ctx, &Bindings::from_settings(&settings.keys), &settings.documents);
            self.menus_built = Some(built);
        }
        if self
            .menus_shown
            .is_none_or(|t| t.elapsed() > std::time::Duration::from_millis(400))
        {
            menus::show(
                self.editor
                    .as_ref()
                    .map_or_else(menus::outside_a_drawing, |e| e.menu_state()),
            );
            self.menus_shown = Some(std::time::Instant::now());
        }
    }

    fn picked(&mut self, ctx: &egui::Context) -> Option<PathBuf> {
        menus::take()
            .into_iter()
            .fold(None, |path, picked| match (picked, self.editor.as_mut()) {
                (Picked::Open(p), _) => Some(p),
                (Picked::Run(cmd), Some(editor)) => {
                    editor.run_later(cmd);
                    path
                }
                (Picked::Run(Cmd::NewDocument), None) => {
                    self.start = Some(Start::new(&self.settings, Section::New));
                    path
                }
                (Picked::Run(Cmd::OpenRecent), None) => {
                    self.start = Some(Start::new(&self.settings, Section::Recent));
                    path
                }
                (Picked::Run(Cmd::OpenDocument), None) if picker::NATIVE => {
                    let folder = self.settings.drawings_folder();
                    picker::choose_drawing(Some(&folder), "Open").or(path)
                }
                (Picked::Run(Cmd::OpenDocument), None) => {
                    self.start = Some(Start::new(&self.settings, Section::Open));
                    path
                }
                (Picked::Run(Cmd::Quit), None) => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    path
                }
                (Picked::Run(_), None) => path,
            })
    }

    fn close(&mut self, ctx: &egui::Context) {
        let Some(editor) = self.editor.take() else {
            return;
        };
        self.settings = editor.settings.clone();
        if !self.parked.is_empty() {
            let index = self.at.min(self.parked.len() - 1);
            let mut next = self.parked.remove(index);
            editor.hand_over(&mut next);
            editor.hand_settings(&mut next);
            self.at = index;
            self.reshow(ctx, next);
            if let Some(path) = &self.settings_path
                && let Err(e) = self.settings.save(path)
                && let Some(shown) = self.editor.as_mut()
            {
                shown.error(format!("saving your settings: {e:#}"));
            }
            return;
        }
        if let Some(path) = &self.settings_path
            && let Err(e) = self.settings.save(path)
        {
            let mut start = Start::new(&self.settings, Section::Recent);
            start.error = Some(format!("saving your settings: {e:#}"));
            self.start = Some(start);
        } else {
            self.start = Some(Start::new(&self.settings, Section::Recent));
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(start::title(None)));
    }

    pub(crate) fn frame(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.tend_menus(&ctx);
        let from_menu = self.picked(&ctx);
        self.checked();
        if self.checking.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
        if let Some(editor) = self.editor.as_mut()
            && editor.update.is_none()
        {
            editor.update.clone_from(&self.update);
        }
        let mut chosen = None;
        let mut closing = false;
        let mut gone = None;
        let mut cycle = std::mem::take(&mut self.cycling);
        if !self.parked.is_empty() {
            cycle += ui.input_mut(|i| {
                let back = i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::Tab);
                let on = i.consume_key(egui::Modifiers::CTRL, egui::Key::Tab);
                i32::from(on) - i32::from(back)
            });
        }
        if let Some((slot, close)) = self.drawing_tabs(ui) {
            match close {
                true => self.close_slot(&ctx, slot),
                false => self.show_slot(&ctx, slot),
            }
        }
        match self.editor.as_mut() {
            Some(editor) => {
                editor.frame(ui);
                match editor.wants.take() {
                    Some(Want::Cycle(by)) => cycle += by,
                    Some(Want::Close) => closing = true,
                    Some(Want::Gone) => gone = Some(editor.scene.bundle.root.clone()),
                    Some(Want::Path(path)) => chosen = Some(path),
                    Some(Want::New) => self.start = Some(Start::new(&editor.settings, Section::New)),
                    Some(Want::Recent) => self.start = Some(Start::new(&editor.settings, Section::Recent)),
                    Some(Want::Open) if picker::NATIVE => {
                        let folder = editor.settings.drawings_folder();
                        chosen = picker::choose_drawing(Some(&folder), "Open").and_then(|path| {
                            start::openable(&path)
                                .map_err(|e| editor.error(format!("{e:#}")))
                                .ok()
                                .map(|_| path)
                        });
                    }
                    Some(Want::Open) => self.start = Some(Start::new(&editor.settings, Section::Open)),
                    None => {}
                }
                if let Some(start) = self.start.as_mut() {
                    start.open = Some(editor.scene.bundle.root.clone());
                    let height = (ctx.content_rect().height() - 120.0).clamp(320.0, 760.0);
                    let modal = egui::Modal::new(egui::Id::unique("start window")).show(&ctx, |ui| {
                        ui.set_width(WIDTH);
                        ui.set_height(height);
                        let back = ui
                            .horizontal(|ui| {
                                ui.heading("Drawings");
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.button("Back to this drawing").clicked()
                                })
                                .inner
                            })
                            .inner;
                        ui.add_space(8.0);
                        chosen = start.ui(ui, &mut editor.settings);
                        back
                    });
                    editor.settings_dirty |= std::mem::take(&mut start.changed);
                    if modal.inner || modal.should_close() {
                        self.start = None;
                    }
                }
            }
            None => {
                if !self.themed {
                    theme::install(&ctx);
                    self.themed = true;
                }
                if (ctx.zoom_factor() - self.settings.ui_scale).abs() > 1e-3 {
                    ctx.set_zoom_factor(self.settings.ui_scale);
                }
                let start = self
                    .start
                    .get_or_insert_with(|| Start::new(&self.settings, Section::New));
                egui::CentralPanel::default().show(ui, |ui| {
                    let margin = ((ui.available_width() - WIDTH) / 2.0).max(16.0);
                    ui.add_space(32.0);
                    ui.horizontal_top(|ui| {
                        ui.add_space(margin);
                        ui.vertical(|ui| {
                            ui.set_max_width(WIDTH);
                            ui.horizontal(|ui| {
                                ui.heading(RichText::new("klad").size(26.0));
                                ui.label(
                                    RichText::new(format!("version {}", crate::VERSION)).color(crate::theme::WEAK),
                                );
                            });
                            if let Some(release) = &self.update {
                                ui.horizontal(|ui| {
                                    ui.label(updates::summary(release));
                                    ui.hyperlink_to("what changed", &release.url);
                                });
                            }
                            ui.add_space(12.0);
                            chosen = start.ui(ui, &mut self.settings);
                        });
                    });
                });
                if std::mem::take(&mut start.changed)
                    && let Some(path) = &self.settings_path
                    && let Err(e) = self.settings.save(path)
                {
                    start.error = Some(format!("saving your settings: {e:#}"));
                }
            }
        }
        if let Some(root) = gone {
            self.close(&ctx);
            let name = root
                .file_stem()
                .map_or_else(|| root.display().to_string(), |s| s.to_string_lossy().to_string());
            if let Some(start) = self.start.as_mut() {
                start.error = Some(format!(
                    "{name} was moved or deleted while it was open, so it is closed here. Every change made until then is saved in its folder: open it from where it is now."
                ));
            }
        }
        if closing {
            self.close(&ctx);
        }
        if cycle != 0 {
            self.cycle(&ctx, cycle);
        }
        let tour = self.start.as_ref().is_some_and(|s| s.tour);
        if let Some(path) = chosen
            .or(from_menu)
            .or_else(|| dropped(&ctx))
            .or_else(crate::opened::take)
            && let Err(e) = self.open(&ctx, &path).inspect(|_| {
                if let Some(editor) = self.editor.as_mut().filter(|_| tour) {
                    editor.start_first_steps();
                }
            })
        {
            let message = open_words(&path, &e);
            match (self.start.as_mut(), self.editor.as_mut()) {
                (Some(start), _) => {
                    start.error = Some(message);
                    start.repair = used_twice(&e).then(|| path.clone());
                }
                (None, Some(editor)) => editor.error(message),
                (None, None) => {}
            }
        }
        self.open_links(&ctx);
    }

    fn open_links(&mut self, ctx: &egui::Context) {
        let links: Vec<String> = ctx.output_mut(|o| {
            let (links, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut o.commands)
                .into_iter()
                .partition(|c| matches!(c, egui::OutputCommand::OpenUrl(_)));
            o.commands = rest;
            links
                .into_iter()
                .filter_map(|c| match c {
                    egui::OutputCommand::OpenUrl(open) => Some(open.url),
                    _ => None,
                })
                .collect()
        });
        links.into_iter().for_each(|url| {
            if let Err(e) = crate::places::open_outside(&url) {
                let message = format!("could not open {url}: {e}");
                match (self.editor.as_mut(), self.start.as_mut()) {
                    (Some(editor), _) => editor.error(message),
                    (None, Some(start)) => start.error = Some(message),
                    (None, None) => {}
                }
            }
        });
    }
}

fn dropped(ctx: &egui::Context) -> Option<PathBuf> {
    ctx.input(|i| {
        i.raw
            .dropped_files
            .iter()
            .map(|f| f.path().to_path_buf())
            .find(|p| start::foreign(p) || p.extension().is_some_and(|e| e.eq_ignore_ascii_case("klad")))
    })
}

impl eframe::App for Home {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.frame(ui);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        self.editor
            .as_ref()
            .map_or_else(|| theme::surround(self.settings.defaults.surround), |e| e.clear())
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        self.cycling += crate::layout::take_ctrl_tabs(raw_input);
        if let Some(editor) = self.editor.as_mut() {
            editor.take_tabs(ctx, raw_input);
        }
    }
}

fn open_words(path: &Path, e: &anyhow::Error) -> String {
    let name = path
        .file_name()
        .map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().to_string());
    if crate::start::is_picture(path) {
        return crate::start::picture_words(&name);
    }
    if e.chain().any(|c| c.downcast_ref::<serde_json::Error>().is_some()) {
        return format!(
            "{name} cannot be opened: its doc.json is damaged (cut short, or changed by hand). The strokes are still in the folder: put doc.json back from a backup or from git if you keep one, or send the drawing with Help > Report a problem"
        );
    }
    format!("opening {}: {e:#}", path.display())
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::accesskit::Role;
    use egui::{Key, Modifiers, vec2};
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use klad_core::bundle::{Blank, Bundle};

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("draw-home-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::canonicalize(&dir).unwrap()
    }

    fn home(dir: &Path, documents: &[&Path]) -> Home {
        let settings = Settings {
            drawings: Some(dir.display().to_string()),
            documents: documents.iter().map(|p| p.display().to_string()).collect(),
            ..Settings::default()
        };
        Home::with(settings, None, None)
    }

    fn harness(home: Home) -> Harness<'static, Home> {
        let mut h = Harness::builder()
            .with_size(vec2(1280.0, 860.0))
            .build_ui_state(|ui, home: &mut Home| home.frame(ui), home);
        h.run_steps(3);
        h
    }

    #[derive(Debug)]
    struct Dropped(PathBuf);

    impl egui::DroppedFile for Dropped {
        fn path(&self) -> &Path {
            &self.0
        }

        fn bytes(&self) -> Result<Vec<u8>, String> {
            std::fs::read(&self.0).map_err(|e| e.to_string())
        }
    }

    fn small() -> Blank {
        Blank {
            width: 320,
            height: 200,
            ..Blank::default()
        }
    }

    fn document_menu<'a>(h: &'a Harness<'static, Home>) -> egui_kittest::Node<'a> {
        h.query_by_label(theme::icon::FILE)
            .unwrap_or_else(|| h.get_by_label(theme::icon::MORE))
    }

    fn opened(h: &Harness<'static, Home>) -> PathBuf {
        h.state()
            .editor
            .as_ref()
            .expect("an open drawing")
            .scene
            .bundle
            .root
            .clone()
    }

    #[test]
    fn a_drawing_dropped_on_the_window_opens_from_the_start_window_and_over_another_drawing() {
        let dir = folder("dropped");
        let (one, two) = (dir.join("one.klad"), dir.join("two.klad"));
        Bundle::blank(&one, &small(), "cli").unwrap();
        Bundle::blank(&two, &small(), "cli").unwrap();
        let mut first = home(&dir, &[]);
        first.begin(&egui::Context::default(), None);
        let mut h = harness(first);
        assert!(h.state().editor.is_none());
        let drop = |h: &mut Harness<'static, Home>, path: &Path| {
            h.input_mut()
                .dropped_files
                .push(std::sync::Arc::new(Dropped(path.to_path_buf())));
            h.run_steps(4);
        };
        drop(&mut h, &one);
        assert_eq!(opened(&h), one, "a drawing dropped on the start window opens");
        drop(&mut h, &two);
        assert_eq!(opened(&h), two, "and one dropped on an open drawing takes its place");
        let layers = h.state().editor.as_ref().unwrap().scene.bundle.doc.layers.len();
        let empty = dir.join("empty.klad");
        std::fs::create_dir_all(&empty).unwrap();
        drop(&mut h, &empty);
        assert_eq!(opened(&h), two, "a folder that is no drawing leaves the open one alone");
        assert!(
            h.query_all_by_label_contains("empty.klad").count() > 0,
            "and says which file it could not open"
        );
        assert_eq!(h.state().editor.as_ref().unwrap().scene.bundle.doc.layers.len(), layers);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_start_window_makes_a_drawing_like_draw_new_and_opens_it() {
        let dir = folder("new");
        let mut first = home(&dir, &[]);
        first.begin(&egui::Context::default(), None);
        let mut h = harness(first);
        assert!(h.state().editor.is_none());
        assert!(h.query_by_label("Drawings you open show up here.").is_some());
        {
            let start = h.state_mut().start.as_mut().unwrap();
            assert_eq!(start.name, "untitled");
            start.name.clear();
            start.preset = None;
            (start.width, start.height) = (320, 200);
            start.paper = true;
        }
        h.get_by_role_and_label(Role::TextInput, "Name").type_text("rock study");
        h.run_steps(2);
        h.get_by_label("Create").click();
        h.run_steps(3);
        let made = dir.join("rock study.klad");
        assert_eq!(opened(&h), made);
        let cli = Bundle::blank(
            dir.join("cli.klad"),
            &Blank {
                background: Some("#f4efe6".into()),
                sketch: true,
                ..small()
            },
            "cli",
        )
        .unwrap();
        assert_eq!(Bundle::open(&made).unwrap().doc, cli.doc);
        let kinds: Vec<(String, bool)> = cli.doc.layers.iter().map(|l| (l.name.clone(), l.visible)).collect();
        assert_eq!(kinds, [("paint 1".to_string(), true), ("sketch".to_string(), true)]);
        assert_eq!(h.state().settings.documents, [made.display().to_string()]);
        let mut again = home(&dir, &[]);
        again.begin(&egui::Context::default(), None);
        assert_eq!(again.start.as_ref().unwrap().name, "untitled");
        let mut taken = Start::new(&again.settings, Section::New);
        taken.name = "rock study".into();
        let refused = format!("{:#}", taken.create(&mut again.settings).unwrap_err());
        assert!(refused.contains("already exists"), "{refused}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_start_window_makes_a_pixel_drawing_like_draw_new_with_pixel() {
        let dir = folder("new-pixel");
        let mut first = home(&dir, &[]);
        first.begin(&egui::Context::default(), None);
        let mut h = harness(first);
        h.state_mut().start.as_mut().unwrap().name.clear();
        h.get_by_role_and_label(Role::TextInput, "Name").type_text("slime");
        h.run_steps(2);
        assert!(h.query_by_label("A sketch layer for blocking in shapes").is_some());
        h.get_by_label("Pixel art").click();
        h.run_steps(2);
        assert!(
            h.query_by_label("A sketch layer for blocking in shapes").is_none(),
            "a pixel drawing has no sketch layer to offer"
        );
        h.get_by_label("Create").click();
        h.run_steps(3);
        let made = dir.join("slime.klad");
        assert_eq!(opened(&h), made);
        let cli = Bundle::blank(
            dir.join("cli.klad"),
            &Blank {
                width: 64,
                height: 64,
                pixel: true,
                palette: true,
                ..Blank::default()
            },
            "cli",
        )
        .unwrap();
        let doc = Bundle::open(&made).unwrap().doc;
        assert_eq!(doc, cli.doc);
        assert!(doc.layers[0].pixel() && doc.layers[0].name == "pixel 1");
        let real_size = |h: &Harness<'static, Home>| {
            h.state()
                .editor
                .as_ref()
                .unwrap()
                .settings
                .layout
                .is_open(crate::layout::Pane::Preview)
        };
        assert!(real_size(&h), "the first pixel drawing opens the real size panel");
        assert!(h.query_by_label("Real size").is_some(), "and it shows in the dock");
        h.state_mut()
            .editor
            .as_mut()
            .unwrap()
            .settings
            .layout
            .toggle(crate::layout::Pane::Preview);
        h.run_steps(2);
        let ctx = h.ctx.clone();
        h.state_mut().open(&ctx, &dir.join("cli.klad")).unwrap();
        h.run_steps(3);
        assert!(!real_size(&h), "once closed, the next pixel drawing leaves it closed");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_aseprite_file_opens_as_a_drawing_made_next_to_it() {
        use klad_render::aseprite::{Cel, Layer, Picture, Sprite, Tag};
        let dir = folder("aseprite");
        let file = dir.join("hero.aseprite");
        let sprite = Sprite {
            width: 8,
            height: 8,
            layers: vec![Layer::image("body", 0)],
            durations: vec![100, 100],
            cels: vec![Cel {
                layer: 0,
                frame: 0,
                x: 1,
                y: 1,
                opacity: 255,
                picture: Picture::Pixels {
                    width: 2,
                    height: 1,
                    rgba: vec![255, 0, 0, 255, 0, 0, 255, 255],
                },
            }],
            tags: vec![Tag {
                name: "idle".into(),
                from: 0,
                to: 1,
                direction: 2,
                repeat: 0,
            }],
            ..Sprite::default()
        };
        std::fs::write(&file, klad_render::aseprite::write(&sprite).unwrap()).unwrap();
        let mut first = home(&dir, &[]);
        first.begin(&egui::Context::default(), None);
        let mut h = harness(first);
        let ctx = h.ctx.clone();
        h.state_mut().open(&ctx, &file).unwrap();
        h.run_steps(3);
        assert_eq!(opened(&h), dir.join("hero.klad"));
        {
            let doc = &h.state().editor.as_ref().unwrap().scene.bundle.doc;
            assert!(doc.pixel_art());
            assert_eq!((doc.timeline.frames, doc.timeline.fps), (2, 10));
            assert_eq!(doc.layers[0].name, "body");
            assert_eq!(doc.timeline.clips[0].name, "idle");
            assert_eq!(doc.palette.len(), 2, "the two colours it uses are its palette");
        }
        assert!(
            h.query_all_by_label_contains("made hero.klad from hero.aseprite")
                .count()
                > 0,
            "the editor says what it made"
        );
        assert!(
            h.query_all_by_label_contains("plays forwards").count() > 0,
            "and what did not come over"
        );
        h.state_mut().open(&ctx, &file).unwrap();
        h.run_steps(2);
        assert_eq!(opened(&h), dir.join("hero.klad"));
        assert!(
            h.query_all_by_label_contains("made from hero.aseprite before").count() > 0,
            "a second time it opens the drawing it made"
        );
        let broken = dir.join("broken.ase");
        std::fs::write(&broken, b"not a sprite").unwrap();
        let error = format!("{:#}", h.state_mut().open(&ctx, &broken).unwrap_err());
        assert!(error.contains("not an Aseprite file"), "{error}");
        assert!(!dir.join("broken.klad").exists());
        assert!(
            start::foreign(&file) && !start::foreign(&dir.join("hero.klad")) && !start::foreign(&dir.join("gone.ase"))
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_drawing_the_system_asks_to_open_opens_from_the_start_window() {
        let dir = folder("finder");
        let made = dir.join("from finder.klad");
        Bundle::blank(&made, &small(), "cli").unwrap();
        let mut first = home(&dir, &[]);
        first.begin(&egui::Context::default(), None);
        let mut h = harness(first);
        assert!(h.state().editor.is_none());
        crate::opened::push(made.clone());
        h.run_steps(2);
        assert_eq!(opened(&h), made);
        assert!(crate::opened::take().is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn menu_picks_open_a_drawing_run_its_commands_and_close_it() {
        let dir = folder("menus");
        let made = dir.join("ledge.klad");
        Bundle::blank(&made, &small(), "cli").unwrap();
        let mut first = home(&dir, &[]);
        first.begin(&egui::Context::default(), None);
        let mut h = harness(first);
        menus::push(Picked::Run(Cmd::Undo));
        menus::push(Picked::Run(Cmd::OpenRecent));
        h.run_steps(2);
        assert!(
            h.state().editor.is_none(),
            "outside a drawing only the file commands run"
        );
        menus::push(Picked::Open(made.clone()));
        h.run_steps(3);
        assert_eq!(opened(&h), made);
        let state = h.state().editor.as_ref().unwrap().menu_state();
        assert!(state.off.contains(&Cmd::Unlink) && !state.off.contains(&Cmd::Export));
        assert!(state.on.contains(&Cmd::Brush), "the tool in use is ticked");
        menus::push(Picked::Run(Cmd::CloseDocument));
        h.run_steps(3);
        assert!(h.state().editor.is_none(), "close goes back to the start window");
        assert!(h.state().start.is_some());
        assert_eq!(h.state().settings.documents, [made.display().to_string()]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_first_start_welcomes_offers_first_steps_and_opens_samples_as_copies() {
        let dir = folder("welcome");
        let shipped = dir.join("shipped/rock/rock.klad");
        Bundle::blank(&shipped, &small(), "cli").unwrap();
        let mut first = home(&dir, &[]);
        first.begin(&egui::Context::default(), None);
        first.start.as_mut().unwrap().samples = vec![klad_core::samples::Sample {
            name: "rock".into(),
            about: "a rock on its own".into(),
            path: shipped.clone(),
        }];
        let mut h = harness(first);
        assert!(
            h.query_by_label("Welcome to klad").is_some(),
            "a first start says hello"
        );
        assert!(h.query_by_label("Guides").is_some(), "and links the guides");
        assert!(
            h.query_by_label("a rock on its own").is_some(),
            "the samples show with what they are"
        );
        h.get_by_label("rock").click();
        h.run_steps(3);
        assert_eq!(
            opened(&h),
            dir.join("rock.klad"),
            "a sample opens as a copy in the drawings folder"
        );
        assert!(shipped.join("doc.json").is_file());
        assert!(h.state().settings.welcomed, "opening anything counts as welcomed");
        let mut again = home(&dir, &[]);
        again.begin(&egui::Context::default(), None);
        let mut h = harness(again);
        h.get_by_label("First steps").click();
        h.run_steps(3);
        assert_eq!(opened(&h), dir.join("first steps.klad"));
        assert!(
            h.query_by_label_contains("Draw a closed shape with the brush")
                .is_some(),
            "first steps open on the new drawing"
        );
        let mut later = home(&dir, &[]);
        later.settings.welcomed = true;
        later.begin(&egui::Context::default(), None);
        let h = harness(later);
        assert!(h.query_by_label("Welcome to klad").is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_new_version_shows_in_the_start_window_with_what_changed() {
        let dir = folder("update");
        let mut first = home(&dir, &[]);
        first.begin(&egui::Context::default(), None);
        first.update = updates::parse(
            r#"{"tag_name":"v9.0.0","html_url":"https://github.com/klad-app/klad/releases/tag/v9.0.0","body":"- Paint over"}"#,
        );
        let h = harness(first);
        assert!(h.query_by_label(&format!("version {}", crate::VERSION)).is_some());
        assert!(h.query_by_label("klad 9.0.0 is out: Paint over").is_some());
        assert!(h.query_by_label("what changed").is_some());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn recent_reopens_a_drawing_and_greys_out_a_moved_one() {
        let dir = folder("recent");
        let (a, b) = (dir.join("ledge.klad"), dir.join("cave.klad"));
        [&a, &b].iter().for_each(|p| {
            Bundle::blank(p, &small(), "cli").unwrap();
        });
        std::fs::rename(&b, dir.join("cave moved.klad")).unwrap();
        let mut first = home(&dir, &[&a, &b]);
        first.begin(&egui::Context::default(), None);
        let mut h = harness(first);
        assert!(h.query_by_label("moved or deleted").is_some());
        h.get_by_label("Remove cave").click();
        h.run_steps(2);
        assert_eq!(h.state().settings.documents, [a.display().to_string()]);
        h.get_by_label("ledge").click();
        h.run_steps(3);
        assert_eq!(opened(&h), a);
        assert!(h.state().start.is_none());
        let mut missing = home(&dir, &[]);
        missing.begin(&egui::Context::default(), Some(&dir.join("nowhere.klad")));
        assert!(missing.editor.is_none());
        let error = missing.start.as_ref().unwrap().error.clone().unwrap();
        assert!(error.contains("nowhere.klad"), "{error}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_drawing_with_an_id_used_twice_is_offered_a_repair() {
        let dir = folder("twice");
        let path = dir.join("merged.klad");
        Bundle::blank(&path, &Blank { layers: 2, ..small() }, "cli").unwrap();
        let doc = std::fs::read_to_string(path.join("doc.json")).unwrap();
        std::fs::write(path.join("doc.json"), doc.replace("\"l3\"", "\"l1\"")).unwrap();
        let mut start = home(&dir, &[]);
        start.begin(&egui::Context::default(), Some(&path));
        assert!(start.editor.is_none());
        let error = start.start.as_ref().unwrap().error.clone().unwrap();
        assert!(error.contains("used twice"), "{error}");
        let mut h = harness(start);
        h.get_by_label("Repair and open").click();
        h.run_steps(3);
        assert_eq!(opened(&h), path);
        assert!(h.state().start.is_none());
        assert!(
            h.query_all_by_label_contains("repaired ids used twice")
                .next()
                .is_some()
        );
        assert!(Bundle::open(&path).is_ok());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn cmd_n_and_cmd_o_in_the_editor_switch_drawings_and_back_keeps_this_one() {
        let dir = folder("switch");
        let (a, b) = (dir.join("first.klad"), dir.join("second.klad"));
        [&a, &b].iter().for_each(|p| {
            Bundle::blank(p, &small(), "cli").unwrap();
        });
        let mut first = home(&dir, &[]);
        first.open(&egui::Context::default(), &a).unwrap();
        let mut h = harness(first);
        h.key_press_modifiers(Modifiers::COMMAND, Key::N);
        h.run_steps(3);
        assert!(h.state().start.is_some());
        h.get_by_label("Back to this drawing").click();
        h.run_steps(3);
        assert!(h.state().start.is_none());
        assert_eq!(opened(&h), a);
        h.key_press_modifiers(Modifiers::COMMAND, Key::N);
        h.run_steps(3);
        {
            let start = h.state_mut().start.as_mut().unwrap();
            start.name = "third".into();
            start.preset = None;
            (start.width, start.height) = (320, 200);
        }
        h.run_steps(2);
        h.get_by_label("Create").click();
        h.run_steps(3);
        assert_eq!(opened(&h), dir.join("third.klad"));
        h.key_press_modifiers(Modifiers::COMMAND, Key::O);
        h.run_steps(3);
        h.get_by_role_and_label(Role::TextInput, "Or paste its path")
            .type_text(&b.display().to_string());
        h.run_steps(2);
        h.get_by_label("Open").click();
        h.run_steps(3);
        assert_eq!(opened(&h), b);
        let order: Vec<String> = h.state().settings.documents.clone();
        assert_eq!(
            order,
            [&b, &dir.join("third.klad"), &a].map(|p| p.display().to_string())
        );
        h.state_mut().editor.as_mut().unwrap().wants = None;
        document_menu(&h).click();
        h.run_steps(2);
        h.get_by_label(&format!("{} Copy path", theme::icon::COPY)).click();
        h.run_steps(1);
        let copied = h
            .output()
            .platform_output
            .commands
            .iter()
            .any(|c| matches!(c, egui::OutputCommand::CopyText(text) if *text == b.display().to_string()));
        assert!(copied);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn many(dir: &Path) -> Vec<PathBuf> {
        (0..14)
            .map(|i| {
                let path = dir.join(format!("drawing {i}.klad"));
                Bundle::blank(&path, &small(), "cli").unwrap();
                path
            })
            .collect()
    }

    fn sideways(h: &mut Harness<'static, Home>, at: egui::Pos2) {
        h.event(egui::Event::PointerMoved(at));
        h.run_steps(2);
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(-400.0, 0.0),
            modifiers: Modifiers::NONE,
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
        });
        h.run_steps(8);
    }

    fn recent_scrolls_sideways(h: &mut Harness<'static, Home>, width: f32) {
        let heading = h.get_by_label(&format!("{} Recent", theme::icon::HISTORY)).rect();
        let first = h.get_by_label("drawing 0").rect();
        assert!(first.max.x < width);
        assert!(
            h.query_by_label("drawing 13")
                .is_none_or(|card| card.rect().min.x > first.max.x)
        );
        sideways(h, egui::pos2(first.center().x, first.min.y + 20.0));
        let after = h.get_by_label("drawing 0").rect();
        assert!(
            after.min.x < first.min.x - 200.0,
            "the first card stayed at {}",
            first.min.x
        );
        assert_eq!(after.min.y, first.min.y);
        assert_eq!(
            h.get_by_label(&format!("{} Recent", theme::icon::HISTORY)).rect(),
            heading
        );
    }

    #[test]
    fn recent_drawings_are_one_row_that_scrolls_sideways() {
        let dir = folder("scroll");
        let made = many(&dir);
        let shown: Vec<&Path> = made.iter().map(PathBuf::as_path).collect();
        [vec2(1600.0, 1000.0), vec2(1000.0, 640.0)]
            .into_iter()
            .for_each(|size| {
                let mut h = Harness::builder()
                    .with_size(size)
                    .build_ui_state(|ui, home: &mut Home| home.frame(ui), home(&dir, &shown));
                h.run_steps(4);
                recent_scrolls_sideways(&mut h, size.x);
            });
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn recent_drawings_scroll_sideways_in_the_window_over_a_drawing() {
        let dir = folder("scroll-over");
        let made = many(&dir);
        let shown: Vec<&Path> = made.iter().map(PathBuf::as_path).collect();
        let mut first = home(&dir, &shown);
        first.open(&egui::Context::default(), &made[0]).unwrap();
        let mut h = Harness::builder()
            .with_size(vec2(1600.0, 1000.0))
            .build_ui_state(|ui, home: &mut Home| home.frame(ui), first);
        h.run_steps(4);
        h.key_press_modifiers(Modifiers::COMMAND, Key::N);
        h.run_steps(6);
        recent_scrolls_sideways(&mut h, 1600.0);
        drop(h);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[ignore]
    fn start_pictures() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let out = root.join("scratch/ui");
        std::fs::create_dir_all(&out).unwrap();
        let shown: Vec<PathBuf> = [
            "samples/solids/solids.klad",
            "samples/lantern/lantern-painted.klad",
            "samples/creature/creature.klad",
        ]
        .iter()
        .map(|p| std::fs::canonicalize(root.join(p)).expect("run just samples and build the painted lantern first"))
        .collect();
        let gone = PathBuf::from("/Users/you/Drawings/old sketch.klad");
        let settings = Settings {
            drawings: Some("~/Drawings".into()),
            documents: shown
                .iter()
                .chain(std::iter::once(&gone))
                .map(|p| p.display().to_string())
                .collect(),
            ..Settings::default()
        };
        let mut first = Home::with(settings, None, None);
        first.begin(&egui::Context::default(), None);
        let mut h = Harness::builder()
            .with_size(vec2(1440.0, 900.0))
            .wgpu()
            .build_ui_state(|ui, home: &mut Home| home.frame(ui), first);
        let started = std::time::Instant::now();
        while started.elapsed() < std::time::Duration::from_secs(60) {
            h.run_steps(2);
            std::thread::sleep(std::time::Duration::from_millis(50));
            if shown.iter().all(|p| p.join(".cache/thumb.png").exists()) && started.elapsed().as_secs() > 2 {
                break;
            }
        }
        h.run_steps(6);
        h.render().unwrap().save(out.join("start-1-window.png")).unwrap();
        h.state_mut().open(&egui::Context::default(), &shown[1]).unwrap();
        h.run_steps(6);
        h.key_press_modifiers(Modifiers::COMMAND, Key::N);
        h.run_steps(8);
        h.render()
            .unwrap()
            .save(out.join("start-2-over-the-editor.png"))
            .unwrap();
    }

    #[test]
    fn the_document_menu_shows_the_licence_and_the_notices() {
        let dir = folder("about");
        let path = dir.join("about.klad");
        Bundle::blank(&path, &small(), "cli").unwrap();
        let mut first = home(&dir, &[]);
        first.open(&egui::Context::default(), &path).unwrap();
        let mut h = harness(first);
        document_menu(&h).click();
        h.run_steps(2);
        assert!(
            h.query_by_label(&format!("{} Install the klad command", theme::icon::DOWNLOAD))
                .is_some()
        );
        assert_eq!(crate::editor::install_job().args, ["install"]);
        h.get_by_label(&format!("{} About klad and licences", theme::icon::INFO))
            .click();
        h.run_steps(3);
        assert!(
            h.query_by_label_contains("Source-available licence, copyright Indicode BV")
                .is_some()
        );
        assert!(h.query_by_label("# Third-party notices").is_some());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    #[ignore]
    fn editor_picture() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let out = root.join("scratch/ui");
        std::fs::create_dir_all(&out).unwrap();
        let source = std::fs::canonicalize(root.join("samples/lantern/lantern-painted.klad")).expect(
            "build it first: PAINTED=1 FORCE=1 bash samples/lantern/build.sh samples/lantern/lantern-painted.klad",
        );
        let dir = folder("editor-picture");
        let path = dir.join("lantern.klad");
        let copied = std::process::Command::new("cp")
            .arg("-R")
            .arg(&source)
            .arg(&path)
            .status()
            .unwrap();
        assert!(copied.success());
        let mut bundle = Bundle::open(&path).unwrap();
        bundle.doc.canvas.background = Some("#ebe6dc".into());
        bundle.save().unwrap();
        let gpu = egui_kittest::wgpu::create_render_state(
            egui_kittest::wgpu::default_wgpu_setup(),
            egui_wgpu::RendererOptions::PREDICTABLE,
        );
        let mut settings = Settings::default();
        settings.layout.timeline_height = 190.0;
        let mut first = Home::with(settings, None, Some(gpu.clone()));
        first.open(&egui::Context::default(), &path).unwrap();
        let mut h = Harness::builder()
            .with_size(vec2(1600.0, 1000.0))
            .renderer(egui_kittest::wgpu::WgpuTestRenderer::from_render_state(gpu))
            .build_ui_state(|ui, home: &mut Home| home.frame(ui), first);
        let started = std::time::Instant::now();
        while !h.state().editor.as_ref().unwrap().canvas_current() && started.elapsed().as_secs() < 60 {
            h.step();
            h.render().unwrap();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        h.state_mut().editor.as_mut().unwrap().redraw_canvas();
        (0..6).for_each(|_| {
            h.step();
            h.render().unwrap();
        });
        h.render().unwrap().save(out.join("editor-lantern.png")).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

#[cfg(test)]
mod open_drawings {
    use super::*;
    use egui::vec2;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use klad_core::bundle::{Blank, Bundle};

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("draw-open-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::canonicalize(&dir).unwrap()
    }

    fn blank(path: &Path) {
        Bundle::blank(
            path,
            &Blank {
                width: 320,
                height: 200,
                ..Blank::default()
            },
            "cli",
        )
        .unwrap();
    }

    fn shown(h: &Harness<'static, Home>) -> PathBuf {
        h.state().editor.as_ref().unwrap().scene.bundle.root.clone()
    }

    #[test]
    fn several_drawings_stay_open_side_by_side_switch_with_tabs_and_ctrl_tab_and_close_one_by_one() {
        let dir = folder("tabs");
        let (one, two, three) = (dir.join("one.klad"), dir.join("two.klad"), dir.join("three.klad"));
        [&one, &two, &three].iter().for_each(|p| blank(p));
        let settings = Settings {
            drawings: Some(dir.display().to_string()),
            ..Settings::default()
        };
        let mut home = Home::with(settings, None, None);
        let ctx = egui::Context::default();
        home.begin(&ctx, Some(&one));
        let mut h = Harness::builder()
            .with_size(vec2(1440.0, 900.0))
            .build_ui_state(|ui, home: &mut Home| home.frame(ui), home);
        h.run_steps(3);
        h.state_mut().open(&ctx, &two).unwrap();
        h.state_mut().open(&ctx, &three).unwrap();
        h.run_steps(3);
        assert_eq!(shown(&h), three);
        assert_eq!(
            h.state().drawings(),
            [one.clone(), two.clone(), three.clone()],
            "every drawing stays open, in the order opened"
        );
        assert!(
            h.query_by_label("one").is_some() && h.query_by_label("two").is_some(),
            "a tab for each drawing"
        );
        h.get_by_label("one").click();
        h.run_steps(3);
        assert_eq!(shown(&h), one, "a tab shows its drawing");
        assert_eq!(
            h.state().drawings(),
            [one.clone(), two.clone(), three.clone()],
            "and the tabs keep their order"
        );
        h.event(egui::Event::Key {
            key: egui::Key::Tab,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::CTRL,
        });
        h.run_steps(3);
        assert_eq!(shown(&h), two, "ctrl Tab goes to the next drawing");
        h.state_mut().open(&ctx, &one).unwrap();
        h.run_steps(2);
        assert_eq!(shown(&h), one, "opening a drawing that is open shows its tab");
        assert_eq!(h.state().drawings().len(), 3, "and does not open it twice");
        h.state_mut().editor.as_mut().unwrap().wants = Some(Want::Close);
        h.run_steps(4);
        assert_eq!(h.state().drawings(), [two.clone(), three.clone()]);
        assert!(h.state().editor.is_some(), "closing one drawing shows another");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_start_window_duplicates_a_drawing_and_moves_one_to_the_trash() {
        use egui_kittest::kittest::NodeT;
        let dir = folder("cards");
        let (one, two) = (dir.join("one.klad"), dir.join("two.klad"));
        blank(&one);
        blank(&two);
        let settings = Settings {
            drawings: Some(dir.display().to_string()),
            documents: vec![one.display().to_string(), two.display().to_string()],
            ..Settings::default()
        };
        let mut home = Home::with(settings, None, None);
        home.start = Some(Start::new(&home.settings, Section::Recent));
        home.start.as_mut().unwrap().trash_to = Some(dir.join("trash"));
        let mut h = Harness::builder()
            .with_size(vec2(1280.0, 860.0))
            .build_ui_state(|ui, home: &mut Home| home.frame(ui), home);
        h.run_steps(3);
        let card = |h: &Harness<'static, Home>, name: &str| {
            h.query_all_by_label(name)
                .find(|n| n.accesskit_node().role() == egui::accesskit::Role::Button)
                .map(|n| n.rect().center())
        };
        h.query_all_by_label("one")
            .find(|n| n.accesskit_node().role() == egui::accesskit::Role::Button)
            .unwrap()
            .click_secondary();
        h.run_steps(3);
        h.get_by_label_contains("Duplicate").click();
        h.run_steps(3);
        assert!(
            dir.join("one copy.klad/doc.json").is_file(),
            "the card's menu duplicates the drawing"
        );
        assert!(
            h.state()
                .settings
                .documents
                .iter()
                .any(|d| d.ends_with("one copy.klad")),
            "and lists the copy"
        );
        assert!(card(&h, "two").is_some());
        h.query_all_by_label("two")
            .find(|n| n.accesskit_node().role() == egui::accesskit::Role::Button)
            .unwrap()
            .click_secondary();
        h.run_steps(3);
        h.get_by_label_contains("Move to the Trash").click();
        h.run_steps(3);
        assert!(
            !two.exists() && dir.join("trash/two.klad/doc.json").is_file(),
            "it goes to the Trash, not away for good"
        );
        assert!(!h.state().settings.documents.iter().any(|d| d.ends_with("two.klad")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
