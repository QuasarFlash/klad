use super::{PenEvent, Phase};
use std::time::{Duration, Instant};

pub(crate) const FLAG_INCONTACT: u32 = 0x4;
pub(crate) const FLAG_CANCELED: u32 = 0x8000;
pub(crate) const FLAG_DOWN: u32 = 0x1_0000;
pub(crate) const FLAG_UP: u32 = 0x4_0000;
const KEPT: usize = 4096;
const TILT_DEGREES: f32 = 60.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Contact {
    Down,
    Touching,
    Up,
    Hover,
}

pub(crate) fn contact(flags: u32) -> Contact {
    match flags {
        f if f & (FLAG_UP | FLAG_CANCELED) != 0 => Contact::Up,
        f if f & FLAG_DOWN != 0 => Contact::Down,
        f if f & FLAG_INCONTACT != 0 => Contact::Touching,
        _ => Contact::Hover,
    }
}

pub(crate) fn tilt_of(degrees_x: i32, degrees_y: i32) -> (f32, f32) {
    (
        (degrees_x as f32 / TILT_DEGREES).clamp(-1.0, 1.0),
        (-degrees_y as f32 / TILT_DEGREES).clamp(-1.0, 1.0),
    )
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Sample {
    pub contact: Contact,
    pub x: f32,
    pub y: f32,
    pub pressure: Option<f32>,
    pub tilt: (f32, f32),
    pub eraser: bool,
    pub tablet: bool,
    pub device: u64,
    pub age: Duration,
}

#[derive(Clone, Copy, Debug)]
struct Raw {
    phase: Phase,
    x: f32,
    y: f32,
    pressure: f32,
    tilt: (f32, f32),
    eraser: bool,
    tablet: bool,
    device: u64,
    at: Instant,
}

#[derive(Default)]
pub(crate) struct Ink {
    queue: Vec<Raw>,
    down: bool,
    by_pen: bool,
    last: Option<Raw>,
}

impl Ink {
    pub const fn new() -> Self {
        Self {
            queue: Vec::new(),
            down: false,
            by_pen: false,
            last: None,
        }
    }

    fn push(&mut self, raw: Raw) {
        if self.queue.len() >= KEPT {
            self.queue.drain(..KEPT / 2);
        }
        self.last = Some(raw);
        self.queue.push(raw);
    }

    pub fn lost(&mut self, at: Instant) {
        if !self.down {
            return;
        }
        self.down = false;
        if let Some(last) = self.last {
            self.push(Raw {
                phase: Phase::Up,
                pressure: 0.0,
                at,
                ..last
            });
        }
    }

    pub fn sample(&mut self, s: Sample, now: Instant) {
        let at = now.checked_sub(s.age).unwrap_or(now);
        let phase = match (s.contact, self.down) {
            (Contact::Down, true) => {
                self.lost(at);
                Phase::Down
            }
            (Contact::Down | Contact::Touching, false) => Phase::Down,
            (Contact::Touching, true) => Phase::Move,
            (Contact::Up, true) => Phase::Up,
            (Contact::Up, false) => return,
            (Contact::Hover, down) => {
                if down {
                    self.lost(at);
                }
                Phase::Move
            }
        };
        self.down = phase != Phase::Up && s.contact != Contact::Hover;
        if phase == Phase::Down {
            self.by_pen = s.tablet;
        }
        let pressure = match phase {
            Phase::Up => 0.0,
            _ if s.contact == Contact::Hover => 0.0,
            _ => s.pressure.unwrap_or(1.0).clamp(0.0, 1.0),
        };
        self.push(Raw {
            phase,
            x: s.x,
            y: s.y,
            pressure,
            tilt: s.tilt,
            eraser: s.eraser,
            tablet: s.tablet,
            device: s.device,
            at,
        });
    }

    pub fn mouse(&mut self, phase: Phase, x: f32, y: f32, now: Instant) {
        if self.down && self.by_pen {
            return;
        }
        let contact = match (phase, self.down) {
            (Phase::Down, _) => Contact::Down,
            (Phase::Up, _) => Contact::Up,
            (Phase::Move, true) => Contact::Touching,
            (Phase::Move, false) => Contact::Hover,
        };
        self.sample(
            Sample {
                contact,
                x,
                y,
                pressure: None,
                tilt: (0.0, 0.0),
                eraser: false,
                tablet: false,
                device: 0,
                age: Duration::ZERO,
            },
            now,
        );
    }

    pub fn take(&mut self, pixels_per_point: f32) -> Vec<PenEvent> {
        let scale = pixels_per_point.max(f32::EPSILON);
        std::mem::take(&mut self.queue)
            .into_iter()
            .map(|r| PenEvent {
                phase: r.phase,
                pos: egui::pos2(r.x / scale, r.y / scale),
                pressure: match (r.tablet, r.phase) {
                    (false, Phase::Up) => 0.0,
                    (false, _) => 1.0,
                    (true, _) => r.pressure,
                },
                tilt: r.tilt,
                eraser: r.eraser,
                tablet: r.tablet,
                device: r.device,
                at: r.at,
            })
            .collect()
    }

    pub fn clear(&mut self) {
        self.queue.clear();
        self.down = false;
        self.by_pen = false;
        self.last = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pen(contact: Contact, x: f32, pressure: Option<f32>) -> Sample {
        Sample {
            contact,
            x,
            y: 10.0,
            pressure,
            tilt: tilt_of(30, -60),
            eraser: false,
            tablet: true,
            device: 9,
            age: Duration::ZERO,
        }
    }

    fn phases(events: &[PenEvent]) -> Vec<(Phase, f32)> {
        events.iter().map(|e| (e.phase, e.pressure)).collect()
    }

    #[test]
    fn the_pointer_flags_of_windows_ink_say_down_touching_lifted_or_hovering() {
        assert_eq!(contact(FLAG_DOWN | FLAG_INCONTACT), Contact::Down);
        assert_eq!(contact(0x2_0000 | FLAG_INCONTACT), Contact::Touching);
        assert_eq!(contact(FLAG_UP), Contact::Up);
        assert_eq!(contact(FLAG_CANCELED | FLAG_UP), Contact::Up);
        assert_eq!(contact(0x2_0000 | 0x2), Contact::Hover);
        assert_eq!(tilt_of(30, -60), (0.5, 1.0));
        assert_eq!(tilt_of(-120, 0), (-1.0, 0.0));
    }

    #[test]
    fn a_pen_lifted_with_no_pressure_ends_its_stroke() {
        let mut ink = Ink::new();
        let now = Instant::now();
        [
            pen(Contact::Hover, 1.0, None),
            pen(Contact::Down, 2.0, Some(0.3)),
            pen(Contact::Touching, 3.0, Some(0.6)),
            pen(Contact::Up, 3.0, None),
            pen(Contact::Hover, 4.0, None),
        ]
        .into_iter()
        .for_each(|s| ink.sample(s, now));
        let events = ink.take(2.0);
        assert_eq!(
            phases(&events),
            [
                (Phase::Move, 0.0),
                (Phase::Down, 0.3),
                (Phase::Move, 0.6),
                (Phase::Up, 0.0),
                (Phase::Move, 0.0)
            ]
        );
        assert_eq!(events[2].pos, egui::pos2(1.5, 5.0), "physical pixels become points");
        assert!(events.iter().all(|e| e.tablet && e.device == 9 && e.tilt == (0.5, 1.0)));
        assert!(ink.take(1.0).is_empty());
    }

    #[test]
    fn a_missed_lift_still_ends_the_stroke() {
        let mut ink = Ink::new();
        let now = Instant::now();
        ink.sample(pen(Contact::Down, 1.0, Some(0.5)), now);
        ink.sample(pen(Contact::Touching, 2.0, Some(0.5)), now);
        ink.sample(pen(Contact::Hover, 5.0, None), now);
        assert_eq!(
            phases(&ink.take(1.0)),
            [
                (Phase::Down, 0.5),
                (Phase::Move, 0.5),
                (Phase::Up, 0.0),
                (Phase::Move, 0.0)
            ],
            "hovering after a stroke lifts it where it last touched"
        );
        ink.sample(pen(Contact::Down, 1.0, Some(0.5)), now);
        ink.sample(pen(Contact::Down, 8.0, Some(0.4)), now);
        let events = ink.take(1.0);
        assert_eq!(
            phases(&events),
            [(Phase::Down, 0.5), (Phase::Up, 0.0), (Phase::Down, 0.4)]
        );
        assert_eq!(events[1].pos.x, 1.0);
        ink.sample(pen(Contact::Touching, 2.0, Some(0.7)), now);
        ink.lost(now);
        ink.lost(now);
        assert_eq!(
            phases(&ink.take(1.0)),
            [(Phase::Move, 0.7), (Phase::Up, 0.0)],
            "losing the pen mid-stroke lifts it once"
        );
        ink.sample(pen(Contact::Up, 2.0, None), now);
        assert!(ink.take(1.0).is_empty(), "a lift with nothing down is dropped");
    }

    #[test]
    fn a_contact_whose_down_was_missed_starts_a_stroke_and_pens_without_pressure_draw_full() {
        let mut ink = Ink::new();
        let now = Instant::now();
        ink.sample(pen(Contact::Touching, 1.0, None), now);
        ink.sample(pen(Contact::Touching, 2.0, None), now);
        ink.sample(pen(Contact::Up, 2.0, None), now);
        assert_eq!(
            phases(&ink.take(1.0)),
            [(Phase::Down, 1.0), (Phase::Move, 1.0), (Phase::Up, 0.0)]
        );
    }

    #[test]
    fn coalesced_samples_keep_their_own_times_and_the_mouse_draws_through_the_same_queue() {
        let mut ink = Ink::new();
        let now = Instant::now();
        let older = Sample {
            age: Duration::from_millis(4),
            ..pen(Contact::Touching, 1.0, Some(0.5))
        };
        ink.sample(pen(Contact::Down, 0.0, Some(0.5)), now);
        ink.sample(older, now);
        ink.mouse(Phase::Move, 9.0, 9.0, now);
        ink.sample(pen(Contact::Touching, 2.0, Some(0.5)), now);
        ink.mouse(Phase::Up, 9.0, 9.0, now);
        ink.sample(pen(Contact::Up, 2.0, None), now);
        ink.mouse(Phase::Move, 4.0, 4.0, now);
        ink.mouse(Phase::Down, 4.0, 4.0, now);
        ink.mouse(Phase::Move, 6.0, 4.0, now);
        ink.mouse(Phase::Up, 6.0, 4.0, now);
        let events = ink.take(1.0);
        assert_eq!(events[2].at - events[1].at, Duration::from_millis(4));
        assert!(
            events[..4].iter().all(|e| e.tablet),
            "the mouse moves Windows makes up during a pen stroke are not drawn"
        );
        assert_eq!(
            phases(&events[4..]),
            [
                (Phase::Move, 1.0),
                (Phase::Down, 1.0),
                (Phase::Move, 1.0),
                (Phase::Up, 0.0)
            ]
        );
        assert!(events[4..].iter().all(|e| !e.tablet && e.device == 0));
    }

    #[test]
    fn the_queue_stays_bounded_when_nothing_drains_it() {
        let mut ink = Ink::new();
        let now = Instant::now();
        (0..KEPT * 3).for_each(|i| ink.mouse(Phase::Move, i as f32, 0.0, now));
        let events = ink.take(1.0);
        assert!(events.len() <= KEPT);
        assert_eq!(events.last().map(|e| e.pos.x), Some((KEPT * 3 - 1) as f32));
        ink.mouse(Phase::Down, 1.0, 1.0, now);
        ink.clear();
        ink.lost(now);
        assert!(ink.take(1.0).is_empty(), "a cleared queue forgets the stroke too");
    }
}
