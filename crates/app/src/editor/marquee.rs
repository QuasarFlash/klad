use super::Editor;
use egui::{Vec2, vec2};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Marquee {
    #[default]
    Box,
    Loop,
    Ellipse,
    Wand,
}

const ROUND: usize = 64;

impl Marquee {
    pub(crate) const ALL: [Marquee; 4] = [Marquee::Box, Marquee::Loop, Marquee::Ellipse, Marquee::Wand];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Marquee::Box => "box",
            Marquee::Loop => "loop",
            Marquee::Ellipse => "ellipse",
            Marquee::Wand => "wand",
        }
    }

    pub(crate) fn hint(self, what: &str) -> String {
        match self {
            Marquee::Box => format!("drag a box around the {what} to select; alt takes away from the selection"),
            Marquee::Loop => format!("draw a loop around the {what} to select; alt takes away from the selection"),
            Marquee::Ellipse => {
                format!("drag an ellipse around the {what} to select (shift: a circle, alt as you start: takes away)")
            }
            Marquee::Wand => format!(
                "click an area of one colour to select the {what} there, up to its edge; shift adds, alt takes away"
            ),
        }
    }
}

pub(super) fn ellipse(a: Vec2, b: Vec2, circle: bool) -> Vec<Vec2> {
    let d = b - a;
    let d = match circle {
        true => {
            let side = d.x.abs().max(d.y.abs());
            vec2(side.copysign(d.x), side.copysign(d.y))
        }
        false => d,
    };
    let (lo, hi) = (a.min(a + d), a.max(a + d));
    klad_core::region::ellipse(lo.x, lo.y, hi.x - lo.x, hi.y - lo.y, ROUND)
        .into_iter()
        .map(|[x, y]| vec2(x, y))
        .collect()
}

impl Editor {
    pub(super) fn wand_polygon(&self, at: Vec2) -> Option<Vec<[f32; 2]>> {
        let raster = self.scene.active_raster()?;
        klad_render::wand::region(raster, at.x, at.y, self.wand_tolerance, self.scene.on_grid())
    }
}
