use crate::vector::{instances, node_matrix, shape_path};
use anyhow::Result;
use klad_core::color::parse_hex;
use klad_core::doc::{Cap, Document, Join, Kind, Layer, Node, Paint, Shape, StrokeStyle};
use klad_core::generators::Align;
use skia_safe::Matrix;
use std::fmt::Write as _;
use std::path::Path;

pub struct Svg<'a> {
    pub frame: u32,
    pub layer: Option<&'a Layer>,
    pub vectors_only: bool,
    pub outline_text: bool,
    pub nodes: Option<&'a [String]>,
}

#[derive(Debug, Default)]
pub struct Written {
    pub svg: String,
    pub pictures: Vec<String>,
    pub left_out: Vec<String>,
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64(bytes: &[u8]) -> String {
    bytes
        .chunks(3)
        .flat_map(|c| {
            let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
            (0..4).map(move |i| match i < c.len() + 1 {
                true => B64[(n >> (18 - 6 * i) & 63) as usize] as char,
                false => '=',
            })
        })
        .collect()
}

pub fn escape(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '&' => "&amp;".to_string(),
            '<' => "&lt;".to_string(),
            '>' => "&gt;".to_string(),
            '"' => "&quot;".to_string(),
            c => c.to_string(),
        })
        .collect()
}

fn num(v: f32) -> String {
    let r = (v * 1000.0).round() / 1000.0;
    match r == r.trunc() {
        true => format!("{}", r as i64),
        false => format!("{r}"),
    }
}

fn colour(doc: &Document, text: &str) -> (String, f32) {
    let hex = doc.resolve_color(text).unwrap_or_else(|_| "#000000".to_string());
    match parse_hex(&hex) {
        Ok([r, g, b, a]) => {
            let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            (format!("#{:02x}{:02x}{:02x}", byte(r), byte(g), byte(b)), a)
        }
        Err(_) => ("#000000".to_string(), 1.0),
    }
}

struct Writer<'a> {
    doc: &'a Document,
    root: &'a Path,
    outline_text: bool,
    defs: String,
    next: usize,
}

impl Writer<'_> {
    fn id(&mut self, kind: &str) -> String {
        self.next += 1;
        format!("{kind}{}", self.next)
    }

    fn paint(&mut self, paint: &Paint, what: &str) -> String {
        match paint {
            Paint::Solid(text) => {
                let (hex, a) = colour(self.doc, text);
                match a < 1.0 {
                    true => format!(" {what}=\"{hex}\" {what}-opacity=\"{}\"", num(a)),
                    false => format!(" {what}=\"{hex}\""),
                }
            }
            Paint::Linear { linear } => {
                let id = self.id("linear");
                let stops = self.stops(&linear.stops);
                let _ = write!(
                    self.defs,
                    "<linearGradient id=\"{id}\" gradientUnits=\"userSpaceOnUse\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\">{stops}</linearGradient>",
                    num(linear.from[0]),
                    num(linear.from[1]),
                    num(linear.to[0]),
                    num(linear.to[1])
                );
                format!(" {what}=\"url(#{id})\"")
            }
            Paint::Radial { radial } => {
                let id = self.id("radial");
                let stops = self.stops(&radial.stops);
                let _ = write!(
                    self.defs,
                    "<radialGradient id=\"{id}\" gradientUnits=\"userSpaceOnUse\" cx=\"{}\" cy=\"{}\" r=\"{}\">{stops}</radialGradient>",
                    num(radial.center[0]),
                    num(radial.center[1]),
                    num(radial.radius)
                );
                format!(" {what}=\"url(#{id})\"")
            }
        }
    }

    fn stops(&self, stops: &[klad_core::doc::Stop]) -> String {
        stops
            .iter()
            .map(|s| {
                let (hex, a) = colour(self.doc, &s.1);
                format!(
                    "<stop offset=\"{}\" stop-color=\"{hex}\" stop-opacity=\"{}\"/>",
                    num(s.0),
                    num(a)
                )
            })
            .collect()
    }

    fn stroke(&mut self, stroke: &StrokeStyle) -> String {
        let cap = match stroke.cap {
            Cap::Butt => "butt",
            Cap::Round => "round",
            Cap::Square => "square",
        };
        let join = match stroke.join {
            Join::Miter => "miter",
            Join::Round => "round",
            Join::Bevel => "bevel",
        };
        let dash = match stroke.dash.is_empty() {
            true => String::new(),
            false => format!(
                " stroke-dasharray=\"{}\"",
                stroke.dash.iter().map(|d| num(*d)).collect::<Vec<_>>().join(" ")
            ),
        };
        format!(
            "{} stroke-width=\"{}\" stroke-linecap=\"{cap}\" stroke-linejoin=\"{join}\"{dash}",
            self.paint(&stroke.paint, "stroke"),
            num(stroke.width)
        )
    }

    fn style(&mut self, node: &Node) -> String {
        let fill = match &node.fill {
            Some(paint) => self.paint(paint, "fill"),
            None => " fill=\"none\"".to_string(),
        };
        let stroke = node.stroke.as_ref().map(|s| self.stroke(s)).unwrap_or_default();
        let opacity = match node.opacity < 1.0 {
            true => format!(" opacity=\"{}\"", num(node.opacity)),
            false => String::new(),
        };
        let blur = match node.blur > 0.0 {
            true => {
                let id = self.id("blur");
                let _ = write!(
                    self.defs,
                    "<filter id=\"{id}\" x=\"-50%\" y=\"-50%\" width=\"200%\" height=\"200%\"><feGaussianBlur stdDeviation=\"{}\"/></filter>",
                    num(node.blur / 2.0)
                );
                format!(" filter=\"url(#{id})\"")
            }
            false => String::new(),
        };
        format!("{fill}{stroke}{opacity}{blur}")
    }

    fn node(&mut self, out: &mut String, node: &Node) -> Result<()> {
        let m = node_matrix(node)?;
        let transform = matrix_attr(&m);
        let name = match node.name.is_empty() {
            true => String::new(),
            false => format!(" data-name=\"{}\"", escape(&node.name)),
        };
        let hidden = if node.visible { "" } else { " display=\"none\"" };
        let head = format!(" id=\"{}\"{name}{transform}{hidden}", escape(&node.id));
        match &node.shape {
            Shape::Rect { x, y, w, h, radius } => {
                let style = self.style(node);
                let round = match *radius > 0.0 {
                    true => format!(" rx=\"{}\" ry=\"{}\"", num(*radius), num(*radius)),
                    false => String::new(),
                };
                let _ = write!(
                    out,
                    "<rect{head} x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{round}{style}/>",
                    num(*x),
                    num(*y),
                    num(*w),
                    num(*h)
                );
            }
            Shape::Ellipse { cx, cy, rx, ry } => {
                let style = self.style(node);
                let _ = write!(
                    out,
                    "<ellipse{head} cx=\"{}\" cy=\"{}\" rx=\"{}\" ry=\"{}\"{style}/>",
                    num(*cx),
                    num(*cy),
                    num(*rx),
                    num(*ry)
                );
            }
            Shape::Path { d } => {
                let style = self.style(node);
                let _ = write!(out, "<path{head} d=\"{}\"{style}/>", escape(d));
            }
            Shape::Text(t) if self.outline_text => {
                let style = self.style(node);
                let d = crate::text::text_path(t)?.to_svg();
                let _ = write!(out, "<path{head} d=\"{d}\"{style}/>");
            }
            Shape::Text(t) => {
                let style = self.style(node);
                let anchor = match t.align {
                    Align::Left => "start",
                    Align::Center => "middle",
                    Align::Right => "end",
                };
                let weight = t
                    .weight
                    .map(|w| format!(" font-weight=\"{}\"", num(w)))
                    .unwrap_or_default();
                let spacing = match t.tracking != 0.0 {
                    true => format!(" letter-spacing=\"{}\"", num(t.tracking)),
                    false => String::new(),
                };
                let lines: String = t
                    .text
                    .lines()
                    .enumerate()
                    .map(|(i, line)| {
                        let y = t.y + t.size * 0.8 + i as f32 * t.size * t.leading.max(0.1);
                        format!("<tspan x=\"{}\" y=\"{}\">{}</tspan>", num(t.x), num(y), escape(line))
                    })
                    .collect();
                let _ = write!(
                    out,
                    "<text{head} font-family=\"{}\" font-size=\"{}\" text-anchor=\"{anchor}\"{weight}{spacing}{style} xml:space=\"preserve\">{lines}</text>",
                    escape(t.font.rsplit_once('.').map_or(t.font.as_str(), |(stem, _)| stem)),
                    num(t.size)
                );
            }
            Shape::Image(p) => {
                let file = self.root.join(crate::assets::IMAGES).join(&p.src);
                let href = match std::fs::read(&file) {
                    Ok(bytes) => format!("data:image/png;base64,{}", base64(&bytes)),
                    Err(_) => escape(&p.src),
                };
                let opacity = match node.opacity < 1.0 {
                    true => format!(" opacity=\"{}\"", num(node.opacity)),
                    false => String::new(),
                };
                let _ = write!(
                    out,
                    "<image{head} x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" preserveAspectRatio=\"none\"{opacity} href=\"{href}\"/>",
                    num(p.x),
                    num(p.y),
                    num(p.w),
                    num(p.h)
                );
            }
            Shape::Rays(_) => {
                let style = self.style(node);
                let d = shape_path(&node.shape)?.map(|p| p.to_svg()).unwrap_or_default();
                let _ = write!(out, "<path{head} d=\"{d}\"{style}/>");
            }
            shape => {
                let opacity = match node.opacity < 1.0 {
                    true => format!(" opacity=\"{}\"", num(node.opacity)),
                    false => String::new(),
                };
                let _ = write!(out, "<g{head}{opacity}>");
                instances(shape).iter().try_for_each(|inst| {
                    let open = !inst.is_identity();
                    if open {
                        let _ = write!(out, "<g{}>", matrix_attr(inst));
                    }
                    shape.children().iter().try_for_each(|child| self.node(out, child))?;
                    if open {
                        out.push_str("</g>");
                    }
                    Ok::<_, anyhow::Error>(())
                })?;
                out.push_str("</g>");
            }
        }
        Ok(())
    }
}

fn matrix_attr(m: &Matrix) -> String {
    match m.is_identity() {
        true => String::new(),
        false => format!(
            " transform=\"matrix({} {} {} {} {} {})\"",
            num(m.scale_x()),
            num(m.skew_y()),
            num(m.skew_x()),
            num(m.scale_y()),
            num(m.translate_x()),
            num(m.translate_y())
        ),
    }
}

fn layer_head(layer: &Layer) -> String {
    let opacity = match layer.opacity < 1.0 {
        true => format!(" opacity=\"{}\"", num(layer.opacity)),
        false => String::new(),
    };
    let hidden = if layer.visible { "" } else { " display=\"none\"" };
    format!(
        " id=\"{}\" data-name=\"{}\"{opacity}{hidden}",
        escape(&layer.id),
        escape(&layer.name)
    )
}

pub fn vectors_only(doc: &Document) -> bool {
    doc.all_layers()
        .iter()
        .all(|l| matches!(l.kind(), Kind::Vector | Kind::Group))
}

pub fn write(
    doc: &Document,
    root: &Path,
    opts: &Svg,
    picture: &mut dyn FnMut(&Layer) -> Result<Vec<u8>>,
) -> Result<Written> {
    let mut w = Writer {
        doc,
        root,
        outline_text: opts.outline_text,
        defs: String::new(),
        next: 0,
    };
    let mut written = Written::default();
    let mut body = String::new();
    match (opts.nodes, opts.layer) {
        (Some(ids), _) => ids.iter().try_for_each(|id| match doc.item(id) {
            Some(klad_core::doc::Item::Node(_, _, node)) => w.node(&mut body, node),
            _ => Ok(()),
        })?,
        (None, Some(layer)) => layer_svg(&mut w, &mut body, layer, opts, picture, &mut written)?,
        (None, None) => doc
            .layers
            .iter()
            .try_for_each(|l| layer_svg(&mut w, &mut body, l, opts, picture, &mut written))?,
    }
    let (cw, ch) = (doc.canvas.width, doc.canvas.height);
    let defs = match w.defs.is_empty() {
        true => String::new(),
        false => format!("<defs>{}</defs>", w.defs),
    };
    written.svg = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" width=\"{cw}\" height=\"{ch}\" viewBox=\"0 0 {cw} {ch}\">{defs}{body}</svg>\n"
    );
    Ok(written)
}

fn layer_svg(
    w: &mut Writer,
    out: &mut String,
    layer: &Layer,
    opts: &Svg,
    picture: &mut dyn FnMut(&Layer) -> Result<Vec<u8>>,
    written: &mut Written,
) -> Result<()> {
    match layer.kind() {
        Kind::Group => {
            let _ = write!(out, "<g{}>", layer_head(layer));
            layer
                .children()
                .iter()
                .try_for_each(|child| layer_svg(w, out, child, opts, picture, written))?;
            out.push_str("</g>");
        }
        Kind::Vector => {
            let _ = write!(out, "<g{}>", layer_head(layer));
            if let Some(cel) = layer.content_at(opts.frame) {
                cel.nodes.iter().try_for_each(|n| w.node(out, n))?;
            }
            out.push_str("</g>");
        }
        Kind::Paint if !opts.vectors_only => {
            let png = picture(layer)?;
            let (cw, ch) = (w.doc.canvas.width, w.doc.canvas.height);
            let _ = write!(
                out,
                "<g{}><image x=\"0\" y=\"0\" width=\"{cw}\" height=\"{ch}\" href=\"data:image/png;base64,{}\"/></g>",
                layer_head(layer),
                base64(&png)
            );
            written.pictures.push(layer.name.clone());
        }
        _ => written.left_out.push(layer.name.clone()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_known_encodings() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(escape("a<b & \"c\""), "a&lt;b &amp; &quot;c&quot;");
        assert_eq!(num(1.0), "1");
        assert_eq!(num(0.12345), "0.123");
    }
}
