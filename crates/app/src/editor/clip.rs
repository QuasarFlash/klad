use super::selection::Selected;
use super::{Editor, Tool, Want};
use crate::messages::Tone;
use anyhow::{Result, bail};
use egui::{Vec2, vec2};
use klad_brush::Raster;
use klad_render::pixelise::Picture;

pub(super) type Own = (u32, u32, f32);

fn mean_alpha(picture: &Picture) -> f32 {
    let n = (picture.width as usize * picture.height as usize).max(1);
    picture.rgba.as_chunks::<4>().0.iter().map(|p| p[3] as f32).sum::<f32>() / (n as f32 * 255.0)
}

fn own(picture: &Picture) -> Own {
    (picture.width, picture.height, mean_alpha(picture))
}

fn cropped(rgba: &[u8], width: u32, lo: Vec2, hi: Vec2, height: u32) -> Option<Picture> {
    let x0 = (lo.x.floor().max(0.0) as u32).min(width);
    let y0 = (lo.y.floor().max(0.0) as u32).min(height);
    let x1 = (hi.x.ceil().max(0.0) as u32).min(width);
    let y1 = (hi.y.ceil().max(0.0) as u32).min(height);
    let (w, h) = (x1.saturating_sub(x0), y1.saturating_sub(y0));
    if w == 0 || h == 0 {
        return None;
    }
    let rgba = (y0..y1)
        .flat_map(|y| {
            let start = (y as usize * width as usize + x0 as usize) * 4;
            rgba[start..start + w as usize * 4].iter().copied()
        })
        .collect();
    Some(Picture {
        width: w,
        height: h,
        rgba,
    })
}

fn rgba_of(raster: &Raster) -> Option<(Vec<u8>, u32, u32)> {
    let image = klad_render::raster_image(raster).ok()?;
    let frame = klad_render::read_rgba8(&image).ok()?;
    Some((frame.rgba, frame.width, frame.height))
}

pub(super) fn picture_of(image: &egui::ColorImage) -> Picture {
    let [w, h] = image.size;
    Picture {
        width: w as u32,
        height: h as u32,
        rgba: image.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect(),
    }
}

impl Editor {
    fn selection_span(&self) -> Option<(Vec2, Vec2)> {
        let (_, corners) = self.handles()?;
        Some(
            corners
                .iter()
                .fold((corners[0], corners[0]), |(lo, hi), c| (lo.min(*c), hi.max(*c))),
        )
    }

    pub(super) fn selection_picture(&self) -> Option<Picture> {
        let (w, h) = self.scene.size();
        let raster = match self.selected()? {
            Selected::Strokes => {
                let marks = self.copied()?.marks();
                let mut raster = Raster::new(w, h);
                klad_brush::replay_onto(&mut raster, &marks, &self.brushes).ok()?;
                raster
            }
            Selected::Shape(_) => {
                let nodes: Vec<klad_core::doc::Node> = self.group_nodes().into_iter().map(|(_, n)| n).collect();
                klad_render::rasterize_nodes(&nodes, w, h).ok()?
            }
            Selected::Points => return None,
        };
        let (rgba, width, height) = rgba_of(&raster)?;
        let (lo, hi) = self.selection_span()?;
        cropped(&rgba, width, lo, hi, height)
    }

    pub(super) fn picture_out(&mut self, ctx: &egui::Context, picture: Picture) {
        self.own_picture = Some(own(&picture));
        ctx.copy_image(egui::ColorImage::from_rgba_unmultiplied(
            [picture.width as usize, picture.height as usize],
            &picture.rgba,
        ));
    }

    pub(super) fn copy_merged(&mut self) -> Result<()> {
        let doc = &self.scene.bundle.doc;
        let image = klad_render::compose_image(doc, self.scene.frame, &self.scene.rasters_now())?;
        let frame = klad_render::read_rgba8(&image)?;
        let (lo, hi) = self
            .selection_span()
            .unwrap_or((Vec2::ZERO, vec2(frame.width as f32, frame.height as f32)));
        let Some(picture) = cropped(&frame.rgba, frame.width, lo, hi, frame.height) else {
            self.say(Tone::Refused, "the selection is outside the canvas: nothing to copy");
            return Ok(());
        };
        let what = match self.selected() {
            Some(Selected::Strokes | Selected::Shape(_)) => "what you see in the selection",
            _ => "the whole picture",
        };
        let (width, height) = (picture.width, picture.height);
        let ctx = self.ctx.clone();
        self.picture_out(&ctx, picture);
        self.say(
            Tone::Done,
            format!("copied {what}, {width} by {height} px, for pasting in another app"),
        );
        Ok(())
    }

    pub(super) fn is_own(&self, picture: &Picture) -> bool {
        self.own_picture.is_some_and(|(w, h, a)| {
            w == picture.width && h == picture.height && (a - mean_alpha(picture)).abs() < 0.02
        })
    }

    pub(super) fn paste_image(&mut self, image: &egui::ColorImage) -> Result<()> {
        let picture = picture_of(image);
        if self.is_own(&picture) && (self.clipboard.is_some() || self.shape_clip.is_some()) {
            return self.paste();
        }
        self.paste_picture(&picture)
    }

    pub(super) fn paste_picture(&mut self, picture: &Picture) -> Result<()> {
        let png = klad_render::clipboard::png(picture);
        if self.scene.on_grid() {
            let file = self.scene.bundle.root.join(".cache").join("pasted.png");
            if let Some(dir) = file.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(&file, &png)?;
            let (cw, ch) = self.scene.size();
            self.pixels = super::pixels::Form {
                source: super::pixels::Source::File,
                path: file.to_string_lossy().to_string(),
                size: picture.width.max(picture.height).min(cw.max(ch)),
                colors: 0,
            };
            return self.make_pixels();
        }
        let centre = self.view.map(|view| {
            let rect = self.canvas_rect;
            view.to_canvas(rect.center() - rect.min, rect.size())
        });
        let before = self.scene.bundle.doc.rev;
        self.import_picture_at(&png, "pasted picture", centre)?;
        if self.scene.bundle.doc.rev == before {
            return Ok(());
        }
        let node = self
            .scene
            .active_layer()
            .and_then(|l| l.cels().first())
            .and_then(|c| c.nodes.first())
            .map(|n| n.id.clone());
        if let Some(id) = node {
            self.tool = Tool::Select;
            self.picked = None;
            self.group.clear();
            self.selection = Some(id);
        }
        self.say(
            Tone::Done,
            format!(
                "pasted a {} by {} px picture on a new layer, selected: drag to move it, a corner to scale it",
                picture.width, picture.height
            ),
        );
        Ok(())
    }

    pub(super) fn new_from_clipboard(&mut self) -> Result<()> {
        let Some(exe) = crate::start::draw_binary() else {
            bail!("the klad command line tool is missing next to the editor: install klad again (it ships both)");
        };
        let folder = self
            .scene
            .bundle
            .root
            .parent()
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(std::env::temp_dir);
        let target = (1..)
            .map(|n| match n {
                1 => folder.join("pasted.klad"),
                n => folder.join(format!("pasted {n}.klad")),
            })
            .find(|p| !p.exists())
            .unwrap_or_else(|| folder.join("pasted.klad"));
        let out = std::process::Command::new(exe)
            .arg("import")
            .arg("--clipboard")
            .arg("-o")
            .arg(&target)
            .output()?;
        if !out.status.success() {
            let said = String::from_utf8_lossy(&out.stderr).to_string();
            self.say(Tone::Refused, said.trim().trim_start_matches("error: ").to_string());
            return Ok(());
        }
        self.wants = Some(Want::Path(target));
        Ok(())
    }
}
