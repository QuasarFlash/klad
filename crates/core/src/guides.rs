use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Grid {
    pub every: [f32; 2],
    pub sub: u32,
    pub offset: [f32; 2],
    pub iso: bool,
    pub colour: String,
    pub opacity: f32,
}

impl Default for Grid {
    fn default() -> Self {
        Self {
            every: [32.0, 32.0],
            sub: 4,
            offset: [0.0, 0.0],
            iso: false,
            colour: "#4f8fe8".into(),
            opacity: 0.35,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Guides {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grid: Option<Grid>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub show_grid: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub x: Vec<f32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub y: Vec<f32>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub locked: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub snap: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub assists: Vec<crate::assist::Assist>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub follow: bool,
}

impl Guides {
    pub fn is_empty(&self) -> bool {
        *self == Guides::default()
    }

    pub fn grid_or_default(&self, tile: Option<[u32; 2]>) -> Grid {
        self.grid.clone().unwrap_or_else(|| match tile {
            Some([w, h]) => Grid {
                every: [w as f32, h as f32],
                ..Grid::default()
            },
            None => Grid::default(),
        })
    }

    pub fn lines_x(&self, grid: &Grid, width: f32) -> Vec<f32> {
        lines(grid.offset[0], grid.every[0], width)
    }

    pub fn lines_y(&self, grid: &Grid, height: f32) -> Vec<f32> {
        lines(grid.offset[1], grid.every[1], height)
    }
}

fn lines(offset: f32, every: f32, length: f32) -> Vec<f32> {
    if every <= 0.0 {
        return Vec::new();
    }
    let first = offset.rem_euclid(every);
    (0..)
        .map(|i| first + i as f32 * every)
        .take_while(|v| *v <= length)
        .collect()
}

pub fn nearest_line(v: f32, offset: f32, every: f32) -> f32 {
    match every > 0.0 {
        true => ((v - offset) / every).round() * every + offset,
        false => v,
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GridEdit {
    pub off: bool,
    pub every: Option<[f32; 2]>,
    pub sub: Option<u32>,
    pub offset: Option<[f32; 2]>,
    pub iso: Option<bool>,
    pub colour: Option<String>,
    pub opacity: Option<f32>,
    pub show: Option<bool>,
    pub snap: Option<bool>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GuideEdit {
    pub add_x: Vec<f32>,
    pub add_y: Vec<f32>,
    pub rm_x: Vec<f32>,
    pub rm_y: Vec<f32>,
    pub clear: bool,
    pub hidden: Option<bool>,
    pub locked: Option<bool>,
    pub snap: Option<bool>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AssistEdit {
    pub add: Vec<crate::assist::Assist>,
    pub set: Option<(usize, crate::assist::Assist)>,
    pub remove: Vec<usize>,
    pub clear: bool,
    pub on: Option<bool>,
}

pub fn edit_assists(g: &mut Guides, e: &AssistEdit) -> Result<()> {
    if e.clear {
        g.assists.clear();
    }
    if let Some((i, a)) = &e.set {
        match g.assists.get_mut(*i) {
            Some(slot) => *slot = a.clone(),
            None => bail!("there is no guide {i}: the drawing has {}", g.assists.len()),
        }
    }
    let mut gone = e.remove.clone();
    gone.sort_unstable();
    gone.dedup();
    gone.iter().rev().try_for_each(|i| match *i < g.assists.len() {
        true => {
            g.assists.remove(*i);
            Ok(())
        }
        false => bail!("there is no guide {i}: the drawing has {}", g.assists.len()),
    })?;
    e.add.iter().try_for_each(|a| {
        if let crate::assist::Assist::Perspective { points } = a
            && !(1..=3).contains(&points.len())
        {
            bail!("perspective takes one, two or three vanishing points");
        }
        g.assists.push(a.clone());
        Ok(())
    })?;
    if let Some(on) = e.on {
        g.follow = on;
    }
    Ok(())
}

pub fn edit_grid(g: &mut Guides, e: &GridEdit, tile: Option<[u32; 2]>) -> Result<()> {
    if e.off {
        g.grid = None;
        g.show_grid = false;
        return Ok(());
    }
    let mut grid = g.grid_or_default(tile);
    if let Some(every) = e.every {
        if every.iter().any(|v| !(1.0..=4096.0).contains(v)) {
            bail!("the grid's spacing takes 1 to 4096 px");
        }
        grid.every = every;
    }
    if let Some(sub) = e.sub {
        if sub > 64 {
            bail!("a stronger line every 0 to 64 lines");
        }
        grid.sub = sub;
    }
    if let Some(offset) = e.offset {
        grid.offset = offset;
    }
    if let Some(iso) = e.iso {
        grid.iso = iso;
    }
    if let Some(colour) = &e.colour {
        crate::color::parse_hex(colour)?;
        grid.colour = colour.clone();
    }
    if let Some(opacity) = e.opacity {
        if !(0.0..=1.0).contains(&opacity) {
            bail!("the grid's opacity takes 0 to 1");
        }
        grid.opacity = opacity;
    }
    g.grid = Some(grid);
    if let Some(show) = e.show {
        g.show_grid = show;
    }
    if let Some(snap) = e.snap {
        g.snap = snap;
    }
    Ok(())
}

pub fn edit_guides(g: &mut Guides, e: &GuideEdit) -> Result<()> {
    if e.clear {
        g.x.clear();
        g.y.clear();
    }
    let gone = |list: &mut Vec<f32>, rm: &[f32]| list.retain(|v| !rm.iter().any(|r| (r - v).abs() < 0.5));
    gone(&mut g.x, &e.rm_x);
    gone(&mut g.y, &e.rm_y);
    let add = |list: &mut Vec<f32>, more: &[f32]| {
        more.iter().for_each(|v| {
            if !list.iter().any(|x| (x - v).abs() < 0.01) {
                list.push(*v);
            }
        });
        list.sort_by(f32::total_cmp);
    };
    add(&mut g.x, &e.add_x);
    add(&mut g.y, &e.add_y);
    if let Some(hidden) = e.hidden {
        g.hidden = hidden;
    }
    if let Some(locked) = e.locked {
        g.locked = locked;
    }
    if let Some(snap) = e.snap {
        g.snap = snap;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grids_and_guides_are_edited_and_lines_fall_on_the_spacing() {
        let mut g = Guides::default();
        assert!(g.is_empty());
        edit_grid(
            &mut g,
            &GridEdit {
                every: Some([16.0, 8.0]),
                show: Some(true),
                snap: Some(true),
                ..GridEdit::default()
            },
            None,
        )
        .unwrap();
        let grid = g.grid.clone().unwrap();
        assert_eq!(g.lines_x(&grid, 40.0), [0.0, 16.0, 32.0]);
        assert_eq!(g.lines_y(&grid, 20.0), [0.0, 8.0, 16.0]);
        assert_eq!(nearest_line(21.0, 0.0, 16.0), 16.0);
        assert_eq!(nearest_line(25.0, 4.0, 16.0), 20.0);
        assert!(
            edit_grid(
                &mut g,
                &GridEdit {
                    every: Some([0.0, 8.0]),
                    ..GridEdit::default()
                },
                None
            )
            .is_err()
        );
        edit_guides(
            &mut g,
            &GuideEdit {
                add_x: vec![120.0, 40.0],
                add_y: vec![64.0],
                ..GuideEdit::default()
            },
        )
        .unwrap();
        assert_eq!(g.x, [40.0, 120.0]);
        edit_guides(
            &mut g,
            &GuideEdit {
                rm_x: vec![40.2],
                ..GuideEdit::default()
            },
        )
        .unwrap();
        assert_eq!(g.x, [120.0]);
        edit_grid(
            &mut g,
            &GridEdit {
                off: true,
                ..GridEdit::default()
            },
            None,
        )
        .unwrap();
        assert!(g.grid.is_none() && !g.show_grid && g.x == [120.0]);
        assert_eq!(Guides::default().grid_or_default(Some([16, 24])).every, [16.0, 24.0]);
    }
}
