use super::Editor;
use crate::theme::{self, Hint};
use anyhow::{Result, bail};
use egui::{Color32, RichText, Stroke, TextureHandle, TextureOptions, pos2, vec2};
use klad_render::sheetcut::{Cut, cells};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Way {
    #[default]
    Cells,
    Grid,
    Files,
}

#[derive(Default)]
pub(super) struct FramesIn {
    pub path: String,
    pub way: Way,
    pub cell: [u32; 2],
    pub grid: [u32; 2],
    pub padding: u32,
    pub margin: u32,
    pub seen: Option<Seen>,
}

pub(super) struct Seen {
    path: String,
    size: [u32; 2],
    picture: klad_render::pixelise::Picture,
    texture: TextureHandle,
}

impl FramesIn {
    pub(super) fn fresh() -> Self {
        Self {
            cell: [32, 32],
            grid: [4, 1],
            ..Self::default()
        }
    }

    pub(super) fn cut(&self) -> Option<Cut> {
        match self.way {
            Way::Cells => Some(Cut {
                cell: Some((self.cell[0], self.cell[1])),
                grid: None,
                padding: self.padding,
                margin: self.margin,
            }),
            Way::Grid => Some(Cut {
                cell: None,
                grid: Some((self.grid[0], self.grid[1])),
                padding: self.padding,
                margin: self.margin,
            }),
            Way::Files => None,
        }
    }

    pub(super) fn args(&self, bundle: &std::path::Path) -> Vec<String> {
        let mut args = vec!["import".to_string(), self.path.trim().to_string()];
        match self.way {
            Way::Cells => args.extend(["--cell".into(), format!("{}x{}", self.cell[0], self.cell[1])]),
            Way::Grid => args.extend(["--grid".into(), format!("{}x{}", self.grid[0], self.grid[1])]),
            Way::Files => {}
        }
        if self.way != Way::Files {
            if self.padding > 0 {
                args.extend(["--padding".into(), self.padding.to_string()]);
            }
            if self.margin > 0 {
                args.extend(["--margin".into(), self.margin.to_string()]);
            }
        }
        args.extend(["--to".into(), bundle.to_string_lossy().to_string()]);
        args
    }
}

impl Editor {
    fn frames_seen(&mut self, ctx: &egui::Context) {
        let path = self.frames_in.path.trim().to_string();
        let file = PathBuf::from(&path);
        if self.frames_in.seen.as_ref().is_some_and(|s| s.path == path) || !file.is_file() {
            return;
        }
        let Some(picture) = std::fs::read(&file)
            .ok()
            .and_then(|b| klad_render::pixelise::decode(&b).ok())
        else {
            self.frames_in.seen = None;
            return;
        };
        let image =
            egui::ColorImage::from_rgba_unmultiplied([picture.width as usize, picture.height as usize], &picture.rgba);
        let texture = ctx.load_texture("frames sheet", image, TextureOptions::NEAREST);
        self.frames_in.seen = Some(Seen {
            path,
            size: [picture.width, picture.height],
            picture,
            texture,
        });
    }

    pub(super) fn frames_window(&mut self, ui: &mut egui::Ui) -> Result<()> {
        ui.set_min_width(440.0);
        ui.label(
            RichText::new("Frames from a sprite sheet cut into cells, or from numbered pictures (a folder of them): a new layer with a frame each and a clip.")
                .color(theme::WEAK),
        );
        ui.add_space(6.0);
        let ctx = ui.ctx().clone();
        self.frames_seen(&ctx);
        let mut run = false;
        let f = &mut self.frames_in;
        egui::Grid::new("frames in")
            .num_columns(2)
            .spacing(vec2(14.0, 10.0))
            .show(ui, |ui| {
                let file = ui.label(RichText::new("file").color(theme::WEAK));
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut f.path)
                            .desired_width(240.0)
                            .hint_text("a sheet, or a folder of frames"),
                    )
                    .labelled_by(file.id)
                    .hint("the sprite sheet, or a folder of numbered pictures");
                    if crate::picker::NATIVE
                        && ui.button("Choose…").hint("pick the sheet").clicked()
                        && let Some(p) = crate::picker::choose_picture(None, "Frames from")
                    {
                        f.path = p.display().to_string();
                    }
                    if crate::picker::NATIVE
                        && ui
                            .button("Folder…")
                            .hint("pick a folder of numbered pictures")
                            .clicked()
                        && let Some(p) = crate::picker::choose_folder(None, "Frames from")
                    {
                        f.path = p.display().to_string();
                        f.way = Way::Files;
                    }
                });
                ui.end_row();
                ui.label(RichText::new("cut").color(theme::WEAK));
                ui.horizontal_wrapped(|ui| {
                    [
                        (Way::Cells, "by cell size", "cells of a size you give"),
                        (Way::Grid, "by columns and rows", "the sheet split evenly"),
                        (
                            Way::Files,
                            "a picture per frame",
                            "every picture in the folder is a frame, in the order of their numbers",
                        ),
                    ]
                    .into_iter()
                    .for_each(|(way, text, hint)| {
                        if ui.selectable_label(f.way == way, text).hint(hint).clicked() {
                            f.way = way;
                        }
                    });
                });
                ui.end_row();
                if f.way != Way::Files {
                    let (label, values, unit) = match f.way {
                        Way::Cells => ("cell", &mut f.cell, "px"),
                        _ => ("columns, rows", &mut f.grid, ""),
                    };
                    ui.label(RichText::new(label).color(theme::WEAK));
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut values[0]).range(1..=4096).suffix(unit));
                        ui.label("x");
                        ui.add(egui::DragValue::new(&mut values[1]).range(1..=4096).suffix(unit));
                    });
                    ui.end_row();
                    ui.label(RichText::new("spacing").color(theme::WEAK));
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::DragValue::new(&mut f.padding)
                                .range(0..=256)
                                .prefix("between ")
                                .suffix(" px"),
                        )
                        .hint("px between the cells");
                        ui.add(
                            egui::DragValue::new(&mut f.margin)
                                .range(0..=256)
                                .prefix("around ")
                                .suffix(" px"),
                        )
                        .hint("px around the cells, at the sheet's edges");
                    });
                    ui.end_row();
                }
            });
        if let (Some(seen), Some(cut)) = (&f.seen, f.cut()) {
            let said = match cells(&seen.picture, &cut) {
                Ok((w, h, found)) => format!("{} frames of {w} x {h} px (empty cells are left out)", found.len()),
                Err(e) => e.to_string(),
            };
            ui.add_space(6.0);
            let room = vec2(ui.available_width().min(420.0), 220.0);
            let k = (room.x / seen.size[0] as f32)
                .min(room.y / seen.size[1] as f32)
                .min(8.0);
            let shown = vec2(seen.size[0] as f32 * k, seen.size[1] as f32 * k);
            let (rect, _) = ui.allocate_exact_size(shown, egui::Sense::hover());
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 0.0, Color32::from_gray(60));
            painter.image(
                seen.texture.id(),
                rect,
                egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
            if let Ok((cw, ch, _)) = cells(&seen.picture, &cut) {
                let line = Stroke::new(1.0, theme::ACCENT);
                let step = (cw + cut.padding, ch + cut.padding);
                let inner = |t: u32| t.saturating_sub(2 * cut.margin);
                let cols = (inner(seen.size[0]) + cut.padding) / step.0.max(1);
                let rows = (inner(seen.size[1]) + cut.padding) / step.1.max(1);
                (0..rows).for_each(|r| {
                    (0..cols).for_each(|c| {
                        let x = (cut.margin + c * step.0) as f32 * k;
                        let y = (cut.margin + r * step.1) as f32 * k;
                        let cell = egui::Rect::from_min_size(rect.min + vec2(x, y), vec2(cw as f32 * k, ch as f32 * k));
                        painter.rect_stroke(cell, 0.0, line, egui::StrokeKind::Inside);
                    })
                });
            }
            ui.label(RichText::new(said).color(theme::WEAK));
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            run = ui
                .button("Import")
                .hint("bring the frames in, as klad import does")
                .clicked();
        });
        if run {
            self.import_frames()?;
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn import_frames_for_test(&mut self) -> Result<()> {
        self.import_frames()
    }

    fn import_frames(&mut self) -> Result<()> {
        let path = PathBuf::from(self.frames_in.path.trim());
        if self.frames_in.path.trim().is_empty() || !path.exists() {
            bail!("choose the sheet or the folder of frames first");
        }
        if self.frames_in.way == Way::Files && path.is_file() {
            bail!(
                "a picture per frame takes a folder of numbered pictures; for one sheet, cut it by cell size or by columns and rows"
            );
        }
        let args = self.frames_in.args(&self.scene.bundle.root);
        self.start_job(crate::jobs::Spec {
            tag: crate::jobs::Tag::Edit,
            name: "bring in frames".to_string(),
            exe: PathBuf::new(),
            args,
            env: Vec::new(),
            reruns: 1,
            summary: |_, err| {
                err.lines()
                    .find_map(|l| {
                        l.trim()
                            .strip_prefix("note: ")
                            .filter(|n| n.contains("frames on a new layer"))
                    })
                    .map_or_else(
                        || "brought the frames in".to_string(),
                        |n| format!("{n} (cmd Z takes it back)"),
                    )
            },
            failure: |_, err| super::tweens::failure(err),
        });
        Ok(())
    }
}
