use anyhow::{Result, anyhow, bail};
use std::collections::BTreeSet;
use std::io::Read;

#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    pub name: String,
    pub visible: bool,
    pub opacity: f32,
    pub blend: String,
    pub clip: bool,
}

impl Look {
    pub fn named(name: &str) -> Self {
        Self {
            name: name.to_string(),
            visible: true,
            opacity: 1.0,
            blend: "norm".into(),
            clip: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Pixels {
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Layer {
        look: Look,
        pixels: Pixels,
    },
    Group {
        look: Look,
        open: bool,
        children: Vec<Item>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Psd {
    pub width: u32,
    pub height: u32,
    pub items: Vec<Item>,
    pub notes: Vec<String>,
}

pub const BLENDS: [(&str, &str); 19] = [
    ("norm", "normal"),
    ("mul ", "multiply"),
    ("scrn", "screen"),
    ("over", "overlay"),
    ("dark", "darken"),
    ("lite", "lighten"),
    ("div ", "color-dodge"),
    ("idiv", "color-burn"),
    ("hLit", "hard-light"),
    ("sLit", "soft-light"),
    ("diff", "difference"),
    ("smud", "exclusion"),
    ("hue ", "hue"),
    ("sat ", "saturation"),
    ("colr", "color"),
    ("lum ", "luminosity"),
    ("lddg", "add"),
    ("fsub", "subtract"),
    ("pass", "pass-through"),
];

pub fn blend_name(key: &str) -> Option<&'static str> {
    BLENDS.iter().find(|(k, _)| *k == key).map(|(_, n)| *n)
}

pub fn blend_key(name: &str) -> &'static str {
    BLENDS.iter().find(|(_, n)| *n == name).map_or("norm", |(k, _)| *k)
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.at.checked_add(n).filter(|e| *e <= self.data.len());
        match end {
            Some(end) => {
                let out = &self.data[self.at..end];
                self.at = end;
                Ok(out)
            }
            None => bail!("the file ends early (wanted {n} bytes at {})", self.at),
        }
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into()?))
    }
    fn i16(&mut self) -> Result<i16> {
        Ok(i16::from_be_bytes(self.take(2)?.try_into()?))
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into()?))
    }
    fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_be_bytes(self.take(4)?.try_into()?))
    }
}

struct Record {
    look: Look,
    rect: [i32; 4],
    channels: Vec<(i16, u32)>,
    section: u32,
    mask: Option<[i32; 4]>,
    mask_default: u8,
    kinds: BTreeSet<&'static str>,
}

const ADJUSTMENTS: [&str; 20] = [
    "levl", "curv", "brit", "hue2", "hue ", "blnc", "selc", "mixr", "grdm", "phfl", "expA", "vibA", "thrs", "post",
    "nvrt", "clrL", "blwh", "GdFl", "SoCo", "PtFl",
];

fn unpack_bits(src: &[u8], want: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(want);
    let mut i = 0;
    while i < src.len() && out.len() < want {
        let n = src[i] as i8;
        i += 1;
        match n {
            0..=127 => {
                let len = n as usize + 1;
                let end = (i + len).min(src.len());
                out.extend_from_slice(&src[i..end]);
                i = end;
            }
            -127..=-1 => {
                if let Some(b) = src.get(i) {
                    out.extend(std::iter::repeat_n(*b, (1 - n as isize) as usize));
                }
                i += 1;
            }
            -128 => {}
        }
    }
    out.resize(want, 0);
    out
}

pub fn pack_bits(row: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(row.len() + row.len() / 128 + 2);
    let mut i = 0;
    while i < row.len() {
        let run = row[i..].iter().take(128).take_while(|b| **b == row[i]).count();
        if run >= 3 {
            out.push((1i16 - run as i16) as i8 as u8);
            out.push(row[i]);
            i += run;
            continue;
        }
        let start = i;
        while i < row.len() && i - start < 128 {
            let ahead = row[i..].iter().take(3).take_while(|b| **b == row[i]).count();
            if ahead >= 3 {
                break;
            }
            i += 1;
        }
        out.push((i - start - 1) as u8);
        out.extend_from_slice(&row[start..i]);
    }
    out
}

fn plane(r: &mut Reader, width: usize, height: usize, depth: u16, length: Option<usize>) -> Result<Vec<u8>> {
    let begin = r.at;
    let compression = r.u16()?;
    let bytes = (depth / 8).max(1) as usize;
    let size = width * height * bytes;
    let raw = match compression {
        0 => r.take(size)?.to_vec(),
        1 => {
            let counts = (0..height)
                .map(|_| r.u16().map(|v| v as usize))
                .collect::<Result<Vec<_>>>()?;
            let mut out = Vec::with_capacity(size);
            counts.iter().try_for_each(|n| {
                out.extend(unpack_bits(r.take(*n)?, width * bytes));
                Ok::<_, anyhow::Error>(())
            })?;
            out
        }
        2 | 3 => {
            let Some(length) = length else {
                bail!("ZIP pictures without a length")
            };
            let packed = r.take(length.saturating_sub(2))?;
            let mut out = Vec::with_capacity(size);
            flate2::read::ZlibDecoder::new(packed).read_to_end(&mut out)?;
            out.resize(size, 0);
            if compression == 3 {
                out.chunks_mut(width * bytes).for_each(|row| match bytes {
                    1 => (1..row.len()).for_each(|i| row[i] = row[i].wrapping_add(row[i - 1])),
                    _ => {
                        let mut last = 0u16;
                        row.chunks_mut(2).for_each(|c| {
                            last = last.wrapping_add(u16::from_be_bytes([c[0], c[1]]));
                            c.copy_from_slice(&last.to_be_bytes());
                        })
                    }
                });
            }
            out
        }
        other => bail!("a picture packed in a way klad does not read ({other})"),
    };
    if let Some(length) = length {
        r.at = begin + length;
    }
    Ok(match bytes {
        1 => raw,
        _ => raw.chunks(bytes).map(|c| c[0]).collect(),
    })
}

fn pascal(r: &mut Reader, pad: usize) -> Result<String> {
    let n = r.u8()? as usize;
    let text = String::from_utf8_lossy(r.take(n)?).to_string();
    let used = n + 1;
    let rest = (pad - used % pad) % pad;
    r.take(rest)?;
    Ok(text)
}

fn record(r: &mut Reader) -> Result<Record> {
    let rect = [r.i32()?, r.i32()?, r.i32()?, r.i32()?];
    let count = r.u16()?;
    let channels = (0..count)
        .map(|_| Ok((r.i16()?, r.u32()?)))
        .collect::<Result<Vec<_>>>()?;
    if r.take(4)? != b"8BIM" {
        bail!("a layer record without its signature");
    }
    let blend = String::from_utf8_lossy(r.take(4)?).to_string();
    let opacity = r.u8()? as f32 / 255.0;
    let clip = r.u8()? != 0;
    let flags = r.u8()?;
    r.u8()?;
    let extra = r.u32()? as usize;
    let end = r.at + extra;
    let mask_len = r.u32()? as usize;
    let mask_start = r.at;
    let (mask, mask_default) = match mask_len >= 18 {
        true => {
            let m = [r.i32()?, r.i32()?, r.i32()?, r.i32()?];
            let default = r.u8()?;
            (Some(m), default)
        }
        false => (None, 0),
    };
    r.at = mask_start + mask_len;
    let ranges = r.u32()? as usize;
    r.take(ranges)?;
    let mut name = pascal(r, 4)?;
    let mut section = 0;
    let mut kinds = BTreeSet::new();
    while r.at + 12 <= end {
        let sig = &r.data[r.at..r.at + 4];
        if sig != b"8BIM" && sig != b"8B64" {
            break;
        }
        r.take(4)?;
        let key: [u8; 4] = r.take(4)?.try_into()?;
        let len = r.u32()? as usize;
        let body_start = r.at;
        match &key {
            b"luni" => {
                let n = r.u32()? as usize;
                let units: Vec<u16> = (0..n).map(|_| r.u16()).collect::<Result<_>>()?;
                name = String::from_utf16_lossy(&units).trim_end_matches('\0').to_string();
            }
            b"lsct" | b"lsdk" => section = r.u32()?,
            b"TySh" | b"tySh" => {
                kinds.insert("text layers");
            }
            b"SoLd" | b"PlLd" | b"SoLE" | b"plLd" => {
                kinds.insert("smart objects");
            }
            b"lfx2" | b"lrFX" | b"lmfx" => {
                kinds.insert("layer effects");
            }
            b"vmsk" | b"vsms" | b"vogk" => {
                kinds.insert("vector masks");
            }
            k if ADJUSTMENTS.iter().any(|a| a.as_bytes() == k) => {
                kinds.insert("adjustment and fill layers");
            }
            _ => {}
        }
        r.at = (body_start + len).min(end);
        if !r.at.is_multiple_of(2) && r.at < end && r.data[r.at] == 0 {
            r.at += 1;
        }
    }
    if r.data[r.at.min(end)..end].iter().all(|b| *b == 0) {
        r.at = end;
    }
    Ok(Record {
        look: Look {
            name,
            visible: flags & 0x02 == 0,
            opacity,
            blend,
            clip,
        },
        rect,
        channels,
        section,
        mask,
        mask_default,
        kinds,
    })
}

fn pixels_of(rec: &Record, planes: &[(i16, Vec<u8>)], notes: &mut BTreeSet<String>) -> Pixels {
    let [top, left, bottom, right] = rec.rect;
    let (w, h) = ((right - left).max(0) as usize, (bottom - top).max(0) as usize);
    let find = |id: i16| planes.iter().find(|(c, _)| *c == id).map(|(_, p)| p);
    let red = find(0);
    let green = find(1).or(red);
    let blue = find(2).or(red);
    let alpha = find(-1);
    let mut rgba: Vec<u8> = (0..w * h)
        .flat_map(|i| {
            let at = |p: Option<&Vec<u8>>, d: u8| p.and_then(|p| p.get(i).copied()).unwrap_or(d);
            [at(red, 0), at(green, 0), at(blue, 0), at(alpha, 255)]
        })
        .collect();
    if let (Some(m), Some(mask)) = (rec.mask, find(-2)) {
        let [mt, ml, mb, mr] = m;
        let mw = (mr - ml).max(0) as usize;
        (0..h).for_each(|y| {
            (0..w).for_each(|x| {
                let (gx, gy) = (left + x as i32, top + y as i32);
                let inside = gx >= ml && gx < mr && gy >= mt && gy < mb;
                let v = match inside {
                    true => mask
                        .get((gy - mt) as usize * mw + (gx - ml) as usize)
                        .copied()
                        .unwrap_or(255),
                    false => rec.mask_default,
                };
                let a = &mut rgba[(y * w + x) * 4 + 3];
                *a = (*a as u32 * v as u32 / 255) as u8;
            })
        });
        notes.insert("layer masks were applied to the pixels".into());
    }
    Pixels {
        left,
        top,
        width: w as u32,
        height: h as u32,
        rgba,
    }
}

pub fn read(data: &[u8]) -> Result<Psd> {
    let mut r = Reader { data, at: 0 };
    if r.take(4)? != b"8BPS" {
        bail!("not a Photoshop file");
    }
    match r.u16()? {
        1 => {}
        2 => bail!("a large document (PSB): save it as a PSD"),
        v => bail!("a Photoshop file of version {v}"),
    }
    r.take(6)?;
    let merged_channels = r.u16()? as usize;
    let height = r.u32()?;
    let width = r.u32()?;
    let depth = r.u16()?;
    let mode = r.u16()?;
    if !matches!(depth, 8 | 16) {
        bail!("a {depth}-bit document: klad reads 8 and 16 bits; save it as 8 or 16 bits");
    }
    match mode {
        1 | 3 => {}
        4 => bail!("a CMYK document: convert it to RGB in your app first"),
        2 => bail!("an indexed-colour document: convert it to RGB first"),
        m => bail!("a document in colour mode {m}: convert it to RGB first"),
    }
    let colour_data = r.u32()? as usize;
    r.take(colour_data)?;
    let resources = r.u32()? as usize;
    r.take(resources)?;
    let mut notes = BTreeSet::new();
    let section = r.u32()? as usize;
    let section_end = r.at + section;
    let mut records = Vec::new();
    let mut planes = Vec::new();
    if section > 0 {
        let info = r.u32()? as usize;
        let info_end = r.at + info;
        if info > 0 {
            let count = r.i16()?.unsigned_abs() as usize;
            records = (0..count).map(|_| record(&mut r)).collect::<Result<Vec<_>>>()?;
            planes = records
                .iter()
                .map(|rec| {
                    rec.channels
                        .iter()
                        .map(|(id, len)| {
                            let (w, h) = match (*id, rec.mask) {
                                (-2, Some([t, l, b, rr])) => ((rr - l).max(0) as usize, (b - t).max(0) as usize),
                                _ => (
                                    (rec.rect[3] - rec.rect[1]).max(0) as usize,
                                    (rec.rect[2] - rec.rect[0]).max(0) as usize,
                                ),
                            };
                            Ok((*id, plane(&mut r, w, h, depth, Some(*len as usize))?))
                        })
                        .collect::<Result<Vec<_>>>()
                })
                .collect::<Result<Vec<_>>>()?;
        }
        r.at = info_end;
    }
    r.at = section_end;
    records
        .iter()
        .for_each(|rec| notes.extend(rec.kinds.iter().map(|k| format!("{k} came in as pixels"))));
    let mut stack: Vec<Vec<Item>> = vec![Vec::new()];
    records.iter().zip(&planes).for_each(|(rec, planes)| match rec.section {
        3 => stack.push(Vec::new()),
        1 | 2 => {
            let children = match stack.len() > 1 {
                true => stack.pop().unwrap_or_default(),
                false => Vec::new(),
            };
            if let Some(top) = stack.last_mut() {
                top.push(Item::Group {
                    look: rec.look.clone(),
                    open: rec.section == 1,
                    children,
                });
            }
        }
        _ => {
            let pixels = pixels_of(rec, planes, &mut notes);
            if let Some(top) = stack.last_mut() {
                top.push(Item::Layer {
                    look: rec.look.clone(),
                    pixels,
                });
            }
        }
    });
    while stack.len() > 1 {
        let children = stack.pop().unwrap_or_default();
        if let Some(top) = stack.last_mut() {
            top.extend(children);
        }
    }
    let mut items = stack.pop().unwrap_or_default();
    if items.is_empty() {
        let (w, h) = (width as usize, height as usize);
        let merged: Vec<(i16, Vec<u8>)> = {
            let compression = r.u16()?;
            let bytes = (depth / 8) as usize;
            let all: Vec<Vec<u8>> = match compression {
                0 => (0..merged_channels)
                    .map(|_| r.take(w * h * bytes).map(|p| p.chunks(bytes).map(|c| c[0]).collect()))
                    .collect::<Result<_>>()?,
                1 => {
                    let counts = (0..merged_channels * h)
                        .map(|_| r.u16().map(|v| v as usize))
                        .collect::<Result<Vec<_>>>()?;
                    counts
                        .chunks(h)
                        .map(|rows| {
                            let mut out = Vec::with_capacity(w * h * bytes);
                            rows.iter().try_for_each(|n| {
                                out.extend(unpack_bits(r.take(*n)?, w * bytes));
                                Ok::<_, anyhow::Error>(())
                            })?;
                            Ok(out.chunks(bytes).map(|c| c[0]).collect())
                        })
                        .collect::<Result<_>>()?
                }
                other => bail!("a merged picture packed in a way klad does not read ({other})"),
            };
            let ids: [i16; 4] = match mode {
                1 => [0, -1, -9, -9],
                _ => [0, 1, 2, -1],
            };
            all.into_iter().zip(ids).map(|(p, id)| (id, p)).collect()
        };
        let rec = Record {
            look: Look::named("Background"),
            rect: [0, 0, height as i32, width as i32],
            channels: Vec::new(),
            section: 0,
            mask: None,
            mask_default: 0,
            kinds: BTreeSet::new(),
        };
        items.push(Item::Layer {
            look: rec.look.clone(),
            pixels: pixels_of(&rec, &merged, &mut notes),
        });
        notes.insert("the file has no layers: its picture came in as one layer".into());
    }
    if depth == 16 {
        notes.insert("16-bit colour came in as 8 bits".into());
    }
    Ok(Psd {
        width,
        height,
        items,
        notes: notes.into_iter().collect(),
    })
}

fn be16(v: u16) -> [u8; 2] {
    v.to_be_bytes()
}

fn be32(v: u32) -> [u8; 4] {
    v.to_be_bytes()
}

fn packed_planes(planes: &[Vec<u8>], width: usize) -> (Vec<u8>, Vec<u8>) {
    let rows: Vec<Vec<u8>> = planes
        .iter()
        .flat_map(|p| p.chunks(width.max(1)).map(pack_bits).collect::<Vec<_>>())
        .collect();
    let counts: Vec<u8> = rows.iter().flat_map(|r| be16(r.len() as u16)).collect();
    (counts, rows.concat())
}

fn split(p: &Pixels) -> [Vec<u8>; 4] {
    let at = |c: usize| p.rgba.iter().skip(c).step_by(4).copied().collect::<Vec<u8>>();
    [at(3), at(0), at(1), at(2)]
}

fn channel(plane: &[u8], width: usize) -> Vec<u8> {
    match plane.is_empty() {
        true => be16(0).to_vec(),
        false => {
            let (counts, data) = packed_planes(&[plane.to_vec()], width);
            [be16(1).to_vec(), counts, data].concat()
        }
    }
}

fn name_block(name: &str) -> Vec<u8> {
    let short: Vec<u8> = name.bytes().filter(|b| b.is_ascii()).take(255).collect();
    let mut out = vec![short.len() as u8];
    out.extend(&short);
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
    let units: Vec<u16> = name.encode_utf16().collect();
    let mut body = be32(units.len() as u32).to_vec();
    body.extend(units.iter().flat_map(|u| be16(*u)));
    if !body.len().is_multiple_of(2) {
        body.push(0);
    }
    out.extend(b"8BIMluni");
    out.extend(be32(body.len() as u32));
    out.extend(body);
    out
}

struct Out {
    record: Vec<u8>,
    data: Vec<u8>,
}

fn record_of(look: &Look, pixels: Option<&Pixels>, section: Option<u32>) -> Out {
    let empty = Pixels {
        left: 0,
        top: 0,
        width: 0,
        height: 0,
        rgba: Vec::new(),
    };
    let p = pixels.unwrap_or(&empty);
    let planes = split(p);
    let ids: [i16; 4] = [-1, 0, 1, 2];
    let chans: Vec<Vec<u8>> = planes.iter().map(|pl| channel(pl, p.width as usize)).collect();
    let mut rec = Vec::new();
    rec.extend(be32(p.top as u32));
    rec.extend(be32(p.left as u32));
    rec.extend(be32((p.top + p.height as i32) as u32));
    rec.extend(be32((p.left + p.width as i32) as u32));
    rec.extend(be16(4));
    ids.iter().zip(&chans).for_each(|(id, c)| {
        rec.extend(id.to_be_bytes());
        rec.extend(be32(c.len() as u32));
    });
    rec.extend(b"8BIM");
    let key = match section {
        Some(1 | 2) if look.blend == "norm" => "pass",
        _ => look.blend.as_str(),
    };
    rec.extend(format!("{key:<4}").as_bytes().iter().take(4));
    rec.push((look.opacity.clamp(0.0, 1.0) * 255.0).round() as u8);
    rec.push(look.clip as u8);
    let mut flags = 0x08u8;
    if !look.visible {
        flags |= 0x02;
    }
    if section.is_some() {
        flags |= 0x10;
    }
    rec.push(flags);
    rec.push(0);
    let mut extra = Vec::new();
    extra.extend(be32(0));
    extra.extend(be32(0));
    extra.extend(name_block(&look.name));
    if let Some(kind) = section {
        extra.extend(b"8BIMlsct");
        match kind {
            3 => {
                extra.extend(be32(4));
                extra.extend(be32(3));
            }
            k => {
                extra.extend(be32(12));
                extra.extend(be32(k));
                extra.extend(b"8BIM");
                extra.extend(format!("{key:<4}").as_bytes().iter().take(4));
            }
        }
    }
    rec.extend(be32(extra.len() as u32));
    rec.extend(extra);
    Out {
        record: rec,
        data: chans.concat(),
    }
}

fn flatten(items: &[Item], out: &mut Vec<Out>) {
    items.iter().for_each(|item| match item {
        Item::Layer { look, pixels } => out.push(record_of(look, Some(pixels), None)),
        Item::Group { look, open, children } => {
            out.push(record_of(&Look::named("</Layer group>"), None, Some(3)));
            flatten(children, out);
            out.push(record_of(look, None, Some(if *open { 1 } else { 2 })));
        }
    });
}

pub fn write(psd: &Psd, merged: &[u8]) -> Result<Vec<u8>> {
    let (w, h) = (psd.width, psd.height);
    if w == 0 || h == 0 || w > 30000 || h > 30000 {
        bail!("a PSD is 1 to 30000 px a side; this one is {w}x{h}");
    }
    if merged.len() != (w * h * 4) as usize {
        return Err(anyhow!("the merged picture is not {w}x{h}"));
    }
    let mut out = Vec::new();
    out.extend(b"8BPS");
    out.extend(be16(1));
    out.extend([0u8; 6]);
    out.extend(be16(4));
    out.extend(be32(h));
    out.extend(be32(w));
    out.extend(be16(8));
    out.extend(be16(3));
    out.extend(be32(0));
    out.extend(be32(0));
    let mut records = Vec::new();
    flatten(&psd.items, &mut records);
    let mut info = Vec::new();
    info.extend((-(records.len() as i16)).to_be_bytes());
    records.iter().for_each(|r| info.extend(&r.record));
    records.iter().for_each(|r| info.extend(&r.data));
    if !info.len().is_multiple_of(2) {
        info.push(0);
    }
    let mut section = Vec::new();
    section.extend(be32(info.len() as u32));
    section.extend(info);
    section.extend(be32(0));
    out.extend(be32(section.len() as u32));
    out.extend(section);
    let whole = Pixels {
        left: 0,
        top: 0,
        width: w,
        height: h,
        rgba: merged.to_vec(),
    };
    let [a, r, g, b] = split(&whole);
    let (counts, data) = packed_planes(&[r, g, b, a], w as usize);
    out.extend(be16(1));
    out.extend(counts);
    out.extend(data);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(left: i32, top: i32, w: u32, h: u32, colour: [u8; 4]) -> Pixels {
        Pixels {
            left,
            top,
            width: w,
            height: h,
            rgba: (0..w * h)
                .flat_map(|i| if i % 7 == 0 { [0, 0, 0, 0] } else { colour })
                .collect(),
        }
    }

    #[test]
    fn layers_groups_and_their_looks_come_back_from_a_written_psd() {
        let psd = Psd {
            width: 40,
            height: 30,
            items: vec![
                Item::Layer {
                    look: Look::named("paper"),
                    pixels: square(0, 0, 40, 30, [240, 230, 210, 255]),
                },
                Item::Group {
                    look: Look {
                        name: "figure ü".into(),
                        visible: true,
                        opacity: 0.5,
                        blend: "norm".into(),
                        clip: false,
                    },
                    open: true,
                    children: vec![
                        Item::Layer {
                            look: Look {
                                name: "shadow".into(),
                                visible: false,
                                opacity: 0.6,
                                blend: "mul ".into(),
                                clip: false,
                            },
                            pixels: square(5, 4, 10, 6, [20, 30, 40, 200]),
                        },
                        Item::Layer {
                            look: Look::named("body"),
                            pixels: square(-3, 2, 12, 12, [200, 60, 50, 255]),
                        },
                    ],
                },
            ],
            notes: Vec::new(),
        };
        let merged: Vec<u8> = (0..40 * 30).flat_map(|i| [(i % 256) as u8, 9, 9, 255]).collect();
        let bytes = write(&psd, &merged).unwrap();
        let back = read(&bytes).unwrap();
        assert_eq!((back.width, back.height), (40, 30));
        let group = match &back.items[1] {
            Item::Group { look, children, .. } => {
                assert_eq!(look.name, "figure ü");
                assert!((look.opacity - 0.5).abs() < 0.01);
                assert_eq!(look.blend, "pass");
                children.clone()
            }
            other => panic!("{other:?}"),
        };
        assert_eq!(back.items[0], psd.items[0]);
        match (&group[0], &group[1]) {
            (
                Item::Layer {
                    look: shadow,
                    pixels: sp,
                },
                Item::Layer { look: body, pixels: bp },
            ) => {
                assert!(!shadow.visible && shadow.blend == "mul " && (shadow.opacity - 0.6).abs() < 0.01);
                assert_eq!(sp, &square(5, 4, 10, 6, [20, 30, 40, 200]));
                assert_eq!(body.name, "body");
                assert_eq!(bp, &square(-3, 2, 12, 12, [200, 60, 50, 255]));
            }
            other => panic!("{other:?}"),
        }
        let row: Vec<u8> = [1, 1, 1, 1, 2, 3, 4, 4, 4, 9].to_vec();
        assert_eq!(unpack_bits(&pack_bits(&row), row.len()), row);
        let long: Vec<u8> = (0..300).map(|i| (i / 5) as u8).collect();
        assert_eq!(unpack_bits(&pack_bits(&long), long.len()), long);
        assert!(read(b"8BPS\0\x02").is_err());
        let names = |items: &[Item]| -> Vec<String> {
            items
                .iter()
                .map(|i| match i {
                    Item::Layer { look, .. } | Item::Group { look, .. } => look.name.clone(),
                })
                .collect()
        };
        let clipped = read(include_bytes!("../testdata/psd/green-clipping-10x10.psd")).unwrap();
        assert_eq!(
            names(&clipped.items),
            ["Clipping base", "First clipped layer", "Second clipped layer"]
        );
        match &clipped.items[1] {
            Item::Layer { look, pixels } => {
                assert!(look.clip, "the clipping is read");
                assert_eq!(
                    (pixels.width, pixels.height, &pixels.rgba[..4]),
                    (10, 5, &[255, 0, 0, 255][..])
                );
            }
            other => panic!("{other:?}"),
        }
        let unicode = read(include_bytes!("../testdata/psd/luni.psd")).unwrap();
        assert!(
            names(&unicode.items).iter().any(|n| n == "2 のコピー"),
            "{:?}",
            names(&unicode.items)
        );
        let rle = read(include_bytes!("../testdata/psd/rle-3-layer-8x8.psd")).unwrap();
        assert_eq!(names(&rle.items), ["Red Layer", "Green Layer", "Blue Layer"]);
        let larger = read(include_bytes!("../testdata/psd/layer-larger.psd")).unwrap();
        match &larger.items[0] {
            Item::Layer { pixels, .. } => {
                assert_eq!((pixels.left, pixels.top, pixels.width, pixels.height), (-6, -3, 13, 9))
            }
            other => panic!("{other:?}"),
        }
        assert!(read(b"GIF89a").is_err());
    }
}
