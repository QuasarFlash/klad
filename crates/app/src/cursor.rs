use egui::CursorIcon;

#[derive(Default)]
pub(crate) struct Settle {
    shown: Option<CursorIcon>,
    stale: bool,
}

impl Settle {
    pub(crate) fn next(&mut self, wanted: CursorIcon, down: bool, inside: bool) -> CursorIcon {
        if !inside {
            return wanted;
        }
        match (down, self.shown) {
            (true, Some(shown)) => {
                self.stale |= wanted != shown;
                wanted
            }
            (false, Some(shown)) if self.stale => {
                self.stale = false;
                shown
            }
            _ => {
                self.shown = Some(wanted);
                wanted
            }
        }
    }

    pub(crate) fn frame(&mut self, ctx: &egui::Context) {
        let wanted = ctx.output(|o| o.cursor_icon);
        let (down, inside) = ctx.input(|i| (i.pointer.any_down(), i.pointer.hover_pos().is_some()));
        ctx.set_cursor_icon(self.next(wanted, down, inside));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cursor_changed_while_a_button_is_down_is_sent_again_after_the_release() {
        let mut settle = Settle::default();
        let resize = CursorIcon::ResizeEast;
        assert_eq!(settle.next(resize, false, true), resize);
        assert_eq!(
            settle.next(CursorIcon::Default, true, true),
            CursorIcon::Default,
            "the change goes out while the pen is down, and macOS drops it"
        );
        assert_eq!(settle.next(CursorIcon::Default, true, true), CursorIcon::Default);
        assert_eq!(
            settle.next(CursorIcon::Default, false, true),
            resize,
            "after the release, the cursor still on screen is asked for once"
        );
        assert_eq!(
            settle.next(CursorIcon::Default, false, true),
            CursorIcon::Default,
            "so the wanted one is a change again and is sent with the button up"
        );
        assert_eq!(settle.next(CursorIcon::Default, false, true), CursorIcon::Default);
    }

    #[test]
    fn a_pen_that_comes_back_down_elsewhere_still_gets_its_cursor() {
        let mut settle = Settle::default();
        let resize = CursorIcon::ResizeEast;
        settle.next(resize, false, true);
        assert_eq!(settle.next(CursorIcon::Default, false, false), CursorIcon::Default);
        assert_eq!(settle.next(CursorIcon::Default, true, true), CursorIcon::Default);
        assert_eq!(
            settle.next(CursorIcon::Default, false, true),
            resize,
            "out of reach and down again at once: the old cursor is asked for once"
        );
        assert_eq!(settle.next(CursorIcon::Default, false, true), CursorIcon::Default);
    }

    #[test]
    fn a_press_that_keeps_the_cursor_changes_nothing() {
        let mut settle = Settle::default();
        let grab = CursorIcon::Grab;
        settle.next(grab, false, true);
        assert_eq!(settle.next(grab, true, true), grab);
        assert_eq!(settle.next(grab, false, true), grab);
        assert_eq!(settle.next(CursorIcon::Default, false, true), CursorIcon::Default);
    }
}
