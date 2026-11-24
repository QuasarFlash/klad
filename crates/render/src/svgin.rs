use anyhow::{Result, anyhow, bail};
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use serde_json::{Value, json};
use skia_safe::{Matrix, Path as SkPath};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Default)]
pub struct Read {
    pub width: f32,
    pub height: f32,
    pub layers: Vec<(String, Vec<Value>)>,
    pub skipped: Vec<String>,
}

#[derive(Clone, Debug)]
struct Style {
    fill: Option<String>,
    fill_opacity: f32,
    stroke: Option<String>,
    stroke_opacity: f32,
    width: f32,
    cap: String,
    join: String,
    dash: Vec<f32>,
    font: String,
    size: f32,
    anchor: String,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            fill: Some("#000000".into()),
            fill_opacity: 1.0,
            stroke: None,
            stroke_opacity: 1.0,
            width: 1.0,
            cap: "butt".into(),
            join: "miter".into(),
            dash: Vec::new(),
            font: String::new(),
            size: 16.0,
            anchor: "start".into(),
        }
    }
}

#[derive(Clone, Debug)]
enum Gradient {
    Linear([f32; 4], Vec<(f32, String)>),
    Radial([f32; 3], Vec<(f32, String)>),
}

struct Frame {
    tag: String,
    matrix: Matrix,
    style: Style,
    nodes: Vec<Value>,
    name: String,
    opacity: f32,
    hidden: bool,
    layer: bool,
}

const NAMED: [(&str, &str); 18] = [
    ("black", "#000000"),
    ("white", "#ffffff"),
    ("red", "#ff0000"),
    ("green", "#008000"),
    ("lime", "#00ff00"),
    ("blue", "#0000ff"),
    ("yellow", "#ffff00"),
    ("orange", "#ffa500"),
    ("purple", "#800080"),
    ("gray", "#808080"),
    ("grey", "#808080"),
    ("silver", "#c0c0c0"),
    ("navy", "#000080"),
    ("teal", "#008080"),
    ("maroon", "#800000"),
    ("olive", "#808000"),
    ("aqua", "#00ffff"),
    ("fuchsia", "#ff00ff"),
];

fn numbers(text: &str) -> Vec<f32> {
    let mut out = Vec::new();
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut Vec<f32>| {
        if let Ok(v) = word.parse::<f32>() {
            out.push(v);
        }
        word.clear();
    };
    text.chars().for_each(|c| match c {
        '0'..='9' | '.' | 'e' | 'E' => {
            if c == '.' && word.contains('.') && !word.ends_with(['e', 'E']) {
                flush(&mut word, &mut out);
            }
            word.push(c);
        }
        '-' | '+' if !word.ends_with(['e', 'E']) => {
            flush(&mut word, &mut out);
            word.push(c);
        }
        '-' | '+' => word.push(c),
        _ => flush(&mut word, &mut out),
    });
    flush(&mut word, &mut out);
    out
}

fn length(text: &str) -> Option<f32> {
    let t = text.trim();
    let digits: String = t
        .chars()
        .take_while(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E'))
        .collect();
    let v = digits.parse::<f32>().ok()?;
    let unit = t[digits.len()..].trim();
    Some(match unit {
        "mm" => v * 96.0 / 25.4,
        "cm" => v * 96.0 / 2.54,
        "in" => v * 96.0,
        "pt" => v * 96.0 / 72.0,
        "pc" => v * 16.0,
        _ => v,
    })
}

fn transform(text: &str) -> Matrix {
    text.split(')')
        .filter_map(|part| {
            let (name, args) = part.split_once('(')?;
            let a = numbers(args);
            let get = |i: usize, or: f32| a.get(i).copied().unwrap_or(or);
            Some(match name.trim().trim_start_matches(',').trim() {
                "matrix" if a.len() >= 6 => Matrix::new_all(a[0], a[2], a[4], a[1], a[3], a[5], 0.0, 0.0, 1.0),
                "translate" => Matrix::translate((get(0, 0.0), get(1, 0.0))),
                "scale" => Matrix::scale((get(0, 1.0), get(1, get(0, 1.0)))),
                "rotate" => Matrix::rotate_deg_pivot(get(0, 0.0), (get(1, 0.0), get(2, 0.0))),
                "skewX" => {
                    let mut m = Matrix::new_identity();
                    m.set_skew_x(get(0, 0.0).to_radians().tan());
                    m
                }
                "skewY" => {
                    let mut m = Matrix::new_identity();
                    m.set_skew_y(get(0, 0.0).to_radians().tan());
                    m
                }
                _ => return None,
            })
        })
        .fold(Matrix::new_identity(), |acc, m| Matrix::concat(&acc, &m))
}

fn colour(text: &str, gradients: &BTreeMap<String, Gradient>) -> Option<Value> {
    let t = text.trim();
    if t == "none" || t == "transparent" {
        return None;
    }
    if let Some(id) = t.strip_prefix("url(#").and_then(|r| r.split(')').next()) {
        return gradients.get(id).map(|g| match g {
            Gradient::Linear([x1, y1, x2, y2], stops) => json!({"linear": {"from": [x1, y1], "to": [x2, y2], "stops": stops.iter().map(|(o, c)| json!([o, c])).collect::<Vec<_>>()}}),
            Gradient::Radial([cx, cy, r], stops) => json!({"radial": {"center": [cx, cy], "radius": r, "stops": stops.iter().map(|(o, c)| json!([o, c])).collect::<Vec<_>>()}}),
        });
    }
    hex(t).map(Value::String)
}

fn hex(text: &str) -> Option<String> {
    let t = text.trim().to_ascii_lowercase();
    if let Some(h) = t.strip_prefix('#') {
        return match h.len() {
            3 => Some(format!("#{}", h.chars().flat_map(|c| [c, c]).collect::<String>())),
            6 | 8 => Some(format!("#{h}")),
            _ => None,
        };
    }
    if let Some(args) = t.strip_prefix("rgb(").or_else(|| t.strip_prefix("rgba(")) {
        let v = numbers(args);
        let pct = args.contains('%');
        let byte = |x: f32| (if pct { x * 2.55 } else { x }).round().clamp(0.0, 255.0) as u8;
        return (v.len() >= 3).then(|| format!("#{:02x}{:02x}{:02x}", byte(v[0]), byte(v[1]), byte(v[2])));
    }
    if t == "currentcolor" {
        return Some("#000000".into());
    }
    NAMED.iter().find(|(n, _)| *n == t).map(|(_, h)| h.to_string())
}

fn with_alpha(hex: &str, alpha: f32) -> String {
    match alpha >= 0.999 || hex.len() != 7 {
        true => hex.to_string(),
        false => format!("{hex}{:02x}", (alpha.clamp(0.0, 1.0) * 255.0).round() as u8),
    }
}

fn attributes(e: &BytesStart) -> BTreeMap<String, String> {
    let mut map: BTreeMap<String, String> = e
        .attributes()
        .flatten()
        .filter_map(|a| {
            let key = String::from_utf8_lossy(a.key.local_name().as_ref()).to_string();
            let value = a.normalized_value(quick_xml::XmlVersion::Implicit1_0).ok()?.to_string();
            Some((key, value))
        })
        .collect();
    if let Some(style) = map.get("style").cloned() {
        style.split(';').filter_map(|kv| kv.split_once(':')).for_each(|(k, v)| {
            map.insert(k.trim().to_string(), v.trim().to_string());
        });
    }
    map
}

fn styled(parent: &Style, a: &BTreeMap<String, String>) -> Style {
    let mut s = parent.clone();
    if let Some(f) = a.get("fill") {
        s.fill = (f != "none").then(|| f.clone());
    }
    if let Some(v) = a.get("stroke") {
        s.stroke = (v != "none").then(|| v.clone());
    }
    let num = |k: &str| a.get(k).and_then(|v| length(v));
    if let Some(v) = num("fill-opacity") {
        s.fill_opacity = v;
    }
    if let Some(v) = num("stroke-opacity") {
        s.stroke_opacity = v;
    }
    if let Some(v) = num("stroke-width") {
        s.width = v;
    }
    if let Some(v) = a.get("stroke-linecap") {
        s.cap = v.clone();
    }
    if let Some(v) = a.get("stroke-linejoin") {
        s.join = v.clone();
    }
    if let Some(v) = a.get("stroke-dasharray") {
        s.dash = if v == "none" { Vec::new() } else { numbers(v) };
    }
    if let Some(v) = a.get("font-family") {
        s.font = v
            .split(',')
            .next()
            .unwrap_or_default()
            .trim()
            .trim_matches(['"', '\''])
            .to_string();
    }
    if let Some(v) = num("font-size") {
        s.size = v;
    }
    if let Some(v) = a.get("text-anchor") {
        s.anchor = v.clone();
    }
    s
}

fn paint_of(s: &Style, a: &BTreeMap<String, String>, gradients: &BTreeMap<String, Gradient>, node: &mut Value) {
    node["fill"] = match s.fill.as_deref().and_then(|f| colour(f, gradients)) {
        Some(Value::String(h)) => json!(with_alpha(&h, s.fill_opacity)),
        Some(g) => g,
        None => Value::Null,
    };
    if let Some(paint) = s.stroke.as_deref().and_then(|f| colour(f, gradients)) {
        let paint = match paint {
            Value::String(h) => json!(with_alpha(&h, s.stroke_opacity)),
            g => g,
        };
        let cap = match s.cap.as_str() {
            "round" => "round",
            "square" => "square",
            _ => "butt",
        };
        let join = match s.join.as_str() {
            "round" => "round",
            "bevel" => "bevel",
            _ => "miter",
        };
        node["stroke"] = json!({"paint": paint, "width": s.width, "cap": cap, "join": join});
        if !s.dash.is_empty() {
            node["stroke"]["dash"] = json!(s.dash);
        }
    }
    if let Some(o) = a.get("opacity").and_then(|v| length(v)).filter(|o| *o < 1.0) {
        node["opacity"] = json!(o);
    }
    if a.get("display").is_some_and(|d| d == "none") || a.get("visibility").is_some_and(|v| v == "hidden") {
        node["visible"] = json!(false);
    }
    if node["fill"].is_null() {
        node.as_object_mut().map(|o| o.remove("fill"));
    }
}

fn plain(m: &Matrix) -> Option<Value> {
    let (a, b, c, d) = (m.scale_x(), m.skew_y(), m.skew_x(), m.scale_y());
    let sx = (a * a + b * b).sqrt();
    if sx < 1e-6 {
        return None;
    }
    let sy = (a * d - b * c) / sx;
    let skew = (a * c + b * d) / sx;
    (skew.abs() < 1e-4).then(|| {
        json!({"x": m.translate_x(), "y": m.translate_y(), "rotate": b.atan2(a).to_degrees(), "sx": sx, "sy": sy, "pivot": [0.0, 0.0]})
    })
}

fn baked(path: &SkPath, m: &Matrix) -> String {
    path.with_transform(m).to_svg()
}

fn shape(tag: &str, a: &BTreeMap<String, String>, m: &Matrix) -> Option<Value> {
    let f = |k: &str| a.get(k).and_then(|v| length(v)).unwrap_or(0.0);
    let geometry = match tag {
        "rect" => {
            let r = f("rx").max(f("ry"));
            json!({"type": "rect", "x": f("x"), "y": f("y"), "w": f("width"), "h": f("height"), "radius": r})
        }
        "circle" => json!({"type": "ellipse", "cx": f("cx"), "cy": f("cy"), "rx": f("r"), "ry": f("r")}),
        "ellipse" => json!({"type": "ellipse", "cx": f("cx"), "cy": f("cy"), "rx": f("rx"), "ry": f("ry")}),
        "line" => json!({"type": "path", "d": format!("M{} {} L{} {}", f("x1"), f("y1"), f("x2"), f("y2"))}),
        "polyline" | "polygon" => {
            let p = numbers(a.get("points")?);
            let pairs: Vec<String> = p
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| format!("{} {}", c[0], c[1]))
                .collect();
            let close = if tag == "polygon" { " Z" } else { "" };
            json!({"type": "path", "d": format!("M{}{close}", pairs.join(" L"))})
        }
        "path" => json!({"type": "path", "d": a.get("d")?.clone()}),
        _ => return None,
    };
    let mut node = geometry;
    match (m.is_identity(), plain(m)) {
        (true, _) => {}
        (false, Some(t)) => node["transform"] = t,
        (false, None) => {
            let f = |k: &str| node[k].as_f64().unwrap_or(0.0) as f32;
            let path = match tag {
                "rect" => SkPath::rect(skia_safe::Rect::from_xywh(f("x"), f("y"), f("w"), f("h")), None),
                "circle" | "ellipse" => SkPath::oval(
                    skia_safe::Rect::from_ltrb(
                        f("cx") - f("rx"),
                        f("cy") - f("ry"),
                        f("cx") + f("rx"),
                        f("cy") + f("ry"),
                    ),
                    None,
                ),
                _ => SkPath::from_svg(node["d"].as_str()?)?,
            };
            node = json!({"type": "path", "d": baked(&path, m)});
        }
    }
    Some(node)
}

fn gradients(text: &str) -> BTreeMap<String, Gradient> {
    let mut reader = Reader::from_str(text);
    let mut out = BTreeMap::new();
    let mut open: Option<(String, Gradient)> = None;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let tag = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                let a = attributes(&e);
                let f = |k: &str, or: f32| a.get(k).and_then(|v| length(v)).unwrap_or(or);
                match tag.as_str() {
                    "linearGradient" => {
                        open = Some((
                            a.get("id").cloned().unwrap_or_default(),
                            Gradient::Linear([f("x1", 0.0), f("y1", 0.0), f("x2", 1.0), f("y2", 0.0)], Vec::new()),
                        ))
                    }
                    "radialGradient" => {
                        open = Some((
                            a.get("id").cloned().unwrap_or_default(),
                            Gradient::Radial([f("cx", 0.5), f("cy", 0.5), f("r", 0.5)], Vec::new()),
                        ))
                    }
                    "stop" => {
                        let offset = a
                            .get("offset")
                            .map(|o| match o.ends_with('%') {
                                true => length(o).unwrap_or(0.0) / 100.0,
                                false => length(o).unwrap_or(0.0),
                            })
                            .unwrap_or(0.0);
                        let hexed = a.get("stop-color").and_then(|c| hex(c)).unwrap_or("#000000".into());
                        let alpha = a.get("stop-opacity").and_then(|v| length(v)).unwrap_or(1.0);
                        let stop = (offset, with_alpha(&hexed, alpha));
                        if let Some((_, Gradient::Linear(_, s) | Gradient::Radial(_, s))) = open.as_mut() {
                            s.push(stop);
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let tag = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                if matches!(tag.as_str(), "linearGradient" | "radialGradient")
                    && let Some((id, g)) = open.take()
                {
                    out.insert(id, g);
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

pub type Picture<'a> = &'a mut dyn FnMut(&[u8]) -> Result<(String, u32, u32)>;

pub fn read(text: &str, near: &Path, picture: Picture) -> Result<Read> {
    let gradients = gradients(text);
    let mut reader = Reader::from_str(text);
    let mut read = Read::default();
    let mut stack: Vec<Frame> = Vec::new();
    let mut loose: Vec<Value> = Vec::new();
    let mut skipping = 0usize;
    let mut text_node: Option<(Value, String)> = None;
    loop {
        let event = reader
            .read_event()
            .map_err(|e| anyhow!("this is not an SVG klad can read: {e}"))?;
        let (e, empty) = match &event {
            Event::Start(e) => (Some(e.clone()), false),
            Event::Empty(e) => (Some(e.clone()), true),
            _ => (None, false),
        };
        if let Some(e) = e {
            let tag = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
            if skipping > 0
                || matches!(
                    tag.as_str(),
                    "defs" | "clipPath" | "mask" | "symbol" | "style" | "metadata" | "title" | "desc"
                )
            {
                if !empty {
                    skipping += 1;
                }
                continue;
            }
            let a = attributes(&e);
            let (parent_m, parent_s) = stack.last().map_or((Matrix::new_identity(), Style::default()), |f| {
                (f.matrix, f.style.clone())
            });
            let own = a
                .get("transform")
                .map(|t| transform(t))
                .unwrap_or_else(Matrix::new_identity);
            let style = styled(&parent_s, &a);
            match tag.as_str() {
                "svg" => {
                    let view = a.get("viewBox").map(|v| numbers(v)).filter(|v| v.len() == 4);
                    let w = a.get("width").and_then(|v| length(v));
                    let h = a.get("height").and_then(|v| length(v));
                    let (vw, vh) = view
                        .as_ref()
                        .map_or((w.unwrap_or(512.0), h.unwrap_or(512.0)), |v| (v[2], v[3]));
                    read.width = w.unwrap_or(vw);
                    read.height = h.unwrap_or(vh);
                    let mut m = Matrix::scale((read.width / vw.max(1e-3), read.height / vh.max(1e-3)));
                    if let Some(v) = view {
                        m.pre_translate((-v[0], -v[1]));
                    }
                    stack.push(Frame {
                        tag,
                        matrix: Matrix::concat(&parent_m, &m),
                        style,
                        nodes: Vec::new(),
                        name: String::new(),
                        opacity: 1.0,
                        hidden: false,
                        layer: false,
                    });
                }
                "g" | "a" | "switch" => {
                    let top = stack.len() == 1;
                    let name = a
                        .get("label")
                        .or_else(|| a.get("data-name"))
                        .or_else(|| a.get("id"))
                        .cloned()
                        .unwrap_or_default();
                    let opacity = a.get("opacity").and_then(|v| length(v)).unwrap_or(1.0);
                    let hidden = a.get("display").is_some_and(|d| d == "none");
                    let baked_in = stack.last().is_some_and(|f| f.layer || f.tag == "svg");
                    let frame = Frame {
                        tag,
                        matrix: match top || baked_in {
                            true => Matrix::concat(&parent_m, &own),
                            false => own,
                        },
                        style,
                        nodes: Vec::new(),
                        name,
                        opacity,
                        hidden,
                        layer: top,
                    };
                    if empty {
                        continue;
                    }
                    stack.push(frame);
                }
                "text" => {
                    let m = match stack.last().is_some_and(|f| f.layer || f.tag == "svg") {
                        true => Matrix::concat(&parent_m, &own),
                        false => own,
                    };
                    let f = |k: &str| {
                        a.get(k)
                            .map(|v| numbers(v))
                            .and_then(|v| v.first().copied())
                            .unwrap_or(0.0)
                    };
                    let align = match style.anchor.as_str() {
                        "middle" => "center",
                        "end" => "right",
                        _ => "left",
                    };
                    let mut node = json!({"type": "text", "text": "", "font": style.font, "x": f("x"), "y": f("y") - style.size * 0.8, "size": style.size, "align": align});
                    paint_of(&style, &a, &gradients, &mut node);
                    if let Some(t) = plain(&m).filter(|_| !m.is_identity()) {
                        node["transform"] = t;
                    }
                    if empty {
                        continue;
                    }
                    text_node = Some((node, String::new()));
                    skipping = 0;
                    stack.push(Frame {
                        tag,
                        matrix: m,
                        style,
                        nodes: Vec::new(),
                        name: String::new(),
                        opacity: 1.0,
                        hidden: false,
                        layer: false,
                    });
                }
                "tspan" if text_node.is_some() => {
                    if let Some((_, words)) = text_node.as_mut().filter(|(_, w)| !w.is_empty()) {
                        words.push('\n');
                    }
                    if let Some((node, words)) = text_node.as_mut().filter(|(_, w)| w.is_empty()) {
                        let first = |k: &str| a.get(k).map(|v| numbers(v)).and_then(|v| v.first().copied());
                        if let Some(x) = first("x") {
                            node["x"] = json!(x);
                        }
                        if let Some(y) = first("y") {
                            node["y"] = json!(y - style.size * 0.8);
                        }
                        let _ = words;
                    }
                    if !empty {
                        stack.push(Frame {
                            tag,
                            matrix: parent_m,
                            style,
                            nodes: Vec::new(),
                            name: String::new(),
                            opacity: 1.0,
                            hidden: false,
                            layer: false,
                        });
                    }
                }
                "image" => {
                    let m = match stack.last().is_some_and(|f| f.layer || f.tag == "svg") {
                        true => Matrix::concat(&parent_m, &own),
                        false => own,
                    };
                    let href = a.get("href").cloned().unwrap_or_default();
                    let bytes = match href.split_once("base64,") {
                        Some((_, data)) => unbase64(data),
                        None => std::fs::read(near.join(&href)).unwrap_or_default(),
                    };
                    let f = |k: &str| a.get(k).and_then(|v| length(v)).unwrap_or(0.0);
                    match picture(&bytes) {
                        Ok((src, w, h)) => {
                            let (pw, ph) = match (f("width"), f("height")) {
                                (0.0, 0.0) => (w as f32, h as f32),
                                (pw, ph) => (pw, ph),
                            };
                            let mut node =
                                json!({"type": "image", "src": src, "x": f("x"), "y": f("y"), "w": pw, "h": ph});
                            if let Some(t) = plain(&m).filter(|_| !m.is_identity()) {
                                node["transform"] = t;
                            }
                            push(&mut stack, &mut loose, node);
                        }
                        Err(_) => read.skipped.push("a picture it cannot read".into()),
                    }
                    if !empty {
                        skipping += 1;
                    }
                }
                _ => {
                    let m = match stack.last().is_some_and(|f| f.layer || f.tag == "svg") {
                        true => Matrix::concat(&parent_m, &own),
                        false => own,
                    };
                    match shape(&tag, &a, &m) {
                        Some(mut node) => {
                            paint_of(&style, &a, &gradients, &mut node);
                            if let Some(id) = a.get("data-name") {
                                node["name"] = json!(id);
                            }
                            push(&mut stack, &mut loose, node);
                        }
                        None => read.skipped.push(tag.clone()),
                    }
                    if !empty {
                        skipping += 1;
                    }
                }
            }
            continue;
        }
        match event {
            Event::End(_) if skipping > 0 => skipping -= 1,
            Event::Text(t) if text_node.is_some() => {
                if let (Some((_, words)), Ok(s)) = (text_node.as_mut(), t.decode()) {
                    words.push_str(s.trim());
                }
            }
            Event::GeneralRef(r) if text_node.is_some() => {
                if let (Some((_, words)), Ok(s)) = (text_node.as_mut(), r.decode()) {
                    words.push_str(match s.as_ref() {
                        "amp" => "&",
                        "lt" => "<",
                        "gt" => ">",
                        "quot" => "\"",
                        "apos" => "'",
                        _ => "",
                    });
                }
            }
            Event::End(_) => {
                let Some(frame) = stack.pop() else { continue };
                match frame.tag.as_str() {
                    "text" => {
                        if let Some((mut node, words)) = text_node.take() {
                            node["text"] = json!(words);
                            if !words.trim().is_empty() {
                                push(&mut stack, &mut loose, node);
                            }
                        }
                    }
                    "tspan" => {}
                    _ if frame.layer => {
                        let mut nodes = frame.nodes;
                        if frame.opacity < 1.0 || frame.hidden {
                            nodes = vec![
                                json!({"type": "group", "children": nodes, "opacity": frame.opacity, "visible": !frame.hidden}),
                            ];
                        }
                        let name = match frame.name.is_empty() {
                            true => format!("svg {}", read.layers.len() + 1),
                            false => frame.name,
                        };
                        read.layers.push((name, nodes));
                    }
                    "svg" => {}
                    _ => {
                        let mut group = json!({"type": "group", "children": frame.nodes});
                        if let Some(t) = plain(&frame.matrix).filter(|_| !frame.matrix.is_identity()) {
                            group["transform"] = t;
                        }
                        if frame.opacity < 1.0 {
                            group["opacity"] = json!(frame.opacity);
                        }
                        if frame.hidden {
                            group["visible"] = json!(false);
                        }
                        if !frame.name.is_empty() {
                            group["name"] = json!(frame.name);
                        }
                        push(&mut stack, &mut loose, group);
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !loose.is_empty() {
        read.layers.insert(0, ("svg".to_string(), loose));
    }
    if read.layers.iter().all(|(_, n)| n.is_empty()) {
        bail!(
            "the SVG holds no shapes klad can read (paths, rects, circles, ellipses, lines, polygons, text, pictures)"
        );
    }
    if read.width <= 0.0 || read.height <= 0.0 {
        read.width = 512.0;
        read.height = 512.0;
    }
    Ok(read)
}

fn push(stack: &mut [Frame], loose: &mut Vec<Value>, node: Value) {
    match stack
        .iter_mut()
        .rev()
        .find(|f| f.tag != "text" && f.tag != "tspan" && f.tag != "svg")
    {
        Some(frame) => frame.nodes.push(node),
        None => loose.push(node),
    }
}

pub fn unbase64(text: &str) -> Vec<u8> {
    let value = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' | b'-' => Some(62),
        b'/' | b'_' => Some(63),
        _ => None,
    };
    let digits: Vec<u8> = text.bytes().filter_map(value).collect();
    digits
        .chunks(4)
        .flat_map(|c| {
            let n = c
                .iter()
                .enumerate()
                .fold(0u32, |n, (i, d)| n | (*d as u32) << (18 - 6 * i));
            let bytes = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
            bytes.into_iter().take(c.len().saturating_sub(1))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_groups_colours_and_transforms_come_in() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100" viewBox="0 0 200 100">
          <defs><linearGradient id="g" x1="0" y1="0" x2="10" y2="0"><stop offset="0" stop-color="red"/><stop offset="100%" stop-color="#00f"/></linearGradient></defs>
          <g id="sky" data-name="sky"><rect x="10" y="10" width="50" height="20" fill="#336699" stroke="black" stroke-width="2"/>
            <circle cx="100" cy="50" r="20" fill="url(#g)"/></g>
          <g data-name="ground" opacity="0.5"><path d="M0 90 L200 90" stroke="rgb(0,128,0)" fill="none"/>
            <g transform="translate(5 5)"><ellipse cx="150" cy="20" rx="10" ry="5" style="fill:#ff0000;fill-opacity:0.5"/></g></g>
          <polygon points="1,1 5,1 5,5" transform="skewX(20)"/>
        </svg>"##;
        let read = read(svg, Path::new("."), &mut |_| bail!("no pictures")).unwrap();
        assert_eq!((read.width, read.height), (200.0, 100.0));
        let names: Vec<&str> = read.layers.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["svg", "sky", "ground"]);
        let sky = &read.layers[1].1;
        assert_eq!(sky[0]["type"], "rect");
        assert_eq!(sky[0]["fill"], "#336699");
        assert_eq!(sky[0]["stroke"]["width"], 2.0);
        assert!(sky[1]["fill"]["linear"].is_object());
        let ground = &read.layers[2].1;
        assert_eq!(ground[0]["opacity"], 0.5);
        let inner = &ground[0]["children"];
        assert_eq!(inner[0]["stroke"]["paint"], "#008000");
        assert_eq!(inner[1]["type"], "group");
        assert_eq!(inner[1]["children"][0]["fill"], "#ff000080");
        assert_eq!(
            read.layers[0].1[0]["type"], "path",
            "a skewed polygon is baked into a path"
        );
        assert_eq!(unbase64("Zm9vYmFy"), b"foobar");
        assert_eq!(unbase64("Zm8="), b"fo");
    }
}
