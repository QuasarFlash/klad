use super::layers;
use super::{Editor, Tool, ms};
use crate::commands::{self, Cmd};
use crate::jobs::{Outcome, Spec, Tag};
use crate::layout::{self, Layout, Pane};
use crate::messages::{Message, Tone};
use crate::stats::lock;
use crate::theme::{self, Hint, icon};
use crate::widgets::PenScroll;
use anyhow::Result;
use egui::{Align, Color32, Id, Key, Layout as Flow, Margin, Modifiers, RichText, Sense, UiBuilder, vec2};
use std::time::{Duration, Instant};

const SETTLE: u8 = 12;
const FINDER_CHROME: f32 = 200.0;
const FINDER_LEAST: f32 = 120.0;
const FINDER_MOST: f32 = 440.0;
const MENU_CHROME: f32 = 72.0;
const MENU_LEAST: f32 = 120.0;
const WINDOW_GAP: f32 = 8.0;
const WINDOW_LEAST: f32 = 160.0;
const CASCADE: egui::Vec2 = vec2(32.0, 32.0);

#[derive(Clone, Copy, Default)]
struct Placing {
    opened: u32,
    seen: u64,
    frames: u8,
    settled: bool,
    size: Option<egui::Vec2>,
    offset: egui::Vec2,
}

fn open_windows() -> Id {
    Id::unique("klad open windows")
}

fn window_area(ctx: &egui::Context) -> egui::Rect {
    let screen = ctx.content_rect();
    egui::Rect::from_min_max(screen.min, egui::pos2(screen.max.x, screen.max.y - layout::STATUS))
}

pub(super) fn window_room(ctx: &egui::Context) -> f32 {
    let chrome = ctx.global_style().spacing.window_margin.sum().y + 2.0;
    (window_area(ctx).height() - 2.0 * WINDOW_GAP - chrome).max(WINDOW_LEAST)
}

#[derive(Clone, Copy)]
pub(super) enum Spot {
    Middle,
    At(egui::Pos2),
}

fn colour_of(tone: Tone) -> Color32 {
    match tone {
        Tone::Info => theme::TEXT,
        Tone::Done => theme::DONE,
        Tone::Refused => theme::REFUSED,
        Tone::Error => theme::ERROR,
    }
}

fn icon_of(tone: Tone) -> &'static str {
    match tone {
        Tone::Info => icon::INFO,
        Tone::Done => icon::CHECK,
        Tone::Refused | Tone::Error => icon::WARNING,
    }
}

fn ago(at: Instant, now: Instant) -> String {
    let s = now.duration_since(at).as_secs();
    match s {
        0..=59 => format!("{s} s ago"),
        60..=3599 => format!("{} min ago", s / 60),
        _ => format!("{} h ago", s / 3600),
    }
}

const STRIP_TOOLS: [Cmd; 14] = [
    Cmd::Brush,
    Cmd::Eraser,
    Cmd::Smudge,
    Cmd::Fill,
    Cmd::EyedropperTool,
    Cmd::Strokes,
    Cmd::Shapes,
    Cmd::Points,
    Cmd::CropTool,
    Cmd::Pen,
    Cmd::ShapeTool,
    Cmd::TextTool,
    Cmd::BoneTool,
    Cmd::Note,
];

impl Editor {
    pub(super) fn check(&mut self, f: impl FnOnce(&mut Self) -> Result<()>) {
        if let Err(e) = f(self) {
            self.failed(e);
        }
    }

    pub(crate) fn frame(&mut self, ui: &mut egui::Ui) {
        crate::pen::note(ui.ctx(), self.pen.as_deref());
        let focused = ui.input(|i| i.focused);
        if focused && !self.window_focused {
            self.yours.reload();
        }
        self.window_focused = focused;
        let start = Instant::now();
        let ctx = ui.ctx().clone();
        if !self.themed {
            theme::install(&ctx);
            self.themed = true;
        }
        if !self.folder_is_there() {
            return;
        }
        theme::clear_hints(&ctx);
        if ctx.options(|o| o.zoom_with_keyboard) {
            ctx.options_mut(|o| o.zoom_with_keyboard = false);
        }
        if (ctx.zoom_factor() - self.settings.ui_scale).abs() > 1e-3 {
            ctx.set_zoom_factor(self.settings.ui_scale);
        }
        let dt = ui.input(|i| i.unstable_dt);
        if dt < 0.1 {
            lock(&self.stats).frame_ms.push(dt * 1000.0);
        }
        self.pointer = ui.input(|i| i.pointer.latest_pos()).or(self.pointer);
        self.field_had_focus = ctx.egui_wants_keyboard_input();
        self.keys_busy =
            self.field_had_focus || self.palette.is_some() || self.windows.settings || self.capturing.is_some();
        self.check(|e| e.sync());
        self.tend_jobs();
        self.follow_pixel_check();
        self.check(|e| e.tab_keys());
        let window = ctx.content_rect().size();
        self.settings.layout.clamp(window);
        let pressing = ui.input(|i| i.pointer.any_down());
        if !crate::menus::native() && !self.settings.layout.hidden {
            egui::Panel::top("menus")
                .resizable(false)
                .show(ui, |ui| egui::MenuBar::new().ui(ui, |ui| self.menu_bar(ui)));
        }
        egui::Panel::bottom("status")
            .exact_size(layout::STATUS)
            .resizable(false)
            .show(ui, |ui| self.status_line(ui));
        self.fold_for_stills();
        self.timeline_shown = 0.0;
        if self.settings.layout.shows_timeline() {
            let folded = self.settings.layout.timeline_folded;
            let margin = egui::Frame::side_top_panel(&ctx.global_style()).total_margin().sum().y;
            let scale = self.settings.ui_scale;
            let (target, lo, hi) = self
                .settings
                .layout
                .timeline_size(window, scale, self.timeline_rows + margin);
            let panel = match folded {
                true => egui::Panel::bottom("timeline folded")
                    .exact_size(self.timeline_bar + margin)
                    .resizable(false),
                false => egui::Panel::bottom(("timeline", (target * 2.0).round() as i32))
                    .default_size(target)
                    .size_range(lo..=hi)
                    .resizable(true),
            };
            let shown = panel.show(ui, |ui| self.check(|e| e.timeline_panel(ui)));
            let height = shown.response.rect.height();
            self.timeline_shown = height;
            let moved = !folded && (height - target).abs() > 0.5;
            match (pressing, moved || self.timeline_dragged) {
                (true, true) => self.timeline_dragged = true,
                (false, true) if self.timeline_dragged => {
                    self.timeline_dragged = false;
                    self.settings.layout.keep_timeline_height(height, scale);
                    self.settings_dirty = true;
                }
                _ => {}
            }
        }
        if self.settings.layout.shows_strip() {
            egui::Panel::left("strip")
                .exact_size(layout::STRIP)
                .resizable(false)
                .show(ui, |ui| self.strip(ui));
        }
        if self.settings.layout.shows_dock() {
            let (lo, hi) = Layout::dock_range(window);
            let shown = egui::Panel::right("dock")
                .default_size(self.settings.layout.dock_width)
                .size_range(lo..=hi)
                .resizable(true)
                .frame(egui::Frame::side_top_panel(&ctx.global_style()).inner_margin(Margin::ZERO))
                .show(ui, |ui| self.dock(ui));
            let width = shown.response.rect.width();
            if !pressing && (width - self.settings.layout.dock_width).abs() > 0.5 {
                self.settings.layout.dock_width = width;
                self.settings_dirty = true;
            }
        }
        if !self.settings.layout.hidden {
            egui::Panel::top("options")
                .resizable(false)
                .show(ui, |ui| self.check(|e| e.tool_options(ui)));
        }
        let panels = ms(start);
        let drawing = Instant::now();
        egui::CentralPanel::no_frame().show(ui, |ui| {
            self.check(|e| e.canvas(ui));
            self.toasts(ui);
        });
        lock(&self.stats).canvas_ms.push(ms(drawing) as f32);
        let after = Instant::now();
        self.windows(&ctx);
        self.first_steps_window(&ctx);
        self.import_window(&ctx);
        self.palettes_window(&ctx);
        self.dropped_pictures(&ctx);
        self.tend_thumbs(&ctx);
        if let Some(path) = self.queued_activate.take() {
            self.check(|e| e.activate(path));
        }
        if let Some(id) = self.solo_later.take() {
            self.check(|e| e.solo(&id));
        }
        self.check(|e| e.tend_pick());
        self.check(|e| e.pin_popover(&ctx));
        self.check(|e| e.run_queued());
        lock(&self.stats).ui_ms.push((panels + ms(after)) as f32);
        self.save_settings(pressing);
        if let Some(wait) = self.messages.next_change(Instant::now()) {
            ctx.request_repaint_after(wait.max(Duration::from_millis(16)));
        }
        if !self.jobs.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    fn save_settings(&mut self, pressing: bool) {
        if !self.settings_dirty || pressing {
            return;
        }
        if let Some(path) = &self.settings_path
            && let Err(e) = self.settings.save(path)
        {
            self.error(format!(
                "your settings could not be saved ({e:#}); they apply until klad closes"
            ));
        }
        self.settings_dirty = false;
    }

    pub(super) fn start_job(&mut self, mut spec: Spec) {
        spec.exe = match crate::start::draw_binary() {
            Some(exe) => exe,
            None => {
                self.error("the klad command line tool is missing next to the editor, so this cannot run: install klad again (it ships both)".to_string());
                return;
            }
        };
        let name = spec.name.clone();
        match self.jobs.start(spec) {
            Ok(_) => self.say(Tone::Info, format!("{name}...")),
            Err(e) => self.error(format!("could not start {name}: {e}")),
        }
    }

    fn say_job(&mut self, tone: Tone, text: String) {
        if tone == Tone::Error {
            crate::report::log(&format!("editor: {text}"));
        }
        self.messages.say(tone, text, Instant::now());
    }

    fn tend_jobs(&mut self) {
        self.jobs.poll().into_iter().for_each(|e| {
            let took = e.took.as_secs_f32();
            self.job_ended = Some(Instant::now());
            match e.outcome {
                Outcome::Rerun => self.say_job(
                    Tone::Info,
                    format!("{}: the document changed while it ran, so it runs again", e.name),
                ),
                Outcome::Done(text) => {
                    if e.tag == Tag::Inbetween {
                        self.check(|ed| ed.reload());
                        self.between.report = super::between::report(&e.output);
                    }
                    if e.tag == Tag::Edit {
                        self.check(|ed| ed.reload());
                    }
                    if e.tag == Tag::Fonts {
                        klad_render::fonts::forget();
                    }
                    self.job_found(e.tag, &e.name, &e.output);
                    let detail = if text.is_empty() {
                        String::new()
                    } else {
                        format!(": {}", self.job_words(&text))
                    };
                    self.measure("last job", format!("{} in {took:.1} s{detail}", e.name));
                    if e.tag == Tag::Export {
                        self.exported(&e.stdout, &e.output);
                    }
                    match e.tag == Tag::Export && !text.is_empty() {
                        true => self.say_job(Tone::Done, text),
                        false => self.say_job(Tone::Done, format!("{} done{detail}", e.name)),
                    }
                }
                Outcome::Failed(text) => {
                    if e.tag == Tag::Inbetween {
                        self.between.report = super::between::report(&e.output);
                    }
                    let why = self.job_words(&text);
                    self.say_job(Tone::Error, crate::report::job_failed(&e.name, &why))
                }
                Outcome::Cancelled => self.say_job(Tone::Info, format!("{} cancelled", e.name)),
            }
        });
    }

    fn strip(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.y = 4.0;
        let column = ui.max_rect().x_range();
        let gap = ui.spacing().item_spacing.y;
        let height = ui.available_height();
        let fits = |size: usize| self.strip_tools[size] + self.strip_foot[size] + 2.0 * gap <= height;
        let small = !fits(0);
        let folded = small && !fits(1);
        let size = usize::from(small);
        let (tool, other, icons) = match small {
            true => (vec2(36.0, 28.0), vec2(36.0, 28.0), (16.0, 15.0)),
            false => (vec2(36.0, 36.0), vec2(36.0, 32.0), (20.0, 18.0)),
        };
        let foot_now = match folded {
            true => other.y + 6.0 + gap,
            false => self.strip_foot[size],
        };
        ui.spacing_mut().scroll = theme::shown_scroll();
        egui::ScrollArea::vertical()
            .id_salt("strip")
            .max_height((height - foot_now - gap).max(0.0))
            .auto_shrink([false, true])
            .scroll_source(egui::scroll_area::ScrollSource {
                drag: egui::scroll_area::DragScroll::Never,
                ..egui::scroll_area::ScrollSource::default()
            })
            .pen_scroll(ui.ctx())
            .show(ui, |ui| {
                let drawn = ui
                    .vertical_centered(|ui| {
                        ui.add_space(6.0);
                        STRIP_TOOLS.iter().for_each(|cmd| {
                            self.command_sized(ui, *cmd, None, icons.0, tool);
                        });
                        ui.separator();
                        self.colour_pair(ui);
                        ui.separator();
                        [Cmd::Undo, Cmd::Redo].iter().for_each(|cmd| {
                            self.command_sized(ui, *cmd, None, icons.1, other);
                        });
                    })
                    .response
                    .rect
                    .height();
                self.strip_measured(size, Some(drawn), None, ui.ctx());
            });
        let below = egui::Rect::from_x_y_ranges(column, ui.available_rect_before_wrap().y_range());
        let foot = UiBuilder::new().max_rect(below).layout(Flow::bottom_up(Align::Center));
        ui.scope_builder(foot, |ui| {
            ui.add_space(6.0);
            if folded {
                self.strip_more(ui, other, icons.1);
                return;
            }
            let developer = self.settings.developer;
            [Cmd::Settings, Cmd::Numbers, Cmd::Palette]
                .iter()
                .filter(|cmd| developer || !commands::DEVELOPER.contains(cmd))
                .for_each(|cmd| {
                    self.command_sized(ui, *cmd, None, icons.1, other);
                });
            self.panels_menu(ui, other, icons.1);
            self.document_menu(ui, other, icons.1);
            self.strip_measured(size, None, Some(ui.min_rect().height()), ui.ctx());
        });
    }

    fn strip_measured(&mut self, size: usize, tools: Option<f32>, foot: Option<f32>, ctx: &egui::Context) {
        let changed = |was: f32, now: Option<f32>| now.filter(|n| (n - was).abs() > 0.5);
        if let Some(t) = changed(self.strip_tools[size], tools) {
            self.strip_tools[size] = t;
            ctx.request_repaint();
        }
        if let Some(f) = changed(self.strip_foot[size], foot) {
            self.strip_foot[size] = f;
            ctx.request_repaint();
        }
    }

    fn strip_more(&mut self, ui: &mut egui::Ui, size: egui::Vec2, glyph: f32) {
        let button = egui::Button::new(RichText::new(icon::MORE).size(glyph)).min_size(size);
        egui::containers::menu::MenuButton::from_button(button)
            .ui(ui, |ui| {
                self.document_entries(ui);
                ui.separator();
                self.panel_checks(ui);
                ui.separator();
                let developer = self.settings.developer;
                [Cmd::Palette, Cmd::Numbers, Cmd::Settings]
                    .into_iter()
                    .filter(|cmd| developer || !commands::DEVELOPER.contains(cmd))
                    .for_each(|cmd| {
                        if self.command_button(ui, cmd, Some(cmd.menu())).clicked() {
                            ui.close();
                        }
                    });
            })
            .0
            .hint("the drawing, the panels, find a command and settings, in one menu while the window is this short");
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        commands::MENUS.iter().for_each(|m| {
            ui.menu_button(m.title, |ui| scrolled_menu(ui, |ui| self.menu_items(ui, m.items)))
                .response
                .hint(commands::menu_hint(m));
        });
    }

    fn menu_items(&mut self, ui: &mut egui::Ui, items: &[commands::Item]) {
        items.iter().for_each(|item| match *item {
            commands::Item::Line => {
                ui.separator();
            }
            commands::Item::Run(cmd) => self.menu_entry(ui, cmd),
            commands::Item::Sub(title, inner) => {
                ui.menu_button(title, |ui| scrolled_menu(ui, |ui| self.menu_items(ui, inner)));
            }
            commands::Item::Recent => {
                ui.menu_button("Open recent", |ui| {
                    let paths: Vec<std::path::PathBuf> = self
                        .settings
                        .documents
                        .iter()
                        .take(crate::menus::RECENT)
                        .map(std::path::PathBuf::from)
                        .collect();
                    paths.iter().for_each(|path| {
                        if ui
                            .button(crate::menus::recent_label(path))
                            .hint(path.display().to_string())
                            .clicked()
                        {
                            self.wants = Some(super::Want::Path(path.clone()));
                            ui.close();
                        }
                    });
                    if !paths.is_empty() {
                        ui.separator();
                    }
                    self.menu_entry(ui, Cmd::OpenRecent);
                });
            }
        });
    }

    pub(super) fn menu_entry(&mut self, ui: &mut egui::Ui, cmd: Cmd) {
        let label = commands::menu_label(cmd);
        let can = self.can_run(cmd).is_none();
        let keys = self.bindings.menu_key(cmd);
        let response = match (self.is_on(cmd).filter(|_| cmd.ticks()), keys.is_empty()) {
            (Some(mut on), true) => ui.add_enabled(can, egui::Checkbox::new(&mut on, label)),
            (Some(mut on), false) => ui.add_enabled(
                can,
                egui::Checkbox::new(&mut on, (label, egui::Atom::grow(), RichText::new(keys).weak())),
            ),
            (None, true) => ui.add_enabled(can, egui::Button::new(label)),
            (None, false) => ui.add_enabled(can, egui::Button::new(label).shortcut_text(keys)),
        }
        .hint(self.command_hint(cmd));
        if response.clicked() {
            self.queued.push(cmd);
            ui.close();
        }
    }

    fn document_menu(&mut self, ui: &mut egui::Ui, size: egui::Vec2, glyph: f32) {
        let button = egui::Button::new(RichText::new(icon::FILE).size(glyph)).min_size(size);
        let name = self
            .scene
            .bundle
            .root
            .file_stem()
            .map_or_else(String::new, |s| s.to_string_lossy().to_string());
        egui::containers::menu::MenuButton::from_button(button)
            .ui(ui, |ui| self.document_entries(ui))
            .0
            .hint(format!(
                "{name}: new, open, recent drawings, export, fonts, copy its path"
            ));
    }

    fn document_entries(&mut self, ui: &mut egui::Ui) {
        let name = self
            .scene
            .bundle
            .root
            .file_stem()
            .map_or_else(String::new, |s| s.to_string_lossy().to_string());
        ui.label(RichText::new(&name).strong());
        ui.separator();
        [
            Cmd::NewDocument,
            Cmd::OpenDocument,
            Cmd::OpenRecent,
            Cmd::Export,
            Cmd::AddFont,
            Cmd::CopyPath,
            Cmd::InstallCommand,
            Cmd::ConnectAgent,
            Cmd::ReportProblem,
            Cmd::About,
        ]
        .into_iter()
        .for_each(|cmd| {
            if self.command_button(ui, cmd, Some(cmd.menu())).clicked() {
                ui.close();
            }
        });
    }

    pub(super) fn update_rows(&mut self, ui: &mut egui::Ui) {
        if crate::sparkle::running() {
            if ui
                .button("check for updates")
                .hint("ask klad.studio whether a newer version is out: klad downloads it, installs it and opens again")
                .clicked()
            {
                crate::sparkle::check_now();
            }
            return;
        }
        if let Some(checked) = self.update_check.as_ref().and_then(|c| c.try_recv().ok()) {
            self.update_check = None;
            self.update_said = Some(crate::updates::outcome(&checked));
            self.settings.updates_checked = crate::updates::now();
            self.settings_dirty = true;
            if let Ok(Some(release)) = checked {
                self.update = Some(release);
                self.update_said = None;
            }
        }
        if let Some(release) = &self.update {
            ui.horizontal(|ui| {
                ui.label(crate::updates::summary(release));
                ui.hyperlink_to("what changed", &release.url);
            });
        }
        ui.horizontal(|ui| {
            let checking = self.update_check.is_some();
            if ui
                .add_enabled(!checking, egui::Button::new("check now"))
                .hint("ask klad's releases whether a newer version is out")
                .clicked()
            {
                self.update_check = Some(crate::updates::spawn(self.update_feed.clone()));
                self.update_said = None;
            }
            if checking {
                ui.label(RichText::new("checking").color(theme::WEAK));
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
            } else if let Some(said) = &self.update_said {
                ui.label(RichText::new(said).color(theme::WEAK));
            }
        });
    }

    fn about_panel(&mut self, ui: &mut egui::Ui) {
        static LINES: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
        let lines = LINES.get_or_init(|| klad_core::notices::NOTICES.lines().collect());
        ui.label(RichText::new(format!("klad {}", crate::VERSION)).strong());
        self.update_rows(ui);
        ui.label("Source-available licence, copyright Indicode. The drawings you make are yours: you may sell them and use them in your own and your clients' commercial work. Built on the open source work listed below; klad licences prints the same.");
        ui.horizontal(|ui| {
            ui.hyperlink_to("klad.studio", klad_core::contact::SITE)
                .hint("klad's website: the guides, what's new and your download");
            ui.label(RichText::new("·").color(theme::WEAK));
            ui.hyperlink_to(
                klad_core::contact::SUPPORT,
                format!("mailto:{}", klad_core::contact::SUPPORT),
            )
            .hint("support, by email");
        });
        ui.add_space(6.0);
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
        let row = ui.text_style_height(&egui::TextStyle::Small);
        egui::ScrollArea::vertical()
            .max_height(420.0)
            .auto_shrink([false, false])
            .pen_scroll(ui.ctx())
            .show_rows(ui, row, lines.len(), |ui, rows| {
                ui.set_width(600.0);
                lines[rows].iter().for_each(|line| {
                    ui.label(RichText::new(*line).small());
                });
            });
    }

    fn panel_checks(&mut self, ui: &mut egui::Ui) {
        [
            Cmd::ToolPane,
            Cmd::BrushPane,
            Cmd::LayersPane,
            Cmd::InspectorPane,
            Cmd::NotesPane,
            Cmd::HistoryPane,
            Cmd::Timeline,
        ]
        .into_iter()
        .for_each(|cmd| {
            self.command_check(ui, cmd, cmd.menu());
        });
    }

    fn panels_menu(&mut self, ui: &mut egui::Ui, size: egui::Vec2, glyph: f32) {
        let button = egui::Button::new(RichText::new(icon::SIDEBAR).size(glyph)).min_size(size);
        egui::containers::menu::MenuButton::from_button(button)
            .ui(ui, |ui| self.panel_checks(ui))
            .0
            .hint("the panels: tick one to show it, untick to close it");
    }

    fn dock(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.spacing_mut().scroll = theme::shown_scroll();
        self.dock_room = ui.available_height();
        self.dock_fit.chosen = self.dock_kept;
        self.folded_for_room = self.settings.layout.folded_for_room(self.dock_room, &self.dock_fit);
        let bodies = self.settings.layout.bodies(self.dock_room, &self.dock_fit);
        let mut previous: Option<(Pane, bool)> = None;
        let content: Vec<(Pane, f32)> = bodies
            .into_iter()
            .filter_map(|(pane, body)| {
                self.pane_title(ui, pane, body.is_none(), previous);
                previous = Some((pane, body.is_some()));
                body.map(|height| match pane {
                    Pane::Layers => (pane, self.layers_body(ui, height)),
                    _ => {
                        let shown = egui::ScrollArea::vertical()
                            .id_salt(("pane", pane))
                            .max_height(height)
                            .min_scrolled_height(height)
                            .auto_shrink([false, false])
                            .pen_scroll(ui.ctx())
                            .show(ui, |ui| {
                                ui.spacing_mut().item_spacing.y = 5.0;
                                egui::Frame::new()
                                    .inner_margin(Margin::symmetric(8, 6))
                                    .show(ui, |ui| self.check(|e| e.pane_body(ui, pane)));
                            });
                        (pane, shown.content_size.y)
                    }
                })
            })
            .collect();
        self.dock_fit.content = content;
    }

    fn pane_title(&mut self, ui: &mut egui::Ui, pane: Pane, collapsed: bool, previous: Option<(Pane, bool)>) {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), layout::TITLE), Sense::hover());
        ui.painter().rect_filled(rect, 0.0, Color32::from_gray(42));
        ui.painter().line_segment(
            [rect.left_top(), rect.right_top()],
            egui::Stroke::new(1.0, Color32::from_gray(24)),
        );
        if let Some((above, true)) = previous {
            let grip = egui::Rect::from_min_size(rect.min, vec2(rect.width(), 5.0));
            let r = ui.interact(grip, Id::unique(("pane grip", pane)), Sense::drag());
            if r.hovered() || r.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
            }
            if r.dragged() {
                let dy = r.drag_delta().y;
                self.settings.layout.resize_pane(above, dy);
            }
            if r.drag_stopped() {
                self.settings_dirty = true;
            }
        }
        let mut child = ui.new_child(
            UiBuilder::new()
                .max_rect(rect.shrink2(vec2(4.0, 3.0)))
                .layout(Flow::left_to_right(Align::Center)),
        );
        let caret = if collapsed { icon::FOLDED } else { icon::OPEN };
        let toggle = crate::widgets::icon_button(
            &mut child,
            caret,
            match collapsed {
                true => format!("open {}", pane.title()),
                false => format!("fold {} to its title", pane.title()),
            },
        );
        child.label(RichText::new(pane.title()).font(theme::medium(theme::BODY)));
        let mut close = false;
        child.with_layout(Flow::right_to_left(Align::Center), |ui| {
            close = crate::widgets::quiet_icon_button(
                ui,
                icon::CLOSE,
                format!("close {}; cmd K brings it back", pane.title()),
            )
            .clicked();
        });
        if toggle.clicked() {
            match collapsed {
                true => self.show_pane(pane),
                false => {
                    if let Some(p) = self.settings.layout.pane_mut(pane) {
                        p.collapsed = true;
                    }
                    self.settings_dirty = true;
                }
            }
        }
        if close {
            self.settings.layout.toggle(pane);
            self.settings_dirty = true;
        }
    }

    fn pane_body(&mut self, ui: &mut egui::Ui, pane: Pane) -> Result<()> {
        match pane {
            Pane::Tool => self.colour_panel(ui),
            Pane::Brush => {
                self.brush_tiles(ui);
                Ok(())
            }
            Pane::Layers => Ok(()),
            Pane::Inspector => match (self.tool, self.selection.clone()) {
                _ if self.on_marks() => {
                    self.strokes_inspector(ui)?;
                    ui.separator();
                    self.layer_inspector(ui)
                }
                (Tool::Direct, _) => {
                    self.path_inspector(ui)?;
                    ui.separator();
                    self.layer_inspector(ui)
                }
                (_, Some(id)) => self.node_inspector(ui, &id),
                (_, None) => self.layer_inspector(ui),
            },
            Pane::Notes => self.notes_pane(ui),
            Pane::History => self.history_pane(ui),
            Pane::Preview => self.real_size_pane(ui),
            Pane::Maps => self.maps_pane(ui),
        }
    }

    fn real_size_pane(&mut self, ui: &mut egui::Ui) -> Result<()> {
        let (w, h) = self.scene.size();
        if w > super::REAL_MAX || h > super::REAL_MAX {
            ui.label(
                RichText::new(format!(
                    "this panel shows drawings up to {} px a side at their real size; this one is {w} x {h}",
                    super::REAL_MAX
                ))
                .color(theme::WEAK),
            );
            return Ok(());
        }
        ui.horizontal(|ui| {
            [1u32, 2, 4].into_iter().for_each(|scale| {
                let chosen = self.settings.real_size_scale == scale;
                if ui
                    .selectable_label(chosen, format!("{scale}x"))
                    .hint(format!(
                        "one pixel of the drawing on {} of the screen",
                        match scale {
                            1 => "one pixel".to_string(),
                            n => format!("{n} by {n} pixels"),
                        }
                    ))
                    .clicked()
                    && !chosen
                {
                    self.settings.real_size_scale = scale;
                    self.settings_dirty = true;
                }
            });
            ui.label(RichText::new(format!("{w} x {h}")).color(theme::WEAK));
        });
        let key = (self.scene.bundle.doc.rev, self.scene.frame, self.painted);
        if self.real.as_ref().is_none_or(|(held, _)| *held != key) {
            let tiles = self.scene.compose(&self.scene.all_tiles(), None)?;
            let image = real_image((w, h), self.scene.cols(), &tiles);
            let texture = ui.ctx().load_texture("real size", image, egui::TextureOptions::NEAREST);
            self.real = Some((key, texture));
        }
        let Some((_, texture)) = self.real.as_ref() else {
            return Ok(());
        };
        let scale = self.settings.real_size_scale.clamp(1, 4) as f32 / ui.ctx().pixels_per_point();
        let size = vec2(w as f32, h as f32) * scale;
        egui::ScrollArea::both()
            .id_salt("real size")
            .pen_scroll(ui.ctx())
            .show(ui, |ui| {
                let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                let painter = ui.painter().with_clip_rect(rect);
                painter.rect_filled(rect, 0.0, Color32::from_gray(235));
                let cell = 8.0;
                let (cols, rows) = ((size.x / cell).ceil() as usize, (size.y / cell).ceil() as usize);
                (0..rows)
                    .flat_map(|j| (0..cols).map(move |i| (i, j)))
                    .filter(|(i, j)| (i + j) % 2 == 1)
                    .for_each(|(i, j)| {
                        let at = rect.min + vec2(i as f32 * cell, j as f32 * cell);
                        painter.rect_filled(
                            egui::Rect::from_min_size(at, vec2(cell, cell)),
                            0.0,
                            Color32::from_gray(204),
                        );
                    });
                painter.image(
                    texture.id(),
                    rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            });
        Ok(())
    }

    fn status_line(&mut self, ui: &mut egui::Ui) {
        let now = Instant::now();
        let mut cancel = None;
        let mut open_log = false;
        ui.horizontal_centered(|ui| {
            if self.settings.layout.hidden {
                if ui
                    .add(egui::Button::new(format!("{} show the panels (Tab)", icon::SIDEBAR)).small())
                    .hint("the panels are hidden: this or Tab brings them back")
                    .clicked()
                {
                    self.queued.push(Cmd::HideAll);
                }
                ui.separator();
            }
            let frames = self.scene.bundle.doc.timeline.frames;
            ui.label(RichText::new(format!("frame {} of {frames}", self.shown_frame())).color(theme::WEAK));
            if self.timeline.player.is_some() {
                ui.label(
                    RichText::new(format!("{} playing {}", icon::PLAY, self.timeline.playing)).color(theme::ACCENT),
                );
            }
            ui.separator();
            ui.label(RichText::new(self.draws_into()).color(theme::WEAK));
            if let Some(keys) = self.drag_keys() {
                ui.separator();
                ui.label(RichText::new(keys).color(theme::ACCENT));
            }
            self.jobs.running(now).into_iter().for_each(|(id, name, took)| {
                ui.separator();
                ui.add(egui::Spinner::new().size(theme::ICON_SMALL));
                ui.label(format!("{name} {:.1} s", took.as_secs_f32()));
                if ui
                    .add(egui::Button::new(icon::CLOSE).min_size(vec2(theme::CONTROL, theme::CONTROL)))
                    .hint(format!("cancel {name}"))
                    .clicked()
                {
                    cancel = Some(id);
                }
            });
            self.internal_work(now).into_iter().for_each(|what| {
                ui.separator();
                ui.add(egui::Spinner::new().size(theme::ICON_SMALL));
                ui.label(what);
            });
            ui.with_layout(Flow::right_to_left(Align::Center), |ui| {
                if let Some(view) = self.view {
                    let degrees = view.angle.to_degrees().rem_euclid(360.0);
                    let flat = |text: String| crate::widgets::ghost_button(RichText::new(text).color(theme::WEAK));
                    let zoom = ui
                        .add(flat(format!("{:.0}%", view.zoom * ui.ctx().pixels_per_point() * 100.0)))
                        .hint("zoom; click for 100%, double-click to fit the canvas");
                    if zoom.double_clicked() {
                        self.queued.push(Cmd::Fit);
                    } else if zoom.clicked() {
                        self.queued.push(Cmd::ActualSize);
                    }
                    if ui
                        .add(flat(format!("{degrees:.0}\u{b0}")))
                        .hint("the view's rotation; click to turn it upright")
                        .clicked()
                    {
                        self.queued.push(Cmd::ResetRotation);
                    }
                    if let Some([x, y]) = self.hover_cell {
                        ui.add(flat(format!("{x}, {y}")))
                            .hint("the pixel under the pointer: across, then down, from the top left");
                    }
                    if view.flip
                        && ui
                            .add(flat(format!("{} flipped", icon::FLIP_H)))
                            .hint("the view is mirrored (H); the document is not")
                            .clicked()
                    {
                        self.queued.push(Cmd::FlipView);
                    }
                }
                ui.separator();
                if let Some(m) = self.messages.latest() {
                    let text = RichText::new(format!("{} {}", icon_of(m.tone), m.text)).color(colour_of(m.tone));
                    open_log = ui
                        .add(egui::Button::new(text).frame(false).truncate())
                        .hint("the last message; click for the message log")
                        .clicked();
                }
            });
        });
        if let Some(id) = cancel {
            self.jobs.cancel(id);
        }
        if open_log {
            self.windows.log = true;
        }
    }

    fn internal_work(&self, now: Instant) -> Vec<String> {
        let slow = Duration::from_millis(600);
        let mut work = Vec::new();
        if self
            .preview
            .as_ref()
            .and_then(|p| p.finish.as_ref())
            .is_some_and(|f| now.duration_since(f.since) > slow)
        {
            work.push("drawing the move".to_string());
        }
        if self.path_commit.is_some() {
            work.push("drawing the edited path".to_string());
        }
        let loading = self.frames.jobs() > 0 && self.timeline.on_screen != Some(self.scene.frame);
        if self.timeline.player.is_none() && (loading || self.canvas_loading()) {
            work.push(format!("loading frame {}", self.scene.frame));
        }
        work.extend(self.preparing_text());
        if self.maps.making {
            work.push("making the light maps".to_string());
        }
        if self.depth.preparing {
            work.push("preparing the planes".to_string());
        }
        work
    }

    pub(super) fn window(
        ctx: &egui::Context,
        id: &str,
        (title, spot): (String, Spot),
        open: &mut bool,
        add: impl FnOnce(&mut egui::Ui),
    ) {
        let key = Id::unique(id);
        let frame = ctx.cumulative_frame_nr();
        let mut place = ctx.data(|d| d.get_temp::<Placing>(key)).unwrap_or_default();
        let mut others: Vec<(Id, u64)> = ctx.data(|d| d.get_temp(open_windows())).unwrap_or_default();
        others.retain(|(id, seen)| *id != key && seen + 1 >= frame);
        if place.seen + 1 < frame && matches!(spot, Spot::Middle) {
            place = Placing {
                opened: place.opened + 1,
                offset: CASCADE * others.len() as f32,
                ..Placing::default()
            };
        }
        let others_open = others.len();
        others.push((key, frame));
        ctx.data_mut(|d| d.insert_temp(open_windows(), others));
        let area = key.with(place.opened);
        let window = egui::Window::new(&title)
            .id(area)
            .title_bar(false)
            .resizable(true)
            .default_width(360.0)
            .max_height(window_room(ctx))
            .constrain_to(window_area(ctx));
        let middle = ctx.content_rect().center() + place.offset;
        let size = ctx.memory(|m| m.area_rect(area)).map(|r| r.size());
        let window = match (spot, size) {
            (Spot::At(at), _) => window.default_pos(at),
            (Spot::Middle, Some(size)) if !place.settled => {
                place.offset = CASCADE * others_open as f32;
                place.frames += 1;
                place.settled = place.frames >= SETTLE || place.size.is_some_and(|was| (was - size).length() < 0.5);
                place.size = Some(size);
                window.current_pos(ctx.content_rect().center() + place.offset - size / 2.0)
            }
            (Spot::Middle, _) => window.default_pos(middle - vec2(180.0, 160.0)),
        };
        place.seen = frame;
        ctx.data_mut(|d| d.insert_temp(key, place));
        window.show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&title).strong());
                ui.with_layout(Flow::right_to_left(Align::Center), |ui| {
                    if ui
                        .add(
                            egui::Button::new(RichText::new(icon::CLOSE).size(theme::ICON))
                                .frame(false)
                                .min_size(vec2(theme::CONTROL, theme::CONTROL)),
                        )
                        .hint(format!("close {title}"))
                        .clicked()
                    {
                        *open = false;
                    }
                });
            });
            ui.separator();
            add(ui);
        });
    }

    fn windows(&mut self, ctx: &egui::Context) {
        if !self.settings.layout.hidden || self.selected().is_some() {
            self.selection_bar(ctx);
        }
        let screen = ctx.content_rect();
        let layout = &self.settings.layout;
        let dock = if layout.shows_dock() { layout.dock_width } else { 0.0 };
        let strip = if layout.shows_strip() { layout::STRIP } else { 0.0 };
        let timeline = self.timeline_shown;
        let above = screen.bottom() - layout::STATUS - timeline;
        let numbers_at = Spot::At(egui::pos2(screen.right() - dock - 380.0, screen.top() + 16.0));
        let log_at = Spot::At(egui::pos2(
            screen.left() + strip + 16.0,
            (above - 380.0).max(screen.top() + 16.0),
        ));
        self.palette_window(ctx);
        self.upright_chip(ctx);
        if self.settings.layout.numbers && self.settings.developer {
            let mut open = true;
            Self::window(
                ctx,
                "numbers",
                (format!("{} Numbers", icon::NUMBERS), numbers_at),
                &mut open,
                |ui| self.numbers_panel(ui),
            );
            if !open {
                self.settings.layout.numbers = false;
                self.settings_dirty = true;
            }
        }
        self.settings_modal(ctx);
        if self.windows.brush {
            let mut open = true;
            Self::window(
                ctx,
                "brush",
                (format!("{} Brush", icon::BRUSH), Spot::Middle),
                &mut open,
                |ui| self.check(|e| e.brush_window_ui(ui)),
            );
            self.windows.brush &= open;
        }
        if self.windows.report {
            let mut open = true;
            Self::window(
                ctx,
                "report",
                (format!("{} Report a problem", icon::LOG), Spot::Middle),
                &mut open,
                |ui| self.report_panel(ui),
            );
            self.windows.report &= open;
        }
        if self.windows.connect {
            let mut open = true;
            Self::window(
                ctx,
                "connect",
                (format!("{} Connect an agent app", icon::LINK), Spot::Middle),
                &mut open,
                |ui| {
                    ui.set_width(crate::connect::WIDTH);
                    if self.connect.panel(ui, &mut self.settings) {
                        self.settings_dirty = true;
                    }
                },
            );
            self.windows.connect &= open;
        }
        if self.windows.about {
            let mut open = true;
            Self::window(
                ctx,
                "about",
                (format!("{} About klad", icon::INFO), Spot::Middle),
                &mut open,
                |ui| self.about_panel(ui),
            );
            self.windows.about &= open;
        }
        if self.windows.log {
            let mut open = true;
            Self::window(
                ctx,
                "log",
                (format!("{} Messages", icon::LOG), log_at),
                &mut open,
                |ui| self.log_panel(ui),
            );
            self.windows.log &= open;
        }
        if self.windows.onion {
            let mut open = true;
            Self::window(
                ctx,
                "onion",
                (format!("{} Onion skin", icon::ONION), Spot::Middle),
                &mut open,
                |ui| self.check(|e| e.onion_settings(ui)),
            );
            self.windows.onion &= open;
        }
        if self.windows.link {
            let mut open = true;
            let mut chosen = None;
            let drawings: Vec<(String, u32)> = self
                .scene
                .active_layer()
                .map(|l| {
                    l.cels()
                        .iter()
                        .filter(|c| c.link.is_none())
                        .map(|c| (c.id.clone(), c.frame))
                        .collect()
                })
                .unwrap_or_default();
            let frame = self.scene.frame;
            let blocked = self.link_blocked();
            let thumbs: Vec<_> = drawings.iter().map(|(id, _)| self.thumb(ctx, id)).collect();
            Self::window(
                ctx,
                "link",
                (format!("{} Show a drawing here", icon::LINK), Spot::Middle),
                &mut open,
                |ui| {
                    match &blocked {
                        Some(why) => ui.colored_label(theme::REFUSED, why.as_str()),
                        None => ui.label(format!("frame {frame} will show the drawing you pick, as a link")),
                    };
                    ui.horizontal_wrapped(|ui| {
                        drawings.iter().zip(&thumbs).for_each(|((id, f), thumb)| {
                            let text = format!("frame {f}");
                            let button = match thumb {
                                Some(t) => egui::Button::image_and_text(
                                    egui::Image::new(t).fit_to_exact_size(egui::vec2(
                                        layers::THUMB.0 as f32 * 2.0,
                                        layers::THUMB.1 as f32 * 2.0,
                                    )),
                                    text,
                                ),
                                None => egui::Button::new(text),
                            };
                            if ui
                                .add_enabled(blocked.is_none(), button)
                                .hint(format!(
                                    "show the drawing of frame {f} again here; drawing on it changes both"
                                ))
                                .clicked()
                            {
                                chosen = Some(id.clone());
                            }
                        });
                    });
                },
            );
            if let Some(id) = chosen {
                self.check(|e| e.link_here(id));
                open = false;
            }
            self.windows.link &= open;
        }
        if self.windows.export {
            let mut open = true;
            Self::window(
                ctx,
                self.export.what.title(),
                (format!("{} {}", icon::EXPORT, self.export.what.title()), Spot::Middle),
                &mut open,
                |ui| self.check(|e| e.export_window(ui)),
            );
            self.windows.export &= open;
        }
        if self.windows.pixels {
            let mut open = true;
            Self::window(
                ctx,
                "pixels",
                (format!("{} Pixels of a picture", icon::PIXELS), Spot::Middle),
                &mut open,
                |ui| self.check(|e| e.pixels_window(ui)),
            );
            self.windows.pixels &= open;
        }
        if self.windows.tiles {
            let mut open = true;
            Self::window(
                ctx,
                "tiles",
                (format!("{} Tileset", icon::PIXELS), Spot::Middle),
                &mut open,
                |ui| self.check(|e| e.tiles_window(ui)),
            );
            self.windows.tiles &= open;
        }
        if self.windows.grid {
            let mut open = true;
            Self::window(
                ctx,
                "grid",
                (format!("{} Grid", icon::PIXELS), Spot::Middle),
                &mut open,
                |ui| self.check(|e| e.grid_window(ui)),
            );
            self.windows.grid &= open;
        }
        if self.windows.assists {
            let mut open = true;
            Self::window(
                ctx,
                "drawing guides",
                (format!("{} Drawing guides", icon::PERSPECTIVE), Spot::Middle),
                &mut open,
                |ui| self.check(|e| e.assists_window(ui)),
            );
            self.windows.assists &= open;
        }
        self.naming_window(ctx);
        if self.windows.frames {
            let mut open = true;
            Self::window(
                ctx,
                "import frames",
                (format!("{} Import frames", icon::IMAGE), Spot::Middle),
                &mut open,
                |ui| self.check(|e| e.frames_window(ui)),
            );
            self.windows.frames &= open;
        }
        if self.windows.canvas {
            let mut open = true;
            Self::window(
                ctx,
                "canvas",
                (format!("{} Canvas", icon::SIZE), Spot::Middle),
                &mut open,
                |ui| self.check(|e| e.canvas_window(ui)),
            );
            self.windows.canvas &= open;
        }
        if self.windows.paintover {
            let mut open = true;
            Self::window(
                ctx,
                "paintover",
                (format!("{} Paint over", icon::BRUSH), Spot::Middle),
                &mut open,
                |ui| self.check(|e| e.paintover_window(ui)),
            );
            self.windows.paintover &= open;
        }
        if self.windows.compare {
            let mut open = true;
            Self::window(
                ctx,
                "compare",
                (format!("{} Compared with the reference", icon::INFO), Spot::Middle),
                &mut open,
                |ui| self.check(|e| e.compare_window(ui)),
            );
            self.windows.compare &= open;
        }
        if self.windows.font {
            let mut open = true;
            Self::window(
                ctx,
                "font",
                (format!("{} Add a font", icon::PLUS), Spot::Middle),
                &mut open,
                |ui| self.check(|e| e.font_window(ui)),
            );
            self.windows.font &= open;
        }
        if self.windows.between {
            let mut open = true;
            Self::window(
                ctx,
                "between",
                (format!("{} In-betweens", icon::INBETWEEN), Spot::Middle),
                &mut open,
                |ui| self.check(|e| e.between_window(ui)),
            );
            self.windows.between &= open;
        }
        if self.windows.clip {
            let mut open = true;
            let title = self
                .current_clip()
                .map_or("Clip".to_string(), |c| format!("Clip {}", c.name));
            Self::window(
                ctx,
                "clip",
                (format!("{} {title}", icon::CLIP_BAR), Spot::Middle),
                &mut open,
                |ui| self.check(|e| e.clip_window(ui)),
            );
            self.windows.clip &= open;
        }
    }

    fn upright_chip(&mut self, ctx: &egui::Context) {
        let Some(degrees) = self.view.and_then(|v| v.turned()) else {
            return;
        };
        let rect = self.canvas_rect;
        egui::Area::new(Id::unique("upright"))
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::CENTER_TOP)
            .fixed_pos(egui::pos2(rect.center().x, rect.top() + 8.0))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("view turned {degrees:.0}\u{b0}")).color(theme::WEAK));
                        self.command_button(ui, Cmd::ResetRotation, Some("upright"));
                    });
                });
            });
    }

    fn numbers_panel(&mut self, ui: &mut egui::Ui) {
        let r = &self.readout;
        ui.add(egui::ProgressBar::new(r.pressure).text(format!("pressure {:.3}", r.pressure)));
        ui.label(format!(
            "tilt {:.2}, {:.2}   {}{}   {} samples/s",
            r.tilt.0,
            r.tilt.1,
            if r.tablet { "tablet" } else { "mouse" },
            if r.eraser { ", eraser end" } else { "" },
            r.samples.len()
        ));
        let lines = {
            let s = lock(&self.stats);
            [
                format!(
                    "frame        {}  ({:.0} fps)",
                    s.frame_ms.summary(),
                    1000.0 / s.frame_ms.avg().max(0.001)
                ),
                format!("pen to GPU   {}", s.latency_ms.summary()),
                format!("ui           {}", s.ui_ms.spread()),
                format!("canvas       {}", s.canvas_ms.spread()),
                format!("compose      {}", s.compose_ms.summary()),
                format!("tiles        {}", s.tiles.summary()),
                format!("upload       {}", s.upload_ms.summary()),
                format!("frame step   {}", s.step_ms.summary()),
                format!("frame upload {}", s.frame_upload_ms.summary()),
            ]
        };
        lines.iter().for_each(|l| {
            ui.label(RichText::new(l).monospace());
        });
        ui.label(
            RichText::new(format!(
                "frame cache  {} frames, {} MB, {} composing",
                self.frames.len(),
                self.frames.bytes() >> 20,
                self.frames.jobs()
            ))
            .monospace(),
        );
        ui.label(RichText::new(format!("drawings     {} MB in memory", self.scene.cache_bytes() >> 20)).monospace());
        ui.separator();
        let measured: Vec<(String, String)> = self.measured.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
        measured.into_iter().for_each(|(k, v)| {
            ui.label(RichText::new(format!("{k}: {v}")).small());
        });
        ui.horizontal_wrapped(|ui| {
            self.command_check(ui, Cmd::Spin, "spin (fps test)");
            self.command_button(ui, Cmd::ResetNumbers, Some("reset"));
        });
    }

    fn log_panel(&mut self, ui: &mut egui::Ui) {
        let now = Instant::now();
        let log: Vec<Message> = self.messages.log().rev().cloned().collect();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .pen_scroll(ui.ctx())
            .show(ui, |ui| {
                if log.is_empty() {
                    ui.label("no messages yet");
                }
                log.iter().for_each(|m| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(icon_of(m.tone)).color(colour_of(m.tone)));
                        ui.label(RichText::new(ago(m.at, now)).color(theme::WEAK).small());
                        let times = if m.times > 1 {
                            format!(" (x{})", m.times)
                        } else {
                            String::new()
                        };
                        ui.label(format!("{}{times}", m.text));
                    });
                });
            });
    }

    fn toasts(&mut self, ui: &mut egui::Ui) {
        const WIDTH: f32 = 380.0;
        let shown: Vec<Message> = self.messages.toasts(Instant::now()).into_iter().cloned().collect();
        let canvas = self.canvas_rect;
        self.held_messages.clear();
        if shown.is_empty() || canvas.width() <= 0.0 || canvas.height() <= 0.0 {
            return;
        }
        let mut dismiss = None;
        let mut held = Vec::new();
        let room = canvas.shrink(12.0);
        let width = WIDTH.min(room.width());
        let tall = (room.height() / shown.len() as f32 - 24.0).max(theme::CONTROL);
        let stack = egui::UiBuilder::new()
            .max_rect(room)
            .layout(egui::Layout::bottom_up(egui::Align::Min));
        ui.scope_builder(stack, |ui| {
            ui.set_clip_rect(canvas);
            ui.spacing_mut().item_spacing.y = 6.0;
            shown.iter().rev().for_each(|m| {
                let sticky = m.tone == Tone::Error;
                let sense = match sticky {
                    true => Sense::click_and_drag(),
                    false => Sense::hover(),
                };
                let toast = ui.scope_builder(egui::UiBuilder::new().id_salt(("toast", m.id)).sense(sense), |ui| {
                    egui::Frame::popup(ui.style())
                        .fill(Color32::from_gray(30))
                        .stroke(egui::Stroke::new(1.0, colour_of(m.tone)))
                        .show(ui, |ui| {
                            ui.set_max_width(width);
                            let words = width - theme::ICON - if sticky { theme::CONTROL + 12.0 } else { 6.0 };
                            let body = egui::TextStyle::Body.resolve(ui.style());
                            let high = ui
                                .painter()
                                .layout(m.text.clone(), body, Color32::WHITE, words)
                                .size()
                                .y;
                            let row = high.min(tall).max(theme::CONTROL);
                            let align = match high > theme::CONTROL {
                                true => egui::Align::Min,
                                false => egui::Align::Center,
                            };
                            ui.allocate_ui_with_layout(vec2(width, row), egui::Layout::left_to_right(align), |ui| {
                                ui.label(
                                    RichText::new(icon_of(m.tone))
                                        .color(colour_of(m.tone))
                                        .size(theme::ICON),
                                );
                                ui.scope(|ui| {
                                    ui.set_max_width(words);
                                    let label = |ui: &mut egui::Ui| {
                                        ui.add(egui::Label::new(&m.text).wrap().selectable(false));
                                    };
                                    match high > tall {
                                        true => {
                                            egui::ScrollArea::vertical()
                                                .id_salt(("toast words", m.id))
                                                .max_height(tall)
                                                .pen_scroll(ui.ctx())
                                                .show(ui, |ui| {
                                                    ui.set_max_width(words);
                                                    label(ui);
                                                });
                                        }
                                        false => label(ui),
                                    }
                                });
                                if sticky
                                    && ui
                                        .add(
                                            egui::Button::new(icon::CLOSE)
                                                .min_size(vec2(theme::CONTROL, theme::CONTROL)),
                                        )
                                        .hint("dismiss; the message log keeps it")
                                        .clicked()
                                {
                                    dismiss = Some(m.id);
                                }
                            });
                        });
                });
                if sticky {
                    held.push(toast.response.rect);
                }
            });
        });
        self.held_messages = held;
        if let Some(id) = dismiss {
            self.messages.dismiss(id);
        }
    }

    fn palette_window(&mut self, ctx: &egui::Context) {
        let Some(mut state) = self.palette.take() else {
            return;
        };
        let recent = self.recent_commands();
        let developer = self.settings.developer;
        let found: Vec<Cmd> = commands::rank(&state.query, &recent)
            .into_iter()
            .filter(|c| developer || !commands::DEVELOPER.contains(c))
            .collect();
        let mut run = None;
        let modal = egui::Modal::new(Id::unique("palette")).show(ctx, |ui| {
            ui.set_width(560.0);
            let (down, up, enter) = ui.input_mut(|i| {
                (
                    i.consume_key(Modifiers::NONE, Key::ArrowDown),
                    i.consume_key(Modifiers::NONE, Key::ArrowUp),
                    i.consume_key(Modifiers::NONE, Key::Enter),
                )
            });
            let last = found.len().saturating_sub(1);
            state.selected = match (down, up) {
                (true, _) => (state.selected + 1).min(last),
                (_, true) => state.selected.saturating_sub(1),
                _ => state.selected.min(last),
            };
            let label = ui.label(RichText::new(format!("{} Find a command", icon::SEARCH)).strong());
            let edit = ui
                .add(
                    egui::TextEdit::singleline(&mut state.query)
                        .hint_text("type what you want to do, or a key")
                        .desired_width(f32::INFINITY),
                )
                .labelled_by(label.id)
                .hint("type a word for what you want to do; arrows pick, Return runs, Esc closes");
            if !state.opened {
                edit.request_focus();
                state.opened = true;
            }
            if edit.changed() {
                state.selected = 0;
            }
            if enter {
                run = found.get(state.selected).copied();
            }
            ui.add_space(4.0);
            let room = (ui.ctx().content_rect().height() - FINDER_CHROME).clamp(FINDER_LEAST, FINDER_MOST);
            egui::ScrollArea::vertical()
                .max_height(room)
                .min_scrolled_height(room)
                .pen_scroll(ui.ctx())
                .show(ui, |ui| {
                    if found.is_empty() {
                        ui.label("nothing matches");
                    }
                    found.iter().enumerate().for_each(|(i, cmd)| {
                        let spec = cmd.spec();
                        let reason = self.can_run(*cmd);
                        let colour = if reason.is_some() { theme::WEAK } else { theme::TEXT };
                        let row = ui
                            .add(
                                egui::Button::new(RichText::new(format!("{}  {}", spec.icon, spec.name)).color(colour))
                                    .right_text(RichText::new(self.bindings.label(*cmd)).color(theme::WEAK))
                                    .selected(i == state.selected)
                                    .min_size(vec2(ui.available_width(), theme::CONTROL)),
                            )
                            .hint(self.command_hint(*cmd));
                        if i == state.selected && (down || up) {
                            row.scroll_to_me(None);
                        }
                        if let Some(reason) = reason.filter(|_| !spec.is_hold()) {
                            ui.label(
                                RichText::new(format!("    {}: {reason}", spec.group.name()))
                                    .small()
                                    .color(theme::WEAK),
                            );
                        }
                        if row.clicked() {
                            run = Some(*cmd);
                        }
                    });
                });
        });
        match run {
            Some(cmd) => self.queued.push(cmd),
            None if !modal.should_close() => self.palette = Some(state),
            None => {}
        }
    }
}

fn scrolled_menu(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    let room = (ui.ctx().content_rect().height() - MENU_CHROME).max(MENU_LEAST);
    egui::ScrollArea::vertical()
        .max_height(room)
        .min_scrolled_height(room)
        .pen_scroll(ui.ctx())
        .show(ui, add);
}

pub(super) fn real_image((w, h): (u32, u32), cols: usize, tiles: &[(usize, Vec<half::f16>)]) -> egui::ColorImage {
    let side = klad_brush::TILE;
    let byte = |v: half::f16| (v.to_f32().clamp(0.0, 1.0) * 255.0).round() as u8;
    let pixels: Vec<Color32> = (0..h as usize)
        .flat_map(|y| (0..w as usize).map(move |x| (x, y)))
        .map(|(x, y)| {
            let index = (y / side) * cols + x / side;
            match tiles.iter().find(|(i, _)| *i == index) {
                Some((_, tile)) => {
                    let at = ((y % side) * side + x % side) * 4;
                    Color32::from_rgba_premultiplied(
                        byte(tile[at]),
                        byte(tile[at + 1]),
                        byte(tile[at + 2]),
                        byte(tile[at + 3]),
                    )
                }
                None => Color32::TRANSPARENT,
            }
        })
        .collect();
    egui::ColorImage::new([w as usize, h as usize], pixels)
}
