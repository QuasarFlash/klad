use crate::theme::{self, Hint, radius, space};
use egui::{
    Button, CornerRadius, Frame, InnerResponse, Margin, Response, RichText, Stroke, TextEdit, Ui, WidgetText,
    class::HasClasses as _, text::LayoutJob, vec2,
};

pub fn primary_button<'a>(text: impl Into<WidgetText>) -> Button<'a> {
    Button::new(text)
        .with_class(theme::PRIMARY)
        .min_size(vec2(0.0, theme::CONTROL))
}

pub fn secondary_button<'a>(text: impl Into<WidgetText>) -> Button<'a> {
    Button::new(text)
        .with_class(theme::CENTRED)
        .min_size(vec2(0.0, theme::CONTROL))
}

pub fn ghost_button<'a>(text: impl Into<WidgetText>) -> Button<'a> {
    Button::new(text)
        .with_class(theme::GHOST)
        .min_size(vec2(0.0, theme::CONTROL))
}

pub fn icon_button(ui: &mut Ui, glyph: &str, hint: impl Into<WidgetText>) -> Response {
    ui.add(
        Button::new(RichText::new(glyph).size(theme::ICON))
            .with_class(theme::GHOST)
            .min_size(vec2(theme::CONTROL, theme::CONTROL)),
    )
    .hint(hint)
}

pub fn quiet_icon_button(ui: &mut Ui, glyph: &str, hint: impl Into<WidgetText>) -> Response {
    let id = ui.next_auto_id();
    let lit = ui.ctx().read_response(id).is_some_and(|r| r.hovered());
    let t = ui.ctx().animate_bool_with_time(id.with("lit"), lit, theme::EASE);
    let colour = theme::WEAK.lerp_to_gamma(theme::TEXT, t);
    ui.add(
        Button::new(RichText::new(glyph).size(theme::ICON).color(colour))
            .with_class(theme::GHOST)
            .min_size(vec2(theme::CONTROL, theme::CONTROL)),
    )
    .hint(hint)
}

pub fn field(text: &mut String, width: f32) -> TextEdit<'_> {
    TextEdit::singleline(text)
        .desired_width(width)
        .align(egui::Align2::LEFT_CENTER)
        .min_size(vec2(0.0, theme::CONTROL))
}

pub fn segmented<T: Copy + PartialEq>(ui: &mut Ui, current: T, options: &[(T, &str, &str)]) -> Option<T> {
    let mut picked = None;
    Frame::new()
        .fill(theme::CONTROL_FILL)
        .corner_radius(CornerRadius::same(radius::CONTROL))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                options.iter().for_each(|(value, label, hint)| {
                    let on = *value == current;
                    let button = Button::selectable(on, *label)
                        .with_class(theme::GHOST)
                        .min_size(vec2(0.0, theme::CONTROL));
                    if ui.add(button).hint(*hint).clicked() && !on {
                        picked = Some(*value);
                    }
                });
            });
        });
    picked
}

pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
    card_frame().show(ui, add)
}

pub fn card_frame() -> Frame {
    Frame::new()
        .fill(theme::PANEL)
        .stroke(Stroke::new(theme::HAIRLINE, theme::BORDER))
        .corner_radius(CornerRadius::same(radius::CARD))
        .inner_margin(Margin::same(space::LG as i8))
}

pub fn heading_text(glyph: &str, text: &str, size: f32) -> WidgetText {
    let mut job = LayoutJob::default();
    if !glyph.is_empty() {
        job.append(
            &format!("{glyph} "),
            0.0,
            egui::TextFormat::simple(theme::regular(size), theme::WEAK),
        );
    }
    job.append(text, 0.0, egui::TextFormat::simple(theme::semibold(size), theme::TEXT));
    WidgetText::LayoutJob(job.into())
}

pub fn section_header(ui: &mut Ui, glyph: &str, text: &str) -> Response {
    ui.label(heading_text(glyph, text, theme::HEADING))
}
