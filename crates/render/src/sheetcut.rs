use crate::pixelise::Picture;
use anyhow::{Result, bail};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Cut {
    pub cell: Option<(u32, u32)>,
    pub grid: Option<(u32, u32)>,
    pub padding: u32,
    pub margin: u32,
}

pub fn size_of(text: &str, what: &str) -> Result<(u32, u32)> {
    let parts: Vec<&str> = text.split(['x', 'X', ',']).map(str::trim).collect();
    match parts[..] {
        [w, h] => match (w.parse::<u32>(), h.parse::<u32>()) {
            (Ok(w), Ok(h)) if w > 0 && h > 0 => Ok((w, h)),
            _ => bail!("{what} takes two whole numbers above 0, like 32x32: not {text:?}"),
        },
        [n] => match n.parse::<u32>() {
            Ok(n) if n > 0 => Ok((n, n)),
            _ => bail!("{what} takes two whole numbers above 0, like 32x32: not {text:?}"),
        },
        _ => bail!("{what} takes two whole numbers above 0, like 32x32: not {text:?}"),
    }
}

fn crop(p: &Picture, x: u32, y: u32, w: u32, h: u32) -> Picture {
    let rgba = (y..y + h)
        .flat_map(|row| {
            let start = ((row * p.width + x) * 4) as usize;
            p.rgba[start..start + (w * 4) as usize].to_vec()
        })
        .collect();
    Picture {
        width: w,
        height: h,
        rgba,
    }
}

fn empty(p: &Picture) -> bool {
    p.rgba.as_chunks::<4>().0.iter().all(|px| px[3] == 0)
}

pub fn cells(sheet: &Picture, cut: &Cut) -> Result<(u32, u32, Vec<Picture>)> {
    let inner = |total: u32| total.saturating_sub(2 * cut.margin);
    let (cw, ch) = match (cut.cell, cut.grid) {
        (Some(cell), _) => cell,
        (None, Some((cols, rows))) => (
            ((inner(sheet.width) + cut.padding) / cols.max(1)).saturating_sub(cut.padding),
            ((inner(sheet.height) + cut.padding) / rows.max(1)).saturating_sub(cut.padding),
        ),
        (None, None) => bail!("say how to cut the sheet: --cell 32x32 or --grid 8x4 (columns by rows)"),
    };
    if cw == 0 || ch == 0 {
        bail!(
            "the cells come out empty: check --grid, --padding and --margin against the sheet's {}x{} px",
            sheet.width,
            sheet.height
        );
    }
    let step = (cw + cut.padding, ch + cut.padding);
    let cols = (inner(sheet.width) + cut.padding) / step.0;
    let rows = (inner(sheet.height) + cut.padding) / step.1;
    if cols == 0 || rows == 0 {
        bail!(
            "a {cw}x{ch} cell does not fit in the {}x{} sheet",
            sheet.width,
            sheet.height
        );
    }
    let pictures: Vec<Picture> = (0..rows)
        .flat_map(|r| (0..cols).map(move |c| (c, r)))
        .map(|(c, r)| crop(sheet, cut.margin + c * step.0, cut.margin + r * step.1, cw, ch))
        .filter(|p| !empty(p))
        .collect();
    Ok((cw, ch, pictures))
}
